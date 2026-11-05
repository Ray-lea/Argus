//! Time-series adapter for GPUI's native plot primitives. Unknown samples remain gaps.
use super::*;
mod raster;
mod rate;
pub(in crate::frontend) use raster::paint_fade;
pub(in crate::frontend) use rate::{
    RateScale, ceiling as rate_ceiling, label as rate_ceiling_label,
};
pub(super) fn raster_metrics() -> (u64, u64, usize) {
    raster::metrics()
}
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::plot::{
    Plot, PlotElement,
    tooltip::{Tooltip, TooltipState},
};
use gpui_kit::gpui::Axis;
#[derive(Clone, Copy)]
pub(super) enum ChartHeight {
    Fixed(Pixels),
    Fill,
}
impl From<Pixels> for ChartHeight {
    fn from(value: Pixels) -> Self {
        Self::Fixed(value)
    }
}

#[derive(Clone)]
pub struct Series {
    pub name: String,
    pub points: Arc<Vec<(f64, Option<f64>)>>,
    pub color: Hsla,
    pub format: MetricFormat,
    /// A secondary series: dashed line, usually without an area fill.
    pub dashed: bool,
    /// Area fill opacity at the chart's top, fading to nothing at its bottom.
    pub fill: f32,
    geometry: Arc<Vec<Vec<(f64, f64)>>>,
    geometry_key: u64,
    /// Identifies the data shown (series key and transform); 0 when unnamed.
    source: u64,
}
/// Default area opacity under a solid line.
pub(in crate::frontend) const FILL: f32 = 0.3;
/// Opacity of a compact chart's secondary series: a solid line, as dashes
/// break up at preview sizes.
const COMPACT_SECONDARY: f32 = 0.55;
/// One series a chart draws, read from the live timeline.
#[derive(Clone, PartialEq)]
pub(in crate::frontend) struct Line {
    pub key: String,
    pub color: Hsla,
    pub dashed: bool,
    pub fill: f32,
    /// Name in the hover readout; single-series charts usually omit it.
    pub label: Option<String>,
    /// Multiplier applied to every sample, after taking the magnitude when `absolute`.
    pub factor: f64,
    pub absolute: bool,
}
impl Line {
    pub(in crate::frontend) fn new(key: impl Into<String>, color: Hsla) -> Self {
        Self {
            key: key.into(),
            color,
            dashed: false,
            fill: FILL,
            label: None,
            factor: 1.,
            absolute: false,
        }
    }
    /// Charts the magnitude of each sample times `factor` (a signed milliwatt rate as watts).
    pub(in crate::frontend) fn magnitude(mut self, factor: f64) -> Self {
        self.absolute = true;
        self.factor = factor;
        self
    }
    /// A secondary series in the same color: dashed and without fill.
    pub(in crate::frontend) fn dashed(mut self) -> Self {
        self.dashed = true;
        self.fill = 0.;
        self
    }
    pub(in crate::frontend) fn fill(mut self, fill: f32) -> Self {
        self.fill = fill;
        self
    }
    pub(in crate::frontend) fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}
