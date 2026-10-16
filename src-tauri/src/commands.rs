use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::time::Duration;

use argus_core::Snapshot;
use argus_core::actions::{self, ActionError, Priority, TreeKillReport};
use argus_core::connections::{self, Connection};
use argus_core::details::{self, ProcessDetails};
use argus_core::filelock::{self, LockReport};
use argus_core::history::{self, HistoryMoment, HistoryPoint, HistoryUsage};
use argus_core::rules::{ProcessRule, RuleStats};
use argus_core::services::{ServiceAction, ServiceInfo, StartType};
use argus_core::startup::{self, StartupItem};
use argus_core::theme::{self, AccentColors};
use argus_core::tr;
use argus_service::{Operation, Reply, ServiceStatus};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};

use crate::caption::{self, ButtonRect};
use crate::hotkey::HotkeyError;
use crate::sampler::Control;
use crate::settings::{Language, SettingsStore, SettingsView, Theme, UpdateChannel, UpdateRoute};
use crate::updater::{self, UpdateStatus};
use crate::{AppState, HistoryDir, shell};

#[tauri::command(async)]
pub fn task_manager_status() -> Result<argus_service::taskmgr::Status, String> {
    argus_service::taskmgr::status()
}

#[tauri::command(async)]
pub fn manage_task_manager(action: String) -> Result<argus_service::taskmgr::Status, String> {
    argus_service::taskmgr::manage(&action).map_err(|failure| {
        log::warn!(
            "task manager {} failed {}",
            argus_core::diagnostics::action(&action),
            failure.log()
        );
        failure.message
    })
}

#[tauri::command(async)]
pub fn task_manager_ready(app: tauri::AppHandle) {
    for completion in argus_service::taskmgr::ready::complete() {
        if completion.should_show() {
            shell::show_main(&app);
            crate::TASKMGR_COMPLETED.store(true, std::sync::atomic::Ordering::Release);
        }
        completion.acknowledge();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    elevated: bool,
    accent: Option<AccentColors>,
    /// Relaunched by the installer of an update.
    updated: bool,
    /// The UI language, resolved; the UI renders nothing before it knows.
    locale: argus_core::i18n::Locale,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        elevated: state.elevated,
        accent: theme::accent_colors(),
        updated: std::env::args().any(|a| a == updater::UPDATED_ARG),
        locale: argus_core::i18n::locale(),
    }
}

#[tauri::command]
pub fn subscribe(state: State<'_, AppState>, on_snapshot: Channel<Snapshot>) {
    state.sampler.send(Control::Subscribe(on_snapshot));
}

#[tauri::command]
pub fn set_interval(state: State<'_, AppState>, ms: u64) {
    let ms = ms.clamp(250, 10_000);
    state
        .sampler
        .send(Control::SetInterval(Duration::from_millis(ms)));
}

#[tauri::command]
pub fn set_paused(state: State<'_, AppState>, paused: bool) {
    state.sampler.send(Control::SetPaused(paused));
}

#[tauri::command]
pub fn refresh(state: State<'_, AppState>) {
    state.sampler.send(Control::Refresh);
}

#[tauri::command(async)]
pub fn process_details(pid: u32, path: Option<String>) -> ProcessDetails {
    details::query(pid, path.as_deref())
}

fn privileged(state: &AppState, operation: Operation) -> Result<Option<Reply>, ActionError> {
    if state.elevated {
        return Ok(None);
    }
    match argus_service::request(operation).map_err(ActionError::Os)? {
        Some(Reply::Error(e)) => Err(e),
        reply => Ok(reply),
    }
}

fn privileged_done(state: &AppState, operation: Operation) -> Result<bool, ActionError> {
    match privileged(state, operation)? {
        Some(Reply::Done) => Ok(true),
        None => Ok(false),
        _ => Err(ActionError::Os(tr!("host.serviceReply"))),
    }
}

fn terminate(state: &AppState, pid: u32, create_time: i64, force: bool) -> Result<(), ActionError> {
    if pid == std::process::id() {
        return Err(ActionError::Refused);
    }
    if privileged_done(state, Operation::Terminate { pid, create_time })? {
        return Ok(());
    }
    actions::terminate(pid, create_time, force)
}

#[tauri::command(async)]
pub fn kill_process(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
    force: bool,
) -> Result<(), ActionError> {
    let result = terminate(&state, pid, create_time, force);
    state.sampler.send(Control::Refresh);
    result
}

