//! Time-series adapter for GPUI's native plot primitives. Unknown samples remain gaps.
use super::*;
mod raster;
pub(super) fn raster_metrics() -> (u64, u64) {
    raster::metrics()
}
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::plot::{
    Plot, PlotElement,
    tooltip::{CrossLine, Tooltip, TooltipState},
};
#[derive(Clone, Copy)]
pub(super) enum ChartHeight {
    Fixed(Pixels),
    Fill,
    Compact,
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
    geometry: Arc<Vec<Vec<(f64, f64)>>>,
    geometry_key: u64,
}
impl Series {
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
            geometry,
            geometry_key,
        }
    }
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
    pub(in crate::frontend) fn value(self, value: f64) -> String {
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
    pub bars: bool,
    pub(in crate::frontend) background_separate: bool,
    pub selection: Option<WeakEntity<Desktop>>,
    pub cursor: Option<f64>,
    pub geometry_start: f64,
    pub measured_width: Rc<Cell<f32>>,
    pub live_clock: Option<Rc<RefCell<motion::Clock>>>,
    pub image_surface: Rc<RefCell<Option<raster::ImageSurface>>>,
    pub bar_surfaces: Rc<RefCell<Vec<BarSurface>>>,
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
    keys: Vec<(String, Hsla)>,
    maximum: Option<f64>,
    interval: u64,
    locale: argus_core::i18n::Locale,
    mode: motion::Mode,
    paused: bool,
    interactive: bool,
    network_bits: bool,
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
        if self.interactive {
            let color = self
                .series
                .first()
                .map(|s| s.color)
                .unwrap_or(cx.theme().border);
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
                    window.paint_quad(gpui_kit::gpui::fill(
                        Bounds::new(origin, size),
                        color.opacity(0.16),
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
        let start = self.start;
        let span = (self.end - self.start).max(1.);
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
        if let Some(owner) = self.selection.clone() {
            let end = self.end;
            let hover_owner = owner.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && bounds.contains(&event.position) {
                    let fraction = (f32::from(event.position.x - bounds.origin.x)
                        / f32::from(bounds.size.width).max(1.))
                    .clamp(0., 1.) as f64;
                    let hovered = Some((start + (end - start) * fraction) as i64);
                    let _ = hover_owner.update(cx, |this, cx| {
                        if this.history_hover != hovered {
                            this.history_hover = hovered;
                            cx.notify();
                        }
                    });
                }
            });
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
        raster::paint(self, bounds, window, cx);
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
        let span = (self.end - self.start).max(1.);
        let end = self
            .live_clock
            .as_ref()
            .map(|clock| {
                let clock = clock.borrow();
                plot_end(self, &clock, clock.end(Instant::now()))
            })
            .unwrap_or(self.end);
        let start = end - span;
        let at =
            start + (f32::from(position.x) / f32::from(bounds.size.width).max(1.)) as f64 * span;
        let first = self.series.first()?;
        let index = first
            .points
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (a.0 - at).abs().total_cmp(&(b.0 - at).abs()))?
            .0;
        let datum = first.points[index];
        let value = datum.1?;
        let x = px(((datum.0 - start) / span) as f32 * f32::from(bounds.size.width));
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
        let end = self
            .live_clock
            .as_ref()
            .map(|clock| {
                let clock = clock.borrow();
                plot_end(self, &clock, clock.end(Instant::now()))
            })
            .unwrap_or(self.end);
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
                seconds = format!("{:.1}", (end - at) / 1000.)
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
        interactive: bool,
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
        let axes = interactive
            && match height {
                ChartHeight::Fill => true,
                ChartHeight::Compact => false,
                ChartHeight::Fixed(height) => height >= theme::dimension("chartMinHeight"),
            };
        let flexible = !matches!(height, ChartHeight::Fixed(_));
        let mut frame = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .when(flexible, |d| d.flex_1())
            .when(!flexible, |d| d.flex_shrink_0());
        if axes {
            frame = frame.child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(self.palette.muted)
                    .child(
                        plot.series
                            .first()
                            .map(|s| s.name.clone())
                            .unwrap_or_default(),
                    )
                    .child(
                        plot.series
                            .first()
                            .map(|s| s.format.value(plot.max))
                            .unwrap_or_default(),
                    ),
            );
        }
        frame = frame.child(
            div()
                .w_full()
                .overflow_hidden()
                .map(|d| match height {
                    ChartHeight::Fixed(height) => d.h(height),
                    ChartHeight::Fill => d.flex_1().min_h(theme::dimension("chartMinHeight")),
                    ChartHeight::Compact => d.flex_1().min_h(px(24.)),
                })
                .when(interactive, |d| {
                    d.border_1().border_color(color.opacity(0.35)).rounded_sm()
                })
                .child(placeholder),
        );
        if axes {
            frame = frame.child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(self.palette.muted)
                    .child(tr!(
                        "performance.span.seconds",
                        count = ((plot.end - plot.start) / 1000.).round() as u64
                    ))
                    .child(label("performance.chart.latest")),
            );
            if plot.series.len() > 1 {
                frame = frame.child(div().flex().gap_4().mt_1().children(plot.series.iter().map(
                    |s| {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(self.palette.muted)
                            .child(div().w_3().h(px(2.)).bg(s.color))
                            .child(s.name.clone())
                    },
                )));
            }
        }
        drop(plot);
        frame.into_any_element()
    }
    pub(in crate::frontend) fn chart(
        &self,
        id: &str,
        keys: &[(&str, Hsla)],
        maximum: Option<f64>,
        height: impl Into<ChartHeight>,
        interactive: bool,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let height = height.into();
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
        if let Some(entry) = self.chart_entries.borrow().get(id)
            && entry.revision == revision
        {
            return self.chart_frame(entry.plot.clone(), height, interactive);
        }
        let end = number(&self.snapshot, "timestamp");
        let span = self.interval as f64
            * (motion::config()["chartSamples"]
                .as_f64()
                .expect("chart samples")
                - 1.);
        let start = end - span;
        let bars = keys.len() > 1 && keys.iter().all(|(key, _)| key.starts_with("system.cores."));
        let series = keys
            .iter()
            .map(|(key, color)| {
                Series::with_geometry(
                    series_name(&self.snapshot, key),
                    self.timeline
                        .samples
                        .get(&model::series_key(&self.snapshot, key))
                        .map(|p| p.iter().copied().collect())
                        .unwrap_or_default(),
                    *color,
                    if key.contains(".nics.") || *key == "totals.network" {
                        MetricFormat::Network(
                            self.prefs.get::<String>("netUnit", "bytes".into()) == "bits",
                        )
                    } else {
                        series_format(key)
                    },
                    !bars,
                )
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
        let mut entries = self.chart_entries.borrow_mut();
        let width = entries
            .get(id)
            .map(|entry| entry.plot.borrow().measured_width.clone())
            .unwrap_or_else(|| Rc::new(Cell::new(1.)));
        let plot = TimePlot {
            id: id.to_owned().into(),
            series,
            start,
            end,
            max,
            min,
            interactive,
            fill: true,
            bars,
            background_separate: false,
            selection: None,
            cursor: None,
            geometry_start: start,
            measured_width: width,
            live_clock: Some(self.chart_clock.clone()),
            image_surface: entries
                .get(id)
                .map(|entry| entry.plot.borrow().image_surface.clone())
                .unwrap_or_default(),
            bar_surfaces: entries
                .get(id)
                .map(|entry| entry.plot.borrow().bar_surfaces.clone())
                .unwrap_or_default(),
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
        self.active_charts.borrow_mut().insert(id.into());
        self.chart_frame(plot_rc, height, interactive)
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