/// How a chart's top is chosen.
#[derive(Clone, Copy, PartialEq)]
pub(in crate::frontend) enum Scale {
    Fixed(f64),
    /// A configured disk or network throughput step in the displayed unit.
    Rate(RateScale),
    /// A round number (watts and the like), at least `floor`, labelled in `unit`.
    Round {
        floor: f64,
        unit: &'static str,
    },
}
/// Captions, frame and readout around a chart.
#[derive(Clone, Default)]
pub(in crate::frontend) struct Look {
    /// Top-left caption; with it the chart shows captions above and below its frame.
    pub caption: Option<String>,
    /// Top-right scale label; derived from the scale when omitted.
    pub top: Option<String>,
    /// Border and grid lines.
    pub frame: bool,
    /// Hover readout.
    pub tooltip: bool,
    /// Readout format for every series.
    pub format: Option<MetricFormat>,
    /// Smallest height of a filling chart's frame.
    pub min_height: Option<Pixels>,
    /// Legend entries under the chart: label and whether its swatch is dashed.
    pub legend: Vec<(String, bool)>,
    /// What the chart layer draws over the curve.
    pub overlay: Option<Rc<Overlay>>,
}
/// What the chart layer draws over a curve, in the box around the plot (a
/// vital card): the chart layer draws curves over the page, so text that the
/// curve passes under is drawn here, after it. Painted when the layer draws
/// in full; the frames that only move curves keep it as painted.
pub(in crate::frontend) struct Overlay {
    /// Where the plot's edges sit inside the box.
    pub inset: Edges<Pixels>,
    /// How far across the box the overlay hides the curve: the curve is
    /// clipped there, so that Windows composes only what shows of it.
    pub covers: f32,
    pub build: Box<dyn Fn() -> AnyElement>,
}
impl Look {
    /// A framed chart with captions and a readout.
    pub(in crate::frontend) fn boxed(caption: impl Into<String>) -> Self {
        Self {
            caption: Some(caption.into()),
            frame: true,
            tooltip: true,
            ..Self::default()
        }
    }
    pub(in crate::frontend) fn top(mut self, top: impl Into<String>) -> Self {
        self.top = Some(top.into());
        self
    }
    pub(in crate::frontend) fn format(mut self, format: MetricFormat) -> Self {
        self.format = Some(format);
        self
    }
    pub(in crate::frontend) fn min_height(mut self, height: Pixels) -> Self {
        self.min_height = Some(height);
        self
    }
    pub(in crate::frontend) fn legend(mut self, entries: Vec<(String, bool)>) -> Self {
        self.legend = entries;
        self
    }
}
impl Series {
    pub(in crate::frontend) fn dashed(mut self, dashed: bool) -> Self {
        self.dashed = dashed;
        self
    }
    pub(in crate::frontend) fn fill(mut self, fill: f32) -> Self {
        self.fill = fill;
        self
    }
    pub(in crate::frontend) fn new(
        name: String,
        points: Vec<(f64, Option<f64>)>,
        color: Hsla,
        format: MetricFormat,
    ) -> Self {
        let geometry = Arc::new(segments(&points));
        let geometry_key = raster::geometry_key(&geometry);
        Self {
            name,
            points: Arc::new(points),
            color,
            format,
            dashed: false,
            fill: FILL,
            geometry,
            geometry_key,
            source: 0,
        }
    }
    /// Names the data shown: a texture drawn for other data is never shown for it.
    pub(in crate::frontend) fn source(mut self, source: u64) -> Self {
        self.source = source;
        self
    }
}
#[derive(Clone, Copy, PartialEq)]
pub enum MetricFormat {
    Percent,
    /// Whole percent, as the performance charts read.
    RoundPercent,
    Memory,
    /// Gigabytes with one decimal, "12.3 GB".
    Gigabytes,
    Rate,
    /// A byte rate with its unit always shown.
    ByteRate,
    Count,
    Network(bool),
    /// A network rate in the chosen unit, then in the other one.
    NetworkAlt(bool),
    /// Watts: one decimal below ten, whole above.
    Watts,
    /// Whole degrees Celsius.
    Celsius,
    Unit(&'static str, usize),
}
impl MetricFormat {
    pub(in crate::frontend) fn value(self, value: f64) -> String {
        match self {
            Self::Percent => format!("{}%", theme::cpu(value)),
            Self::RoundPercent => format!("{}%", value.round()),
            Self::Memory => theme::memory(value),
            Self::Gigabytes => format!("{:.1} GB", value / GIB),
            Self::Rate => theme::bytes(value, true),
            Self::ByteRate => byte_rate(value),
            Self::Count => theme::count(value),
            Self::Network(bits) => theme::network(value, bits),
            Self::NetworkAlt(bits) => tr!(
                "performance.withAlt",
                value = net_rate(value, bits),
                alt = net_rate(value, !bits)
            ),
            Self::Watts => format!("{} W", watts(value)),
            Self::Celsius => format!("{} °C", value.round()),
            Self::Unit(unit, digits) => format!("{value:.digits$} {unit}"),
        }
    }
}
pub(in crate::frontend) const KIB: f64 = 1024.;
pub(in crate::frontend) const MIB: f64 = KIB * 1024.;
pub(in crate::frontend) const GIB: f64 = MIB * 1024.;
/// Watts with sensible precision: 7.4, 78, and the sub-watt readings of an
/// idle integrated GPU kept: 0.012.
pub(in crate::frontend) fn watts(value: f64) -> String {
    if value > 0. && value < 1. {
        format!("{value:.3}")
    } else {
        watts_tenths(value)
    }
}
/// Watts to a tenth below 10 W, as the history page shows them: 7.4, 78.
pub(in crate::frontend) fn watts_tenths(value: f64) -> String {
    if value < 10. {
        format!("{value:.1}")
    } else {
        format!("{}", value.round())
    }
}
/// A byte rate with its unit always shown: "0 KB/s", "312 KB/s", "1.2 MB/s".
pub(in crate::frontend) fn byte_rate(bytes: f64) -> String {
    if bytes < 1. {
        "0 KB/s".into()
    } else if bytes < MIB {
        format!("{} KB/s", (bytes / KIB).round().max(1.))
    } else if bytes < GIB {
        format!("{:.1} MB/s", bytes / MIB)
    } else {
        format!("{:.2} GB/s", bytes / GIB)
    }
}
/// A rate in bits per second, the unit links and broadband plans are sold in.
pub(in crate::frontend) fn bit_rate(bytes: f64) -> String {
    let bits = bytes * 8.;
    if bits < 1. {
        "0 Kbps".into()
    } else if bits < 1e6 {
        format!("{:.*} Kbps", if bits < 1e4 { 1 } else { 0 }, bits / 1e3)
    } else if bits < 1e9 {
        format!("{:.*} Mbps", if bits < 1e7 { 2 } else { 1 }, bits / 1e6)
    } else {
        format!("{:.2} Gbps", bits / 1e9)
    }
}
/// A network rate in the chosen unit.
pub(in crate::frontend) fn net_rate(bytes: f64, bits: bool) -> String {
    if bits {
        bit_rate(bytes)
    } else {
        byte_rate(bytes)
    }
}
/// Floating-point slack, so 30.000000000004 rounds to 30, not 40.
const EPSILON: f64 = 1e-9;
/// The least whole number at or above `x`, at least 1; above ten, the least multiple of ten.
pub(in crate::frontend) fn round_ceiling(x: f64) -> f64 {
    if x.is_nan() || x <= 1. {
        return 1.;
    }
    let whole = (x - EPSILON).ceil();
    if whole <= 10. {
        whole
    } else {
        (x / 10. - EPSILON).ceil() * 10.
    }
}
/// The time a live chart spans at the current refresh interval: "60 s", "2 min".
pub(in crate::frontend) fn span_label(interval: u64) -> String {
    let seconds = motion::config()["chartSamples"]
        .as_f64()
        .expect("chart samples")
        * interval as f64
        / 1000.;
    if seconds >= 120. {
        tr!("performance.span.minutes", count = seconds / 60.)
    } else {
        tr!("performance.span.seconds", count = seconds)
    }
}
/// Seconds as the hover readout shows them: tenths below ten.
fn ago(seconds: f64) -> String {
    if seconds < 10. {
        let tenths = (seconds * 10.).round() / 10.;
        if tenths.fract() == 0. {
            format!("{tenths:.0}")
        } else {
            format!("{tenths:.1}")
        }
    } else {
        format!("{}", seconds.round())
    }
}
#[derive(Clone)]
pub struct TimePlot {
    pub id: SharedString,
    pub series: Arc<Vec<Series>>,
    pub start: f64,
    pub end: f64,
    pub max: f64,
    pub min: f64,
    pub interactive: bool,
    /// Grid lines in the first series' color.
    pub grid: bool,
    pub fill: bool,
    pub(in crate::frontend) background_separate: bool,
    /// Smooth monotone curves; history charts join their points with straight lines.
    pub smooth: bool,
    /// Vertical grid lines at these times instead of even columns (history's clock ticks).
    pub ticks: Option<Arc<Vec<f64>>>,
    pub geometry_start: f64,
    pub measured_width: Rc<Cell<f32>>,
    pub live_clock: Option<Rc<RefCell<motion::Clock>>>,
    pub image_surface: Rc<RefCell<raster::Surface>>,
    /// Dash phase per series and sample time, so dashes keep their place as the curve scrolls.
    pub dash_phases: raster::DashPhases,
    /// A new top eases in: the top shown before, the target, and when the change began.
    pub rescale: Rc<Cell<Rescale>>,
    /// When the hover readout's text last read the curve, kept from sample to sample.
    pub readout: Rc<Cell<ReadoutClock>>,
    /// Captions drawn inside the frame, over the curve (set by the chart layer).
    pub(in crate::frontend) captions: Option<Rc<FrameCaptions>>,
}
/// How often a hover readout's text follows a curve gliding under a resting
/// pointer, as the interface's text timer: its dots ride the curve on every
/// frame, while the text reads it ten times a second, and at once when a
/// sample arrives or the pointer moves.
const READOUT_EVERY: Duration = Duration::from_millis(100);
/// When a live chart's hover readout last read the curve, and for what.
#[derive(Clone, Copy, Default)]
pub(in crate::frontend) struct ReadoutClock {
    /// The pointer's x, the drawing's first time (a new one for every
    /// sample) and the chart mode the text was read for.
    key: Option<(u32, u64, motion::Mode)>,
    /// The window end the text was read at, and when.
    end: f64,
    read: Option<Instant>,
}
/// A chart's captions drawn inside its frame, over the curve and under the
/// readout in performance cards at most 640 px high, where the chart box
/// keeps only its frame.
pub(in crate::frontend) struct FrameCaptions {
    /// Name and top value, at the frame's top.
    top: (String, String),
    /// Time span and zero, at its bottom.
    bottom: (String, String),
    color: Hsla,
}
/// Where frame captions sit: the interface's 7 px from the chart box's sides,
/// 4 px from its top and 3 px from its bottom, less the frame's 1 px border
/// (the plot's bounds are inside it).
const CAPTION_INSET_X: f32 = 6.;
const CAPTION_INSET_TOP: f32 = 3.;
const CAPTION_INSET_BOTTOM: f32 = 2.;
impl FrameCaptions {
    /// The two caption rows, laid out across `bounds` (the plot's).
    fn rows(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) -> Vec<AnyElement> {
        let width = (bounds.size.width - px(2. * CAPTION_INSET_X)).max(px(0.));
        let family = cx.theme().font_family.clone();
        [(&self.top, true), (&self.bottom, false)]
            .into_iter()
            .map(|((left, right), top)| {
                // As the captions above and below a frame, at 11 instead of 12.
                let mut row = div()
                    .w(width)
                    .flex()
                    .justify_between()
                    .gap_3()
                    .font_family(family.clone())
                    .text_size(theme::font(11.))
                    .line_height(relative(1.4))
                    .text_color(self.color)
                    .child(div().min_w_0().truncate().child(left.clone()))
                    .child(div().flex_shrink_0().child(right.clone()))
                    .into_any_element();
                let height = row
                    .layout_as_root(
                        size(AvailableSpace::Definite(width), AvailableSpace::MinContent),
                        window,
                        cx,
                    )
                    .height;
                let y = if top {
                    bounds.top() + px(CAPTION_INSET_TOP)
                } else {
                    bounds.bottom() - px(CAPTION_INSET_BOTTOM) - height
                };
                row.prepaint_at(point(bounds.left() + px(CAPTION_INSET_X), y), window, cx);
                row
            })
            .collect()
    }
}
pub(in crate::frontend) fn hash_of(value: impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
fn content(series: &[Series]) -> u64 {
    hash_of(series.iter().map(|s| s.source).collect::<Vec<_>>())
}
/// How long a chart takes to ease to a new top.
const RESCALE: Duration = Duration::from_millis(420);
#[derive(Clone, Copy, Default)]
pub(in crate::frontend) struct Rescale {
    from: f64,
    to: f64,
    start: Option<Instant>,
}
impl Rescale {
    /// The top on screen at `now`: geometric, easing out, with an eased scale transition.
    fn at(&self, now: Instant) -> Option<f64> {
        let start = self.start?;
        let progress = now.saturating_duration_since(start).as_secs_f64() / RESCALE.as_secs_f64();
        if progress >= 1. || self.from <= 0. || self.to <= 0. || self.from == self.to {
            return None;
        }
        let eased = 1. - (1. - progress.max(0.)).powi(3);
        Some(self.from * (self.to / self.from).powf(eased))
    }
}
impl TimePlot {
    /// Identifies the data the chart shows: a texture drawn for other series,
    /// such as the previously selected device's, is never shown.
    pub(in crate::frontend) fn content(&self) -> u64 {
        content(&self.series)
    }
    /// How far a texture drawn for `max` stretches to the top on screen: the
    /// easing top while a rescale runs, else the chart's own (a texture still
    /// waiting for its redraw after a scale change).
    pub(in crate::frontend) fn rescale_factor(&self, max: f64, now: Instant) -> f32 {
        let shown = self.rescale.get().at(now).unwrap_or(self.max);
        Some((max / shown) as f32)
            .filter(|k| k.is_finite() && *k > 0.)
            .unwrap_or(1.)
    }
}
pub(super) struct ChartEntry {
    plot: Rc<RefCell<TimePlot>>,
    revision: ChartRevision,
    /// The desktop render that last built the chart; a chart that skipped one
    /// was off screen and appears afresh.
    shown: u64,
}
#[derive(Clone)]
pub(super) struct Placement {
    bounds: Bounds<Pixels>,
    mask: Bounds<Pixels>,
    plot: Rc<RefCell<TimePlot>>,
    slide: Option<DeviceSlide>,
    /// Captions the chart layer draws inside the frame.
    captions: Option<Rc<FrameCaptions>>,
    overlay: Option<Rc<Overlay>>,
    /// The live transforms the page draws the chart and its surroundings
    /// inside, outermost first (`shift::live`): the chart layer draws the
    /// curve inside the same ones, so that it moves, scales and fades with
    /// its frame in every frame.
    lives: Vec<Live>,
}
impl Placement {
    /// Whether the chart layer draws this chart other than at its laid-out place.
    fn transformed(&self) -> bool {
        self.slide.is_some() || !self.lives.is_empty()
    }
}

/// The plot's frame and local clip move with the device. Its scroll viewport stays put.
#[derive(Clone)]
pub(super) struct DeviceSlide {
    pub(super) offset: Point<Pixels>,
    pub(super) viewport: Rc<Cell<Bounds<Pixels>>>,
}
pub(super) type Placements = Rc<RefCell<HashMap<String, Placement>>>;

/// A live transform the page draws a chart inside (`shift::live`), and the
/// point it is anchored at: in window coordinates, written when the page
/// prepaints, so that the chart layer, which paints later, reads this
/// layout's (the page replays its paint while the transform changes).
#[derive(Clone)]
pub(super) struct Live {
    pub(super) transform: LiveTransform,
    pub(super) anchor: Rc<Cell<Point<Pixels>>>,
}

/// Paints `f` inside `lives` (outermost first), their anchors moved by
/// `translation`: drawn inside a move by the same amount, so that a point
/// lands where the page draws it (`live(p) + translation`).
fn with_lives(
    lives: &[Live],
    translation: Point<Pixels>,
    window: &mut Window,
    f: impl FnOnce(&mut Window),
) {
    match lives.split_first() {
        None => f(window),
        Some((live, rest)) => {
            window.with_live_transform(&live.transform, live.anchor.get() + translation, |window| {
                with_lives(rest, translation, window, f)
            })
        }
    }
}

/// How the details pane is drawn while it opens or closes, away from where it
/// is laid out (`Desktop::details_pane`): its charts move and fade with it,
/// clipped to the pane's own clip.
pub(super) struct PaneMotion {
    /// The charts the details region drew when it last rendered.
    pub(super) charts: HashSet<String>,
    pub(super) offset: Point<Pixels>,
    pub(super) opacity: f32,
    /// Where the pane is drawn: where it is laid out, widened to the left
    /// while it is drawn wider (easing to a narrower width). It does not
    /// move with the offset.
    pub(super) clip: Option<Bounds<Pixels>>,
}
impl Default for PaneMotion {
    fn default() -> Self {
        Self {
            charts: HashSet::new(),
            offset: Point::default(),
            opacity: 1.,
            clip: None,
        }
    }
}

// One animation entity owns all plot surfaces. Layout still belongs to Desktop;
// chart frames reuse its measured rectangles and never build a flex subtree.
pub(super) struct ChartLayer {
    placements: Placements,
    clock: Rc<RefCell<motion::Clock>>,
    active: Rc<RefCell<HashSet<String>>>,
    /// Where the entering page is drawn, from where it is laid out (`shift.rs`).
    shift: Rc<Cell<Point<Pixels>>>,
    pane: Rc<RefCell<PaneMotion>>,
    /// Animations of live transforms, stepped before each frame (`shift::LiveMotions`).
    motions: Rc<super::shift::LiveMotions>,
    last_end: f64,
    pixel_ends: HashMap<String, f64>,
    pub(super) fast_frames: u64,
    pub(super) frozen_groups: HashSet<String>,
}
impl ChartLayer {
    pub(super) fn diagnostics(&self, window: &Window) -> Vec<Value> {
        let active = self.active.borrow();
        let placements = self.placements.borrow();
        let clock = self.clock.borrow();
        let now = clock.end(Instant::now());
        let mut plots = active.iter().filter_map(|id| {
            let placement = placements.get(id)?;
            let plot = placement.plot.borrow();
            let clip = placement.bounds.intersect(&placement.mask);
            let surface = plot.image_surface.borrow();
            Some(serde_json::json!({
                "chart":id,"transformed":placement.transformed(),"range":[plot.min,plot.max],
                "hovered":plot.interactive && clip.contains(&window.mouse_position()),
                "clip":[f32::from(clip.left()),f32::from(clip.top()),f32::from(clip.size.width),f32::from(clip.size.height)],
                "surface":surface.diagnostics(&plot, placement.bounds, plot_end(&plot, &clock, now))
            }))
        }).collect::<Vec<_>>();
        plots.sort_by(|a, b| a["chart"].as_str().cmp(&b["chart"].as_str()));
        plots
    }

    pub(super) fn new(
        placements: Placements,
        clock: Rc<RefCell<motion::Clock>>,
        active: Rc<RefCell<HashSet<String>>>,
        shift: Rc<Cell<Point<Pixels>>>,
        pane: Rc<RefCell<PaneMotion>>,
        motions: Rc<super::shift::LiveMotions>,
    ) -> Self {
        Self {
            pane,
            motions,
            placements,
            clock,
            active,
            shift,
            last_end: f64::NAN,
            pixel_ends: HashMap::new(),
            fast_frames: 0,
            frozen_groups: HashSet::new(),
        }
    }
    pub(super) fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let clock = self.clock.borrow();
        let now = clock.end(Instant::now());
        let changed = if clock.mode == motion::Mode::Pixel {
            let placements = self.placements.borrow();
            let mut changed = false;
            for key in self.active.borrow().iter() {
                if self.frozen_groups.contains(chart_group(key)) {
                    continue;
                }
                let Some(placement) = placements.get(key) else {
                    continue;
                };
                let plot = placement.plot.borrow();
                let end = plot_end(&plot, &clock, now);
                changed |= self.pixel_ends.insert(key.clone(), end) != Some(end);
            }
            changed
        } else {
            self.last_end != now
        };
        self.last_end = now;
        if changed {
            // Fast texture updates use layout coordinates. Repaint while the entire page moves.
            if self.shift.get() != Point::default() {
                cx.notify();
                return;
            }
            let placements = self.placements.borrow();
            let fast = self.active.borrow().iter().all(|key| {
                if self.frozen_groups.contains(chart_group(key)) {
                    return true;
                }
                let Some(placement) = placements.get(key) else {
                    return true;
                };
                // A surface update would place the curve at its laid-out
                // bounds, outside the live transforms it is drawn inside.
                if placement.transformed() {
                    return false;
                }
                let clip = placement.bounds.intersect(&placement.mask);
                if clip.is_empty() {
                    return true;
                }
                let plot = placement.plot.borrow();
                if plot.interactive && clip.contains(&window.mouse_position()) {
                    return false;
                }
                let end = plot_end(&plot, &clock, now);
                plot.image_surface
                    .borrow()
                    .translate(&plot, placement.bounds, end, window)
            });
            if fast {
                self.fast_frames = self.fast_frames.wrapping_add(1);
            } else {
                cx.notify();
            }
        }
    }
}
fn chart_group(id: &str) -> &'static str {
    if id.starts_with("vital-") {
        "vitals"
    } else if id.starts_with("details-") {
        "details"
    } else {
        "other"
    }
}
/// A chart the layer prepainted this frame, ready to paint.
struct Drawn {
    element: AnyElement,
    clip: Bounds<Pixels>,
    /// The plot again, for its grid (painted before every curve).
    background: TimePlot,
    bounds: Bounds<Pixels>,
    alpha: f32,
    lives: Vec<Live>,
    translation: Point<Pixels>,
    overlay: Option<DrawnOverlay>,
}
/// A chart's overlay, prepainted in the box around the plot.
struct DrawnOverlay {
    element: AnyElement,
    clip: Bounds<Pixels>,
}
fn plot_end(plot: &TimePlot, clock: &motion::Clock, now: f64) -> f64 {
    if clock.mode == motion::Mode::Pixel {
        let step = (plot.end - plot.start) / f64::from(plot.measured_width.get().max(1.));
        plot.geometry_start + ((now - plot.geometry_start) / step).floor() * step
    } else {
        now
    }
}
impl Render for ChartLayer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Every frame draws this layer: the live transforms take this frame's
        // values here, before the cached views that paint them replay.
        self.motions.tick(Instant::now(), cx);
        let motions = self.motions.clone();
        let placements = self.placements.clone();
        let clock = self.clock.clone();
        let active = self.active.clone();
        let page_shift = self.shift.clone();
        let pane = self.pane.clone();
        div().absolute().inset_0().child(
            canvas(
                move |bounds, window, cx| {
                    // A provisional layout can have a different plot size. Keep its
                    // raster out of the cache used by the first visible frame.
                    if window.is_layout_only() {
                        return Vec::new();
                    }
                    // A view may have started an animation in this frame.
                    if motions.active() {
                        window.request_animation_frame();
                    }
                    // Read the current offset after the cached desktop has rendered.
                    let shift = page_shift.get();
                    let pane = pane.borrow();
                    let placements = placements.borrow();
                    let clock = clock.borrow();
                    let now = clock.end(Instant::now());
                    let mut elements = Vec::new();
                    for key in active.borrow().iter() {
                        let Some(placement) = placements.get(key) else {
                            continue;
                        };
                        let (moved, mask, alpha) = if pane.charts.contains(key) {
                            // The chart's own clip (its frame) moves with the
                            // pane; the pane's clip stays where it is drawn.
                            let mask = placement.mask + pane.offset;
                            (
                                placement.bounds + pane.offset,
                                pane.clip.map_or(mask, |clip| mask.intersect(&clip)),
                                pane.opacity,
                            )
                        } else if let Some(slide) = &placement.slide {
                            (
                                placement.bounds + slide.offset,
                                (placement.mask + slide.offset).intersect(&slide.viewport.get()),
                                1.,
                            )
                        } else {
                            (placement.bounds, placement.mask, 1.)
                        };
                        let plot_bounds = moved + shift;
                        let mut clip = plot_bounds.intersect(&(mask + shift)).intersect(&bounds);
                        if let Some(overlay) = &placement.overlay {
                            let left = plot_bounds.left() - overlay.inset.left;
                            let width =
                                plot_bounds.size.width + overlay.inset.left + overlay.inset.right;
                            let hidden = (left + width * overlay.covers).min(clip.right());
                            if hidden > clip.left() {
                                clip = Bounds::from_corners(
                                    point(hidden, clip.top()),
                                    clip.bottom_right(),
                                );
                            }
                        }
                        if clip.size.width <= px(0.) || clip.size.height <= px(0.) {
                            continue;
                        }
                        // The live transforms' anchors move with the chart, as
                        // the page draws them inside the same move.
                        let translation = plot_bounds.origin - placement.bounds.origin;
                        let mut plot = placement.plot.borrow().clone();
                        plot.captions = placement.captions.clone();
                        let span = plot.end - plot.start;
                        plot.end = plot_end(&plot, &clock, now);
                        plot.start = plot.end - span;
                        plot.background_separate = true;
                        let background = plot.clone();
                        let mut element = div()
                            .size_full()
                            .opacity(alpha)
                            .child(plot)
                            .into_any_element();
                        window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                            element.layout_as_root(plot_bounds.size.into(), window, cx);
                            element.prepaint_at(plot_bounds.origin, window, cx);
                        });
                        let overlay = placement.overlay.as_ref().map(|overlay| {
                            let inset = overlay.inset;
                            let frame = Bounds::from_corners(
                                plot_bounds.origin - point(inset.left, inset.top),
                                plot_bounds.bottom_right() + point(inset.right, inset.bottom),
                            );
                            let clip = frame.intersect(&(mask + shift)).intersect(&bounds);
                            let mut element = div()
                                .size_full()
                                .opacity(alpha)
                                .child((overlay.build)())
                                .into_any_element();
                            window.with_content_mask(
                                Some(ContentMask { bounds: clip }),
                                |window| {
                                    element.layout_as_root(frame.size.into(), window, cx);
                                    element.prepaint_at(frame.origin, window, cx);
                                },
                            );
                            DrawnOverlay { element, clip }
                        });
                        elements.push(Drawn {
                            element,
                            clip,
                            background,
                            bounds: plot_bounds,
                            alpha,
                            lives: placement.lives.clone(),
                            translation,
                            overlay,
                        });
                    }
                    elements
                },
                |bounds, elements, window, cx| {
                    // Grids are static, even though their data moves.
                    // Paint every backdrop before the shared animated layer.
                    for drawn in &elements {
                        if !drawn.background.background_separate {
                            continue;
                        }
                        with_lives(&drawn.lives, drawn.translation, window, |window| {
                            window.with_content_mask(
                                Some(ContentMask { bounds: drawn.clip }),
                                |window| {
                                    drawn.background.paint_base(
                                        drawn.bounds,
                                        drawn.alpha,
                                        window,
                                        cx,
                                    )
                                },
                            )
                        });
                    }
                    let mut overlays = Vec::new();
                    window.paint_layer(bounds, |window| {
                        for mut drawn in elements {
                            with_lives(&drawn.lives, drawn.translation, window, |window| {
                                window.with_content_mask(
                                    Some(ContentMask { bounds: drawn.clip }),
                                    |window| drawn.element.paint(window, cx),
                                )
                            });
                            if let Some(overlay) = drawn.overlay {
                                overlays.push((overlay, drawn.lives, drawn.translation));
                            }
                        }
                    });
                    // Outside the curves' layer, so that they take a later
                    // draw order than the curves they cover. Windows composes
                    // the moving curves apart only while what stays put is
                    // drawn over all that moves under it.
                    for (mut overlay, lives, translation) in overlays {
                        with_lives(&lives, translation, window, |window| {
                            window.with_content_mask(
                                Some(ContentMask {
                                    bounds: overlay.clip,
                                }),
                                |window| overlay.element.paint(window, cx),
                            )
                        });
                    }
                },
            )
            .size_full(),
        )
    }
}

