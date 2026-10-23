//! Time-series adapter for GPUI's native plot primitives. Unknown samples remain gaps.
use super::*;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::plot::{
    Curve, PathCaches, Plot, PlotElement,
    shape::{Area, Line},
    tooltip::{CrossLine, Tooltip, TooltipState},
};

#[derive(Clone)]
pub struct Series {
    pub name: String,
    pub points: Arc<Vec<(f64, Option<f64>)>>,
    pub color: Hsla,
    pub format: MetricFormat,
}
#[derive(Clone, Copy)]
pub enum MetricFormat {
    Percent,
    Memory,
    Rate,
    Count,
    Network(bool),
    Unit(&'static str, usize),
}
impl MetricFormat {
    fn value(self, value: f64) -> String {
        match self {
            Self::Percent => format!("{}%", theme::cpu(value)),
            Self::Memory => theme::memory(value),
            Self::Rate => theme::bytes(value, true),
            Self::Count => theme::count(value),
            Self::Network(bits) => theme::network(value, bits),
            Self::Unit(unit, digits) => format!("{value:.digits$} {unit}"),
        }
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
    pub fill: bool,
    pub selection: Option<WeakEntity<Desktop>>,
    pub cursor: Option<f64>,
    pub geometry_start: f64,
    pub measured_width: Rc<Cell<f32>>,
}
pub(super) struct ChartView {
    plot: TimePlot,
    clock: Rc<RefCell<motion::Clock>>,
    last_end: f64,
    revision: ChartRevision,
}
#[derive(PartialEq)]
struct ChartRevision {
    sequence: u64,
    keys: Vec<(String, Hsla)>,
    maximum: Option<f64>,
    interval: u64,
    locale: argus_core::i18n::Locale,
    mode: motion::Mode,
    paused: bool,
    interactive: bool,
    network_bits: bool,
}
impl Render for ChartView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut plot = self.plot.clone();
        let clock = self.clock.borrow();
        let span = plot.end - plot.start;
        let mut end = clock.end(Instant::now());
        if clock.mode == motion::Mode::Pixel {
            let step = span / f64::from(plot.measured_width.get().max(1.));
            end = plot.geometry_start + ((end - plot.geometry_start) / step).floor() * step;
        }
        plot.end = end;
        plot.start = end - span;
        plot
    }
}
impl ChartView {
    fn advance(&mut self, cx: &mut Context<Self>) {
        let clock = self.clock.borrow();
        let mut end = clock.end(Instant::now());
        if clock.mode == motion::Mode::Pixel {
            let step = (self.plot.end - self.plot.start)
                / f64::from(self.plot.measured_width.get().max(1.));
            end =
                self.plot.geometry_start + ((end - self.plot.geometry_start) / step).floor() * step;
        }
        if end != self.last_end {
            self.last_end = end;
            cx.notify();
        }
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
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let start = self.start;
        let geometry_start = self.geometry_start;
        let span = (self.end - self.start).max(1.);
        let min = self.min;
        let scale = (self.max - self.min).max(f64::EPSILON);
        let fill = self.fill;
        let drawing_bounds = Bounds::new(
            point(
                bounds.origin.x + px(((geometry_start - start) / span) as f32 * width),
                bounds.origin.y,
            ),
            bounds.size,
        );
        if let Some(owner) = self.selection.clone() {
            let end = self.end;
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == MouseButton::Left
                    && bounds.contains(&event.position)
                {
                    let fraction = (f32::from(event.position.x - bounds.origin.x)
                        / f32::from(bounds.size.width).max(1.))
                    .clamp(0., 1.) as f64;
                    let _ = owner.update(cx, |this, cx| {
                        this.history_cursor = Some((start + (end - start) * fraction) as i64);
                        this.load_history(window, cx);
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
            });
        }
        let caches = PathCaches::for_paint("argus-series", window, cx);
        caches.update(cx, |cache, _| {
            let mut index = 0;
            for series in &self.series {
                for run in segments(&series.points) {
                    let x =
                        move |p: &(f64, f64)| Some(((p.0 - geometry_start) / span) as f32 * width);
                    let y =
                        move |p: &(f64, f64)| Some(height - ((p.1 - min) / scale) as f32 * height);
                    if fill {
                        let (area_cache, line_cache) = cache.slot_pair(index);
                        Area::new()
                            .data(run.clone())
                            .x(x)
                            .y1(y)
                            .y0(height - ((-min / scale) as f32 * height).clamp(0., height))
                            .curve(Curve::Linear)
                            .fill(series.color.opacity(0.15))
                            .stroke(series.color)
                            .paint_cached(&drawing_bounds, area_cache, line_cache, window);
                        index += 1;
                    } else {
                        Line::new()
                            .data(run)
                            .x(x)
                            .y(y)
                            .curve(Curve::Linear)
                            .stroke(series.color)
                            .paint_cached(&drawing_bounds, cache.slot(index), window);
                        index += 1;
                    }
                }
            }
        });
        if let Some(cursor) = self
            .cursor
            .filter(|at| *at >= self.start && *at <= self.end)
        {
            let x = bounds.origin.x + px(((cursor - self.start) / span) as f32 * width);
            window.paint_quad(gpui_kit::gpui::fill(
                Bounds::new(point(x, bounds.origin.y), size(px(1.), bounds.size.height)),
                cx.theme().foreground,
            ));
        }
    }
    fn tooltip_state(
        &self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _cx: &App,
    ) -> Option<TooltipState> {
        let at = self.start
            + (f32::from(position.x) / f32::from(bounds.size.width).max(1.)) as f64
                * (self.end - self.start);
        let first = self.series.first()?;
        let index = first
            .points
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (a.0 - at).abs().total_cmp(&(b.0 - at).abs()))?
            .0;
        let datum = first.points[index];
        let value = datum.1?;
        let x = px(
            ((datum.0 - self.start) / (self.end - self.start).max(1.)) as f32
                * f32::from(bounds.size.width),
        );
        let y = bounds.size.height
            - px(
                ((value - self.min) / (self.max - self.min).max(f64::EPSILON)) as f32
                    * f32::from(bounds.size.height),
            );
        Some(TooltipState::new(
            index,
            point(x, position.y),
            vec![point(x, y)],
        ))
    }
    fn tooltip(
        &self,
        state: &TooltipState,
        cursor: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let first = self.series.first()?;
        let (at, _) = *first.points.get(state.index)?;
        let rows = self
            .series
            .iter()
            .filter_map(|s| {
                s.points
                    .iter()
                    .min_by(|a, b| (a.0 - at).abs().total_cmp(&(b.0 - at).abs()))
                    .and_then(|p| p.1.map(|v| (s.color, s.name.clone(), s.format.value(v))))
            })
            .collect::<Vec<_>>();
        let mut tooltip = Tooltip::new(cursor, bounds.size)
            .title(tr!(
                "performance.chart.ago",
                seconds = format!("{:.1}", (self.end - at) / 1000.)
            ))
            .cross_line(CrossLine::new(state.cross_line));
        for (color, name, value) in rows {
            tooltip = tooltip.row(color, name, value);
        }
        let _ = cx;
        Some(tooltip.into_any_element())
    }
}
impl Desktop {
    pub(in crate::frontend) fn chart(
        &self,
        id: &str,
        keys: &[(&str, Hsla)],
        maximum: Option<f64>,
        height: Pixels,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let revision = ChartRevision {
            sequence: self.sequence,
            keys: keys
                .iter()
                .map(|(key, color)| ((*key).to_owned(), *color))
                .collect(),
            maximum,
            interval: self.interval,
            locale: argus_core::i18n::locale(),
            mode: self.chart_clock.borrow().mode,
            paused: self.paused || self.control_paused,
            interactive,
            network_bits: self.prefs.get::<String>("netUnit", "bytes".into()) == "bits",
        };
        self.active_charts.borrow_mut().insert(id.into());
        if let Some(view) = self.chart_views.borrow().get(id)
            && view.read(cx).revision == revision
        {
            return div()
                .h(height)
                .w_full()
                .overflow_hidden()
                .child(view.clone().cached(StyleRefinement::default().size_full()))
                .into_any_element();
        }
        let end = number(&self.snapshot, "timestamp");
        let span = self.interval as f64
            * (motion::config()["chartSamples"]
                .as_f64()
                .expect("chart samples")
                - 1.);
        let start = end - span;
        let series = keys
            .iter()
            .map(|(key, color)| Series {
                name: series_name(&self.snapshot, key),
                points: Arc::new(
                    self.timeline
                        .samples
                        .get(&model::series_key(&self.snapshot, key))
                        .map(|p| p.iter().copied().collect())
                        .unwrap_or_default(),
                ),
                color: *color,
                format: if key.contains(".nics.") || *key == "totals.network" {
                    MetricFormat::Network(
                        self.prefs.get::<String>("netUnit", "bytes".into()) == "bits",
                    )
                } else {
                    series_format(key)
                },
            })
            .collect::<Vec<_>>();
        let observed = series
            .iter()
            .flat_map(|s| s.points.iter().filter_map(|(_, v)| *v))
            .fold(0f64, f64::max);
        let max = maximum.unwrap_or_else(|| nice_max(observed));
        let min = series
            .iter()
            .flat_map(|s| s.points.iter().filter_map(|p| p.1))
            .fold(0., f64::min);
        let mut views = self.chart_views.borrow_mut();
        let width = views
            .get(id)
            .map(|view| view.read(cx).plot.measured_width.clone())
            .unwrap_or_else(|| Rc::new(Cell::new(1.)));
        let plot = TimePlot {
            id: id.to_owned().into(),
            series,
            start,
            end,
            max,
            min,
            interactive,
            fill: keys.len() == 1,
            selection: None,
            cursor: None,
            geometry_start: start,
            measured_width: width,
        };
        let view = if let Some(view) = views.get(id) {
            view.update(cx, |view, cx| {
                view.plot = plot;
                view.revision = revision;
                cx.notify();
            });
            view.clone()
        } else {
            let clock = self.chart_clock.clone();
            let view = cx.new(|_| ChartView {
                plot,
                clock,
                last_end: f64::NAN,
                revision,
            });
            views.insert(id.into(), view.clone());
            view
        };
        self.active_charts.borrow_mut().insert(id.into());
        div()
            .h(height)
            .w_full()
            .overflow_hidden()
            .child(view.cached(StyleRefinement::default().size_full()))
            .into_any_element()
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
                    this.animate_charts(cx);
                }
                this.schedule_chart_tick(window, cx);
            });
        });
    }
    fn animate_charts(&mut self, cx: &mut Context<Self>) {
        if !self.chart_clock.borrow().moving() {
            return;
        }
        let views = self.chart_views.borrow();
        for key in self.active_charts.borrow().iter() {
            if let Some(view) = views.get(key) {
                view.update(cx, |view, cx| view.advance(cx));
            }
        }
    }
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
    use super::{nice_max, segments};
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
