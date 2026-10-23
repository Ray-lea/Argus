use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::time::Duration;

use crate::native_runtime::State;

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
use argus_service::{Operation, Reply, Routed, ServiceStatus};
use serde::Serialize;

use crate::hotkey::HotkeyError;
use crate::sampler::Control;
use crate::settings::{
    BACKGROUND_INTERVALS, Language, SettingsStore, SettingsView, Theme, UpdateChannel, UpdateRoute,
};
use crate::updater::{self, UpdateStatus};
use crate::{AppState, HistoryDir, shell};

pub(crate) mod autostart;

pub fn minimize_main(window: crate::native_runtime::NativeWindow) {
    let _ = window.minimize();
}

pub fn desktop_shell() -> Option<crate::explorer::Identity> {
    crate::explorer::current()
}

pub fn restart_explorer(pid: u32, create_time: i64) -> Result<(), crate::explorer::RestartError> {
    crate::explorer::restart(pid, create_time)
}

pub fn run_task(
    app: crate::native_runtime::Host,
    command: String,
    elevated: bool,
) -> Result<(), crate::tasks::TaskError> {
    let owner = app
        .get_window("main")
        .and_then(|w| w.hwnd().ok())
        .map_or(0, |h| h.0 as usize);
    crate::tasks::launch(owner, command, elevated)
}

pub fn task_manager_status() -> Result<argus_service::taskmgr::Status, String> {
    argus_service::taskmgr::status()
}

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

pub fn task_manager_ready(app: crate::native_runtime::Host) {
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
    /// Whether the native window supplies a Mica backdrop.
    mica: bool,
    /// Relaunched by the installer of an update.
    updated: bool,
    /// Started by the task manager replacement: the system may be struggling,
    /// so the UI holds back what is not urgent.
    task_manager: bool,
    /// The UI language, resolved; the UI renders nothing before it knows.
    locale: argus_core::i18n::Locale,
}

pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        elevated: state.elevated,
        accent: theme::accent_colors(),
        mica: shell::supports_mica(),
        updated: std::env::args().any(|a| a == updater::UPDATED_ARG),
        task_manager: state.task_manager_start,
        locale: argus_core::i18n::locale(),
    }
}

pub fn set_interval(state: State<'_, AppState>, ms: u64) {
    let ms = ms.clamp(250, 10_000);
    state
        .sampler
        .send(Control::SetInterval(Duration::from_millis(ms)));
}

pub fn set_paused(state: State<'_, AppState>, paused: bool) {
    state.sampler.send(Control::SetPaused(paused));
}

pub fn refresh(state: State<'_, AppState>) {
    state.sampler.send(Control::Refresh);
}

pub fn process_details(pid: u32, path: Option<String>) -> ProcessDetails {
    details::query(pid, path.as_deref())
}

/// Runs `operation` in the privileged service when one runs for this account,
/// else `local` with Argus's own rights. `replied` takes the service's answer,
/// None for one this host does not expect.
fn privileged<T>(
    state: &AppState,
    operation: Operation,
    replied: impl FnOnce(Reply) -> Option<T>,
    local: impl FnOnce() -> Result<T, ActionError>,
) -> Result<T, ActionError> {
    let operation = match &operation {
        Operation::Terminate { .. } => ("terminate process", operation),
        Operation::Priority { .. } => ("set process priority", operation),
        Operation::Efficiency { .. } => ("set process efficiency", operation),
        Operation::ControlService { .. } => ("control Windows service", operation),
        Operation::ServiceStartType { .. } => ("set service start type", operation),
        Operation::CloseConnection { .. } => ("close TCP connection", operation),
        _ => ("privileged operation", operation),
    };
    if state.elevated {
        return action_result(operation.0, "local elevated", local());
    }
    let routed = argus_service::route(operation.1).map_err(ActionError::Os);
    let routed = action_result(operation.0, "service routing", routed)?;
    let source = match &routed {
        Routed::Absent => "local fallback (no service)",
        Routed::Outdated => "local fallback (incompatible service)",
        Routed::Replied(_) => "privileged service",
    };
    action_result(operation.0, source, settle(routed, replied, local))
}

fn action_result<T>(
    operation: &'static str,
    source: &'static str,
    result: Result<T, ActionError>,
) -> Result<T, ActionError> {
    if let Err(error) = &result {
        argus_core::diagnostics::warn(
            operation,
            format!("source={source}; {}", error.diagnostic()),
        );
    }
    result
}

/// What an operation comes to once the service has `routed` it.
fn settle<T>(
    routed: Routed,
    replied: impl FnOnce(Reply) -> Option<T>,
    local: impl FnOnce() -> Result<T, ActionError>,
) -> Result<T, ActionError> {
    match routed {
        Routed::Absent => local(),
        Routed::Outdated => {
            if !OUTDATED_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log::warn!("privileged service: incompatible protocol; acting with own rights");
            }
            outdated(local())
        }
        Routed::Replied(reply) => match *reply {
            Reply::Error(e) => Err(e),
            reply => replied(reply).ok_or_else(|| ActionError::Os(tr!("host.serviceReply"))),
        },
    }
}

