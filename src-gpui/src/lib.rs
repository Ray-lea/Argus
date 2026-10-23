mod async_runtime;

mod catalog;
mod commands;

mod diagnostics;
mod dispatch;
mod explorer;
mod frontend;
mod hotkey;
mod installer;
mod instance;
mod leftovers;
mod logging;
mod native_runtime;
mod packaging;
mod platform;
mod sampler;
mod settings;
mod shell;
mod tasks;
mod update_transport;
mod updater;

include!(concat!(env!("OUT_DIR"), "/taskmgr-assets.rs"));

use argus_core::{Collector, MetaStore, Snapshot, services::ServiceCatalog};
use argus_service::RuleRunner;
use native_runtime::{Channel, Host};
use sampler::{Control, Sampler};
use settings::SettingsStore;
use std::{
    path::PathBuf,
    sync::{
        Arc, OnceLock, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};
static TASKMGR_COMPLETED: AtomicBool = AtomicBool::new(false);
static NETWORK_OWNED: AtomicBool = AtomicBool::new(false);
static NETWORK_SESSION: OnceLock<String> = OnceLock::new();
pub(crate) fn stop_network_session() {
    if NETWORK_OWNED.swap(false, Ordering::AcqRel)
        && let Some(session) = NETWORK_SESSION.get()
    {
        argus_core::net_process::stop_session(session);
    }
}
pub struct HistoryDir(pub PathBuf);
pub struct AppState {
    sampler: Sampler,
    elevated: bool,
    task_manager_start: bool,
    meta: Arc<MetaStore>,
    services: ServiceCatalog,
    rules: Arc<RuleRunner>,
}
#[derive(Default)]
pub struct Latest(pub RwLock<Option<Arc<Snapshot>>>);
#[derive(Clone)]
pub struct Options {
    pub hidden: bool,
    pub identifier: String,
    pub fixture: Option<PathBuf>,
    pub page: String,
    pub stop_after: Option<u64>,
    pub test: bool,
    pub control_stdin: bool,
    pub width: f32,
    pub height: f32,
}
impl Options {
    fn parse() -> Self {
        let args: Vec<_> = std::env::args().collect();
        let value = |name: &str| {
            args.iter()
                .position(|s| s == name)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let fixture = value("--fixture").map(PathBuf::from);
        let identifier = value("--identifier").unwrap_or_else(|| {
            if cfg!(debug_assertions) {
                format!("{}.devbuild", argus_app::config::identifier())
            } else {
                argus_app::config::identifier().into()
            }
        });
        let window = &argus_app::config::manifest()["window"];
        Self {
            hidden: args.iter().any(|s| s == "--hidden"),
            test: args.iter().any(|s| s == "--test") || fixture.is_some(),
            control_stdin: args.iter().any(|s| s == "--control-stdin"),
            identifier,
            fixture,
            page: value("--page").unwrap_or_default(),
            stop_after: value("--stop-after").and_then(|s| s.parse().ok()),
            width: value("--width")
                .and_then(|s| s.parse().ok())
                .unwrap_or(window["width"].as_f64().expect("configured window width") as f32),
            height: value("--height")
                .and_then(|s| s.parse().ok())
                .unwrap_or(window["height"].as_f64().expect("configured window height") as f32),
        }
    }
}

pub fn run() -> anyhow::Result<()> {
    for mode in [
        packaging::run_mode,
        installer::run_mode,
        explorer::run_mode,
        tasks::run_mode,
    ] {
        if let Some(code) = mode() {
            std::process::exit(code);
        }
    }
    if let Some(code) = argus_service::taskmgr::run_mode(ASSETS) {
        std::process::exit(code);
    }
    if let Some(code) = argus_service::run_mode() {
        std::process::exit(code);
    }
    let mut options = Options::parse();
    anyhow::ensure!(
        !options.test || options.identifier != argus_app::config::identifier(),
        "tests require an isolated identifier"
    );
    anyhow::ensure!(
        argus_app::config::valid_identifier(&options.identifier),
        "invalid application identifier"
    );
    anyhow::ensure!(
        options.width.is_finite()
            && options.height.is_finite()
            && options.width > 0.
            && options.height > 0.,
        "invalid window dimensions"
    );
    let _ = NETWORK_SESSION.set(format!(
        "{} Network ({})",
        argus_app::config::product_name(),
        options.identifier
    ));
    let host = Host::new(options.identifier.clone());
    if std::env::args().any(|a| leftovers::is_close_arg(&a)) {
        leftovers::close_copies();
        return Ok(());
    }
    let _owner = match instance::acquire(&options.identifier, &std::env::args().collect::<Vec<_>>())
    {
        Ok(Some(owner)) => owner,
        Ok(None) => return Ok(()),
        Err(error) => {
            error.report();
            return Err(anyhow::anyhow!(
                "single-instance startup failed: HRESULT={:#x}",
                error.code().0
            ));
        }
    };
    catalog::register();
    let settings = SettingsStore::load(host.path().app_config_dir()?.join("settings.json"));
    argus_core::i18n::set_locale(settings.get().language.resolve());
    logging::init(host.path().app_log_dir()?.join("diagnostics"))?;
    log::info!(
        "Argus {} started; renderer=GPUI; hidden={}; isolated_test={}",
        env!("CARGO_PKG_VERSION"),
        options.hidden,
        options.test
    );
    let elevated = argus_core::privilege::is_elevated();
    diagnostics::log_environment(elevated);
    if !options.test {
        diagnostics::log_privileged_state();
    }
    if !options.test {
        argus_core::privilege::raise_self_priority();
        if elevated {
            argus_core::privilege::enable_debug_privilege();
        }
    }
    let meta = MetaStore::new();
    let rules = Arc::new(RuleRunner::spawn(elevated));
    let sampler = Sampler::spawn(
        {
            let meta = meta.clone();
            let test = options.test;
            move || {
                let mut collector = Collector::new(meta);
                if !test {
                    let owned = collector.enable_process_network(
                        NETWORK_SESSION.get().expect("network identity initialized"),
                    );
                    NETWORK_OWNED.store(owned, Ordering::Release);
                }
                collector
            }
        },
        rules.clone(),
    );
    let task_manager_start =
        argus_service::taskmgr::ready::queue(&std::env::args().collect::<Vec<_>>());
    options.hidden |= task_manager_start;
    host.manage(AppState {
        sampler,
        elevated,
        task_manager_start,
        meta,
        services: ServiceCatalog::default(),
        rules,
    });
    host.manage(settings);
    host.manage(hotkey::HotkeyState::default());
    host.manage(Latest::default());
    let history = host.path().app_local_data_dir()?.join("history");
    if !options.test {
        argus_core::history::prune_expired(&history);
    }
    host.manage(HistoryDir(history.clone()));
    let saved = host.state::<SettingsStore>().get();
    let state = host.state::<AppState>();
    if !options.test {
        state.rules.set_rules(saved.rules.clone());
        state
            .sampler
            .send(Control::SetHistoryDir(saved.history.then_some(history)));
        state
            .sampler
            .send(Control::SetKernelSensors(saved.kernel_sensors));
    }
    state.sampler.send(saved.background_intervals());
    state.sampler.send(Control::Subscribe(Channel::new({
        let host = host.clone();
        move |snapshot: Snapshot| {
            diagnostics::log_hardware_once(&snapshot);
            *host.state::<Latest>().0.write().unwrap() = Some(Arc::new(snapshot));
            host.wake();
            Ok(())
        }
    })));
    drop(state);
    updater::init(&host);
    options.hidden |= updater::take_relaunch_hidden(&host);
    if let Some(seconds) = options.stop_after {
        let host = host.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            host.exit(0);
        });
    }
    let output = frontend::run(host.clone(), options);
    shell::shutdown(&host);
    instance::shutdown(&host);
    stop_network_session();
    output
}
