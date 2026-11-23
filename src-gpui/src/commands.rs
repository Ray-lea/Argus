use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::time::Duration;

use crate::native_runtime::State;

use argus_core::actions::{
    self, ActionError, IoPriority, MemoryPriority, Priority, TreeKillReport,
};
use argus_core::connections::{self, Connection};
use argus_core::details::{self, ProcessDetails};
use argus_core::filelock::{self, LockReport};
use argus_core::history::{self, HistoryMoment, HistoryPoint, HistoryUsage};
use argus_core::rules::{ProcessRule, RuleStats};
use argus_core::services::{ServiceAction, ServiceInfo, StartType};
use argus_core::startup::{self, StartupItem};
use argus_core::tr;
use argus_service::{Operation, Reply, Routed, ServiceStatus};
use serde::Serialize;

use crate::hotkey::HotkeyError;
use crate::sampler::Control;
use crate::settings::{
    BACKGROUND_INTERVALS, DeskGrouping, DeskLayout, DeskStyle, Language, MONITOR_OPACITIES, Metric,
    Reading, SettingsStore, SettingsView, TaskbarPlace, TaskbarStyle, Theme, TrayIcon, TrayMenu,
    UpdateChannel, UpdateRoute,
};
use crate::updater::{self, UpdateStatus};
use crate::{AppState, HistoryDir, shell};

pub(crate) mod autostart;

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

/// Opens Control Panel's User Accounts, as Task Manager's "Manage user
/// accounts" does. Started with the desktop's rights also when Argus is
/// elevated: Control Panel asks for elevation where a change needs it.
pub fn open_user_accounts(app: crate::native_runtime::Host) -> Result<(), crate::tasks::TaskError> {
    let owner = app
        .get_window("main")
        .and_then(|w| w.hwnd().ok())
        .map_or(0, |h| h.0 as usize);
    crate::tasks::launch(
        owner,
        "control.exe /name Microsoft.UserAccounts".into(),
        false,
    )
}

/// Signs a user out of the sessions the process list showed for them. Acts
/// with Argus's own rights: another user's session needs an elevated Argus.
pub fn sign_out_user(
    state: State<'_, AppState>,
    user: String,
    domain: String,
    sessions: Vec<u32>,
) -> Result<(), ActionError> {
    let result = action_result(
        "sign out user",
        "local",
        argus_core::sessions::sign_out(&user, &domain, &sessions),
    );
    state.sampler.send(Control::Refresh);
    result
}

pub fn task_manager_status() -> Result<argus_service::taskmgr::Status, String> {
    argus_service::taskmgr::status()
}

pub fn open_native_task_manager() -> Result<(), String> {
    argus_service::taskmgr::open_native()
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
    /// The release version, for comparisons.
    version: &'static str,
    /// The version shown: a build that is not of a release adds its commit
    /// (`build-support/build-version.rs`).
    build: &'static str,
    build_kind: argus_app::config::BuildKind,
    windows: Option<&'static str>,
    elevated: bool,
    /// Relaunched by the installer of an update.
    updated: bool,
    /// Started by the task manager replacement: the system may be struggling,
    /// so the UI holds back what is not urgent.
    task_manager: bool,
    /// The standalone exe of a release or test build, which the interface
    /// tells what it lacks beside the installed version. Not a development
    /// build: it is no download either.
    portable: bool,
}