static OUTDATED_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The outdated service refused the operation without running it, so it ran
/// here: where Argus's own rights fall short, updating the service helps, and
/// the UI offers it.
fn outdated<T>(result: Result<T, ActionError>) -> Result<T, ActionError> {
    result.map_err(|e| match e {
        ActionError::AccessDenied => ActionError::ServiceOutdated,
        e => e,
    })
}

fn done(reply: Reply) -> Option<()> {
    matches!(reply, Reply::Done).then_some(())
}

fn terminate(state: &AppState, pid: u32, create_time: i64, force: bool) -> Result<(), ActionError> {
    if pid == std::process::id() {
        return Err(ActionError::Refused);
    }
    privileged(
        state,
        Operation::Terminate { pid, create_time },
        done,
        || actions::terminate(pid, create_time, force),
    )
}

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

pub fn set_priority(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
    priority: Priority,
) -> Result<Priority, ActionError> {
    let result = privileged(
        &state,
        Operation::Priority {
            pid,
            create_time,
            priority,
        },
        |reply| match reply {
            Reply::Priority(p) => Some(p),
            _ => None,
        },
        || actions::set_priority(pid, create_time, priority),
    );
    state.sampler.send(Control::Refresh);
    result
}

pub fn set_efficiency_mode(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
    on: bool,
) -> Result<(), ActionError> {
    let result = privileged(
        &state,
        Operation::Efficiency {
            pid,
            create_time,
            on,
        },
        done,
        || actions::set_efficiency_mode(pid, create_time, on),
    );
    state.sampler.send(Control::Refresh);
    result
}

/// Opens Explorer with the file selected.
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

fn settings_view(app: &crate::native_runtime::Host) -> SettingsView {
    let store = app.state::<SettingsStore>();
    SettingsView {
        settings: store.get(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        locale: argus_core::i18n::locale(),
        hotkey_error: crate::hotkey::error(app),
        default_hotkey: crate::settings::DEFAULT_HOTKEY,
    }
}

pub fn get_settings(app: crate::native_runtime::Host) -> SettingsView {
    autostart::repair_once(&app.package_info().name);
    settings_view(&app)
}

pub fn privileged_service_status() -> Result<ServiceStatus, String> {
    argus_service::status()
}

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
pub fn set_language(
    app: crate::native_runtime::Host,
    language: Language,
) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.language = language)
        .map_err(|e| e.to_string())?;
    argus_core::i18n::set_locale(language.resolve());
    shell::apply_language(&app);
    Ok(settings_view(&app))
}

pub fn set_theme(app: crate::native_runtime::Host, theme: Theme) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.theme = theme)
        .map_err(|e| e.to_string())?;
    shell::apply_theme(&app, theme);
    Ok(settings_view(&app))
}

/// Records that the onboarding of `version` was completed or skipped.
pub fn complete_onboarding(
    app: crate::native_runtime::Host,
    version: u32,
) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.onboarding = s.onboarding.max(version))
        .map_err(|e| e.to_string())?;
    Ok(settings_view(&app))
}

/// Whether the first start of a new version shows what it changed.
pub fn set_whats_new(
    app: crate::native_runtime::Host,
    enabled: bool,
) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.whats_new = enabled)
        .map_err(|e| e.to_string())?;
    Ok(settings_view(&app))
}

/// Records that the first start of this version has been dealt with, so what
/// it changed does not show again.
pub fn note_version_seen(app: crate::native_runtime::Host) -> Result<SettingsView, String> {
    let store = app.state::<SettingsStore>();
    let version = env!("CARGO_PKG_VERSION");
    if store.get().seen_version != version {
        store
            .update(|s| s.seen_version = version.into())
            .map_err(|e| e.to_string())?;
    }
    Ok(settings_view(&app))
}

pub fn set_close_to_tray(
    app: crate::native_runtime::Host,
    value: bool,
) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.close_to_tray = value)
        .map_err(|e| e.to_string())?;
    Ok(settings_view(&app))
}

/// How often to sample while minimized and while in the tray, ms; takes
/// effect at once.
pub fn set_background_intervals(
    app: crate::native_runtime::Host,
    state: State<'_, AppState>,
    minimized: u64,
    tray: u64,
) -> Result<SettingsView, String> {
    if ![minimized, tray]
        .iter()
        .all(|ms| BACKGROUND_INTERVALS.contains(ms))
    {
        return Err(tr!("host.invalidInterval"));
    }
    let saved = app
        .state::<SettingsStore>()
        .update(|s| {
            s.minimized_interval = minimized;
            s.tray_interval = tray;
        })
        .map_err(|e| e.to_string())?;
    state.sampler.send(saved.background_intervals());
    Ok(settings_view(&app))
}