#[derive(PartialEq)]
struct ChartRevision {
    sequence: u64,
    lines: Vec<Line>,
    scale: Option<Scale>,
    interval: u64,
    locale: argus_core::i18n::Locale,
    mode: motion::Mode,
    paused: bool,
    interactive: bool,
    grid: bool,
    format: Option<MetricFormat>,
    network_bits: bool,
}
/// Grid lines and frames in the chart's own color.
const GRID_OPACITY: f32 = 0.13;
const FRAME_OPACITY: f32 = 0.45;
/// The top-right caption a scale implies.
fn scale_label(scale: Option<Scale>, max: f64) -> Option<String> {
    match scale? {
        Scale::Rate(rate) => Some(rate_ceiling_label(max, rate.bits())),
        Scale::Round { unit, .. } => Some(format!("{max} {unit}")),
        Scale::Fixed(_) => None,
    }
}
impl IntoElement for TimePlot {
    type Element = PlotElement<Self>;
    fn into_element(self) -> Self::Element {
        PlotElement::new(self)
    }
}
fn segments(points: &[(f64, Option<f64>)]) -> Vec<Vec<(f64, f64)>> {
    let mut runs = Vec::new();
    let mut run = Vec::new();
    for &(at, value) in points {
        if let Some(value) = value.filter(|v| v.is_finite()) {
            run.push((at, value));
        } else if !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}
impl TimePlot {
    /// Grid lines, at `alpha` of their own opacity.
    fn paint_base(&self, bounds: Bounds<Pixels>, alpha: f32, window: &mut Window, cx: &mut App) {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        if self.grid {
            let color = self
                .series
                .first()
                .map(|s| s.color)
                .unwrap_or(cx.theme().border);
            if let Some(ticks) = &self.ticks {
                // Quarters across, the clock ticks down.
                let line = color.opacity(GRID_OPACITY * alpha);
                for fraction in [0.25, 0.5, 0.75] {
                    window.paint_rule(
                        Axis::Horizontal,
                        Bounds::new(
                            point(
                                bounds.left(),
                                bounds.top() + px((height * fraction).round()),
                            ),
                            size(bounds.size.width, px(1.)),
                        ),
                        line,
                    );
                }
                let span = (self.end - self.start).max(1.);
                for tick in ticks.iter() {
                    let x = ((tick - self.start) / span) as f32 * width;
                    if (0. ..=width).contains(&x) {
                        window.paint_rule(
                            Axis::Vertical,
                            Bounds::new(
                                point(bounds.left() + px(x.round()), bounds.top()),
                                size(px(1.), bounds.size.height),
                            ),
                            line,
                        );
                    }
                }
                return;
            }
            for (count, vertical) in [
                (
                    theme::design()["layout"]["chartGridRows"].as_u64().unwrap(),
                    false,
                ),
                (
                    theme::design()["layout"]["chartGridColumns"]
                        .as_u64()
                        .unwrap(),
                    true,
                ),
            ] {
                // The frame already draws the four outside edges.
                for index in 1..count {
                    let fraction = index as f32 / count as f32;
                    let origin = if vertical {
                        point(bounds.left() + px(width * fraction), bounds.top())
                    } else {
                        point(bounds.left(), bounds.top() + px(height * fraction))
                    };
                    let size = if vertical {
                        size(px(1.), bounds.size.height)
                    } else {
                        size(bounds.size.width, px(1.))
                    };
                    window.paint_rule(
                        if vertical {
                            Axis::Vertical
                        } else {
                            Axis::Horizontal
                        },
                        Bounds::new(origin, size),
                        color.opacity(GRID_OPACITY * alpha),
                    );
                }
            }
        }
    }
}
impl Plot for TimePlot {
    fn prepaint(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        self.measured_width
            .set(f32::from(bounds.size.width) * window.scale_factor());
        raster::prepare(self, bounds, window, cx);
        // Laid out only when the chart layer draws in full; the frames that
        // just move the curve keep them as painted.
        match self.captions.clone() {
            Some(captions) => captions.rows(bounds, window, cx),
            None => vec![],
        }
    }
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone().into())
    }
    fn interactive(&self) -> bool {
        self.interactive
    }
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        if !self.background_separate {
            self.paint_base(bounds, 1., window, cx);
        }
        raster::paint(self, bounds, window, cx);
    }
    fn tooltip_state(
        &self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _cx: &App,
    ) -> Option<TooltipState> {
        let (start, _, span) = self.live_window();
        let width = f32::from(bounds.size.width).max(1.);
        let at = start + (f32::from(position.x) / width) as f64 * span;
        let dots = self
            .series
            .iter()
            .filter_map(|s| {
                curve_at(&s.points, at).map(|v| point(position.x, self.y_of(v, bounds)))
            })
            .collect::<Vec<_>>();
        if dots.is_empty() {
            return None;
        }
        Some(TooltipState::new(
            f32::from(position.x).max(0.) as usize,
            point(position.x, position.y),
            dots,
        ))
    }
    fn tooltip(
        &self,
        _state: &TooltipState,
        cursor: Point<Pixels>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let (start, end, span) = self.live_window();
        let width = f32::from(bounds.size.width).max(1.);
        let x = cursor.x.clamp(px(0.), bounds.size.width);
        // The cursor line is laid out at `x`, snapped to a device pixel and a
        // device pixel or so wide (rounding as layout does); the curve is read
        // at its middle, where the dots sit.
        let scale = window.scale_factor();
        let device = |v: f32| (v - 0.5).ceil();
        let line = (device(f32::from(x) * scale) + device(scale).max(1.) / 2.) / scale;
        let at = start + (line / width) as f64 * span;
        let read = |at: f64| {
            self.series
                .iter()
                .filter_map(|s| curve_at(&s.points, at).map(|v| (s, v)))
                .collect::<Vec<_>>()
        };
        // The dots sit on the curve as drawn this frame; the text reads it on
        // the readout's own clock (see `readout_end`).
        let dots = read(at);
        let mut end_read = self.readout_end(x, end, false);
        let mut rows = read(at + (end_read - end));
        if rows.is_empty() && end_read != end {
            // The curve under the cursor only just began: read it now.
            end_read = self.readout_end(x, end, true);
            rows = read(at);
        }
        if rows.is_empty() || dots.is_empty() {
            return None;
        }
        let (end, at) = (end_read, at + (end_read - end));
        // Age counted from now: a moving trace keeps its age at a fixed point.
        let reference = match &self.live_clock {
            Some(clock) if clock.borrow().mode != motion::Mode::Off => {
                let interval = span
                    / (motion::config()["chartSamples"]
                        .as_f64()
                        .expect("chart samples")
                        - 1.)
                        .max(1.);
                end + interval * motion::config()["chartLagIntervals"].as_f64().expect("lag")
                    + motion::config()["chartLagSlackMs"]
                        .as_f64()
                        .expect("lag slack")
            }
            _ => end,
        };
        let age = ago(((reference - at) / 1000.).max(0.));
        let theme = cx.theme();
        let ink = theme.foreground;
        let muted = theme.muted_foreground;
        let palette = Palette::current(cx);
        let flip = x > bounds.size.width / 2.;
        let mut tip = div()
            .absolute()
            .top(px(6.))
            .map(|d| {
                if flip {
                    d.right(bounds.size.width - x + px(10.))
                } else {
                    d.left(x + px(10.))
                }
            })
            .px(px(9.))
            .py(px(5.))
            .border_1()
            .border_color(palette.stroke_strong)
            .rounded(px(6.))
            .bg(palette.tip)
            .text_size(theme::font(12.))
            .whitespace_nowrap()
            .flex()
            .flex_col()
            .child(div().text_color(muted).child(if age == "0" {
                label("performance.chart.latest")
            } else {
                tr!("performance.chart.ago", seconds = age)
            }));
        // The dots sit exactly on the curve as this frame draws it: at the
        // height the texture places it (`ImageSurface::curve_y`, read in
        // paint, after the plot placed its texture), at fractional pixels.
        let plot = self.clone();
        let ring = palette.tip;
        let marks = dots
            .into_iter()
            .map(|(series, value)| (series.color, value))
            .collect::<Vec<_>>();
        let dots = canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let surface = plot.image_surface.borrow();
                for &(color, value) in &marks {
                    let y = match &surface.image {
                        Some(surface) => surface.curve_y(&plot, bounds, value),
                        None => bounds.top() + plot.y_of(value, bounds),
                    };
                    raster::paint_dot(window, point(bounds.left() + px(line), y), color, ring);
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        for (series, value) in rows {
            tip = tip.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(swatch(series.color, series.dashed, px(12.)))
                    .child(div().text_color(ink).child(series.name.clone()))
                    .child(
                        div()
                            .ml_auto()
                            .pl(px(8.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ink)
                            .child(series.format.value(value)),
                    ),
            );
        }
        let overlay = Tooltip::new(cursor, bounds.size)
            .appearance(false)
            .glide(false)
            .child(
                div()
                    .absolute()
                    .left(x)
                    .top_0()
                    .w(px(1.))
                    .h(bounds.size.height)
                    .bg(ink.opacity(0.3)),
            )
            .child(dots)
            .child(tip);
        Some(overlay.into_any_element())
    }
}
impl TimePlot {
    /// The window on screen in this frame: the chart layer moves a live
    /// chart's window to the scroll clock before drawing it, and its texture
    /// is placed by that same window, so the readout reads the curve where it
    /// is drawn (reading the clock again here could land on the next pixel
    /// step).
    fn live_window(&self) -> (f64, f64, f64) {
        let span = (self.end - self.start).max(1.);
        (self.end - span, self.end, span)
    }
    /// The window end the hover readout's text reads the curve at, for the
    /// pointer at `x` with the window ending at `end` this frame. While the
    /// curve glides (fluid and balanced) the text holds for `READOUT_EVERY`
    /// unless the pointer moves, a sample arrives or the mode changes.
    /// Pixel and off modes move the curve a pixel or a sample at a time, so the
    /// text follows each step, as there. `fresh` reads anew regardless.
    fn readout_end(&self, x: Pixels, end: f64, fresh: bool) -> f64 {
        let Some(clock) = &self.live_clock else {
            return end;
        };
        let mode = clock.borrow().mode;
        // The layer moves `start` and `end` on every frame; the drawing's
        // first time changes only with the data.
        let key = (f32::from(x).to_bits(), self.geometry_start.to_bits(), mode);
        let now = Instant::now();
        let shown = self.readout.get();
        let held = !fresh
            && matches!(mode, motion::Mode::Fluid | motion::Mode::Balanced)
            && shown.key == Some(key)
            && shown
                .read
                .is_some_and(|read| now.saturating_duration_since(read) < READOUT_EVERY);
        if held {
            return shown.end;
        }
        self.readout.set(ReadoutClock {
            key: Some(key),
            end,
            read: Some(now),
        });
        end
    }
    /// The height a value is drawn at, as the raster lays it out.
    fn y_of(&self, value: f64, bounds: Bounds<Pixels>) -> Pixels {
        let height = f32::from(bounds.size.height);
        let stroke = f32::from(theme::dimension("chartStrokeWidth"));
        px(height
            - ((value - self.min) / (self.max - self.min).max(f64::EPSILON)) as f32
                * (height - stroke / 2.))
    }
}
/// A legend or readout swatch: a short line, dashed for a secondary series.
pub(in crate::frontend) fn swatch(color: Hsla, dashed: bool, width: Pixels) -> AnyElement {
    if !dashed {
        return div()
            .w(width)
            .h(px(2.))
            .bg(color)
            .flex_shrink_0()
            .into_any_element();
    }
    let dash = px(4.);
    let gap = px(3.);
    let count = ((f32::from(width) + f32::from(gap)) / f32::from(dash + gap)).ceil() as usize;
    div()
        .w(width)
        .h(px(2.))
        .flex()
        .gap(gap)
        .overflow_hidden()
        .flex_shrink_0()
        .children((0..count).map(|_| div().w(dash).h_full().flex_shrink_0().bg(color)))
        .into_any_element()
}
impl Desktop {
    pub(in crate::frontend) fn chart_bounds(&self, id: &str) -> Option<Bounds<Pixels>> {
        self.chart_placements
            .borrow()
            .get(id)
            .map(|placement| placement.bounds)
    }
    fn chart_frame(
        &self,
        plot_rc: Rc<RefCell<TimePlot>>,
        height: ChartHeight,
        look: &Look,
    ) -> AnyElement {
        let plot = plot_rc.borrow();
        let id = plot.id.to_string();
        let placements = self.chart_placements.clone();
        let placed_plot = plot_rc.clone();
        let slide = self.chart_slide.clone();
        let lives = self.chart_lives.borrow().clone();
        let overlay = look.overlay.clone();
        // The name and top above the frame, the span and zero below it; in a
        // short performance card, inside the frame (drawn by the chart layer).
        let captions = look.caption.as_ref().map(|caption| {
            let top = look.top.clone().unwrap_or_else(|| {
                look.format
                    .or_else(|| plot.series.first().map(|s| s.format))
                    .map(|format| format.value(plot.max))
                    .unwrap_or_default()
            });
            (
                (caption.clone(), top),
                (span_label(self.interval), "0".to_owned()),
            )
        });
        let (outside, inside) = match captions {
            Some((top, bottom)) if self.chart_captions_inside.get() => (
                None,
                Some(Rc::new(FrameCaptions {
                    top,
                    bottom,
                    color: self.palette.muted,
                })),
            ),
            captions => (captions, None),
        };
        let placeholder = canvas(
            move |bounds, window, _| {
                placements.borrow_mut().insert(
                    id.clone(),
                    Placement {
                        bounds,
                        mask: window.content_mask().bounds,
                        plot: placed_plot.clone(),
                        slide: slide.clone(),
                        captions: inside.clone(),
                        overlay: overlay.clone(),
                        lives: lives.clone(),
                    },
                );
            },
            |_, _, _, _| {},
        )
        .size_full();
        let color = plot
            .series
            .first()
            .map(|s| s.color)
            .unwrap_or(self.palette.accent);
        let flexible = !matches!(height, ChartHeight::Fixed(_));
        let caption_row = |(left, right): (String, String)| {
            div()
                .flex()
                .flex_shrink_0()
                .justify_between()
                .gap_3()
                .my(px(2.))
                .text_size(theme::font(12.))
                .text_color(self.palette.muted)
                .child(div().min_w_0().truncate().child(left))
                .child(div().flex_shrink_0().child(right))
        };
        let mut frame = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .when(flexible, |d| d.flex_1())
            .when(!flexible, |d| d.flex_shrink_0());
        let (top, bottom) = outside.unzip();
        if let Some(top) = top {
            frame = frame.child(caption_row(top));
        }
        frame = frame.child(
            div()
                .w_full()
                // An overlay is drawn over the box around the plot, which the
                // plot's own clip would cut off; the curve keeps to the plot.
                .when(look.overlay.is_none(), |d| d.overflow_hidden())
                .map(|d| match height {
                    ChartHeight::Fixed(height) => d.h(height),
                    ChartHeight::Fill => d.flex_1().min_h(
                        look.min_height
                            .unwrap_or(theme::dimension("chartMinHeight")),
                    ),
                })
                .map(|d| {
                    if look.frame {
                        // Put the opaque matte behind the antialiased border. The rectangular
                        // plot clip can include corner texels that a rounded fill leaves clear.
                        d.bg(self.palette.chart_surface).child(
                            div()
                                .size_full()
                                .overflow_hidden()
                                .border_1()
                                .border_color(color.opacity(FRAME_OPACITY))
                                .rounded(px(4.))
                                .child(placeholder),
                        )
                    } else {
                        d.child(placeholder)
                    }
                }),
        );
        if let Some(bottom) = bottom {
            frame = frame.child(caption_row(bottom));
        }
        if !look.legend.is_empty() {
            frame = frame.child(
                self.legend(
                    look.legend
                        .iter()
                        .map(|(name, dashed)| (name.clone(), color, *dashed))
                        .collect(),
                ),
            );
        }
        drop(plot);
        frame.into_any_element()
    }
    /// Lines under a chart naming its series, each with its stroke.
    pub(in crate::frontend) fn legend(&self, entries: Vec<(String, Hsla, bool)>) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .flex_shrink_0()
            .gap_x_4()
            .mt(px(6.))
            .text_size(theme::font(12.))
            .text_color(self.palette.muted)
            .children(entries.into_iter().map(|(name, color, dashed)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(swatch(color, dashed, px(14.)))
                    .child(name)
            }))
            .into_any_element()
    }
    /// A live chart with the legacy shorthand: solid series, the first one's name and scale as captions.
    pub(in crate::frontend) fn chart(
        &self,
        id: &str,
        keys: &[(&str, Hsla)],
        maximum: Option<f64>,
        height: impl Into<ChartHeight>,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let height = height.into();
        let axes = interactive
            && match height {
                ChartHeight::Fill => true,
                ChartHeight::Fixed(height) => height >= theme::dimension("chartMinHeight"),
            };
        let lines = keys
            .iter()
            .map(|(key, color)| Line::new(*key, *color).label(series_name(&self.snapshot, key)))
            .collect::<Vec<_>>();
        let look = Look {
            caption: axes.then(|| {
                lines
                    .first()
                    .and_then(|line| line.label.clone())
                    .unwrap_or_default()
            }),
            frame: interactive,
            tooltip: interactive,
            ..Look::default()
        };
        self.chart_view(id, &lines, maximum.map(Scale::Fixed), height, look, cx)
    }
    /// A live chart of `lines` read from the timeline, scaled by `scale` (a round top when none).
    pub(in crate::frontend) fn chart_view(
        &self,
        id: &str,
        lines: &[Line],
        scale: Option<Scale>,
        height: impl Into<ChartHeight>,
        mut look: Look,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let height = height.into();
        let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
        // Dashes break up at preview sizes: a compact chart's secondary series is
        // a fainter solid line without fill.
        let compacted;
        let lines = if look.caption.is_none() && lines.iter().any(|l| l.dashed) {
            compacted = lines
                .iter()
                .cloned()
                .map(|mut line| {
                    if line.dashed {
                        line.dashed = false;
                        line.fill = 0.;
                        line.color = line.color.opacity(COMPACT_SECONDARY);
                    }
                    line
                })
                .collect::<Vec<_>>();
            &compacted[..]
        } else {
            lines
        };
        let revision = ChartRevision {
            sequence: self.sequence,
            lines: lines.to_vec(),
            scale,
            interval: self.interval,
            locale: argus_core::i18n::locale(),
            mode: self.chart_clock.borrow().mode,
            paused: self.paused || self.control_paused,
            interactive: look.tooltip,
            grid: look.frame,
            format: look.format,
            network_bits: bits,
        };
        self.active_charts.borrow_mut().insert(id.into());
        let render = self.render_count;
        let mut entries = self.chart_entries.borrow_mut();
        // Built in the previous desktop render too: on screen without a break.
        let continuous = entries
            .get(id)
            .is_some_and(|entry| entry.shown.wrapping_add(1) >= render);
        if let Some(entry) = entries.get_mut(id) {
            entry.shown = render;
            if entry.revision == revision {
                let plot = entry.plot.clone();
                drop(entries);
                if look.top.is_none() {
                    look.top = scale_label(scale, plot.borrow().max);
                }
                return self.chart_frame(plot, height, &look);
            }
        }
        let end = number(&self.snapshot, "timestamp");
        let span = self.interval as f64
            * (motion::config()["chartSamples"]
                .as_f64()
                .expect("chart samples")
                - 1.);
        let start = end - span;
        let series = lines
            .iter()
            .map(|line| {
                let key = line.key.as_str();
                let stable = model::series_key(&self.snapshot, key);
                let source = hash_of((&stable, line.factor.to_bits(), line.absolute));
                let mut points = self.timeline.points(&stable).unwrap_or_default();
                if line.factor != 1. || line.absolute {
                    for (_, value) in &mut points {
                        *value =
                            value.map(|v| (if line.absolute { v.abs() } else { v }) * line.factor);
                    }
                }
                Series::new(
                    line.label.clone().unwrap_or_default(),
                    points,
                    line.color,
                    look.format.unwrap_or_else(|| {
                        if key.contains(".nics.") || key == "totals.network" {
                            MetricFormat::Network(bits)
                        } else {
                            series_format(key)
                        }
                    }),
                )
                .dashed(line.dashed)
                .fill(line.fill)
                .source(source)
            })
            .collect::<Vec<_>>();
        let content = content(&series);
        let observed = series
            .iter()
            .flat_map(|s| s.points.iter().filter_map(|(_, v)| *v))
            .filter(|v| v.is_finite())
            .fold(0f64, f64::max);
        let max = match scale {
            Some(Scale::Fixed(max)) => max.max(f64::EPSILON),
            Some(Scale::Rate(rate)) => rate_ceiling(observed, rate),
            Some(Scale::Round { floor, .. }) => round_ceiling(observed.max(floor)),
            None => nice_max(observed),
        };
        let min = series
            .iter()
            .flat_map(|s| s.points.iter().filter_map(|p| p.1))
            .fold(0., f64::min);
        // The same series again without a break: a new top eases in. Anything
        // else (a first display, another device, a return to the page) starts
        // at its own top.
        let previous_max = entries
            .get(id)
            .filter(|entry| continuous && entry.plot.borrow().content() == content)
            .map(|entry| entry.plot.borrow().max);
        let rescale = entries
            .get(id)
            .map(|entry| entry.plot.borrow().rescale.clone())
            .unwrap_or_default();
        let readout = entries
            .get(id)
            .map(|entry| entry.plot.borrow().readout.clone())
            .unwrap_or_default();
        let now = Instant::now();
        let moving = self.chart_clock.borrow().mode != motion::Mode::Off
            && !(self.paused || self.control_paused);
        match previous_max {
            Some(previous) if previous != max && moving => {
                let shown = rescale.get().at(now).unwrap_or(previous);
                rescale.set(Rescale {
                    from: shown,
                    to: max,
                    start: Some(now),
                });
            }
            Some(previous) if previous == max => {}
            _ => rescale.set(Rescale {
                from: max,
                to: max,
                start: None,
            }),
        }
        let (width, image_surface, dash_phases) = entries
            .get(id)
            .map(|entry| {
                let plot = entry.plot.borrow();
                (
                    plot.measured_width.clone(),
                    plot.image_surface.clone(),
                    // Dash phases belong to the series they were measured on.
                    if plot.content() == content {
                        plot.dash_phases.clone()
                    } else {
                        Default::default()
                    },
                )
            })
            .unwrap_or_else(|| {
                (
                    Rc::new(Cell::new(1.)),
                    Default::default(),
                    Default::default(),
                )
            });
        let plot = TimePlot {
            id: id.to_owned().into(),
            series: Arc::new(series),
            start,
            end,
            max,
            min,
            interactive: look.tooltip,
            grid: look.frame,
            fill: true,
            background_separate: false,
            smooth: true,
            ticks: None,
            geometry_start: start,
            measured_width: width,
            live_clock: Some(self.chart_clock.clone()),
            image_surface,
            dash_phases,
            rescale,
            readout,
            captions: None,
        };
        let plot_rc = if let Some(entry) = entries.get_mut(id) {
            *entry.plot.borrow_mut() = plot;
            entry.revision = revision;
            entry.plot.clone()
        } else {
            let plot_rc = Rc::new(RefCell::new(plot));
            entries.insert(
                id.into(),
                ChartEntry {
                    plot: plot_rc.clone(),
                    revision,
                    shown: render,
                },
            );
            plot_rc
        };
        drop(entries);
        if look.top.is_none() {
            look.top = scale_label(scale, max);
        }
        self.chart_frame(plot_rc, height, &look)
    }
    pub(in crate::frontend) fn apply_motion(&mut self, cx: &mut Context<Self>) {
        let system = crate::platform::system_motion();
        let motion = self
            .prefs
            .get::<Option<bool>>("motion.ui", None)
            .unwrap_or(system);
        cx.set_reduce_motion(!motion);
        // The window fades in and out in place with interface motion on.
        crate::window_motion::set_enabled(
            self.host
                .get_window("main")
                .and_then(|window| window.hwnd().ok()),
            motion,
        );
        let saved = self
            .prefs
            .get::<Option<String>>("motion.chartMode", None)
            .or_else(|| {
                (self.prefs.get::<Option<bool>>("motion.charts", None) == Some(false))
                    .then(|| "off".into())
            });
        self.chart_clock.borrow_mut().mode = motion::Mode::parse(
            saved
                .as_deref()
                .unwrap_or(if system { "fluid" } else { "off" }),
        );
        if self.options.fixture.is_some() && !self.fixture_samples {
            self.chart_clock.borrow_mut().set_paused(true);
        }
    }
    pub(in crate::frontend) fn schedule_chart_tick(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active = matches!(self.page.as_str(), "processes" | "performance")
            && self.modal.is_none()
            && (self.options.fixture.is_none() || self.fixture_samples)
            && self.host.get_window("main").is_some_and(|w| {
                w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)
            });
        if self.chart_tick_queued
            || !active
            || !self.chart_clock.borrow().moving()
            || self.active_charts.borrow().is_empty()
        {
            return;
        }
        self.chart_tick_queued = true;
        let owner = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.chart_tick_queued = false;
                this.chart_frame = this.chart_frame.wrapping_add(1);
                let mode = this.chart_clock.borrow().mode;
                let refresh =
                    motion::frame_period(motion::Mode::Fluid, window.refresh_interval(cx));
                let every = (motion::frame_period(mode, Some(refresh)).as_secs_f64()
                    / refresh.as_secs_f64())
                .round()
                .max(1.) as u64;
                if this.chart_frame.is_multiple_of(every) {
                    this.animate_charts(window, cx);
                }
                this.schedule_chart_tick(window, cx);
            });
        });
    }
    fn animate_charts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.chart_clock.borrow().moving() {
            return;
        }
        if let Some(layer) = &self.chart_layer {
            layer.update(cx, |layer, cx| layer.advance(window, cx));
        }
    }
}

