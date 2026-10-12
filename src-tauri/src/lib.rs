mod commands;
mod sampler;
mod settings;
mod shell;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use argus_core::rules::RuleEngine;
use argus_core::services::ServiceCatalog;
use argus_core::{Collector, MetaStore, Snapshot, privilege};
use tauri::Manager;
use tauri::http::{Response, StatusCode};

use tauri_plugin_autostart::MacosLauncher;

use crate::sampler::{Control, Sampler};
use crate::settings::SettingsStore;

static WAS_MINIMIZED: AtomicBool = AtomicBool::new(false);

/// ETW session for per-process network traffic; one per build, like the app
/// identifier, so a development build never stops an installed one's session.
const NETWORK_SESSION: &str = if cfg!(debug_assertions) {
    "Argus Network (dev)"
} else {
    "Argus Network"
};

/// Where history is recorded (set once the app's data folder is known).
pub struct HistoryDir(pub std::path::PathBuf);

pub struct AppState {
    sampler: Sampler,
    elevated: bool,
    meta: Arc<MetaStore>,
    services: ServiceCatalog,
    rules: Arc<Mutex<RuleEngine>>,
}

pub fn run() {
    privilege::raise_self_priority();
    let elevated = privilege::is_elevated();
    if elevated {
        privilege::enable_debug_privilege();
    }

    // Start sampling before the webview exists so the first snapshot is ready
    // (with meaningful CPU deltas) by the time the UI subscribes.
    let meta = MetaStore::new();
    let rules = Arc::new(Mutex::new(RuleEngine::default()));
    let sampler = Sampler::spawn(
        {
            let meta = meta.clone();
            move || {
                let mut collector = Collector::new(meta);
                // Needs administrator rights; otherwise the column stays hidden.
                collector.enable_process_network(NETWORK_SESSION);
                collector
            }
        },
        rules.clone(),
    );

    // Autostart launches with --hidden: stay in the tray until summoned.
    let start_hidden = std::env::args().any(|a| a == "--hidden");

    tauri::Builder::default()
        // Must come first: a second launch just brings the running one forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            shell::show_main(app)
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            sampler,
            elevated,
            meta: meta.clone(),
            services: ServiceCatalog::default(),
            rules,
        })
        .register_uri_scheme_protocol("argus", icon_protocol(meta))
        .setup(move |app| {
            let config = app.path().app_config_dir()?.join("settings.json");
            let settings = SettingsStore::load(config);
            if let Ok(mut engine) = app.state::<AppState>().rules.lock() {
                engine.set_rules(settings.get().rules);
            }
            let history = app.path().app_local_data_dir()?.join("history");
            // Expired days go even while recording is off.
            argus_core::history::prune_expired(&history);
            app.state::<AppState>().sampler.send(Control::SetHistoryDir(
                settings.get().history.then(|| history.clone()),
            ));
            app.manage(settings);
            app.manage(HistoryDir(history));
            shell::build_tray(app.handle())?;
            shell::register_saved_hotkey(app.handle());

            let handle = app.handle().clone();
            let last_tip = Mutex::new(String::new());
            let observer = move |s: &Snapshot| {
                let mem = if s.system.mem_total > 0 {
                    s.system.mem_used as f64 / s.system.mem_total as f64 * 100.0
                } else {
                    0.0
                };
                let tip = format!(
                    "{}\nCPU {:.0}%  内存 {:.0}%",
                    shell::APP_TITLE,
                    s.system.cpu,
                    mem
                );
                let mut last = last_tip.lock().unwrap();
                if *last != tip {
                    shell::set_tray_tooltip(&handle, &tip);
                    *last = tip;
                }
            };
            app.state::<AppState>()
                .sampler
                .send(Control::SetObserver(Box::new(observer)));

            if let Some(window) = app.get_webview_window("main") {
                let hwnd = window.hwnd()?.0 as isize;
                let probe = move || unsafe {
                    use windows::Win32::Foundation::HWND;
                    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};
                    let hwnd = HWND(hwnd as *mut _);
                    IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool()
                };
                app.state::<AppState>()
                    .sampler
                    .send(Control::SetHiddenProbe(Box::new(probe)));
                window.set_title(shell::APP_TITLE)?;
                shell::extend_backdrop(&window);
                shell::guard_resize_seam(&window);
                if !start_hidden {
                    window.show()?;
                    window.set_focus()?;
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            let main = || window.app_handle().get_webview_window("main");
            match event {
                // Coming back from the tray or minimized: refresh immediately.
                tauri::WindowEvent::Focused(true) => {
                    window.state::<AppState>().sampler.send(Control::Refresh);
                }
                // Resized fires continuously while dragging; act on transitions only.
                tauri::WindowEvent::Resized(_) => {
                    let minimized = window.is_minimized().unwrap_or(false);
                    if WAS_MINIMIZED.swap(minimized, Ordering::Relaxed) != minimized
                        && let Some(w) = main()
                    {
                        shell::set_webview_active(&w, !minimized);
                    }
                }
                tauri::WindowEvent::CloseRequested { api, .. }
                    if window.state::<SettingsStore>().get().close_to_tray =>
                {
                    api.prevent_close();
                    if let Some(w) = main() {
                        shell::hide_main(&w);
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::subscribe,
            commands::set_interval,
            commands::set_paused,
            commands::refresh,
            commands::process_details,
            commands::kill_process,
            commands::kill_process_tree,
            commands::set_priority,
            commands::set_efficiency_mode,
            commands::reveal_in_explorer,
            commands::get_settings,
            commands::set_close_to_tray,
            commands::set_hotkey,
            commands::set_autostart,
            commands::set_rules,
            commands::rule_stats,
            commands::history_range,
            commands::history_series,
            commands::history_moment,
            commands::history_usage,
            commands::set_history_enabled,
            commands::clear_history,
            commands::quit,
            commands::list_services,
            commands::control_service,
            commands::set_service_start_type,
            commands::open_services_console,
            commands::list_startup,
            commands::set_startup_enabled,
            commands::list_connections,
            commands::close_connection,
            commands::find_lockers,
            commands::pick_path,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Argus")
        .run(|_app, event| {
            // Trace sessions outlive their process, and threads get no chance
            // to clean up on exit, so stop it here.
            if let tauri::RunEvent::Exit = event {
                argus_core::net_process::stop_session(NETWORK_SESSION);
            }
        });
}

/// Serves process icons at `argus://icon/<id>` (`http://argus.localhost/icon/<id>`).
fn icon_protocol(
    meta: Arc<MetaStore>,
) -> impl Fn(tauri::UriSchemeContext<'_, tauri::Wry>, tauri::http::Request<Vec<u8>>) -> Response<Vec<u8>>
+ Send
+ Sync
+ 'static {
    move |_ctx, request| {
        let png = request
            .uri()
            .path()
            .strip_prefix("/icon/")
            .and_then(|id| id.parse::<u32>().ok())
            .and_then(|id| meta.icon_png(id));
        match png {
            Some(bytes) => Response::builder()
                .header("Content-Type", "image/png")
                .header("Cache-Control", "max-age=31536000, immutable")
                .header("Access-Control-Allow-Origin", "*")
                .body(bytes.to_vec())
                .unwrap(),
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Vec::new())
                .unwrap(),
        }
    }
}