/// Replaces the show/hide hotkey and turns it on. On failure the old one
/// stays.
pub fn set_hotkey(
    app: crate::native_runtime::Host,
    hotkey: String,
) -> Result<SettingsView, HotkeyError> {
    crate::hotkey::set(&app, &hotkey)?;
    Ok(settings_view(&app))
}

/// Turns the hotkey on or off, keeping the combination.
pub fn set_hotkey_enabled(
    app: crate::native_runtime::Host,
    enabled: bool,
) -> Result<SettingsView, HotkeyError> {
    crate::hotkey::set_enabled(&app, enabled)?;
    Ok(settings_view(&app))
}

/// Frees the hotkey while the settings page records a new one.
pub fn suspend_hotkey(app: crate::native_runtime::Host) {
    crate::hotkey::suspend(&app);
}

/// Registers the saved hotkey again: after recording, or to retry one that
/// another program held.
pub fn resume_hotkey(app: crate::native_runtime::Host) -> SettingsView {
    crate::hotkey::resume(&app);
    settings_view(&app)
}

pub fn set_autostart(
    app: crate::native_runtime::Host,
    enabled: bool,
) -> Result<SettingsView, String> {
    let launcher = app.autolaunch();
    if !enabled {
        launcher.disable().map_err(|e| e.to_string())?;
    } else if autostart::running_as_copy() {
        // The plugin would name this copy of the task manager replacement.
        autostart::enable_installed(&app.package_info().name)?;
    } else {
        launcher.enable().map_err(|e| e.to_string())?;
    }
    Ok(settings_view(&app))
}