/// The value the drawn curve passes through at `at`: the same monotone cubic the
/// raster strokes, so a readout's dot sits on the line. Unknown samples stay gaps.
fn curve_at(points: &[(f64, Option<f64>)], at: f64) -> Option<f64> {
    let known = |index: usize| {
        points
            .get(index)
            .and_then(|p| p.1.filter(|v| v.is_finite()).map(|v| (p.0, v)))
    };
    let right = points.partition_point(|p| p.0 < at);
    if right == 0 || right >= points.len() {
        return known(right).filter(|p| p.0 == at).map(|p| p.1);
    }
    let (before, after) = (known(right - 1)?, known(right)?);
    // Tangents depend only on neighbouring slopes, so four points reproduce the run's.
    let first = right.checked_sub(2).and_then(known);
    let mut window = Vec::with_capacity(4);
    window.extend(first);
    window.push(before);
    window.push(after);
    window.extend(known(right + 1));
    let tangent = curve_tangents(&window);
    let offset = usize::from(first.is_some());
    let span = (after.0 - before.0).max(f64::EPSILON);
    let t = ((at - before.0) / span).clamp(0., 1.);
    let (t2, t3) = (t * t, t * t * t);
    Some(
        (2. * t3 - 3. * t2 + 1.) * before.1
            + (t3 - 2. * t2 + t) * span * tangent[offset]
            + (-2. * t3 + 3. * t2) * after.1
            + (t3 - t2) * span * tangent[offset + 1],
    )
}
// Shape-preserving Hermite interpolation is prepared only when samples change.
// Native plot paths keep the original timestamps and never bridge unknown runs.
fn curve_tangents(points: &[(f64, f64)]) -> Vec<f64> {
    if points.len() < 2 {
        return vec![0.; points.len()];
    }
    let spans = points
        .windows(2)
        .map(|pair| (pair[1].0 - pair[0].0).max(f64::EPSILON))
        .collect::<Vec<_>>();
    let slopes = points
        .windows(2)
        .zip(&spans)
        .map(|(pair, span)| (pair[1].1 - pair[0].1) / span)
        .collect::<Vec<_>>();
    let mut tangent = vec![0.; points.len()];
    tangent[0] = slopes[0];
    tangent[points.len() - 1] = slopes[slopes.len() - 1];
    for index in 1..points.len() - 1 {
        if slopes[index - 1] * slopes[index] > 0. {
            let left = 2. * spans[index] + spans[index - 1];
            let right = spans[index] + 2. * spans[index - 1];
            tangent[index] = (left + right) / (left / slopes[index - 1] + right / slopes[index]);
        }
    }
    tangent
}