#[tauri::command(async)]
pub fn kill_process_tree(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
) -> Result<TreeKillReport, ActionError> {
    let result = actions::terminate_tree_with(pid, create_time, |pid, created| {
        terminate(&state, pid, created, false)
    });
    state.sampler.send(Control::Refresh);
    result
}

#[tauri::command(async)]
pub fn set_priority(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
    priority: Priority,
) -> Result<Priority, ActionError> {
    let result = match privileged(
        &state,
        Operation::Priority {
            pid,
            create_time,
            priority,
        },
    ) {
        Ok(Some(Reply::Priority(p))) => Ok(p),
        Ok(None) => actions::set_priority(pid, create_time, priority),
        Ok(_) => Err(ActionError::Os(tr!("host.serviceReply"))),
        Err(e) => Err(e),
    };
    state.sampler.send(Control::Refresh);
    result
}

#[tauri::command(async)]
pub fn set_efficiency_mode(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
    on: bool,
) -> Result<(), ActionError> {
    let result = match privileged_done(
        &state,
        Operation::Efficiency {
            pid,
            create_time,
            on,
        },
    ) {
        Ok(true) => Ok(()),
        Ok(false) => actions::set_efficiency_mode(pid, create_time, on),
        Err(e) => Err(e),
    };
    state.sampler.send(Control::Refresh);
    result
}

/// Opens Explorer with the file selected.
#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err(tr!("host.fileMissing"));
    }
    // Windows paths cannot contain `"`, so quoting is safe; explorer needs the
    // `/select,"path"` form verbatim, hence raw_arg.
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{path}\""))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn settings_view<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> SettingsView {
    use tauri_plugin_autostart::ManagerExt;
    let store = app.state::<SettingsStore>();
    SettingsView {
        settings: store.get(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        locale: argus_core::i18n::locale(),
        hotkey_error: crate::hotkey::error(app),
        default_hotkey: crate::settings::DEFAULT_HOTKEY,
    }
}

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> SettingsView {
    settings_view(&app)
}

#[tauri::command(async)]
pub fn privileged_service_status() -> Result<ServiceStatus, String> {
    argus_service::status()
}

#[tauri::command(async)]
pub fn manage_privileged_service(
    state: State<'_, AppState>,
    action: String,
) -> Result<ServiceStatus, String> {
    let status = argus_service::manage(&action).inspect_err(|e| {
        log::warn!(
            "privileged service {} failed: {}",
            argus_core::diagnostics::action(&action),
            argus_core::diagnostics::failure(e)
        )
    })?;
    state.sampler.send(Control::Refresh);
    Ok(status)
}

/// Switches the language of the native text (tray, errors); the UI follows
/// the returned `locale`.
#[tauri::command]
pub fn set_language(app: tauri::AppHandle, language: Language) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.language = language)
        .map_err(|e| e.to_string())?;
    argus_core::i18n::set_locale(language.resolve());
    shell::apply_language(&app);
    Ok(settings_view(&app))
}

#[tauri::command]
pub fn set_theme(app: tauri::AppHandle, theme: Theme) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.theme = theme)
        .map_err(|e| e.to_string())?;
    shell::apply_theme(&app, theme);
    Ok(settings_view(&app))
}

/// Where the page's maximize button is, for the snap layout overlay.
#[tauri::command]
pub fn set_caption_button(rect: ButtonRect) {
    caption::set_maximize_button(rect);
}

/// Records that the onboarding of `version` was completed or skipped.
#[tauri::command]
pub fn complete_onboarding(app: tauri::AppHandle, version: u32) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.onboarding = s.onboarding.max(version))
        .map_err(|e| e.to_string())?;
    Ok(settings_view(&app))
}

#[tauri::command]
pub fn set_close_to_tray(app: tauri::AppHandle, value: bool) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.close_to_tray = value)
        .map_err(|e| e.to_string())?;
    Ok(settings_view(&app))
}

/// Replaces the show/hide hotkey and turns it on. On failure the old one
/// stays.
#[tauri::command]
pub fn set_hotkey(app: tauri::AppHandle, hotkey: String) -> Result<SettingsView, HotkeyError> {
    crate::hotkey::set(&app, &hotkey)?;
    Ok(settings_view(&app))
}

/// Turns the hotkey on or off, keeping the combination.
#[tauri::command]
pub fn set_hotkey_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<SettingsView, HotkeyError> {
    crate::hotkey::set_enabled(&app, enabled)?;
    Ok(settings_view(&app))
}

