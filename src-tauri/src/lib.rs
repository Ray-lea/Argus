mod caption;
mod commands;
mod diagnostics;
mod leftovers;
mod sampler;
mod settings;
mod shell;
mod updater;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use argus_core::services::ServiceCatalog;
use argus_core::{Collector, MetaStore, Snapshot, privilege};
use argus_service::RuleRunner;
use tauri::http::{Response, StatusCode};
use tauri::{Emitter, Manager};

use tauri_plugin_autostart::MacosLauncher;

use crate::sampler::{Control, Sampler};
use crate::settings::SettingsStore;

static WAS_MINIMIZED: AtomicBool = AtomicBool::new(false);
static TASKMGR_COMPLETED: AtomicBool = AtomicBool::new(false);

/// ETW session for per-process network traffic; one per build, like the app
/// identifier, so a development build never stops an installed one's session.
const NETWORK_SESSION: &str = if cfg!(debug_assertions) {
    "Argus Network (dev)"
} else {
    "Argus Network"
};

/// Stops this build's trace session; nothing else stops it when Argus exits.
pub(crate) fn stop_network_session() {
    argus_core::net_process::stop_session(NETWORK_SESSION);
}

/// Logs to `%LOCALAPPDATA%\<identifier>\logs\argus.log` for bug reports:
/// Reviewed Argus events only. Legacy logs stay outside the shareable folder.
fn logger(path: std::path::PathBuf) -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};
    let mut builder = tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::Folder {
            path,
            file_name: Some("argus".into()),
        }))
        .max_file_size(1024 * 1024)
        .rotation_strategy(RotationStrategy::KeepOne)
        .timezone_strategy(TimezoneStrategy::UseLocal)
        // Third-party log payloads have not passed Argus's privacy boundary.
        .filter(|record| record.target().starts_with("argus"))
        .level(log::LevelFilter::Info);
    if cfg!(debug_assertions) {
        builder = builder.target(Target::new(TargetKind::Stdout));
    }
    builder.build()
}

/// Release builds abort on panic; the log is the only trace one leaves.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(location) = info.location() {
            log::error!(
                "panic at line {}, column {} (payload and file omitted)",
                location.line(),
                location.column()
            );
        } else {
            log::error!("panic (payload omitted)");
        }
        default(info);
    }));
}

/// Where history is recorded (set once the app's data folder is known).
pub struct HistoryDir(pub std::path::PathBuf);

pub struct AppState {
    sampler: Sampler,
    elevated: bool,
    meta: Arc<MetaStore>,
    services: ServiceCatalog,
    rules: Arc<RuleRunner>,
}