#[cfg(test)]
fn smooth_run(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let tangent = curve_tangents(points);
    let spans = points
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
        .collect::<Vec<_>>();
    let subdivisions = 12;
    let mut geometry = Vec::with_capacity((points.len() - 1) * subdivisions + 1);
    for index in 0..points.len() - 1 {
        let (x, y) = points[index];
        let (end_x, end_y) = points[index + 1];
        for step in 0..subdivisions {
            let t = step as f64 / subdivisions as f64;
            let t2 = t * t;
            let t3 = t2 * t;
            let value = (2. * t3 - 3. * t2 + 1.) * y
                + (t3 - 2. * t2 + t) * spans[index] * tangent[index]
                + (-2. * t3 + 3. * t2) * end_y
                + (t3 - t2) * spans[index] * tangent[index + 1];
            geometry.push((x + (end_x - x) * t, value.clamp(y.min(end_y), y.max(end_y))));
        }
    }
    geometry.push(*points.last().unwrap());
    geometry
}

pub(super) fn series_format(key: &str) -> MetricFormat {
    if key.starts_with("hardware.power.") || key.ends_with("Watts") {
        return MetricFormat::Unit("W", 1);
    }
    if key.ends_with("Celsius") {
        return MetricFormat::Unit("°C", 1);
    }
    let field = key.rsplit('.').next().unwrap_or(key);
    if key.contains(".engines.")
        || key.contains(".cores.")
        || matches!(
            field,
            "cpu" | "cpuKernel" | "util" | "percent" | "active" | "battery" | "gpu"
        )
    {
        MetricFormat::Percent
    } else if matches!(field, "mem" | "memUsed" | "dedicatedUsed" | "sharedUsed") {
        MetricFormat::Memory
    } else if matches!(
        field,
        "rx" | "tx" | "read" | "write" | "disk" | "network" | "net"
    ) {
        MetricFormat::Rate
    } else {
        MetricFormat::Count
    }
}
fn series_name(snapshot: &Value, key: &str) -> String {
    let field = key.rsplit('.').next().unwrap_or(key);
    let name = match field {
        "packageCelsius" => "common.resource.temperature",
        "packageWatts" | "package" => "performance.cpu.packagePower",
        "cores" if key.starts_with("hardware.power.") => "performance.cpu.corePower",
        "cpu" => "common.resource.cpu",
        "cpuKernel" => "performance.cpu.kernel",
        "mem" | "memUsed" => "common.resource.memory",
        "disk" => "common.resource.disk",
        "net" | "network" => "common.resource.network",
        "gpu" | "util" => "common.resource.gpu",
        "rx" => "performance.network.receive",
        "tx" => "performance.network.send",
        "read" => "performance.disk.read",
        "write" => "performance.disk.write",
        "active" => "performance.disk.activeTime",
        "percent" | "battery" => "performance.battery.level",
        "dedicatedUsed" => "performance.gpu.dedicatedMemory",
        "sharedUsed" => "performance.gpu.sharedMemory",
        _ => "",
    };
    if !name.is_empty() {
        return label(name);
    }
    let parts = key.split('.').collect::<Vec<_>>();
    if key.starts_with("system.cores.") {
        return tr!("performance.cpu.coreName", index = field);
    }
    if parts.len() == 5
        && parts[1] == "gpus"
        && parts[3] == "engines"
        && let (Ok(gpu), Ok(engine)) = (parts[2].parse::<usize>(), parts[4].parse::<usize>())
    {
        return snapshot["hardware"]["gpus"][gpu]["info"]["engines"][engine]
            .as_str()
            .unwrap_or_default()
            .into();
    }
    label("common.resource.cpu")
}
pub(in crate::frontend) fn nice_max(value: f64) -> f64 {
    if value <= 0. {
        return 1.;
    }
    let unit = 10f64.powf(value.log10().floor());
    (value / unit).ceil() * unit
}
#[cfg(test)]
mod tests {
    use super::{MetricFormat, READOUT_EVERY, Series, TimePlot, nice_max, segments, smooth_run};
    use crate::frontend::motion;
    use gpui_kit::gpui::{px, rgb};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    fn live_plot(clock: Rc<RefCell<motion::Clock>>) -> TimePlot {
        TimePlot {
            id: "readout-test".into(),
            series: vec![Series::new(
                String::new(),
                vec![(0., Some(30.)), (1000., Some(60.))],
                rgb(0x28b8e0).into(),
                MetricFormat::Percent,
            )]
            .into(),
            start: 0.,
            end: 1000.,
            max: 100.,
            min: 0.,
            interactive: true,
            grid: false,
            fill: false,
            background_separate: false,
            smooth: true,
            ticks: None,
            geometry_start: 0.,
            measured_width: Rc::new(Cell::new(100.)),
            live_clock: Some(clock),
            image_surface: Default::default(),
            dash_phases: Default::default(),
            rescale: Default::default(),
            readout: Default::default(),
            captions: None,
        }
    }
    #[test]
    fn readout_text_reads_a_gliding_curve_ten_times_a_second() {
        let clock = Rc::new(RefCell::new(motion::Clock::new(motion::Mode::Fluid)));
        let mut plot = live_plot(clock.clone());
        assert_eq!(plot.readout_end(px(40.), 1000., false), 1000.);
        // The next frames: the text still reads where it did.
        assert_eq!(plot.readout_end(px(40.), 1006., false), 1000.);
        // The pointer moves, or a sample arrives: it reads at once.
        assert_eq!(plot.readout_end(px(41.), 1012., false), 1012.);
        plot.geometry_start += 1000.;
        assert_eq!(plot.readout_end(px(41.), 1018., false), 1018.);
        assert_eq!(plot.readout_end(px(41.), 1024., true), 1024.);
        std::thread::sleep(READOUT_EVERY);
        assert_eq!(plot.readout_end(px(41.), 1130., false), 1130.);
        // A curve moving a pixel (or a sample) at a time is read at each step.
        clock.borrow_mut().mode = motion::Mode::Pixel;
        assert_eq!(plot.readout_end(px(41.), 1131., false), 1131.);
        assert_eq!(plot.readout_end(px(41.), 1132., false), 1132.);
    }
    #[test]
    fn smoothing_preserves_peaks_and_uneven_sample_times() {
        let points = [(0., 0.), (10., 100.), (37., 20.), (40., 20.), (100., 80.)];
        let curve = smooth_run(&points);
        for point in points {
            assert!(curve.contains(&point));
        }
        for (at, value) in curve {
            let right = points.partition_point(|p| p.0 < at).min(points.len() - 1);
            let left = right.saturating_sub(1);
            assert!(value >= points[left].1.min(points[right].1) - 1e-8);
            assert!(value <= points[left].1.max(points[right].1) + 1e-8);
        }
    }
    #[test]
    pub(in crate::frontend) fn unavailable_readings_split_the_line() {
        let runs = segments(&[(1., Some(2.)), (2., None), (3., Some(4.))]);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len(), 1);
        assert_eq!(runs[1].len(), 1);
    }
    #[test]
    pub(in crate::frontend) fn scale_never_clips_the_observed_peak() {
        for peak in [0., 0.2, 4., 11., 999., 1025.] {
            assert!(nice_max(peak) >= peak);
        }
    }
}