pub fn app_info(app: crate::native_runtime::Host) -> AppInfo {
    let state = app.state::<AppState>();
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        build: env!("ARGUS_VERSION"),
        build_kind: argus_app::config::build_kind(env!("ARGUS_VERSION"), cfg!(debug_assertions)),
        windows: crate::diagnostics::windows_version(),
        elevated: state.elevated,
        updated: std::env::args().any(|a| a == updater::UPDATED_ARG),
        task_manager: state.task_manager_start,
        portable: !cfg!(debug_assertions) && updater::status(&app).portable,
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

pub fn process_details(pid: u32, path: Option<String>) -> ProcessDetails {
    details::query(pid, path.as_deref())
}

/// Reads a disk's details, on the caller's worker thread. The read reaches the
/// disk's storage driver: it is made with the marker on disk (`disk_reads`),
/// so that a read which brings the system down is not made again by itself at
/// the next start.
pub fn disk_details(index: u32) -> argus_core::hardware::disk_details::DiskDetails {
    crate::disk_reads::read(index, || argus_service::disk_details(index))
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
        Operation::IoPriority { .. } => ("set process I/O priority", operation),
        Operation::MemoryPriority { .. } => ("set process memory priority", operation),
        Operation::Suspend { .. } => ("suspend or resume process", operation),
        Operation::ControlService { .. } => ("control Windows service", operation),
        Operation::ServiceStartType { .. } => ("set service start type", operation),
        Operation::DeleteService { .. } => ("delete Windows service", operation),
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
    crate::explorer::check_termination(pid)?;
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
    crate::explorer::check_termination(pid)?;
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

pub fn set_io_priority(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
    priority: IoPriority,
) -> Result<(), ActionError> {
    let result = privileged(
        &state,
        Operation::IoPriority {
            pid,
            create_time,
            priority,
        },
        done,
        || actions::set_io_priority(pid, create_time, priority).map(|_| ()),
    );
    state.sampler.send(Control::Refresh);
    result
}

pub fn set_memory_priority(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
    priority: MemoryPriority,
) -> Result<(), ActionError> {
    let result = privileged(
        &state,
        Operation::MemoryPriority {
            pid,
            create_time,
            priority,
        },
        done,
        || actions::set_memory_priority(pid, create_time, priority).map(|_| ()),
    );
    state.sampler.send(Control::Refresh);
    result
}

pub fn set_suspended(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
    suspend: bool,
) -> Result<(), ActionError> {
    let result = privileged(
        &state,
        Operation::Suspend {
            pid,
            create_time,
            suspend,
        },
        done,
        || actions::set_suspended(pid, create_time, suspend),
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
        hotkey_error: crate::hotkey::error(app),
        default_hotkey: crate::settings::DEFAULT_HOTKEY,
    }
}

pub fn get_settings(app: crate::native_runtime::Host) -> SettingsView {
    autostart::repair_once(&app.package_info().name);
    crate::admin::follow_layout_once(&app.config().identifier);
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

/// Switches the language of all text; the tray follows at once and the UI
/// redraws on `argus-language`.
pub fn set_language(
    app: crate::native_runtime::Host,
    language: Language,
) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.language = language)
        .map_err(|e| e.to_string())?;
    crate::catalog::set_locale(language.resolve());
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

/// The administrator start's state, for settings and onboarding.
pub fn administrator_status(app: crate::native_runtime::Host) -> argus_service::admin::Status {
    crate::admin::status(&app)
}

/// Turns the administrator start on or off (`admin::set_mode`).
pub fn set_administrator_mode(
    app: crate::native_runtime::Host,
    enabled: bool,
    defer: bool,
    repair: Option<bool>,
) -> Result<argus_service::admin::Status, String> {
    crate::admin::set_mode(&app, enabled, defer, repair)
}

/// Applies a deferred administrator start change.
pub fn apply_administrator_mode(app: crate::native_runtime::Host) -> Result<(), String> {
    crate::admin::apply_mode(&app)
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

/// Draws the interface with the graphics card or, turned off, with the
/// system's software rasterizer. The open windows change renderer in place.
pub fn set_hardware_acceleration(
    app: crate::native_runtime::Host,
    enabled: bool,
) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.hardware_acceleration = enabled)
        .map_err(|e| e.to_string())?;
    log::info!("hardware acceleration set; enabled={enabled}");
    gpui_kit::gpui::set_windows_software_rendering(!enabled);
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

/// The tray icon's look, its hover card and what a right click opens; each
/// one given changes, and the icon follows at once.
pub fn set_tray(
    app: crate::native_runtime::Host,
    icon: Option<TrayIcon>,
    card: Option<bool>,
    menu: Option<TrayMenu>,
) -> Result<SettingsView, String> {
    let saved = app
        .state::<SettingsStore>()
        .update(|s| {
            if let Some(icon) = icon {
                s.tray_icon = icon;
            }
            if let Some(card) = card {
                s.tray_card = card;
            }
            if let Some(menu) = menu {
                s.tray_menu = menu;
            }
        })
        .map_err(|e| e.to_string())?;
    shell::apply_tray(&saved);
    Ok(settings_view(&app))
}

/// A monitor's readings or metrics as asked for: each once, in the order
/// given.
fn distinct<T: PartialEq>(items: Vec<T>) -> Vec<T> {
    let mut kept = Vec::with_capacity(items.len());
    for item in items {
        if !kept.contains(&item) {
            kept.push(item);
        }
    }
    kept
}

/// The desktop monitor's settings; each one given changes. The interface
/// shows, hides or rearranges the monitor's window from the settings
/// returned.
#[allow(clippy::too_many_arguments)]
pub fn set_desktop_monitor(
    app: crate::native_runtime::Host,
    enabled: Option<bool>,
    metrics: Option<Vec<Reading>>,
    order: Option<Vec<Metric>>,
    layout: Option<DeskLayout>,
    style: Option<DeskStyle>,
    grouping: Option<DeskGrouping>,
    topmost: Option<bool>,
    locked: Option<bool>,
    click_through: Option<bool>,
    opacity: Option<u8>,
    position: Option<[i32; 2]>,
) -> Result<SettingsView, String> {
    if opacity.is_some_and(|value| !MONITOR_OPACITIES.contains(&value)) {
        return Err(tr!("app.invalidRequest"));
    }
    app.state::<SettingsStore>()
        .update(|s| {
            let monitor = &mut s.desktop_monitor;
            if let Some(enabled) = enabled {
                monitor.enabled = enabled;
            }
            if let Some(metrics) = metrics {
                monitor.metrics = distinct(metrics);
            }
            if let Some(order) = order {
                monitor.order = distinct(order);
            }
            if let Some(layout) = layout {
                monitor.layout = layout;
            }
            if let Some(style) = style {
                monitor.style = style;
            }
            if let Some(grouping) = grouping {
                monitor.grouping = grouping;
            }
            if let Some(topmost) = topmost {
                monitor.topmost = topmost;
            }
            if let Some(locked) = locked {
                monitor.locked = locked;
            }
            if let Some(click_through) = click_through {
                monitor.click_through = click_through;
            }
            if let Some(opacity) = opacity {
                monitor.opacity = opacity;
            }
            if let Some(position) = position {
                monitor.position = Some(position);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(settings_view(&app))
}

/// The taskbar monitor's settings; each one given changes, and the monitor
/// follows at once.
pub fn set_taskbar_monitor(
    app: crate::native_runtime::Host,
    enabled: Option<bool>,
    metrics: Option<Vec<Reading>>,
    order: Option<Vec<Metric>>,
    place: Option<TaskbarPlace>,
    style: Option<TaskbarStyle>,
    display: Option<String>,
) -> Result<SettingsView, String> {
    let saved = app
        .state::<SettingsStore>()
        .update(|s| {
            let monitor = &mut s.taskbar_monitor;
            if let Some(enabled) = enabled {
                monitor.enabled = enabled;
            }
            if let Some(metrics) = metrics {
                monitor.metrics = distinct(metrics);
            }
            if let Some(order) = order {
                monitor.order = distinct(order);
            }
            if let Some(place) = place {
                monitor.place = place;
            }
            if let Some(style) = style {
                monitor.style = style;
            }
            if let Some(display) = display {
                monitor.display = display;
            }
        })
        .map_err(|e| e.to_string())?;
    shell::apply_taskbar_monitor(&app, &saved.taskbar_monitor);
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
    } else if let Some(path) = argus_service::admin::launcher(&app.config().identifier) {
        autostart::enable_admin(&app, &path)?;
    } else if autostart::running_as_copy() {
        // Enabling as usual would name this copy of the task manager replacement.
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
    history::range(&dir.read())
}

pub fn history_series(
    dir: State<'_, HistoryDir>,
    from: i64,
    to: i64,
    buckets: usize,
) -> Vec<HistoryPoint> {
    history::legacy_series(&dir.read(), from, to, buckets.min(4096))
}

pub fn history_points(
    dir: State<'_, HistoryDir>,
    from: i64,
    to: i64,
    bucket: i64,
    origin: i64,
    keys: Vec<String>,
) -> history::HistorySeries {
    history::series(&dir.read(), from, to, bucket, origin, &keys)
}
pub fn history_events(
    dir: State<'_, HistoryDir>,
    from: i64,
    to: i64,
) -> Result<history::HistoryEvents, String> {
    history::events(&dir.read(), from, to).ok_or_else(|| tr!("app.invalidRequest"))
}
pub fn history_channels(
    dir: State<'_, HistoryDir>,
    from: i64,
    to: i64,
) -> Vec<history::HistoryChannel> {
    history::channels(&dir.read(), from, to)
}
pub fn history_frame(dir: State<'_, HistoryDir>, ts: i64) -> Option<history::HistoryFrame> {
    history::frame(&dir.read(), ts)
}
pub fn history_table(
    dir: State<'_, HistoryDir>,
    from: i64,
    to: i64,
    trend: String,
    points: usize,
) -> Result<history::HistoryTable, String> {
    if points > 4096 || !matches!(trend.as_str(), "cpu" | "mem" | "disk" | "net" | "gpu") {
        return Err(tr!("app.invalidRequest"));
    }
    Ok(history::table(&dir.read(), from, to, &trend, points))
}
pub fn history_moment(dir: State<'_, HistoryDir>, ts: i64) -> Option<HistoryMoment> {
    history::moment(&dir.read(), ts)
}

/// What the history keeps, and where: the settings show the folder and
/// whether it is the default one.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryUsageView {
    #[serde(flatten)]
    usage: HistoryUsage,
    dir: String,
    default_dir: String,
    custom: bool,
}

pub fn history_usage(
    dir: State<'_, HistoryDir>,
    settings: State<'_, SettingsStore>,
) -> HistoryUsageView {
    let path = dir.read().clone();
    HistoryUsageView {
        usage: history::usage_with(&path, settings.get().history_options()),
        dir: path.display().to_string(),
        default_dir: dir.default.display().to_string(),
        custom: path != dir.default,
    }
}

/// Explicit observation runs on the sampler, where its file and instance handle live.
pub fn watch_process(
    state: State<'_, AppState>,
    dir: State<'_, HistoryDir>,
    action: argus_core::watch::WatchAction,
) -> Result<Option<argus_core::watch::WatchView>, String> {
    use argus_core::watch::WatchError;
    let path = dir.read();
    let (tx, rx) = std::sync::mpsc::channel();
    state.sampler.send(Control::Watch(path.clone(), action, tx));
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| tr!("history.watch.error.unavailable"))?;
    result.map_err(|error| {
        argus_core::i18n::text(match error {
            WatchError::Identity => "history.watch.error.identity",
            WatchError::Access => "history.watch.error.access",
            WatchError::Existing => "history.watch.error.existing",
            WatchError::Missing => "history.watch.error.missing",
            WatchError::Storage => "history.watch.error.storage",
            WatchError::Invalid => "history.watch.error.invalid",
        })
    })
}

/// Starts or stops recording; what is already recorded stays until it expires.
pub fn set_history_enabled(
    app: crate::native_runtime::Host,
    state: State<'_, AppState>,
    dir: State<'_, HistoryDir>,
    enabled: bool,
) -> Result<SettingsView, String> {
    let path = dir.write();
    app.state::<SettingsStore>()
        .update(|s| s.history = enabled)
        .map_err(|e| e.to_string())?;
    state.sampler.send(Control::SetHistoryDir(
        (enabled && dir.recording_allowed).then(|| path.clone()),
    ));
    Ok(settings_view(&app))
}

pub fn set_history_options(
    app: crate::native_runtime::Host,
    options: crate::settings::HistoryOptionsPatch,
) -> Result<SettingsView, String> {
    if !options.valid() {
        return Err(tr!("app.invalidRequest"));
    }
    let dir = app.state::<HistoryDir>();
    let path = dir.write();
    let saved = app
        .state::<SettingsStore>()
        .update(|s| options.apply(s))
        .map_err(|_| tr!("history.error.save"))?;
    if dir.recording_allowed {
        app.state::<AppState>()
            .sampler
            .send(Control::SetHistoryOptions(
                path.clone(),
                saved.history_options(),
            ));
    }
    Ok(settings_view(&app))
}

fn resume_history(
    state: &AppState,
    path: &std::path::Path,
    saved: &crate::settings::Settings,
    keep_observation: bool,
) {
    state.sampler.send(Control::SetHistoryOptions(
        path.into(),
        saved.history_options(),
    ));
    state
        .sampler
        .send(Control::SetHistoryDir(saved.history.then(|| path.into())));
    state.sampler.send(Control::ResumeWatch {
        dir: path.into(),
        keep_observation,
    });
}
fn pause_history(
    state: &AppState,
    path: &std::path::Path,
    saved: &crate::settings::Settings,
) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    state.sampler.send(Control::PauseHistory(tx));
    if rx.recv_timeout(Duration::from_secs(30)).is_err() {
        // FIFO order restores recording even if the stalled sampler handles the pause later.
        resume_history(state, path, saved, true);
        return Err(tr!("host.samplerStalled"));
    }
    Ok(())
}

pub fn history_move(
    app: crate::native_runtime::Host,
    dir: String,
    move_records: bool,
) -> Result<SettingsView, String> {
    let location = app.state::<HistoryDir>();
    let mut current = location.write();
    let store = app.state::<SettingsStore>();
    let saved = store.get();
    let requested = (!dir.is_empty()).then(|| std::path::PathBuf::from(dir));
    let destination = match &requested {
        Some(path) => path.clone(),
        None => location.default.clone(),
    };
    if destination == *current && requested.is_none() {
        store
            .update(|s| s.history_dir = None)
            .map_err(|_| tr!("history.error.save"))?;
        return Ok(settings_view(&app));
    }
    history::validate_directory(&current, &destination).map_err(|e| {
        argus_core::diagnostics::file_failure("history validate directory", &destination, &e);
        tr!("history.error.directory")
    })?;
    let state = app.state::<AppState>();
    if location.recording_allowed {
        pause_history(&state, &current, &saved)?;
    }
    let mut save_failed = false;
    let save = || {
        store
            .update(|s| s.history_dir = requested)
            .inspect_err(|_| save_failed = true)
    };
    let result = if move_records {
        history::move_records(&current, &destination, save)
    } else {
        save()
    };
    if result.is_ok() {
        *current = destination;
    }
    if location.recording_allowed {
        resume_history(&state, &current, &saved, move_records || result.is_err());
    }
    result.map_err(|e| {
        argus_core::diagnostics::file_failure("history move", &current, &e);
        if save_failed {
            tr!("history.error.save")
        } else {
            tr!("history.error.move")
        }
    })?;
    Ok(settings_view(&app))
}

pub fn history_reset_dir(
    app: crate::native_runtime::Host,
    move_records: bool,
) -> Result<SettingsView, String> {
    history_move(app, String::new(), move_records)
}

pub fn history_open_dir(dir: State<'_, HistoryDir>) -> Result<(), String> {
    open_folder(&dir.read()).map_err(|_| tr!("history.error.open"))
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
pub fn clear_history(app: crate::native_runtime::Host) -> Result<(), String> {
    let dir = app.state::<HistoryDir>();
    let path = dir.write();
    let state = app.state::<AppState>();
    let saved = app.state::<SettingsStore>().get();
    if dir.recording_allowed {
        pause_history(&state, &path, &saved)?;
    }
    let result = history::clear(&path).and_then(|()| {
        argus_core::watch::WatchRecorder::delete_saved(&path)
            .map_err(|_| std::io::Error::other("observation cleanup failed"))
    });
    if dir.recording_allowed {
        resume_history(&state, &path, &saved, false);
    }
    result.map_err(|e| {
        argus_core::diagnostics::file_failure("history clear", &path, &e);
        tr!("history.error.clear")
    })
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

pub fn open_sensor_driver_page() -> Result<(), String> {
    crate::shell::open_url("https://pawnio.eu/")
}

/// Opens the same project homepage shown in Settings and the installer.
pub fn open_homepage() -> Result<(), String> {
    crate::shell::open_url(
        argus_app::config::manifest()["homepage"]
            .as_str()
            .expect("project homepage"),
    )
}

/// Opens the source repository, where releases and issues are too.
pub fn open_repository() -> Result<(), String> {
    crate::shell::open_url(updater::REPO)
}

/// The website's page at `path` in the language `locale`: Simplified Chinese
/// at the root, every other language under its own folder.
fn website_page(homepage: &str, locale: argus_core::i18n::Locale, path: &str) -> String {
    use argus_core::i18n::Locale;
    let folder = match locale {
        Locale::ZhCn => "",
        Locale::ZhTw => "/zh-TW",
        Locale::EnUs => "/en",
        Locale::EsEs => "/es",
        Locale::DeDe => "/de",
        Locale::JaJp => "/ja",
        Locale::FrFr => "/fr",
        Locale::RuRu => "/ru",
    };
    format!("{}{folder}{path}", homepage.trim_end_matches('/'))
}

fn homepage() -> &'static str {
    argus_app::config::manifest()["homepage"]
        .as_str()
        .expect("project homepage")
}

/// Opens the website's download page in the interface language: where a
/// portable copy is sent for the installer.
pub fn open_download_page() -> Result<(), String> {
    crate::shell::open_url(&website_page(
        homepage(),
        argus_core::i18n::locale(),
        "/download/",
    ))
}

/// The address of one of the pages the About page's link cards lead to, by
/// the name the interface gives it; `None` for any other name, so the
/// interface can only open these.
fn project_page(homepage: &str, locale: argus_core::i18n::Locale, page: &str) -> Option<String> {
    let repository = updater::REPO;
    Some(match page {
        "download" => website_page(homepage, locale, "/download/"),
        "changelog" => website_page(homepage, locale, "/changelog/"),
        // The guide is written in Simplified Chinese alone; the website's
        // own navigation links it from every language.
        "guide" => format!("{}/docs/guide/", homepage.trim_end_matches('/')),
        // The issue forms, with the questions forum and the guide among them.
        "issues" => format!("{repository}/issues/new/choose"),
        "discussions" => format!("{repository}/discussions"),
        "releases" => format!("{repository}/releases"),
        _ => return None,
    })
}

/// Opens one of the project's pages named by `project_page`.
pub fn open_project_page(page: String) -> Result<(), String> {
    let url = project_page(homepage(), argus_core::i18n::locale(), &page)
        .ok_or_else(|| format!("unknown project page {page}"))?;
    crate::shell::open_url(&url)
}

/// Opens the newest release on GitHub, with the installer among its files.
pub fn open_latest_release() -> Result<(), String> {
    crate::shell::open_url(&format!("{}/releases/latest", updater::REPO))
}

/// Opens the group invitation in the default browser, which hands it to QQ.
pub fn open_community() -> Result<(), String> {
    crate::shell::open_url(
        argus_app::config::manifest()["community"]["url"]
            .as_str()
            .expect("community URL"),
    )
}

/// Opens the Telegram channel in the default browser, which offers to hand it
/// to Telegram.
pub fn open_channel() -> Result<(), String> {
    crate::shell::open_url(
        argus_app::config::manifest()["community"]["channel"]
            .as_str()
            .expect("channel URL"),
    )
}

/// Opens the folder with `argus.log`, for attaching to bug reports.
pub fn open_log_dir(app: crate::native_runtime::Host) -> Result<(), String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| e.to_string())?
        .join("diagnostics");
    open_folder(&dir)
}

fn open_folder(dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let explorer = argus_app::explorer::system_explorer().map_err(|_| tr!("host.fileMissing"))?;
    let quote = argus_app::tasks::quote;
    let command = format!(
        "{} {}",
        quote(&explorer),
        quote(dir.to_str().ok_or_else(|| tr!("host.fileMissing"))?)
    );
    argus_app::tasks::launch(0, command, false)
        .map_err(|e| std::io::Error::from_raw_os_error(e.code as i32).to_string())
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

/// What deleting a service came to: gone, or `pending` until it has
/// stopped and every handle to it is closed.
#[derive(Debug, Serialize)]
pub struct ServiceDeletion {
    pub pending: bool,
}

pub fn delete_service(
    state: State<'_, AppState>,
    name: String,
) -> Result<ServiceDeletion, ActionError> {
    // The privileged service refuses its own deletion; so does Argus with
    // its own rights.
    let result = if argus_service::is_own_service(&name) {
        action_result("delete Windows service", "local", Err(ActionError::Refused))
    } else {
        privileged(
            &state,
            Operation::DeleteService { name: name.clone() },
            |reply| {
                done(reply)?;
                state.services.invalidate(&name);
                Some(())
            },
            || state.services.delete(&name),
        )
    };
    state.sampler.send(Control::Refresh);
    result.map(|()| ServiceDeletion {
        pending: argus_core::services::remains(&name),
    })
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

pub fn delete_startup_item(id: String) -> Result<(), ActionError> {
    action_result("delete startup item", "local", startup::delete(&id))
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

    #[test]
    fn history_dispatch_serves_both_page_contracts() {
        use crate::{dispatch::invoke, native_runtime::Host};
        use serde_json::json;
        let base = 1_800_000_000_000i64;
        let dir =
            std::env::temp_dir().join(format!("argus-history-dispatch-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let host = Host::new("dev.argus.taskmanager.test.history-dispatch".into());
        host.manage(HistoryDir {
            path: std::sync::RwLock::new(dir.clone()),
            default: dir.clone(),
            recording_allowed: true,
        });
        host.manage(SettingsStore::load(dir.join("settings.json")));
        let mut rec = history::HistoryRecorder::new(dir.clone());
        for n in 0..10 {
            rec.record(&argus_core::Snapshot {
                seq: n as u64,
                timestamp: base + n * 1000,
                system: argus_core::SystemStats {
                    cpu: 25.,
                    mem_used: 85,
                    mem_total: 100,
                    ..Default::default()
                },
                hardware: Default::default(),
                processes: Vec::new(),
                sessions: Vec::new(),
                leaks: Vec::new(),
                process_network: false,
            });
        }
        let live = invoke(
            &host,
            "history_points",
            json!({"from":base,"to":base+9000,"bucket":1000,"origin":base,"keys":["cpu","absent"]}),
        )
        .unwrap();
        assert_eq!(live["ts"].as_array().unwrap().len(), 10);
        assert_eq!(live["series"]["cpu"]["mean"][0], 25.);
        assert!(live["series"]["absent"]["mean"][0].is_null());
        drop(rec);
        let query = |name, args| invoke(&host, name, args).unwrap();
        let interval = json!({"from":base,"to":base+10000});
        assert_eq!(
            query("history_events", interval.clone()),
            json!({"events":[], "truncated":false})
        );
        assert_eq!(
            query("history_events", json!({"from":base,"to":base+60_000})),
            json!({"events":[{"kind":"mem","from":base,"to":base+60_000,"peak":85.0,"peakAt":base,"top":null}],"truncated":false})
        );
        for invalid in [
            json!({"from":base,"to":base}),
            json!({"from":2,"to":1}),
            json!({"from":0,"to":34_560_000_001i64}),
            json!({"from":i64::MIN,"to":i64::MAX}),
            json!({"from":"0","to":1}),
            json!({"to":1}),
        ] {
            assert!(invoke(&host, "history_events", invalid).is_err());
        }
        assert!(
            invoke(
                &host,
                "history_events",
                json!({"from":0,"to":34_560_000_000i64})
            )
            .is_ok()
        );
        assert_eq!(
            query("history_range", json!({})),
            json!([base, base + 5000])
        );
        assert_eq!(
            query(
                "history_series",
                json!({"from":base,"to":base+10000,"buckets":2})
            )[0]["frameTs"],
            base
        );
        assert_eq!(
            query("history_moment", json!({"ts":base}))["system"]["cpuPeak"],
            25.
        );
        assert_eq!(
            query("history_frame", json!({"ts":base}))["system"]["cpuKernel"],
            0.
        );
        assert!(
            query("history_channels", interval)
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["key"] == "cpuVoltage" && v["service"] == true)
        );
        assert_eq!(
            query(
                "history_table",
                json!({"from":base,"to":base+10000,"trend":"cpu","points":4})
            )["recorded"],
            10_000
        );
        let usage = query("history_usage", json!({}));
        assert_eq!(usage["days"], 1);
        assert_eq!(usage["breakdown"]["watch"], 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn history_settings_move_clear_and_reset_through_sampler_and_dispatch() {
        use crate::{dispatch::invoke, native_runtime::Host};
        use serde_json::json;
        use std::sync::{Arc, RwLock, mpsc};
        let root = std::env::temp_dir().join(format!(
            "argus-history-host-{:?}",
            unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap()
        ));
        let default = root.join("default");
        let other = root.join("other");
        std::fs::create_dir_all(&default).unwrap();
        let store = SettingsStore::load(root.join("settings.json"));
        let host = Host::new("dev.argus.taskmanager.test.history-options".into());
        let meta = argus_core::MetaStore::new();
        let rules = Arc::new(argus_service::RuleRunner::spawn(false));
        let sampler = crate::sampler::Sampler::spawn(
            {
                let meta = meta.clone();
                move || argus_core::Collector::new(meta)
            },
            rules.clone(),
        );
        let (tx, rx) = mpsc::channel();
        sampler.send(Control::Subscribe(argus_app::channel::Channel::new(
            move |s: argus_core::Snapshot| {
                tx.send(
                    s.processes
                        .iter()
                        .find(|p| p.pid == std::process::id())
                        .map(|p| p.create_time),
                )?;
                Ok(())
            },
        )));
        host.manage(AppState {
            meta,
            rules,
            sampler,
            elevated: false,
            task_manager_start: false,
            services: Default::default(),
        });
        host.manage(crate::hotkey::HotkeyState::default());
        host.manage(HistoryDir {
            path: RwLock::new(default.clone()),
            default: default.clone(),
            recording_allowed: true,
        });
        host.manage(store);
        let ask = |name, args| invoke(&host, name, args).unwrap();
        ask("set_history_enabled", json!({"enabled":true}));
        let v = ask(
            "set_history_options",
            json!({"options":{"keepDays":null,"keepBytes":null,"record":{"cpuExtra":false},"processes":40}}),
        );
        assert_eq!(v["historyProcesses"], 40);
        assert_eq!(v["historyRecord"]["cpuExtra"], false);
        assert!(v["historyDir"].is_null() && v["historyKeepDays"].is_null());
        assert!(
            invoke(
                &host,
                "set_history_options",
                json!({"options":{"processes":2}})
            )
            .is_err()
        );
        let sample = || rx.recv_timeout(Duration::from_secs(25)).unwrap();
        sample();
        let created = sample().unwrap();
        ask(
            "watch_process",
            json!({"action":{"op":"start","pid":std::process::id(),"createTime":created,"name":"test"}}),
        );
        std::fs::create_dir_all(&other).unwrap();
        let existing = std::fs::read_dir(&default)
            .unwrap()
            .flatten()
            .find(|f| f.path().extension().is_some_and(|ext| ext == "hist"))
            .unwrap()
            .file_name();
        std::fs::write(other.join(&existing), b"existing").unwrap();
        assert!(
            invoke(
                &host,
                "history_move",
                json!({"dir":other,"moveRecords":true})
            )
            .is_err()
        );
        assert!(host.state::<SettingsStore>().get().history_dir.is_none());
        assert_eq!(
            ask("watch_process", json!({"action":{"op":"status"}}))["liveRuns"],
            1
        );
        assert_eq!(std::fs::read(other.join(&existing)).unwrap(), b"existing");
        std::fs::remove_file(other.join(existing)).unwrap();
        // The move barrier flushes the just-observed frame and closes its writer.
        let v = ask("history_move", json!({"dir":other,"moveRecords":true}));
        assert_eq!(v["historyDir"], json!(other));
        assert_eq!(
            ask("watch_process", json!({"action":{"op":"status"}}))["liveRuns"],
            1
        );
        assert!(other.join("program-watch.v1.bin").is_file());
        assert_eq!(std::fs::read_dir(&default).unwrap().count(), 0);
        let range = ask("history_range", json!({}));
        let frame = ask("history_frame", json!({"ts":range[1]}));
        assert!(frame["system"].get("cpuKernel").is_none());
        assert!(frame["system"]["cpu"].is_number());
        let usage = ask("history_usage", json!({}));
        assert_eq!(usage["limit"], json!({"days":null,"bytes":null}));
        assert!(usage["estimate"]["bytesPerDay"].as_u64().unwrap() > 0);
        assert!(
            invoke(
                &host,
                "history_move",
                json!({"dir":other.join("child"),"moveRecords":true})
            )
            .is_err()
        );
        ask("set_history_enabled", json!({"enabled":false}));
        let v = ask("history_reset_dir", json!({"moveRecords":false}));
        assert!(v["historyDir"].is_null());
        assert!(ask("history_range", json!({})).is_null());
        ask("history_move", json!({"dir":other,"moveRecords":false}));
        assert!(ask("history_range", json!({})).is_array());
        assert!(ask("clear_history", json!({})).is_null());
        assert_eq!(ask("history_usage", json!({}))["bytes"], 0);
        let expired = other.join("2001-01-01.hist");
        std::fs::write(&expired, b"ARGH\x03").unwrap();
        ask("set_history_options", json!({"options":{"keepDays":1}}));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while expired.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!expired.exists());
        host.state::<AppState>().sampler.shutdown();
        std::fs::remove_dir_all(root).unwrap();
    }

    fn denied() -> Result<(), ActionError> {
        Err(ActionError::AccessDenied)
    }

    /// A deletion that fails comes back with its cause, which the log keeps
    /// too, with the stage that failed but not the entry's name.
    #[test]
    fn a_failed_deletion_is_logged_with_its_cause() {
        argus_core::diagnostics::capture_helper_failures();
        let name = format!("Argus missing entry {}", std::process::id());
        let result = delete_startup_item(format!("hklmRun|{name}"));
        let log = argus_core::diagnostics::take_helper_failures();
        // Nothing of that name is there, so nothing is deleted either way:
        // without administrator rights the key does not open.
        if argus_core::privilege::is_elevated() {
            assert!(matches!(result, Err(ActionError::NotFound)));
        } else {
            assert!(matches!(result, Err(ActionError::AccessDenied)));
            assert!(
                log.iter()
                    .any(|line| line.contains("startup delete RegOpenKeyExW: ")
                        && line.contains("(0x80070005)")),
                "{log:#?}"
            );
        }
        assert!(
            log.iter()
                .any(|line| line.contains("delete startup item: source=local; ")),
            "{log:#?}"
        );
        assert!(log.iter().all(|line| !line.contains(&name)));
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

    #[test]
    fn the_download_page_follows_the_interface_language() {
        use argus_core::i18n::Locale;
        // The website keeps Simplified Chinese at its root.
        assert_eq!(
            website_page("https://argustask.com", Locale::ZhCn, "/download/"),
            "https://argustask.com/download/"
        );
        assert_eq!(
            website_page("https://argustask.com/", Locale::ZhTw, "/download/"),
            "https://argustask.com/zh-TW/download/"
        );
        // Every other language has a folder of its own, named apart.
        let pages = Locale::ALL
            .iter()
            .map(|locale| website_page("https://argustask.com", *locale, "/download/"))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(pages.len(), Locale::ALL.len());
        assert!(pages.contains("https://argustask.com/en/download/"));
    }

    #[test]
    fn the_link_cards_open_only_their_own_pages() {
        use argus_core::i18n::Locale;
        let page = |locale, name| project_page("https://argustask.com/", locale, name);
        assert_eq!(
            page(Locale::EnUs, "changelog").as_deref(),
            Some("https://argustask.com/en/changelog/")
        );
        assert_eq!(
            page(Locale::ZhCn, "download").as_deref(),
            Some("https://argustask.com/download/")
        );
        // The guide has one language: every interface opens the same page.
        assert_eq!(page(Locale::JaJp, "guide"), page(Locale::ZhCn, "guide"));
        assert_eq!(
            page(Locale::ZhCn, "guide").as_deref(),
            Some("https://argustask.com/docs/guide/")
        );
        for name in ["issues", "discussions", "releases"] {
            let address = page(Locale::DeDe, name).expect("repository page");
            assert!(
                address.starts_with(&format!("{}/", updater::REPO)),
                "{address}"
            );
        }
        // Nothing else, so a request cannot name an address of its own.
        assert_eq!(page(Locale::ZhCn, "https://example.com"), None);
        assert_eq!(page(Locale::ZhCn, ""), None);
    }
}
