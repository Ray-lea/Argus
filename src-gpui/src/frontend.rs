//! Native views share the existing sampling and application services.
mod appearance;
mod assets;
mod backend;
mod charts;
mod collation;
mod dialogs;
mod gpu_usage;
mod inline;
mod layout;
mod lists;
mod menus;
mod model;
mod motion;
mod pages;
mod prefs;
mod recorder;
mod regions;
mod scrollbar;
mod shift;
mod tables;
mod theme;
mod tips;
mod welcome;
mod widgets;
use crate::{AppState, Options, native_runtime::Host};
use argus_core::tr;
use gpui_kit::base::Button;
use gpui_kit::component::{
    ThemeMode, TitleBar,
    input::{Input, InputEvent, InputState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use model::{Row, boolean, number, text, values};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use theme::{Palette, icon, label};
const PAGES: [(&str, &str); 7] = [
    ("processes", "ProcessesIcon"),
    ("performance", "PulseIcon"),
    ("services", "ServicesIcon"),
    ("startup", "PowerIcon"),
    ("network", "GlobeIcon"),
    ("history", "HistoryIcon"),
    ("settings", "SettingsIcon"),
];
#[derive(Clone)]
enum Action {
    Sort(String),
    Page(String),
    View(String),
    Select(String),
    Fold(String),
    Details,
    Pause,
    Interval(u64),
    Device(String),
    CpuMode(String),
    Invoke(String, Value),
    Dialog(String, Value),
    Popup(Vec<MenuItem>),
    Preference(String, Value),
    /// Text to copy and the locale key naming it in the confirmation.
    Copy(String, &'static str),
    CloseOverlay,
    Confirm,
}
#[derive(Clone)]
struct MenuItem {
    label: String,
    action: Action,
    disabled: bool,
    checked: bool,
    icon: Option<&'static str>,
    danger: bool,
    hint: Option<&'static str>,
    separator: bool,
    children: Vec<MenuItem>,
}
impl MenuItem {
    fn new(label: impl Into<String>, action: Action) -> Self {
        Self {
            label: label.into(),
            action,
            disabled: false,
            checked: false,
            icon: None,
            danger: false,
            hint: None,
            separator: false,
            children: Vec::new(),
        }
    }
    fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
struct Pending {
    command: String,
    cache: Option<String>,
    args: Value,
}
type TablePair = (
    Entity<gpui_kit::component::table::TableState<tables::NativeTable>>,
    Entity<tables::TableFrame>,
);
#[derive(Default)]
struct WorkCounters {
    serialize_us: u64,
    timeline_us: u64,
    rows_us: u64,
    rows_max_us: u64,
    row_builds: u64,
    background_group_us: u64,
    background_list_us: u64,
}
struct Desktop {
    table: Option<Entity<gpui_kit::component::table::TableState<tables::NativeTable>>>,
    table_frame: Option<Entity<tables::TableFrame>>,
    tables: HashMap<String, TablePair>,
    sorts: HashMap<String, (String, bool)>,
    queries: HashMap<String, String>,
    host: Host,
    options: Options,
    sequence: u64,
    palette: Palette,
    snapshot: Value,
    timeline: model::Timeline,
    chart_clock: Rc<RefCell<motion::Clock>>,
    chart_entries: RefCell<HashMap<String, charts::ChartEntry>>,
    chart_layer: Option<Entity<charts::ChartLayer>>,
    /// The dim behind dialogs and its layer (over the chart layer).
    scrim: Rc<dialogs::ScrimState>,
    scrim_view: Option<Entity<dialogs::Scrim>>,
    active_charts: Rc<RefCell<HashSet<String>>>,
    chart_placements: charts::Placements,
    page_opacity: Rc<Cell<f32>>,
    /// How far the entering page is drawn from its place (the chart layer follows).
    page_shift: Rc<Cell<Point<Pixels>>>,
    /// The details pane's drawn motion, for its charts (`details_pane`).
    pane_motion: Rc<RefCell<charts::PaneMotion>>,
    /// Opacity of the live charts being built: a view settling in fades its charts.
    chart_fade: Cell<f32>,
    /// How far the live charts being built are drawn from their place, inside
    /// their clip: the performance view arriving moves its charts.
    chart_shift: Cell<Point<Pixels>>,
    /// Whether the charts being built draw their captions inside the frame
    /// (a short performance card).
    chart_captions_inside: Cell<bool>,
    /// The viewport height less the performance card's content height, as
    /// last laid out: the card's height in a viewport of any height.
    perf_card_chrome: Rc<Cell<Option<Pixels>>>,
    chart_tick_queued: bool,
    chart_frame: u64,
    grid_bounds: RefCell<HashMap<String, Bounds<Pixels>>>,
    core_grid_size: Rc<Cell<Size<Pixels>>>,
    settings_scroll: ScrollHandle,
    device_scroll: ScrollHandle,
    settings_target: Option<usize>,
    page: String,
    page_epoch: u64,
    /// Where the last page change came from: -1, 0 (start-up) or 1.
    page_direction: f32,
    /// When the page last changed; the page fades in for `PAGE_FADE` after it.
    page_switched: Instant,
    /// The entrance starts with the first frame drawn after a page change.
    page_enter_pending: bool,
    render_count: u64,
    regions: regions::Regions,
    work: WorkCounters,
    frame_trace: Option<gpui_kit::gpui::profiler::FrameTimingCollector>,
    view_trace: Option<gpui_kit::gpui::profiler::journal::ForegroundJournalCollector>,
    snapshot_pending: bool,
    rows_epoch: u64,
    /// The sample each running process first appeared in; 0 for those present at start.
    first_seen: HashMap<String, u64>,
    header_bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    /// Hover hints on table cells, and the view that draws them (`tips`).
    tips: tips::Tips,
    tip_layer: Entity<tips::TipLayer>,
    /// The title row's start and end controls, and the row itself.
    toolbar_regions: Rc<RefCell<[Bounds<Pixels>; 3]>>,
    navigation_regions: Rc<RefCell<[Bounds<Pixels>; PAGES.len() + 1]>>,
    query: String,
    search: Entity<InputState>,
    focus: FocusHandle,
    subscriptions: Vec<Subscription>,
    prefs: prefs::Prefs,
    /// Interface scale (percent) and base font size (px), see `appearance`.
    ui_scale: u32,
    font_size: u32,
    view: String,
    sort: String,
    descending: bool,
    expanded: HashSet<String>,
    collapsed: HashSet<String>,
    sections: HashSet<String>,
    rows: Arc<Vec<Row>>,
    list: ListState,
    selected: HashMap<String, String>,
    data: HashMap<String, Value>,
    /// The connection list as last read, shared with row builds off the UI thread.
    connections: Option<Arc<Value>>,
    /// Reads of the connection list that changed it.
    connections_read: u64,
    /// The owners of the connections in the current snapshot, and how often
    /// they changed.
    owners: Arc<lists::Owners>,
    owners_read: u64,
    /// The sample and list read the owners were last taken from.
    owners_sequence: Option<(u64, u64)>,
    /// The network page's rows as last built, shown or not.
    connection_rows: Option<lists::ConnectionRows>,
    /// The latest connection build; a build that is no longer the latest is dropped.
    connection_build: u64,
    /// What the build in progress, if any, is built from.
    connection_building: Option<lists::ConnectionInputs>,
    /// A desktop render (and connection rows) waiting for the end of a
    /// list page's entrance (`after_entrance`).
    entrance_queued: bool,
    pending: HashMap<u64, Pending>,
    next_request: u64,
    nav_top: bool,
    nav_expanded: bool,
    /// The side navigation's expanded width (`navWidth`), set by dragging its edge.
    nav_width: f32,
    details_open: bool,
    details_width: f32,
    perf_sidebar_width: f32,
    device: String,
    cpu_mode: String,
    cpu_page: String,
    interval: u64,
    paused: bool,
    control_paused: bool,
    pointer_in_table: bool,
    held_order: bool,
    horizontal: f32,
    drag: Option<(String, Point<Pixels>, f32)>,
    popup: Option<menus::Popup>,
    /// The menu that just closed, fading out (`menus::Popup`).
    popup_leaving: Option<(menus::Popup, Instant)>,
    modal: Option<dialogs::Modal>,
    ui: widgets::UiState,
    last_poll: Instant,
    last_privilege: Instant,
    last_select: Option<(String, Instant)>,
    filters: HashMap<String, String>,
    forms: HashMap<String, Entity<InputState>>,
    first_frame: bool,
    /// When the first snapshot was drawn; the other lists are read soon after.
    first_frame_at: Option<Instant>,
    prefetched: bool,
    welcome_checked: bool,
    history_range: String,
    history_metric: String,
    history_cursor: Option<i64>,
    history_hover: Option<i64>,
    settings_section: String,
}
impl Desktop {
    fn new(
        host: Host,
        options: Options,
        prefs: prefs::Prefs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx));
        let settings =
            crate::dispatch::invoke(&host, "get_settings", json!({})).unwrap_or(Value::Null);
        let info = crate::dispatch::invoke(&host, "app_info", json!({})).unwrap_or(Value::Null);
        let palette = Palette::current(cx);
        let saved_sort: Value = prefs.get("sort", json!({"key":"cpu","desc":true}));
        let tips = tips::Tips::default();
        let tip_layer = cx.new(|cx| tips::TipLayer::new(tips.clone(), cx));
        let mut this = Self {
            host: host.clone(),
            options: options.clone(),
            sequence: 0,
            palette,
            snapshot: Value::Null,
            timeline: model::Timeline::new(),
            chart_clock: Rc::new(RefCell::new(motion::Clock::new(motion::Mode::Off))),
            chart_entries: RefCell::new(HashMap::new()),
            chart_layer: None,
            scrim: Rc::new(dialogs::ScrimState::default()),
            scrim_view: None,
            active_charts: Rc::new(RefCell::new(HashSet::new())),
            chart_placements: Rc::new(RefCell::new(HashMap::new())),
            page_opacity: Rc::new(Cell::new(1.)),
            page_shift: Rc::new(Cell::new(Point::default())),
            pane_motion: Rc::default(),
            chart_fade: Cell::new(1.),
            chart_shift: Cell::new(Point::default()),
            chart_captions_inside: Cell::new(false),
            perf_card_chrome: Rc::new(Cell::new(None)),
            chart_tick_queued: false,
            chart_frame: 0,
            grid_bounds: RefCell::new(HashMap::new()),
            core_grid_size: Rc::new(Cell::new(size(px(0.), px(0.)))),
            settings_scroll: ScrollHandle::new(),
            device_scroll: ScrollHandle::new(),
            settings_target: None,
            page: if options.page.is_empty() {
                prefs.get("page", "processes".into())
            } else {
                options.page.clone()
            },
            page_epoch: 0,
            page_direction: 0.,
            page_switched: Instant::now(),
            page_enter_pending: false,
            render_count: 0,
            regions: regions::Regions::default(),
            work: WorkCounters::default(),
            frame_trace: None,
            view_trace: None,
            snapshot_pending: false,
            rows_epoch: 0,
            first_seen: HashMap::new(),
            header_bounds: Rc::new(RefCell::new(HashMap::new())),
            tips,
            tip_layer,
            toolbar_regions: Rc::new(RefCell::new([Bounds::default(); 3])),
            navigation_regions: Rc::new(RefCell::new([Bounds::default(); PAGES.len() + 1])),
            query: String::new(),
            search: search.clone(),
            focus: cx.focus_handle(),
            subscriptions: Vec::new(),
            view: prefs.get("view", "apps".into()),
            sort: saved_sort["key"].as_str().unwrap_or("cpu").into(),
            descending: saved_sort["desc"].as_bool().unwrap_or(true),
            expanded: HashSet::new(),
            collapsed: HashSet::new(),
            sections: prefs
                .get::<Vec<String>>("collapsedSections", vec![])
                .into_iter()
                .collect(),
            rows: Arc::new(vec![]),
            table: None,
            table_frame: None,
            tables: HashMap::new(),
            sorts: HashMap::new(),
            queries: HashMap::new(),
            list: ListState::new(0, ListAlignment::Top, px(360.)),
            selected: HashMap::new(),
            data: HashMap::from([("settings".into(), settings), ("info".into(), info)]),
            connections: None,
            connections_read: 0,
            owners: Arc::default(),
            owners_read: 0,
            owners_sequence: None,
            connection_rows: None,
            connection_build: 0,
            connection_building: None,
            entrance_queued: false,
            pending: HashMap::new(),
            next_request: 0,
            nav_top: prefs.get::<String>("navPosition", "top".into()) == "top",
            nav_expanded: prefs.get("navExpanded", false),
            nav_width: Desktop::clamp_nav_width(prefs.get("navWidth", layout::NAV_WIDTH)),
            details_open: prefs.get("detailsPanel", true),
            details_width: prefs
                .get("detailsWidth", f32::from(theme::dimension("detailsWidth")))
                .clamp(Self::DETAILS_MIN_WIDTH, Self::DETAILS_MAX_WIDTH),
            perf_sidebar_width: prefs.get(
                "perfSideWidth",
                f32::from(theme::dimension("deviceRailWidth")),
            ),
            device: prefs.get("perfDevice", "cpu".into()),
            cpu_mode: prefs.get("cpuMode", "overall".into()),
            cpu_page: prefs.get("cpuPage", "overview".into()),
            interval: prefs.get("interval", 1000),
            paused: false,
            control_paused: false,
            pointer_in_table: false,
            held_order: false,
            horizontal: 0.,
            drag: None,
            popup: None,
            popup_leaving: None,
            modal: None,
            ui: widgets::UiState::default(),
            last_poll: Instant::now() - Duration::from_secs(10),
            last_privilege: Instant::now() - Duration::from_secs(60),
            last_select: None,
            filters: tables::saved_filters(&prefs),
            forms: HashMap::new(),
            first_frame: false,
            first_frame_at: None,
            prefetched: false,
            welcome_checked: false,
            history_range: "1h".into(),
            history_metric: "cpu".into(),
            history_cursor: None,
            history_hover: None,
            settings_section: "language".into(),
            ui_scale: appearance::SCALE.load(&prefs),
            font_size: appearance::FONT_SIZE.load(&prefs),
            prefs,
        };
        // A list page opens in its saved order, before the first rows are built.
        if this.page != "processes" {
            (this.sort, this.descending) = tables::saved_sort(&this.page, &this.prefs);
        }
        this.subscriptions.push(
            cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.query = this.search.read(cx).value().to_string();
                    this.rebuild_rows(false, cx);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    this.focus.focus(window, cx);
                }
                _ => {}
            }),
        );
        this.watch_menu_window(window, cx);
        if let Some(path) = &options.fixture {
            match std::fs::read(path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            {
                Some(fixture) => {
                    if let Some(data) = fixture.as_object() {
                        for (key, value) in data {
                            this.data.insert(key.clone(), value.clone());
                        }
                    }
                    this.take_connections();
                    for snapshot in values(&fixture["snapshots"]) {
                        this.accept_snapshot(snapshot.clone(), cx);
                    }
                    if let Some(snapshot) = fixture.get("snapshot") {
                        this.accept_snapshot(snapshot.clone(), cx);
                    }
                }
                None => log::warn!("fixture unavailable"),
            }
        }
        this.request(
            "set_interval",
            json!({"ms":this.interval}),
            None,
            window,
            cx,
        );
        // Whether an administrator start fell back to standard permissions.
        this.request(
            "administrator_status",
            json!({}),
            Some("administrator"),
            window,
            cx,
        );
        if let Some(outcomes) = this.prefs.outcomes() {
            cx.spawn_in(window, async move |this, cx| {
                while let Ok(saved) = outcomes.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            this.preferences_written(saved, window, cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let receiver = host.notifications();
        cx.spawn_in(window, async move |this, cx| {
            while receiver.recv().await.is_ok() {
                if this
                    .update_in(cx, |this, window, cx| this.notifications(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                smol::Timer::after(Duration::from_millis(250)).await;
                if this
                    .update_in(cx, |this, window, cx| {
                        if this.host.exit_code().is_some() {
                            cx.quit();
                            return;
                        }
                        this.tick_ui(window, cx);
                        if this.host.get_window("main").is_some_and(|w| {
                            w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)
                        }) {
                            if this.last_poll.elapsed() >= this.poll_interval() {
                                this.last_poll = Instant::now();
                                this.poll_page(window, cx);
                            }
                            this.prefetch(window, cx);
                            // The privilege badge follows the service: it may start after Argus.
                            if this.last_privilege.elapsed() >= Duration::from_secs(30)
                                && !boolean(&this.data["info"], "elevated")
                            {
                                this.last_privilege = Instant::now();
                                this.request(
                                    "privileged_service_status",
                                    json!({}),
                                    Some("privilege"),
                                    window,
                                    cx,
                                );
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        if options.test || options.control_stdin {
            let host = host.clone();
            let test = options.test;
            std::thread::spawn(move || {
                use std::io::BufRead;
                for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                    if let Ok(value) = serde_json::from_str::<Value>(&line) {
                        if value["exit"].as_bool() == Some(true) {
                            host.exit(0);
                        } else if test {
                            let _ = host.emit("test-control", value);
                        }
                    }
                }
            });
        }
        this.poll_page(window, cx);
        this.apply_motion(cx);
        this
    }
}
pub fn run(host: Host, options: Options) -> anyhow::Result<()> {
    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::install_surface(cx);
            let opts = WindowOptions {
                window_bounds: Some(WindowBounds::centered(
                    size(px(options.width), px(options.height)),
                    cx,
                )),
                focus: !options.hidden && !options.test,
                show: !options.hidden,
                app_id: Some(options.identifier.clone()),
                window_min_size: Some(size(
                    px(argus_app::config::manifest()["window"]["minWidth"]
                        .as_f64()
                        .expect("minimum width") as f32),
                    px(argus_app::config::manifest()["window"]["minHeight"]
                        .as_f64()
                        .expect("minimum height") as f32),
                )),
                inactive_frame_interval: None,
                window_background: if crate::platform::supports_backdrop() {
                    WindowBackgroundAppearance::MicaBackdrop
                } else {
                    WindowBackgroundAppearance::Opaque
                },
                ..TitleBar::window_options()
            };
            let owner = host.clone();
            gpui_kit::open_window(opts, cx, move |window, cx| {
                window.set_window_title(&argus_app::config::app_title());
                let prefs = prefs::Prefs::load(
                    host.path()
                        .app_config_dir()
                        .expect("application config")
                        .join("preferences.json"),
                );
                // Before the theme and the first layout: text sizes and the
                // viewport depend on them.
                appearance::apply_font_size(appearance::FONT_SIZE.load(&prefs));
                appearance::apply_scale(appearance::SCALE.load(&prefs), window, cx);
                let saved = host.state::<crate::settings::SettingsStore>().get();
                theme::apply(
                    if crate::platform::is_dark(saved.theme) {
                        ThemeMode::Dark
                    } else {
                        ThemeMode::Light
                    },
                    window,
                    cx,
                );
                scrollbar::style_library(crate::platform::is_dark(saved.theme), cx);
                let view =
                    cx.new(|cx| Desktop::new(host.clone(), options.clone(), prefs, window, cx));
                // Apply platform integration after the component root is constructed.
                window.defer(cx, move |window, cx| {
                    crate::platform::attach_window(&host, &options, window, cx);
                });
                let placements = view.read(cx).chart_placements.clone();
                let clock = view.read(cx).chart_clock.clone();
                let active = view.read(cx).active_charts.clone();
                let opacity = view.read(cx).page_opacity.clone();
                let shift = view.read(cx).page_shift.clone();
                let pane = view.read(cx).pane_motion.clone();
                let charts = cx.new(|_| {
                    charts::ChartLayer::new(placements, clock, active, opacity, shift, pane)
                });
                view.update(cx, |desktop, _| desktop.chart_layer = Some(charts.clone()));
                let state = view.read(cx).scrim.clone();
                let scrim = cx.new(|_| dialogs::Scrim { state });
                view.update(cx, |desktop, _| desktop.scrim_view = Some(scrim.clone()));
                cx.new(|_| DesktopRoot {
                    desktop: view,
                    charts,
                    scrim,
                })
            })
            .expect("create native window");
            cx.on_app_quit(move |_| {
                owner.exit(0);
                crate::window_state::flush();
                // Preferences are written in the background: finish before exit.
                prefs::flush(Duration::from_secs(2));
                async {}
            })
            .detach();
        });
    Ok(())
}
/// Writes the interface preferences still queued (they are saved in the
/// background), for exits that skip the application quit handlers.
pub(crate) fn flush_preferences() {
    prefs::flush(Duration::from_secs(2));
}
/// Opt-in record of UI-thread work outside drawing, for the frame trace
/// (`ARGUS_GPUI_FRAME_TRACE`): what ran and for how long, on the probe's clock.
fn trace_work(event: &str, detail: Option<&str>, started: Instant) {
    use gpui_kit::gpui::profiler;
    if profiler::phase_trace_enabled() {
        let us = started.elapsed().as_micros();
        if us >= 300 {
            println!(
                "{}",
                json!({"event":"ui-work-slice","what":event,"detail":detail,"us":us,"atQpc":profiler::trace_clock_100ns()})
            );
        }
    }
}
struct DesktopRoot {
    desktop: Entity<Desktop>,
    charts: Entity<charts::ChartLayer>,
    scrim: Entity<dialogs::Scrim>,
}

impl Render for DesktopRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .child(
                self.desktop
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
            .child(self.charts.clone())
            .child(self.scrim.clone())
    }
}