pub fn run() {
    #[cfg(not(debug_assertions))]
    let assets = Some((
        include_bytes!("../../target/release/argus-taskmgr-launcher.exe").as_slice(),
        include_bytes!("../../target/release/argus-taskmgr-recover.exe").as_slice(),
    ));
    #[cfg(debug_assertions)]
    let assets = None;
    if let Some(code) = argus_service::taskmgr::run_mode(assets) {
        std::process::exit(code);
    }
    if let Some(code) = argus_service::run_mode() {
        std::process::exit(code);
    }
    privilege::raise_self_priority();
    let elevated = privilege::is_elevated();
    if elevated {
        privilege::enable_debug_privilege();
    }

    // Start sampling before the webview exists so the first snapshot is ready
    // (with meaningful CPU deltas) by the time the UI subscribes.
    let meta = MetaStore::new();
    let rules = Arc::new(RuleRunner::spawn(elevated));
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
    let taskmgr_start = argus_service::taskmgr::ready::queue(&std::env::args().collect::<Vec<_>>());
    let start_hidden = taskmgr_start || std::env::args().any(|a| a == "--hidden");
    let updated = std::env::args().any(|a| a == updater::UPDATED_ARG);

    let context = tauri::generate_context!();
    // Before the window and its WebView exist (see leftovers.rs). Tauri keeps
    // WebView2's data in the app's local data folder.
    let leftovers = std::env::var_os("LOCALAPPDATA").map_or(0, |dir| {
        let folder = std::path::Path::new(&dir).join(&context.config().identifier);
        leftovers::end(&folder.join("EBWebView"))
    });

    tauri::Builder::default()
        // Must come first: a second launch just brings the running one forward.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if argus_service::taskmgr::ready::queue(&args) {
                let _ = app.emit("argus-taskmgr-ready", ());
            } else {
                TASKMGR_COMPLETED.store(true, Ordering::Release);
                shell::show_main(app)
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            sampler,
            elevated,
            meta: meta.clone(),
            services: ServiceCatalog::default(),
            rules,
        })
        .register_uri_scheme_protocol("argus", icon_protocol(meta))
        .setup(move |app| {
            app.handle()
                .plugin(logger(app.path().app_log_dir()?.join("diagnostics")))?;
            log_panics();
            log::info!(
                "Argus {} started ({}{}{}{})",
                env!("CARGO_PKG_VERSION"),
                if elevated {
                    "administrator"
                } else {
                    "standard user"
                },
                if start_hidden { ", hidden" } else { "" },
                if updated { ", after an update" } else { "" },
                if taskmgr_start {
                    ", as the task manager"
                } else {
                    ""
                },
            );
            if leftovers > 0 {
                log::info!("ended {leftovers} WebView2 processes an earlier Argus left behind");
            }
            let config = app.path().app_config_dir()?.join("settings.json");
            let settings = SettingsStore::load(config);
            argus_core::i18n::set_locale(settings.get().language.resolve());
            diagnostics::log_environment(elevated);
            diagnostics::log_privileged_state();
            app.state::<AppState>()
                .rules
                .set_rules(settings.get().rules);
            let history = app.path().app_local_data_dir()?.join("history");
            // Expired days go even while recording is off.
            argus_core::history::prune_expired(&history);
            app.state::<AppState>().sampler.send(Control::SetHistoryDir(
                settings.get().history.then(|| history.clone()),
            ));
            app.manage(settings);
            app.manage(HistoryDir(history));
            updater::init(app.handle());
            // An update started while Argus was in the tray relaunches it there.
            let relaunch_hidden = updater::take_relaunch_hidden(app.handle());
            let start_hidden = start_hidden || (updated && relaunch_hidden);
            shell::build_tray(app.handle())?;
            shell::register_saved_hotkey(app.handle());

            if taskmgr_start {
                let app = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(15));
                    if !TASKMGR_COMPLETED.load(Ordering::Acquire) {
                        app.exit(0);
                    }
                });
            }

            let handle = app.handle().clone();
            let last_tip = Mutex::new(String::new());
            let observer = move |s: &Snapshot| {
                diagnostics::log_hardware_once(s);
                let mem = if s.system.mem_total > 0 {
                    s.system.mem_used as f64 / s.system.mem_total as f64 * 100.0
                } else {
                    0.0
                };
                let usage = argus_core::tr!(
                    "tray.tooltip",
                    cpu = format_args!("{:.0}", s.system.cpu),
                    memory = format_args!("{mem:.0}"),
                );
                let tip = format!("{}\n{usage}", shell::app_title());
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
                window.set_title(&shell::app_title())?;
                shell::apply_theme(app.handle(), app.state::<SettingsStore>().get().theme);
                shell::extend_backdrop(&window);
                caption::install(&window, app.handle());
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
            commands::set_language,
            commands::set_theme,
            commands::set_caption_button,
            diagnostics::log_ui_error,
            commands::complete_onboarding,
            commands::privileged_service_status,
            commands::manage_privileged_service,
            commands::task_manager_status,
            commands::manage_task_manager,
            commands::task_manager_ready,
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
            commands::update_status,
            commands::check_update,
            commands::install_update,
            commands::set_update_settings,
            commands::open_release_page,
            commands::open_log_dir,
            commands::open_community,
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
        .build(context)
        .expect("error while building Argus")
        .run(|_app, event| {
            // Trace sessions outlive their process, and threads get no chance
            // to clean up on exit, so stop it here.
            if let tauri::RunEvent::Exit = event {
                stop_network_session();
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