/// Frees the hotkey while the settings page records a new one.
#[tauri::command]
pub fn suspend_hotkey(app: tauri::AppHandle) {
    crate::hotkey::suspend(&app);
}

/// Registers the saved hotkey again: after recording, or to retry one that
/// another program held.
#[tauri::command]
pub fn resume_hotkey(app: tauri::AppHandle) -> SettingsView {
    crate::hotkey::resume(&app);
    settings_view(&app)
}

#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<SettingsView, String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|e| e.to_string())?;
    Ok(settings_view(&app))
}

/// Replaces the process rules and applies them to running processes at once.
#[tauri::command]
pub fn set_rules(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    rules: Vec<ProcessRule>,
) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.rules = rules.clone())
        .map_err(|e| e.to_string())?;
    state.rules.set_rules(rules);
    state.sampler.send(Control::Refresh);
    Ok(settings_view(&app))
}

#[tauri::command]
pub fn rule_stats(state: State<'_, AppState>) -> HashMap<String, RuleStats> {
    state.rules.stats()
}

// ---- History --------------------------------------------------------------------

/// Unix ms of the first and last recorded frame.
#[tauri::command(async)]
pub fn history_range(dir: State<'_, HistoryDir>) -> Option<(i64, i64)> {
    history::range(&dir.0)
}

#[tauri::command(async)]
pub fn history_series(
    dir: State<'_, HistoryDir>,
    from: i64,
    to: i64,
    buckets: usize,
) -> Vec<HistoryPoint> {
    history::series(&dir.0, from, to, buckets.min(4096))
}

#[tauri::command(async)]
pub fn history_moment(dir: State<'_, HistoryDir>, ts: i64) -> Option<HistoryMoment> {
    history::moment(&dir.0, ts)
}

#[tauri::command(async)]
pub fn history_usage(dir: State<'_, HistoryDir>) -> HistoryUsage {
    history::usage(&dir.0)
}

/// Starts or stops recording; what is already recorded stays until it expires.
#[tauri::command]
pub fn set_history_enabled(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    dir: State<'_, HistoryDir>,
    enabled: bool,
) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.history = enabled)
        .map_err(|e| e.to_string())?;
    state
        .sampler
        .send(Control::SetHistoryDir(enabled.then(|| dir.0.clone())));
    Ok(settings_view(&app))
}

static SENSOR_CHANGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The explicit switch action may request UAC; startup never does. Persist
/// only after the driver is ready, so cancellation/failure leaves it off.
#[tauri::command(async)]
pub fn set_kernel_sensors(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<SettingsView, String> {
    use std::sync::atomic::Ordering;
    if SENSOR_CHANGE.swap(true, Ordering::AcqRel) {
        return Err(tr!("sensorSetup.busy"));
    }
    struct Change;
    impl Drop for Change {
        fn drop(&mut self) {
            SENSOR_CHANGE.store(false, Ordering::Release);
        }
    }
    let _change = Change;
    if enabled {
        argus_service::driver::ensure_ready().inspect_err(|error| {
            log::warn!(
                "kernel sensor preparation failed: {}",
                argus_core::diagnostics::failure(error)
            );
        })?;
    }
    app.state::<SettingsStore>()
        .update(|s| s.kernel_sensors = enabled)
        .map_err(|e| e.to_string())?;
    state.sampler.send(Control::SetKernelSensors(enabled));
    Ok(settings_view(&app))
}

/// Deletes all recorded history.
#[tauri::command(async)]
pub fn clear_history(state: State<'_, AppState>, dir: State<'_, HistoryDir>) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    state.sampler.send(Control::ClearHistory(dir.0.clone(), tx));
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(result) => result.map_err(|e| e.to_string()),
        Err(_) => Err(tr!("host.samplerStalled")),
    }
}

#[tauri::command]
pub fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

// ---- Updates --------------------------------------------------------------------

#[tauri::command]
pub fn update_status(app: tauri::AppHandle) -> UpdateStatus {
    updater::status(&app)
}

/// Starts a check; progress arrives as `update-status` events.
#[tauri::command]
pub fn check_update(app: tauri::AppHandle) -> UpdateStatus {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move { updater::check(&handle).await });
    updater::status(&app)
}

/// Runs the downloaded installer; Argus exits and the installer relaunches it.
#[tauri::command(async)]
pub fn install_update(app: tauri::AppHandle) -> Result<(), updater::InstallError> {
    updater::install(&app)
}