/// Replaces the process rules and applies them to running processes at once.
pub fn set_rules(
    app: crate::native_runtime::Host,
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

pub fn rule_stats(state: State<'_, AppState>) -> HashMap<String, RuleStats> {
    state.rules.stats()
}

// ---- History --------------------------------------------------------------------

/// Unix ms of the first and last recorded frame.
pub fn history_range(dir: State<'_, HistoryDir>) -> Option<(i64, i64)> {
    history::range(&dir.0)
}

pub fn history_series(
    dir: State<'_, HistoryDir>,
    from: i64,
    to: i64,
    buckets: usize,
) -> Vec<HistoryPoint> {
    history::series(&dir.0, from, to, buckets.min(4096))
}

pub fn history_moment(dir: State<'_, HistoryDir>, ts: i64) -> Option<HistoryMoment> {
    history::moment(&dir.0, ts)
}

pub fn history_usage(dir: State<'_, HistoryDir>) -> HistoryUsage {
    history::usage(&dir.0)
}

/// Starts or stops recording; what is already recorded stays until it expires.
pub fn set_history_enabled(
    app: crate::native_runtime::Host,
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
pub fn set_kernel_sensors(
    app: crate::native_runtime::Host,
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
            argus_core::diagnostics::warn(
                "kernel sensor preparation failed",
                format!(
                    "{}; PawnIO state after failure: {}",
                    argus_core::diagnostics::failure(error),
                    argus_service::driver::diagnostic_status()
                ),
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
pub fn clear_history(state: State<'_, AppState>, dir: State<'_, HistoryDir>) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    state.sampler.send(Control::ClearHistory(dir.0.clone(), tx));
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(result) => result.map_err(|e| {
            argus_core::diagnostics::file_failure("history clear", &dir.0, &e);
            e.to_string()
        }),
        Err(error) => {
            argus_core::diagnostics::warn("history clear", format!("sampler_reply={error:?}"));
            Err(tr!("host.samplerStalled"))
        }
    }
}

pub fn quit(app: crate::native_runtime::Host) {
    app.exit(0);
}

// ---- Updates --------------------------------------------------------------------

pub fn update_status(app: crate::native_runtime::Host) -> UpdateStatus {
    updater::status(&app)
}

/// Starts a check; progress arrives as `update-status` events.
pub fn check_update(app: crate::native_runtime::Host) -> UpdateStatus {
    let handle = app.clone();
    crate::async_runtime::spawn(async move { updater::check(&handle).await });
    updater::status(&app)
}

/// Runs the downloaded installer; Argus exits and the installer relaunches it.
pub fn install_update(app: crate::native_runtime::Host) -> Result<(), updater::InstallError> {
    updater::install(&app)
}

pub fn set_update_settings(
    app: crate::native_runtime::Host,
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
        crate::async_runtime::spawn(async move { updater::check(&handle).await });
    }
    Ok(settings_view(&app))
}

pub fn open_release_page(version: String) -> Result<(), String> {
    updater::open_release_page(&version)
}

/// The invitation to the QQ group for feedback and discussion.
const COMMUNITY_URL: &str = "https://qm.qq.com/q/97YqJAQRNK";

pub fn open_sensor_driver_page() -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg("https://pawnio.eu/")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Opens the project's page on GitHub in the default browser.
pub fn open_homepage() -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(crate::updater::REPO)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Opens the group invitation in the default browser, which hands it to QQ.
pub fn open_community() -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(COMMUNITY_URL)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Opens the folder with `argus.log`, for attaching to bug reports.
pub fn open_log_dir(app: crate::native_runtime::Host) -> Result<(), String> {
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

pub fn list_services(state: State<'_, AppState>) -> Result<Vec<ServiceInfo>, ActionError> {
    action_result("list Windows services", "local", state.services.list())
}

pub fn control_service(
    state: State<'_, AppState>,
    name: String,
    action: ServiceAction,
) -> Result<(), ActionError> {
    let result = privileged(
        &state,
        Operation::ControlService {
            name: name.clone(),
            action,
        },
        |reply| {
            done(reply)?;
            state.services.invalidate(&name);
            Some(())
        },
        || state.services.control(&name, action),
    );
    state.sampler.send(Control::Refresh);
    result
}

pub fn set_service_start_type(
    state: State<'_, AppState>,
    name: String,
    start_type: StartType,
) -> Result<(), ActionError> {
    privileged(
        &state,
        Operation::ServiceStartType {
            name: name.clone(),
            start_type,
        },
        |reply| {
            done(reply)?;
            state.services.invalidate(&name);
            Some(())
        },
        || state.services.set_start_type(&name, start_type),
    )
}

/// Opens the Windows Services console, for what Argus does not cover.
pub fn open_services_console() -> Result<(), String> {
    std::process::Command::new("mmc.exe")
        .arg("services.msc")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---- Startup ------------------------------------------------------------------

pub fn list_startup(state: State<'_, AppState>) -> Vec<StartupItem> {
    startup::list(&state.meta)
}

pub fn set_startup_enabled(id: String, enabled: bool) -> Result<(), ActionError> {
    action_result(
        if enabled {
            "enable startup item"
        } else {
            "disable startup item"
        },
        "local",
        startup::set_enabled(&id, enabled),
    )
}

// ---- Connections ----------------------------------------------------------------

pub fn list_connections() -> Vec<Connection> {
    connections::list()
}

pub fn close_connection(
    state: State<'_, AppState>,
    local_address: String,
    local_port: u16,
    remote_address: String,
    remote_port: u16,
) -> Result<(), ActionError> {
    privileged(
        &state,
        Operation::CloseConnection {
            local_address: local_address.clone(),
            local_port,
            remote_address: remote_address.clone(),
            remote_port,
        },
        done,
        || connections::close_tcp(&local_address, local_port, &remote_address, remote_port),
    )
}

// ---- File locks -----------------------------------------------------------------

pub fn find_lockers(paths: Vec<String>) -> Result<LockReport, ActionError> {
    let base = filelock::find_lockers(&paths);
    action_result(
        "find file locks",
        "Restart Manager and handle scan",
        LockReport::combine(base, argus_service::find_handle_lockers(paths)),
    )
}

/// Native file or folder picker.
pub fn pick_path(
    _app: crate::native_runtime::Host,
    folder: bool,
    task: Option<bool>,
) -> Option<String> {
    let dialog = rfd::FileDialog::new().set_title(if folder {
        tr!("host.pickFolder")
    } else if task.unwrap_or(false) {
        tr!("host.pickTask")
    } else {
        tr!("host.pickFile")
    });
    let picked = if folder {
        dialog.pick_folder()
    } else {
        dialog.pick_file()
    };
    picked.map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied() -> Result<(), ActionError> {
        Err(ActionError::AccessDenied)
    }

    #[test]
    fn a_denial_while_the_service_is_outdated_asks_for_its_update() {
        assert!(matches!(
            settle(Routed::Outdated, done, denied),
            Err(ActionError::ServiceOutdated)
        ));
        // Other failures and success are what they are.
        assert!(matches!(
            settle(Routed::Outdated, done, || Err(ActionError::NotFound)),
            Err(ActionError::NotFound)
        ));
        assert!(matches!(settle(Routed::Outdated, done, || Ok(())), Ok(())));
    }

    #[test]
    fn other_denials_stay_plain() {
        // Without a service, updating one would not help.
        assert!(matches!(
            settle(Routed::Absent, done, denied),
            Err(ActionError::AccessDenied)
        ));
        // The service itself was denied (a protected process, say).
        let replied = Routed::Replied(Box::new(Reply::Error(ActionError::AccessDenied)));
        assert!(matches!(
            settle(replied, done, || unreachable!("acted locally")),
            Err(ActionError::AccessDenied)
        ));
    }
}
