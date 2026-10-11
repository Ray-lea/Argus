use std::os::windows::process::CommandExt;
use std::time::Duration;

use argus_core::Snapshot;
use argus_core::actions::{self, ActionError, TreeKillReport};
use argus_core::connections::{self, Connection};
use argus_core::details::{self, ProcessDetails};
use argus_core::filelock::{self, LockReport};
use argus_core::services::{ServiceAction, ServiceInfo, StartType};
use argus_core::startup::{self, StartupItem};
use argus_core::theme::{self, AccentColors};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};

use crate::AppState;
use crate::sampler::Control;
use crate::settings::{SettingsStore, SettingsView};
use crate::shell;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    elevated: bool,
    accent: Option<AccentColors>,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        elevated: state.elevated,
        accent: theme::accent_colors(),
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

#[tauri::command(async)]
pub fn kill_process(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
    force: bool,
) -> Result<(), ActionError> {
    let result = actions::terminate(pid, create_time, force);
    state.sampler.send(Control::Refresh);
    result
}

#[tauri::command(async)]
pub fn kill_process_tree(
    state: State<'_, AppState>,
    pid: u32,
    create_time: i64,
) -> Result<TreeKillReport, ActionError> {
    let result = actions::terminate_tree(pid, create_time);
    state.sampler.send(Control::Refresh);
    result
}

/// Opens Explorer with the file selected.
#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err("文件不存在".into());
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
        hotkey_error: store.hotkey_error.lock().unwrap().clone(),
    }
}

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> SettingsView {
    settings_view(&app)
}

#[tauri::command]
pub fn set_close_to_tray(app: tauri::AppHandle, value: bool) -> Result<SettingsView, String> {
    app.state::<SettingsStore>()
        .update(|s| s.close_to_tray = value)
        .map_err(|e| e.to_string())?;
    Ok(settings_view(&app))
}

/// Replaces the show/hide hotkey. An empty string turns it off.
#[tauri::command]
pub fn set_hotkey(app: tauri::AppHandle, hotkey: String) -> Result<SettingsView, String> {
    let store = app.state::<SettingsStore>();
    let previous = store.get().hotkey;
    if let Err(e) = shell::apply_hotkey(&app, Some(&previous), &hotkey) {
        // Put the old one back so the user is never left without a hotkey by accident.
        let _ = shell::apply_hotkey(&app, None, &previous);
        return Err(e);
    }
    store
        .update(|s| s.hotkey = hotkey)
        .map_err(|e| e.to_string())?;
    *store.hotkey_error.lock().unwrap() = None;
    Ok(settings_view(&app))
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

#[tauri::command]
pub fn quit(app: tauri::AppHandle) {
    app.exit(0);
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
    let result = state.services.control(&name, action);
    state.sampler.send(Control::Refresh);
    result
}

#[tauri::command(async)]
pub fn set_service_start_type(
    state: State<'_, AppState>,
    name: String,
    start_type: StartType,
) -> Result<(), ActionError> {
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
    local_address: String,
    local_port: u16,
    remote_address: String,
    remote_port: u16,
) -> Result<(), ActionError> {
    connections::close_tcp(&local_address, local_port, &remote_address, remote_port)
}

// ---- File locks -----------------------------------------------------------------

#[tauri::command(async)]
pub fn find_lockers(paths: Vec<String>) -> Result<LockReport, ActionError> {
    filelock::find_lockers(&paths)
}

/// Native file or folder picker.
#[tauri::command(async)]
pub fn pick_path(app: tauri::AppHandle, folder: bool) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file();
    let picked = if folder {
        dialog
            .set_title("选择要检查的文件夹")
            .blocking_pick_folder()
    } else {
        dialog.set_title("选择要检查的文件").blocking_pick_file()
    };
    picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}