#[tauri::command]
pub fn set_update_settings(
    app: tauri::AppHandle,
    check: bool,
    channel: UpdateChannel,
    route: UpdateRoute,
    proxy: String,
) -> Result<SettingsView, String> {
    let proxy = updater::normalize_proxy(&proxy)?;
    let store = app.state::<SettingsStore>();
    let previous = store.get().update_channel;
    store
        .update(|s| {
            s.update_check = check;
            s.update_channel = channel;
            s.update_route = route;
            s.update_proxy = proxy;
        })
        .map_err(|e| e.to_string())?;
    // A downloaded update belongs to the old channel; look at the new one now.
    if previous != channel {
        updater::reset(&app);
        let handle = app.clone();
        tauri::async_runtime::spawn(async move { updater::check(&handle).await });
    }
    Ok(settings_view(&app))
}

#[tauri::command]
pub fn open_release_page(version: String) -> Result<(), String> {
    updater::open_release_page(&version)
}

/// The invitation to the QQ group for feedback and discussion.
const COMMUNITY_URL: &str = "https://qm.qq.com/q/97YqJAQRNK";

#[tauri::command]
pub fn open_sensor_driver_page() -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg("https://pawnio.eu/")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Opens the group invitation in the default browser, which hands it to QQ.
#[tauri::command]
pub fn open_community() -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(COMMUNITY_URL)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Opens the folder with `argus.log`, for attaching to bug reports.
#[tauri::command]
pub fn open_log_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| e.to_string())?
        .join("diagnostics");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer.exe")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---- Services -----------------------------------------------------------------

#[tauri::command(async)]
pub fn list_services(state: State<'_, AppState>) -> Result<Vec<ServiceInfo>, ActionError> {
    state.services.list()
}

#[tauri::command(async)]
pub fn control_service(
    state: State<'_, AppState>,
    name: String,
    action: ServiceAction,
) -> Result<(), ActionError> {
    let result = match privileged_done(
        &state,
        Operation::ControlService {
            name: name.clone(),
            action,
        },
    ) {
        Ok(true) => {
            state.services.invalidate(&name);
            Ok(())
        }
        Ok(false) => state.services.control(&name, action),
        Err(e) => Err(e),
    };
    state.sampler.send(Control::Refresh);
    result
}

#[tauri::command(async)]
pub fn set_service_start_type(
    state: State<'_, AppState>,
    name: String,
    start_type: StartType,
) -> Result<(), ActionError> {
    if privileged_done(
        &state,
        Operation::ServiceStartType {
            name: name.clone(),
            start_type,
        },
    )? {
        state.services.invalidate(&name);
        return Ok(());
    }
    state.services.set_start_type(&name, start_type)
}

/// Opens the Windows Services console, for what Argus does not cover.
#[tauri::command]
pub fn open_services_console() -> Result<(), String> {
    std::process::Command::new("mmc.exe")
        .arg("services.msc")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---- Startup ------------------------------------------------------------------

#[tauri::command(async)]
pub fn list_startup(state: State<'_, AppState>) -> Vec<StartupItem> {
    startup::list(&state.meta)
}

#[tauri::command(async)]
pub fn set_startup_enabled(id: String, enabled: bool) -> Result<(), ActionError> {
    startup::set_enabled(&id, enabled)
}

// ---- Connections ----------------------------------------------------------------

#[tauri::command(async)]
pub fn list_connections() -> Vec<Connection> {
    connections::list()
}

#[tauri::command(async)]
pub fn close_connection(
    state: State<'_, AppState>,
    local_address: String,
    local_port: u16,
    remote_address: String,
    remote_port: u16,
) -> Result<(), ActionError> {
    if privileged_done(
        &state,
        Operation::CloseConnection {
            local_address: local_address.clone(),
            local_port,
            remote_address: remote_address.clone(),
            remote_port,
        },
    )? {
        return Ok(());
    }
    connections::close_tcp(&local_address, local_port, &remote_address, remote_port)
}

// ---- File locks -----------------------------------------------------------------

#[tauri::command(async)]
pub fn find_lockers(paths: Vec<String>) -> Result<LockReport, ActionError> {
    let base = filelock::find_lockers(&paths);
    LockReport::combine(base, argus_service::find_handle_lockers(paths))
}

/// Native file or folder picker.
#[tauri::command(async)]
pub fn pick_path(app: tauri::AppHandle, folder: bool) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file();
    let picked = if folder {
        dialog
            .set_title(tr!("host.pickFolder"))
            .blocking_pick_folder()
    } else {
        dialog.set_title(tr!("host.pickFile")).blocking_pick_file()
    };
    picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}
