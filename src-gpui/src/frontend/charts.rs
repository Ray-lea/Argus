//! Time-series adapter for GPUI's native plot primitives. Unknown samples remain gaps.
use super::*;
mod raster;
pub(super) fn raster_metrics() -> (u64, u64) {
    raster::metrics()
}
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::plot::{
    Plot, PlotElement,
    tooltip::{Tooltip, TooltipState},
};
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
}
/// Default area opacity under a solid line.
pub(in crate::frontend) const FILL: f32 = 0.3;
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
    /// A round rate in the unit its label shows, at least `floor` bytes per second.
    Rate {
        floor: f64,
        bits: bool,
    },
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
        Self::with_geometry(name, points, color, format, true)
    }
    pub(in crate::frontend) fn with_geometry(
        name: String,
        points: Vec<(f64, Option<f64>)>,
        color: Hsla,
        format: MetricFormat,
        compute_geometry: bool,
    ) -> Self {
        let geometry = Arc::new(if compute_geometry {
            segments(&points)
        } else {
            Vec::new()
        });
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
        }
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
/// Watts with sensible precision: 7.4, 78.
pub(in crate::frontend) fn watts(value: f64) -> String {
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
fn rate_units(bits: bool) -> [(f64, &'static str); 3] {
    if bits {
        [(1e3 / 8., "Kbps"), (1e6 / 8., "Mbps"), (1e9 / 8., "Gbps")]
    } else {
        [(KIB, "KB/s"), (MIB, "MB/s"), (GIB, "GB/s")]
    }
}
fn rate_unit_of(bytes: f64, units: &[(f64, &str)]) -> usize {
    let mut index = 0;
    while index + 1 < units.len() && bytes >= units[index + 1].0 * (1. - EPSILON) {
        index += 1;
    }
    index
}
/// A round top for a rate chart whose highest point is `peak` bytes per second.
pub(in crate::frontend) fn rate_ceiling(peak: f64, bits: bool) -> f64 {
    let units = rate_units(bits);
    let mut index = rate_unit_of(peak.max(0.), &units);
    loop {
        let top = round_ceiling(peak / units[index].0) * units[index].0;
        // Rounded up into the next unit: round there, "1 MB/s" rather than "1030 KB/s".
        if index + 1 < units.len() && top >= units[index + 1].0 * (1. - EPSILON) {
            index += 1;
            continue;
        }
        return top;
    }
}
/// A rate chart's top as its label shows it: "8 MB/s", "150 Mbps".
pub(in crate::frontend) fn rate_ceiling_label(top: f64, bits: bool) -> String {
    let units = rate_units(bits);
    let (size, unit) = units[rate_unit_of(top, &units)];
    format!("{} {unit}", (top / size).round())
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
    pub series: Vec<Series>,
    pub start: f64,
    pub end: f64,
    pub max: f64,
    pub min: f64,
    pub interactive: bool,
    /// Grid lines in the first series' color.
    pub grid: bool,
    pub fill: bool,
    pub bars: bool,
    pub(in crate::frontend) background_separate: bool,
    /// Smooth monotone curves; history charts join their points with straight lines.
    pub smooth: bool,
    /// Vertical grid lines at these times instead of even columns (history's clock ticks).
    pub ticks: Option<Arc<Vec<f64>>>,
    pub geometry_start: f64,
    pub measured_width: Rc<Cell<f32>>,
    pub live_clock: Option<Rc<RefCell<motion::Clock>>>,
    pub image_surface: Rc<RefCell<Option<raster::ImageSurface>>>,
    pub bar_surfaces: Rc<RefCell<Vec<BarSurface>>>,
    /// Dash phase per series and sample time, so dashes keep their place as the curve scrolls.
    pub dash_phases: raster::DashPhases,
    /// A new top eases in: the top shown before, the target, and when the change began.
    pub rescale: Rc<Cell<Rescale>>,
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
    /// The top on screen at `now`: geometric, easing out, as the original charts rescale.
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
    /// The top a texture drawn for `max` is stretched to while a rescale eases in.
    pub(in crate::frontend) fn rescale_factor(&self, max: f64, now: Instant) -> f32 {
        self.rescale
            .get()
            .at(now)
            .map(|shown| (max / shown) as f32)
            .filter(|k| k.is_finite() && *k > 0.)
            .unwrap_or(1.)
    }
}
pub(in crate::frontend) struct BarSurface {
    handle: Option<PaintedPrimitive>,
    bounds: Bounds<Pixels>,
}
pub(super) struct ChartEntry {
    plot: Rc<RefCell<TimePlot>>,
    revision: ChartRevision,
}
#[derive(Clone)]
pub(super) struct Placement {
    bounds: Bounds<Pixels>,
    mask: Bounds<Pixels>,
    plot: Rc<RefCell<TimePlot>>,
}
pub(super) type Placements = Rc<RefCell<HashMap<String, Placement>>>;

// One animation entity owns all plot surfaces. Layout still belongs to Desktop;
// chart frames reuse its measured rectangles and never build a flex subtree.
pub(super) struct ChartLayer {
    placements: Placements,
    clock: Rc<RefCell<motion::Clock>>,
    active: Rc<RefCell<HashSet<String>>>,
    opacity: Rc<Cell<f32>>,
    last_end: f64,
    pixel_ends: HashMap<String, f64>,
    pub(super) fast_frames: u64,
}
impl ChartLayer {
    pub(super) fn new(
        placements: Placements,
        clock: Rc<RefCell<motion::Clock>>,
        active: Rc<RefCell<HashSet<String>>>,
        opacity: Rc<Cell<f32>>,
    ) -> Self {
        Self {
            placements,
            clock,
            active,
            opacity,
            last_end: f64::NAN,
            pixel_ends: HashMap::new(),
            fast_frames: 0,
        }
    }
    pub(super) fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let clock = self.clock.borrow();
        let now = clock.end(Instant::now());
        let changed = if clock.mode == motion::Mode::Pixel {
            let placements = self.placements.borrow();
            let mut changed = false;
            for key in self.active.borrow().iter() {
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
            let placements = self.placements.borrow();
            let fast = self.active.borrow().iter().all(|key| {
                let Some(placement) = placements.get(key) else {
                    return true;
                };
                let clip = placement.bounds.intersect(&placement.mask);
                if clip.is_empty() {
                    return true;
                }
                let plot = placement.plot.borrow();
                if plot.interactive && clip.contains(&window.mouse_position()) {
                    return false;
                }
                let end = plot_end(&plot, &clock, now);
                if plot.bars {
                    let surfaces = plot.bar_surfaces.borrow();
                    if surfaces.len() != plot.series.len() {
                        return false;
                    }
                    surfaces.iter().zip(&plot.series).all(|(surface, series)| {
                        let Some(handle) = surface.handle else {
                            return false;
                        };
                        let value = sample_at(&series.points, end).unwrap_or(0.);
                        let height =
                            surface.bounds.size.height * (value / 100.).clamp(0., 1.) as f32;
                        let bounds = Bounds::new(
                            point(surface.bounds.left(), surface.bounds.bottom() - height),
                            size(surface.bounds.size.width, height),
                        );
                        window.update_painted_bounds(&handle, bounds)
                    })
                } else {
                    plot.image_surface.borrow().as_ref().is_some_and(|surface| {
                        surface.translate(&plot, placement.bounds, end, window)
                    })
                }
            });
            if fast {
                self.fast_frames = self.fast_frames.wrapping_add(1);
            } else {
                cx.notify();
            }
        }
    }
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
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let placements = self.placements.clone();
        let clock = self.clock.clone();
        let active = self.active.clone();
        div()
            .absolute()
            .inset_0()
            .opacity(self.opacity.get())
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let placements = placements.borrow();
                        let clock = clock.borrow();
                        let now = clock.end(Instant::now());
                        let mut elements = Vec::new();
                        for key in active.borrow().iter() {
                            let Some(placement) = placements.get(key) else {
                                continue;
                            };
                            let clip = placement
                                .bounds
                                .intersect(&placement.mask)
                                .intersect(&bounds);
                            if clip.size.width <= px(0.) || clip.size.height <= px(0.) {
                                continue;
                            }
                            let mut plot = placement.plot.borrow().clone();
                            let span = plot.end - plot.start;
                            plot.end = plot_end(&plot, &clock, now);
                            plot.start = plot.end - span;
                            let step = f32::from(placement.bounds.size.width)
                                / plot.series.len().max(1) as f32;
                            // Keep interleaved painting when densely packed bars could overlap after pixel snapping.
                            plot.background_separate = !plot.bars
                                || (step - f32::from(bar_width(step))) * window.scale_factor()
                                    >= 1.;
                            let background = plot.clone();
                            let mut element = plot.into_any_element();
                            window.with_content_mask(
                                Some(ContentMask { bounds: clip }),
                                |window| {
                                    element.layout_as_root(
                                        placement.bounds.size.into(),
                                        window,
                                        cx,
                                    );
                                    element.prepaint_at(placement.bounds.origin, window, cx);
                                },
                            );
                            elements.push((element, clip, background, placement.bounds));
                        }
                        elements
                    },
                    |bounds, elements, window, cx| {
                        // Grids and bar tracks are static, even though their data moves.
                        // Paint every backdrop before the shared animated layer.
                        for (_, clip, plot, bounds) in &elements {
                            if !plot.background_separate {
                                continue;
                            }
                            window
                                .with_content_mask(Some(ContentMask { bounds: *clip }), |window| {
                                    plot.paint_base(*bounds, window, cx)
                                });
                        }
                        window.paint_layer(bounds, |window| {
                            for (mut element, clip, _, _) in elements {
                                window.with_content_mask(
                                    Some(ContentMask { bounds: clip }),
                                    |window| element.paint(window, cx),
                                );
                            }
                        });
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
/// Grid lines and frames in the chart's own color, as the original charts mix them.
const GRID_OPACITY: f32 = 0.13;
const FRAME_OPACITY: f32 = 0.45;
/// The top-right caption a scale implies.
fn scale_label(scale: Option<Scale>, max: f64) -> Option<String> {
    match scale? {
        Scale::Rate { bits, .. } => Some(rate_ceiling_label(max, bits)),
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
fn bar_width(step: f32) -> Pixels {
    px((step * 0.65).max(1.))
}
impl TimePlot {
    fn paint_base(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        if self.bars {
            let step = width / self.series.len().max(1) as f32;
            for (index, series) in self.series.iter().enumerate() {
                let x = bounds.origin.x + px(index as f32 * step);
                let bar_width = bar_width(step);
                window.paint_quad(gpui_kit::gpui::fill(
                    Bounds::new(
                        point(x, bounds.origin.y),
                        size(bar_width, bounds.size.height),
                    ),
                    series.color.opacity(0.12),
                ));
            }
            return;
        }
        if self.grid {
            let color = self
                .series
                .first()
                .map(|s| s.color)
                .unwrap_or(cx.theme().border);
            if let Some(ticks) = &self.ticks {
                // Quarters across, the clock ticks down.
                let line = color.opacity(GRID_OPACITY);
                for fraction in [0.25, 0.5, 0.75] {
                    window.paint_quad(gpui_kit::gpui::fill(
                        Bounds::new(
                            point(
                                bounds.left(),
                                bounds.top() + px((height * fraction).round()),
                            ),
                            size(bounds.size.width, px(1.)),
                        ),
                        line,
                    ));
                }
                let span = (self.end - self.start).max(1.);
                for tick in ticks.iter() {
                    let x = ((tick - self.start) / span) as f32 * width;
                    if (0. ..=width).contains(&x) {
                        window.paint_quad(gpui_kit::gpui::fill(
                            Bounds::new(
                                point(bounds.left() + px(x.round()), bounds.top()),
                                size(px(1.), bounds.size.height),
                            ),
                            line,
                        ));
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
                // A line at the top of every band, the frame's top and left edges included.
                for index in 0..count {
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
                    window.paint_quad(gpui_kit::gpui::fill(
                        Bounds::new(origin, size),
                        color.opacity(GRID_OPACITY),
                    ));
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
        _cx: &mut App,
    ) -> Vec<AnyElement> {
        self.measured_width
            .set(f32::from(bounds.size.width) * window.scale_factor());
        vec![]
    }
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone().into())
    }
    fn interactive(&self) -> bool {
        self.interactive
    }
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        if !self.background_separate && !self.bars {
            self.paint_base(bounds, window, cx);
        }
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        if self.bars {
            let mut surfaces = Vec::with_capacity(self.series.len());
            let step = width / self.series.len().max(1) as f32;
            for (index, series) in self.series.iter().enumerate() {
                let value = sample_at(&series.points, self.end);
                let x = bounds.origin.x + px(index as f32 * step);
                let bar_width = bar_width(step);
                let h = px((value.unwrap_or(0.) / 100.).clamp(0., 1.) as f32 * height);
                let region =
                    Bounds::new(point(x, bounds.top()), size(bar_width, bounds.size.height));
                if !self.background_separate {
                    window.paint_quad(gpui_kit::gpui::fill(region, series.color.opacity(0.12)));
                }
                let handle = window.paint_animated_rect(
                    Bounds::new(point(x, bounds.bottom() - h), size(bar_width, h)),
                    region,
                    series.color,
                );
                surfaces.push(BarSurface {
                    handle,
                    bounds: region,
                });
            }
            *self.bar_surfaces.borrow_mut() = surfaces;
            return;
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
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let (start, end, span) = self.live_window();
        let width = f32::from(bounds.size.width).max(1.);
        let x = cursor.x.clamp(px(0.), bounds.size.width);
        let at = start + (f32::from(x) / width) as f64 * span;
        let rows = self
            .series
            .iter()
            .filter_map(|s| curve_at(&s.points, at).map(|v| (s, v)))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return None;
        }
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
            .text_size(px(12.))
            .whitespace_nowrap()
            .flex()
            .flex_col()
            .child(div().text_color(muted).child(if age == "0" {
                label("performance.chart.latest")
            } else {
                tr!("performance.chart.ago", seconds = age)
            }));
        let mut dots = Vec::with_capacity(rows.len());
        for (series, value) in rows {
            let y = self.y_of(value, bounds);
            dots.push(
                div()
                    .absolute()
                    .left(x - px(4.5))
                    .top(y - px(4.5))
                    .size(px(9.))
                    .rounded_full()
                    .border_2()
                    .border_color(palette.tip)
                    .bg(series.color),
            );
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
            .children(dots)
            .child(tip);
        Some(overlay.into_any_element())
    }
}
impl TimePlot {
    /// The window on screen now: live charts follow the scroll clock between renders.
    fn live_window(&self) -> (f64, f64, f64) {
        let span = (self.end - self.start).max(1.);
        let end = self
            .live_clock
            .as_ref()
            .map(|clock| {
                let clock = clock.borrow();
                plot_end(self, &clock, clock.end(Instant::now()))
            })
            .unwrap_or(self.end);
        (end - span, end, span)
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
    pub(in crate::frontend) fn chart_center(&self, id: &str) -> Option<Point<Pixels>> {
        self.chart_placements
            .borrow()
            .get(id)
            .map(|placement| placement.bounds.center())
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
        let placeholder = canvas(
            move |bounds, window, _| {
                placements.borrow_mut().insert(
                    id.clone(),
                    Placement {
                        bounds,
                        mask: window.content_mask().bounds,
                        plot: placed_plot.clone(),
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
        let caption_row = |left: String, right: String| {
            div()
                .flex()
                .flex_shrink_0()
                .justify_between()
                .gap_3()
                .my(px(2.))
                .text_size(px(12.))
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
        if let Some(caption) = &look.caption {
            let top = look.top.clone().unwrap_or_else(|| {
                look.format
                    .or_else(|| plot.series.first().map(|s| s.format))
                    .map(|format| format.value(plot.max))
                    .unwrap_or_default()
            });
            frame = frame.child(caption_row(caption.clone(), top));
        }
        frame = frame.child(
            div()
                .w_full()
                .overflow_hidden()
                .map(|d| match height {
                    ChartHeight::Fixed(height) => d.h(height),
                    ChartHeight::Fill => d.flex_1().min_h(
                        look.min_height
                            .unwrap_or(theme::dimension("chartMinHeight")),
                    ),
                })
                .when(look.frame, |d| {
                    d.border_1()
                        .border_color(color.opacity(FRAME_OPACITY))
                        .rounded(px(4.))
                })
                .child(placeholder),
        );
        if look.caption.is_some() {
            frame = frame.child(caption_row(span_label(self.interval), "0".into()));
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
            .text_size(px(12.))
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
        if let Some(entry) = self.chart_entries.borrow().get(id)
            && entry.revision == revision
        {
            if look.top.is_none() {
                look.top = scale_label(scale, entry.plot.borrow().max);
            }
            return self.chart_frame(entry.plot.clone(), height, &look);
        }
        let end = number(&self.snapshot, "timestamp");
        let span = self.interval as f64
            * (motion::config()["chartSamples"]
                .as_f64()
                .expect("chart samples")
                - 1.);
        let start = end - span;
        let bars = lines.len() > 1
            && lines
                .iter()
                .all(|line| line.key.starts_with("system.cores."));
        let series = lines
            .iter()
            .map(|line| {
                let key = line.key.as_str();
                let mut points = self
                    .timeline
                    .points(&model::series_key(&self.snapshot, key))
                    .unwrap_or_default();
                if line.factor != 1. || line.absolute {
                    for (_, value) in &mut points {
                        *value =
                            value.map(|v| (if line.absolute { v.abs() } else { v }) * line.factor);
                    }
                }
                Series::with_geometry(
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
                    !bars,
                )
                .dashed(line.dashed)
                .fill(line.fill)
            })
            .collect::<Vec<_>>();
        let observed = series
            .iter()
            .flat_map(|s| s.points.iter().filter_map(|(_, v)| *v))
            .filter(|v| v.is_finite())
            .fold(0f64, f64::max);
        let max = match scale {
            Some(Scale::Fixed(max)) => max.max(f64::EPSILON),
            Some(Scale::Rate { floor, bits }) => rate_ceiling(observed.max(floor), bits),
            Some(Scale::Round { floor, .. }) => round_ceiling(observed.max(floor)),
            None => nice_max(observed),
        };
        let min = series
            .iter()
            .flat_map(|s| s.points.iter().filter_map(|p| p.1))
            .fold(0., f64::min);
        let mut entries = self.chart_entries.borrow_mut();
        let previous_max = entries.get(id).map(|entry| entry.plot.borrow().max);
        let rescale = entries
            .get(id)
            .map(|entry| entry.plot.borrow().rescale.clone())
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
        let (width, image_surface, bar_surfaces, dash_phases) = entries
            .get(id)
            .map(|entry| {
                let plot = entry.plot.borrow();
                (
                    plot.measured_width.clone(),
                    plot.image_surface.clone(),
                    plot.bar_surfaces.clone(),
                    plot.dash_phases.clone(),
                )
            })
            .unwrap_or_else(|| {
                (
                    Rc::new(Cell::new(1.)),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                )
            });
        let plot = TimePlot {
            id: id.to_owned().into(),
            series,
            start,
            end,
            max,
            min,
            interactive: look.tooltip,
            grid: look.frame,
            fill: true,
            bars,
            background_separate: false,
            smooth: true,
            ticks: None,
            geometry_start: start,
            measured_width: width,
            live_clock: Some(self.chart_clock.clone()),
            image_surface,
            bar_surfaces,
            dash_phases,
            rescale,
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
        cx.set_reduce_motion(
            !self
                .prefs
                .get::<Option<bool>>("motion.ui", None)
                .unwrap_or(system),
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
        if self.options.fixture.is_some() {
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
            && self.options.fixture.is_none()
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

fn sample_at(points: &[(f64, Option<f64>)], at: f64) -> Option<f64> {
    let right = points.partition_point(|p| p.0 < at);
    if right == 0 {
        return points.first().and_then(|p| p.1);
    }
    let before = points[right - 1];
    let Some(after) = points.get(right) else {
        return before.1;
    };
    before
        .1
        .zip(after.1)
        .map(|(a, b)| a + (b - a) * ((at - before.0) / (after.0 - before.0).max(1.)).clamp(0., 1.))
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
    use super::{nice_max, sample_at, segments, smooth_run};
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
    fn animated_core_bars_do_not_fill_unknown_samples() {
        let points = [
            (0., Some(0.)),
            (10., Some(100.)),
            (20., None),
            (30., Some(20.)),
        ];
        assert_eq!(sample_at(&points, 5.), Some(50.));
        assert_eq!(sample_at(&points, 15.), None);
        assert_eq!(sample_at(&points, 25.), None);
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
