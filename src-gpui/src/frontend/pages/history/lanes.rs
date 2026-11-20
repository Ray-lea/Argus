//! The chart card: lanes of readings, each with its title, axes and plot; the
//! time axis under them; the cursor, selection and drop marks over them; and
//! the navigator below.
use super::bar::ReadingMenu;
use super::readings::Reading;
use super::series::{BANDED, FRAME, Part, Points, rows_for, tick_step, ticks};
use super::state::{Drag, Drop, Glide, Hover, Lane};
use super::*;

/// The axis labels' columns either side of the plots; every lane keeps both,
/// so that the plots line up under one time axis.
pub(super) const GUTTER: f32 = 48.;
/// A lane's title row.
pub(super) const HEADER: f32 = 30.;
/// The room between lanes, where the divider sits.
pub(super) const GAP_Y: f32 = 14.;
/// The time axis under the lanes.
const AXIS: f32 = 22.;
/// The chart card's padding.
const PAD: f32 = 10.;
/// The navigator's row.
const NAV: f32 = 32.;
/// The lanes' tooltip: its width, fixed so that it keeps its size and place
/// as the pointer passes readings and process names of other lengths; how
/// far it sits from the pointer; and the room it keeps from the window's edges.
const TIP_WIDTH: f32 = 300.;
const TIP_GAP: f32 = 14.;
const TIP_MARGIN: f32 = 8.;

/// The chart card's height that gives `lanes` lanes room to read: about
/// 128 px each, with the axis and the navigator.
pub(super) fn comfortable_height(lanes: f32) -> f32 {
    lanes * 128. + (lanes - 1.).max(0.) * GAP_Y + AXIS + NAV + 2. * PAD + 8.
}

/// A lane's axis, the overlay's, and the rows dividing them.
#[derive(Clone, Copy, Debug)]
pub(super) struct Axes {
    pub(super) left: (f64, f64),
    pub(super) right: Option<(f64, f64)>,
    /// The overlay shares the left axis (same unit).
    pub(super) shared: bool,
    pub(super) rows: u32,
}

/// While the window glides, where a mark goes: laid out on the device-pixel
/// grid at or before its time and moved the rest of the way when drawn
/// (`shift::shifted`), so that it glides with the curves. Laid out at its
/// time, a mark stepped a whole pixel at a time beside them. Borders, glyphs
/// and icons still keep to whole pixels.
#[derive(Clone, Copy)]
struct Nudge {
    /// The plot's width and the window's scale factor.
    width: f32,
    scale: f32,
}

impl Nudge {
    /// A length on the device-pixel grid, so that a band moves whole.
    fn length(self, length: f32) -> Pixels {
        px((length * self.scale).round() / self.scale)
    }
}

/// `mark` placed with its left edge at fraction `f` of the plot's width and
/// `dx` beyond it.
fn place<M: Styled + IntoElement + 'static>(
    nudge: Option<Nudge>,
    mark: M,
    f: f32,
    dx: f32,
) -> AnyElement {
    match nudge {
        Some(nudge) => {
            let x = f * nudge.width + dx;
            let laid = (x * nudge.scale).floor() / nudge.scale;
            crate::frontend::shift::shifted(mark.left(px(laid)), point(px(x - laid), px(0.)), 1.)
                .into_any_element()
        }
        None => mark.left(relative(f)).ml(px(dx)).into_any_element(),
    }
}

/// The width of the widest of `samples` at `size` and `weight`: a value slot
/// that keeps its width while the value in it changes.
pub(super) fn slot_width(
    samples: &[String],
    size: Pixels,
    weight: FontWeight,
    window: &Window,
    cx: &App,
) -> Pixels {
    samples
        .iter()
        .map(|text| {
            window
                .text_system()
                .layout_line(text, size, &[theme::text_run(text, weight, cx)], None)
                .width
        })
        .fold(px(0.), Pixels::max)
        .ceil()
        + px(1.)
}

/// Maps a right-axis value onto the left axis the plot draws.
fn onto(value: f64, from: (f64, f64), to: (f64, f64)) -> f64 {
    let span = (from.1 - from.0).max(f64::EPSILON);
    to.0 + (value - from.0) / span * (to.1 - to.0)
}

impl Desktop {
    /// The moment the readings describe: the pointer's, the pinned one, else
    /// the newest reading shown.
    pub(super) fn history_read_at(&self) -> i64 {
        if let Some(hover) = self.history.hover {
            return hover.at;
        }
        if let Some(pin) = self.history.pin {
            return pin;
        }
        if self.history.follow {
            self.history_clock()
        } else {
            self.history.view.1
        }
    }

    /// The point the readings show, if one covers the moment.
    pub(super) fn history_read_index(&self) -> Option<usize> {
        let points = self.history.points.as_ref()?;
        let at = self.history_read_at();
        points.index_at(at).or_else(|| {
            // Following, the newest point covers the present.
            (self.history.hover.is_none() && self.history.pin.is_none())
                .then(|| points.ts.len().checked_sub(1))
                .flatten()
        })
    }

    /// The axes of a lane over the window `from..to`.
    pub(super) fn history_axes(
        &self,
        lane: &Lane,
        points: Option<&Points>,
        (from, to): (i64, i64),
        bucket: i64,
        height: f32,
    ) -> Axes {
        let end = self.history_clock();
        let banded = bucket >= BANDED && self.history.band;
        let reading = self.history_reading(&lane.reading);
        let empty = Points::default();
        let points = points.unwrap_or(&empty);
        let mut left = reading.map_or((0., 100.), |r| r.axis_range(points, from, to, end, banded));
        let overlay = lane
            .overlay
            .as_deref()
            .and_then(|id| self.history_reading(id));
        let mut right = overlay.map(|r| r.axis_range(points, from, to, end, false));
        let shared = matches!((reading, overlay), (Some(a), Some(b)) if a.unit == b.unit);
        if shared && let Some(r) = right {
            left = (left.0.min(r.0), left.1.max(r.1));
            right = Some(left);
        }
        let range = left.1 - left.0;
        let unit = reading.map_or(1., |r| r.label_unit(left.1.abs().max(left.0.abs())));
        let scaled = range / unit;
        let half = scaled / 2.;
        let round_half = (half - half.round()).abs() < 1e-6
            || (half < 1. && (half * 10. - (half * 10.).round()).abs() < 1e-6);
        let rows = if height < 50. {
            1
        } else if height < 100. {
            if round_half { 2 } else { 1 }
        } else {
            rows_for(scaled)
        };
        Axes {
            left,
            right,
            shared,
            rows,
        }
    }

    /// The series a lane draws: the band's top (CPU's highest samples), the
    /// reading (a pair's second part as a dim line), the overlay on its axis.
    fn history_lane_series(
        &self,
        lane: &Lane,
        points: &Points,
        axes: &Axes,
        bucket: i64,
    ) -> Vec<charts::Series> {
        let mut series = Vec::new();
        let Some(reading) = self.history_reading(&lane.reading) else {
            return series;
        };
        let end = self.history_clock();
        let source = |part: &str| charts::hash_of(("history", lane.id, &reading.id, part));
        let format = charts::MetricFormat::Percent;
        // From the lowest to the highest reading of each bucket (CPU up to its
        // highest sample), under the line.
        let banded = bucket >= BANDED && self.history.band;
        if banded && !reading.is_pair() {
            let key = &reading.keys[0];
            let high = reading
                .peak
                .as_deref()
                .filter(|peak| points.has(peak))
                .unwrap_or(key);
            series.push(
                charts::Series::band(
                    String::new(),
                    points.band(key, high, end),
                    reading.color,
                    format,
                )
                .source(source("band")),
            );
        }
        let main = if reading.is_pair() && !points.has(&reading.keys[0]) {
            reading
                .fallback
                .clone()
                .unwrap_or_else(|| reading.keys[0].clone())
        } else {
            reading.keys[0].clone()
        };
        series.push(
            charts::Series::new(
                String::new(),
                points.line(&main, Part::Mean, end, |v| v),
                reading.color,
                format,
            )
            .fill(0.22)
            .source(source("main")),
        );
        if reading.is_pair() && points.has(&reading.keys[1]) {
            series.push(
                charts::Series::new(
                    String::new(),
                    points.line(&reading.keys[1], Part::Mean, end, |v| v),
                    reading.color.opacity(0.5),
                    format,
                )
                .fill(0.)
                .source(source("second")),
            );
        }
        if let Some(overlay) = lane
            .overlay
            .as_deref()
            .and_then(|id| self.history_reading(id))
            && let Some(right) = axes.right
        {
            let left = axes.left;
            let key = overlay.keys[0].clone();
            series.push(
                charts::Series::new(
                    String::new(),
                    points.line(&key, Part::Mean, end, |v| onto(v, right, left)),
                    overlay.color,
                    format,
                )
                .fill(0.)
                .source(charts::hash_of((
                    "history",
                    lane.id,
                    &overlay.id,
                    "overlay",
                ))),
            );
        }
        // A process laid over the lanes: its own readings of the lane's kind, dashed.
        if let Some(curve) = self.history_process_curve(reading) {
            series.push(
                charts::Series::new(String::new(), curve, self.palette.ink.opacity(0.6), format)
                    .dashed(true)
                    .fill(0.)
                    .source(charts::hash_of((
                        "history",
                        lane.id,
                        &reading.id,
                        "process",
                        &self.history.curve,
                    ))),
            );
        }
        series
    }

    /// The chosen process's readings for `reading`'s kind over the window it was
    /// asked for, its instances summed: points spread evenly across it.
    fn history_process_curve(&self, reading: &Reading) -> Option<Vec<(f64, Option<f64>)>> {
        let name = self.history.curve.as_deref()?;
        let key = reading.process?;
        let answer = if self.options.fixture.is_some() {
            self.data.get("historyTable")?
        } else {
            self.history.curves.get(key)?
        };
        let (from, to) = (answer["from"].as_f64()?, answer["to"].as_f64()?);
        let mut sum: Vec<Option<f64>> = Vec::new();
        for process in values(&answer["processes"]) {
            if !text(process, "name").eq_ignore_ascii_case(name) {
                continue;
            }
            for (i, v) in values(&process["trend"]).iter().enumerate() {
                if sum.len() <= i {
                    sum.resize(i + 1, None);
                }
                if let Some(v) = v.as_f64() {
                    sum[i] = Some(sum[i].unwrap_or(0.) + v);
                }
            }
        }
        if sum.is_empty() {
            return None;
        }
        let step = (to - from) / sum.len() as f64;
        Some(
            sum.into_iter()
                .enumerate()
                .map(|(i, v)| (from + step * (i as f64 + 0.5), v))
                .collect(),
        )
    }

    /// The chart card's height: its share of the page, enough for every lane.
    pub(super) fn history_chart_height(&self, window: &Window) -> f32 {
        let content = self.history.geometry.borrow().content.size.height;
        let content = if content > px(0.) {
            f32::from(content)
        } else {
            f32::from(window.viewport_size().height) - 140.
        };
        let lanes_count = self.history.live_lanes().count() as f32;
        // Room for every lane at its narrowest, the axis and the navigator.
        let least = lanes_count * (state::MIN_LANE + 6.)
            + (lanes_count - 1.) * GAP_Y
            + AXIS
            + NAV
            + 2. * PAD
            + 8.;
        (content * self.history.split).max(260.).max(least)
    }

    /// The chart card: the lanes over the time axis, and the navigator,
    /// filling the region it is drawn in.
    pub(super) fn history_chart(
        &mut self,
        shown: (f64, f64),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let lanes = self.history_lanes(shown, window, cx);
        let navigator = self.history_navigator(window, cx);
        div()
            .id("history-chart")
            .flex_none()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(PAD))
            .pb(px(8.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            // Opaque, as the live charts' surface: Windows moves curves on
            // the compositor only over an opaque backdrop, and draws those
            // over the window material again in every frame.
            .bg(p.chart_surface)
            .child(lanes)
            .child(navigator)
            .into_any_element()
    }

    fn history_lanes(
        &mut self,
        (from, to): (f64, f64),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let geometry = self.history.geometry.clone();
        let span = (to - from).max(1.);
        let width = self.history_plot_width(window);
        let bucket = series::bucket_for(self.history.span(), width);
        let window_ms = (from.floor() as i64, to.ceil() as i64);
        let multi = self.history.live_lanes().count() > 1;
        let lanes = self.history.lanes.clone();
        let measured: HashMap<u64, f32> = geometry
            .borrow()
            .lanes
            .iter()
            .map(|(id, _, plot)| (*id, f32::from(plot.size.height)))
            .collect();
        // Each lane records where it was laid out, for the pointer.
        geometry.borrow_mut().lanes.clear();
        let tick = tick_step(span as i64, width);
        let tick_list = Arc::new(ticks(window_ms.0, window_ms.1, tick));
        // While the window glides, the clock lines are drawn into the curves'
        // textures and move with them: the ticks of the texture's own window
        // and the raster's overscan, an eighth of the window either side. They
        // change only with the texture's window, as the newest reading does.
        let clock_lines = self.history.flow_origin.map(|(origin, _)| {
            let overscan = span / 8.;
            let lines = ticks(
                (origin - overscan).floor() as i64,
                (origin + span + overscan).ceil() as i64,
                tick,
            );
            (
                Arc::new(lines.into_iter().map(|t| t as f64).collect::<Vec<_>>()),
                p.stroke.opacity(0.6),
            )
        });
        let read_index = self.history_read_index();
        let points = self.history.points.clone();
        let mut column = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child({
                let geometry = geometry.clone();
                canvas(
                    move |bounds, _, _| geometry.borrow_mut().area = bounds,
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
            });
        let mut previous: Option<f32> = None;
        for (index, lane) in lanes.iter().enumerate() {
            let presence = lane.presence();
            if let Some(before) = previous {
                column = column.child(self.history_divider(index - 1, presence.min(before), cx));
            }
            previous = Some(presence);
            let plot_height = measured.get(&lane.id).copied().unwrap_or(120.);
            let axes = self.history_axes(lane, points.as_ref(), window_ms, bucket, plot_height);
            column = column.child(self.history_lane(
                lane,
                presence,
                multi,
                &axes,
                bucket,
                (from, to),
                &tick_list,
                clock_lines.as_ref(),
                read_index,
                window,
                cx,
            ));
        }
        // The time axis, and the button adding a lane at its left end.
        let axis = div()
            .flex_none()
            .h(px(AXIS) + theme::text_growth(11., 1.4))
            .flex()
            .child(
                div()
                    .w(px(GUTTER))
                    .flex_shrink_0()
                    .relative()
                    .child(self.history_add_lane(cx)),
            )
            .child({
                // Painted at their times to the fraction of a pixel (glyphs
                // take quarter pixels), not laid out on whole device pixels:
                // they move with the curves while the window glides.
                let labels = tick_list
                    .iter()
                    .map(|at| {
                        (
                            ((*at as f64 - from) / span) as f32,
                            SharedString::from(tick_label(*at, tick)),
                        )
                    })
                    .collect::<Vec<_>>();
                let color = p.faint;
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .text_size(theme::font(11.))
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, cx| {
                                let width = f32::from(bounds.size.width);
                                let size =
                                    window.text_style().font_size.to_pixels(window.rem_size());
                                let line_height = window.line_height();
                                for (f, text) in &labels {
                                    let x = f * width;
                                    if !(x > 18. && x < width - 18.) {
                                        continue;
                                    }
                                    let mut run = theme::text_run(text, FontWeight::NORMAL, cx);
                                    run.color = color;
                                    let line = window.text_system().shape_line(
                                        text.clone(),
                                        size,
                                        &[run],
                                        None,
                                    );
                                    let origin = point(
                                        bounds.left() + px(x) - line.width / 2.,
                                        bounds.top() + px(4.),
                                    );
                                    let _ = line.paint(
                                        origin,
                                        line_height,
                                        gpui_kit::gpui::TextAlign::Left,
                                        None,
                                        window,
                                        cx,
                                    );
                                }
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
            })
            .child(div().w(px(GUTTER)).flex_shrink_0());
        column = column.child(axis);
        if let Some(drop) = self.history_drop_mark() {
            column = column.child(drop);
        }
        if let Some((text, _)) = &self.history.toast {
            column = column.child(
                div()
                    .absolute()
                    .top(px(HEADER + 8.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .px(px(10.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(p.stroke_strong)
                            .bg(widgets::solid(p.flyout))
                            .text_size(theme::font(12.))
                            .child(text.clone()),
                    ),
            );
        }
        if let Some(tip) = self.history_tip(window, cx) {
            column = column.child(tip);
        }
        if let Some(ghost) = self.history_ghost() {
            column = column.child(ghost);
        }
        column.into_any_element()
    }

    /// The divider between lane `index` and the next: dragged, it shares their heights.
    fn history_divider(&self, index: usize, presence: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let active =
            matches!(self.history.drag, Some(Drag::Divider { index: i, .. }) if i == index);
        div()
            .id(("history-divider", index))
            .flex_none()
            .h(px(GAP_Y * presence))
            .flex()
            .items_center()
            .justify_center()
            .cursor_row_resize()
            .opacity(presence)
            .child(div().h(px(1.)).flex_1().mx(px(GUTTER)).bg(if active {
                p.accent
            } else {
                p.stroke
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    let ids = this
                        .history
                        .live_lanes()
                        .map(|l| (l.id, l.weight))
                        .collect::<Vec<_>>();
                    let (Some(a), Some(b)) = (ids.get(index), ids.get(index + 1)) else {
                        return;
                    };
                    let height = this
                        .history
                        .geometry
                        .borrow()
                        .lanes
                        .iter()
                        .map(|(_, lane, _)| f32::from(lane.size.height))
                        .sum::<f32>();
                    this.history.drag = Some(Drag::Divider {
                        index,
                        y: f32::from(event.position.y),
                        weights: (a.1, b.1),
                        height,
                    });
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn history_lane(
        &mut self,
        lane: &Lane,
        presence: f32,
        multi: bool,
        axes: &Axes,
        bucket: i64,
        (from, to): (f64, f64),
        tick_list: &Arc<Vec<i64>>,
        clock_lines: Option<&(Arc<Vec<f64>>, Hsla)>,
        read_index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let id = lane.id;
        let span = (to - from).max(1.);
        let reading = self.history_reading(&lane.reading).cloned();
        let overlay = lane
            .overlay
            .as_deref()
            .and_then(|id| self.history_reading(id))
            .cloned();
        let bits = self.history_bits();
        let header = self.history_lane_header(lane, presence, multi, axes, read_index, window, cx);
        let points = self.history.points.clone();
        let geometry = self.history.geometry.clone();
        let lane_id = lane.id;
        // The plot: its grid, the curves, the gaps and the marks.
        let mut plot = div()
            .id(("history-plot", id))
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .child({
                let geometry = geometry.clone();
                canvas(
                    move |bounds, _, _| {
                        let mut geometry = geometry.borrow_mut();
                        if let Some(entry) =
                            geometry.lanes.iter_mut().find(|(i, _, _)| *i == lane_id)
                        {
                            entry.2 = bounds;
                        } else {
                            geometry.lanes.push((lane_id, Bounds::default(), bounds));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
            });
        for row in 0..=axes.rows {
            let f = row as f32 / axes.rows as f32;
            plot = plot.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(f))
                    .when(row == axes.rows, |d| d.mt(px(-1.)))
                    .h(px(1.))
                    .bg(p.stroke),
            );
        }
        // The clock lines: in the curve's texture while it glides (they move
        // with it), else laid out here.
        let clock_lines = clock_lines.filter(|_| points.is_some() && reading.is_some());
        for at in tick_list.iter().filter(|_| clock_lines.is_none()) {
            let f = ((*at as f64 - from) / span) as f32;
            if f > 0.002 && f < 0.998 {
                plot = plot.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(f))
                        .w(px(1.))
                        .bg(p.stroke.opacity(0.6)),
                );
            }
        }
        // Marks placed by time glide with the curves (`Nudge`).
        let nudge = self.history.flow_origin.map(|_| Nudge {
            width: self.history_plot_width(window),
            scale: window.scale_factor(),
        });
        // A band from fraction `a` to `b` of the plot's width.
        let band = |mark: Div, a: f32, b: f32| {
            let mark = match nudge {
                Some(nudge) => mark.w(nudge.length((b - a) * nudge.width)),
                None => mark.w(relative(b - a)),
            };
            place(nudge, mark, a, 0.)
        };
        // Gaps: hatched, named where there is room.
        if let Some(points) = &points {
            for gap in &points.gaps {
                let a = ((gap.from as f64 - from) / span).clamp(0., 1.) as f32;
                let b = ((gap.to as f64 - from) / span).clamp(0., 1.) as f32;
                if b - a <= 0. {
                    continue;
                }
                let wide = (b - a) * self.history_plot_width(window) > 58.;
                plot = plot.child(band(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .bg(gpui_kit::gpui::pattern_slash(p.faint.opacity(0.22), 1., 6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .overflow_hidden()
                        .when(wide, |d| {
                            d.child(
                                div()
                                    .text_size(theme::font(11.))
                                    .text_color(p.faint)
                                    .whitespace_nowrap()
                                    .child(label(gap.reason.key())),
                            )
                        }),
                    a,
                    b,
                ));
            }
        }
        let recorded = reading.as_ref().is_some_and(|r| r.recorded);
        if let (Some(points), Some(_)) = (&points, &reading) {
            let series = self.history_lane_series(lane, points, axes, bucket);
            let curve = charts::TimePlot {
                id: format!("history-lane-{id}").into(),
                series: Arc::new(series),
                start: from,
                end: to,
                max: axes.left.1,
                min: axes.left.0,
                interactive: false,
                grid: false,
                fill: true,
                background_separate: false,
                smooth: false,
                ticks: clock_lines.cloned(),
                // While the window glides, the texture stays the one drawn for
                // the newest reading's window, moved under the window on screen.
                geometry_start: self.history.flow_origin.map_or(from, |(lanes, _)| lanes),
                measured_width: Rc::new(Cell::new(1.)),
                live_clock: None,
                image_surface: Default::default(),
                dash_phases: Default::default(),
                rescale: Default::default(),
                readout: Default::default(),
                captions: None,
                redraw_alone: true,
            };
            if self.history.flow_origin.is_some() {
                // The plot fills the box: its bounds are the box's.
                let bounds = Rc::new(Cell::new(Bounds::default()));
                self.history.glides.push(Glide {
                    plot: curve.clone(),
                    bounds: bounds.clone(),
                });
                plot = plot.child(
                    canvas(move |laid, _, _| bounds.set(laid), |_, _, _, _| {})
                        .absolute()
                        .inset_0(),
                );
            }
            plot = plot.child(curve);
        }
        if !recorded && let Some(reading) = &reading {
            plot = plot.child(self.history_unrecorded(reading, cx));
        }
        // The selection, the pinned moment and the pointer.
        if let Some((a, b)) = self.history.selection {
            let fa = ((a as f64 - from) / span).clamp(0., 1.) as f32;
            let fb = ((b as f64 - from) / span).clamp(0., 1.) as f32;
            if fb > fa {
                // Its edges are lines of their own: a bordered box keeps to
                // whole pixels when it is moved.
                let edge = || {
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .w(px(1.))
                        .bg(p.accent.opacity(0.7))
                };
                plot = plot.child(band(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .bg(p.accent.opacity(0.12))
                        .child(edge().left_0())
                        .child(edge().right_0()),
                    fa,
                    fb,
                ));
            }
        }
        let first = self.history.live_lanes().next().map(|l| l.id) == Some(id);
        if let Some(pin) = self.history.pin {
            let f = ((pin as f64 - from) / span) as f32;
            if (0. ..=1.).contains(&f) {
                plot = plot.child(place(
                    nudge,
                    div().absolute().top_0().bottom_0().w(px(1.5)).bg(p.accent),
                    f,
                    -0.75,
                ));
                // Over the first lane, the pin: pointed at, it clears the cursor.
                if first {
                    let mark = |name: &str| {
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(name, p.on_accent).size(px(12.)))
                    };
                    plot = plot.child(place(
                        nudge,
                        div()
                            .id("history-pin")
                            .group("history-pin")
                            .absolute()
                            .top(px(2.))
                            .size(px(18.))
                            .rounded_full()
                            .bg(p.accent)
                            .cursor_pointer()
                            .child(mark("MapPinIcon").group_hover("history-pin", |s| s.opacity(0.)))
                            .child(
                                mark("CloseIcon")
                                    .opacity(0.)
                                    .group_hover("history-pin", |s| s.opacity(1.)),
                            )
                            .tooltip(|window, cx| {
                                widgets::tooltip(label("history.clearCursor"), window, cx)
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.history_command(Command::ClearCursor, window, cx)
                            })),
                        f,
                        -9.,
                    ));
                }
            }
        }
        // Over the first lane, what to do with a selection: zoom to it, or let it go.
        if first
            && self.history.drag.is_none()
            && let Some((a, b)) = self.history.selection
        {
            let middle = (((a + b) as f64 / 2. - from) / span) as f32;
            if (0. ..=1.).contains(&middle) {
                let text = label("history.selection.zoom");
                let width = f32::from(theme::text_width(&text, theme::font(12.), window, cx)) + 78.;
                let plot_width = self.history_plot_width(window);
                let x =
                    (middle * plot_width - width / 2.).clamp(4., (plot_width - width - 4.).max(4.));
                plot = plot.child(
                    div()
                        .absolute()
                        .top(px(6.))
                        .left(px(x))
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .p(px(2.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(p.stroke_strong)
                        .bg(widgets::solid(p.flyout))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .id("history-zoom-selection")
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .px(px(8.))
                                .h(px(24.))
                                .rounded(px(4.))
                                .text_size(theme::font(12.))
                                .cursor_pointer()
                                .hover(move |s| s.bg(p.hover))
                                .child(icon("InterfaceScaleIcon", p.muted).size(px(13.)))
                                .child(text)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.history_command(Command::ZoomSelection, window, cx)
                                })),
                        )
                        .child(
                            div()
                                .id("history-clear-selection")
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .hover(move |s| s.bg(p.hover))
                                .child(icon("CloseIcon", p.muted).size(px(13.)))
                                .tooltip(|window, cx| {
                                    widgets::tooltip(label("history.clearCursor"), window, cx)
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.history_command(Command::ClearCursor, window, cx)
                                })),
                        ),
                );
            }
        }
        if let Some(hover) = self.history.hover
            && let Some(points) = &points
            && let Some(index) = points.index_at(hover.at)
        {
            let end = self.history_clock();
            let at = points.x_time(index, end) as f64;
            let f = ((at - from) / span) as f32;
            if (-0.002..=1.002).contains(&f) {
                plot = plot.child(place(
                    nudge,
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .w(px(1.))
                        .bg(p.ink.opacity(0.4)),
                    f,
                    0.,
                ));
                let mut dots = Vec::new();
                if let Some(reading) = &reading {
                    let main = reading.value(points, index).map(|v| {
                        if reading.is_pair() {
                            reading.part_values(points, index).map_or(v, |(a, _)| a)
                        } else {
                            v
                        }
                    });
                    dots.push((main, reading.color, axes.left));
                    if let Some((_, second)) = reading.part_values(points, index) {
                        dots.push((Some(second), reading.color.opacity(0.6), axes.left));
                    }
                }
                if let (Some(overlay), Some(right)) = (&overlay, axes.right) {
                    dots.push((overlay.value(points, index), overlay.color, right));
                }
                for (value, color, (low, high)) in dots {
                    let Some(value) = value else { continue };
                    let y = (1. - ((value - low) / (high - low).max(f64::EPSILON)).clamp(0., 1.))
                        as f32;
                    plot = plot.child(place(
                        nudge,
                        div()
                            .absolute()
                            .top(relative(y))
                            .mt(px(-4.5))
                            .size(px(9.))
                            .rounded_full()
                            .border_2()
                            .border_color(p.tip)
                            .bg(color),
                        f,
                        -4.5,
                    ));
                }
            }
        }
        // The overlay a dragged reading would make.
        if let Some(Drag::Reading {
            target: Some(Drop::Overlay(target)),
            moved: true,
            ..
        }) = &self.history.drag
            && *target == id
        {
            plot = plot.child(
                div()
                    .absolute()
                    .inset_0()
                    .border_1()
                    .border_dashed()
                    .border_color(p.accent)
                    .rounded(px(4.))
                    .bg(p.accent.opacity(0.06))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px(px(8.))
                            .py(px(3.))
                            .rounded(px(5.))
                            .bg(widgets::solid(p.flyout))
                            .border_1()
                            .border_color(p.stroke_strong)
                            .text_size(theme::font(12.))
                            .child(icon("LayersIcon", p.accent).size(px(13.)))
                            .child(label("history.drop.overlay")),
                    ),
            );
        }
        let plot = plot
            .cursor_crosshair()
            .on_mouse_move(
                cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                    if this.history.drag.is_some() {
                        return;
                    }
                    let hover = this
                        .history_time_at(event.position.x, false)
                        .map(|at| Hover {
                            position: event.position,
                            lane: lane_id,
                            at,
                        });
                    if this.history.hover != hover {
                        this.history.hover = hover;
                        this.history_ask_tip(window, cx);
                        this.history_redraw(window, cx);
                    }
                }),
            )
            .on_hover(cx.listener(|this, hovered: &bool, window, cx| {
                if !*hovered && this.history.hover.is_some() && this.history.drag.is_none() {
                    this.history.hover = None;
                    this.history_redraw(window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.history.active = lane_id;
                    if event.click_count >= 2 {
                        this.history.reset_zoom();
                        this.history.drag = None;
                        this.load_history(window, cx);
                        cx.notify();
                        return;
                    }
                    if let Some(at) = this.history_time_at(event.position.x, false) {
                        this.history.drag = Some(Drag::Select {
                            start: at,
                            moved: false,
                            x: f32::from(event.position.x),
                        });
                    }
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, window, cx| {
                    this.history_command(Command::Latest, window, cx);
                }),
            )
            .on_scroll(cx.listener(move |this, event: &ScrollEvent, window, cx| {
                let delta = event.delta.pixel_delta(px(20.));
                let dy = f32::from(delta.y);
                let dx = f32::from(delta.x);
                if dy == 0. && dx == 0. {
                    return;
                }
                // Over the plots the wheel zooms, and with Shift pans.
                cx.stop_propagation();
                if event.modifiers.shift {
                    let span = this.history.span();
                    let step = if dx != 0. { dx } else { dy };
                    this.history
                        .pan(-(span as f64 * f64::from(step) / 600.) as i64, state::FAST);
                } else if let Some(at) = this.history_time_at(event.position.x, true) {
                    this.history
                        .zoom(1.0018f64.powf(-f64::from(dy)), at, state::FAST);
                }
                this.load_history(window, cx);
                cx.notify();
            }));
        // The axes' labels either side.
        let label_column =
            |values: Option<(f64, f64)>, reading: Option<&Reading>, color: Hsla, right: bool| {
                let mut column = div().relative().w(px(GUTTER)).flex_shrink_0().h_full();
                if let (Some((low, high)), Some(reading)) = (values, reading) {
                    for row in 0..=axes.rows {
                        let f = row as f32 / axes.rows as f32;
                        let value = high - (high - low) * f64::from(f);
                        let text = reading.axis_label(value, high, bits);
                        column = column.child(
                            div()
                                .absolute()
                                .top(relative(f))
                                .mt(px(-8.))
                                .h(px(16.))
                                .when(!right, |d| d.right(px(6.)))
                                .when(right, |d| d.left(px(6.)))
                                .flex()
                                .items_center()
                                .text_size(theme::font(11.))
                                .text_color(color)
                                .whitespace_nowrap()
                                .child(text),
                        );
                    }
                }
                column
            };
        let left_labels = label_column(Some(axes.left), reading.as_ref(), p.faint, false);
        let right_labels = label_column(
            axes.right.filter(|_| !axes.shared),
            overlay.as_ref(),
            overlay.as_ref().map_or(p.faint, |o| o.color),
            true,
        );
        let weight = lane.shown.max(0.);
        let geometry_lane = geometry.clone();
        div()
            .id(("history-lane", id))
            .relative()
            .map(|mut d| {
                d.style().flex_grow = Some(weight);
                d.style().flex_shrink = Some(1.);
                d.style().flex_basis = Some(px(0.).into());
                d
            })
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                canvas(
                    move |bounds, _, _| {
                        let mut geometry = geometry_lane.borrow_mut();
                        if let Some(entry) =
                            geometry.lanes.iter_mut().find(|(i, _, _)| *i == lane_id)
                        {
                            entry.1 = bounds;
                        } else {
                            geometry.lanes.push((lane_id, bounds, Bounds::default()));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .pt(px(2.))
                    .pb(px(2.))
                    .flex()
                    .child(left_labels)
                    .child(plot)
                    .child(right_labels),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if this.history.active != lane_id {
                        this.history.active = lane_id;
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
    }

    /// A reading not recorded: why, and what records it.
    fn history_unrecorded(&self, reading: &Reading, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let (title, detail) = if reading.service {
            (
                tr!(
                    "history.unrecorded.serviceTitle",
                    name = reading.long.clone()
                ),
                label("history.unrecorded.serviceHint"),
            )
        } else {
            (
                tr!("history.unrecorded.title", name = reading.long.clone()),
                label("history.unrecorded.hint"),
            )
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .px_4()
            .text_size(theme::font(12.))
            .text_color(p.muted)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(p.ink)
                    .child(
                        icon(
                            if reading.service {
                                "ShieldAlertIcon"
                            } else {
                                "InfoIcon"
                            },
                            p.muted,
                        )
                        .size(px(16.)),
                    )
                    .child(title),
            )
            .child(div().text_color(p.faint).text_center().child(detail))
            .when(reading.service, |d| {
                d.child(
                    self.btn_small(
                        "history-service",
                        label("history.unrecorded.install"),
                        None,
                        widgets::Tone::Plain,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.act(
                            Action::Preference("settingsSection".into(), json!("permissions")),
                            window,
                            cx,
                        );
                        this.act(Action::Page("settings".into()), window, cx);
                    })),
                )
            })
            .into_any_element()
    }

    /// The button under the lanes adding one.
    fn history_add_lane(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        div()
            .id("history-add-lane")
            .absolute()
            .top(px(2.))
            .left(px(0.))
            .flex()
            .items_center()
            .gap(px(3.))
            .px(px(6.))
            .h(px(20.))
            .rounded(px(5.))
            .text_size(theme::font(11.5))
            .text_color(p.faint)
            .cursor_pointer()
            .hover(move |s| s.bg(p.hover_strong).text_color(p.ink))
            .child(icon("PlusIcon", p.faint).size(px(12.)))
            .child(label("history.lane.add"))
            .child(layout::anchor_probe(
                "history-add-lane",
                self.ui.anchors.clone(),
            ))
            .tooltip(|window, cx| widgets::tooltip(label("history.lane.addTitle"), window, cx))
            .on_click(cx.listener(|this, _, window, cx| {
                let exclude = this
                    .history
                    .live_lanes()
                    .map(|lane| lane.reading.clone())
                    .collect::<Vec<_>>();
                let menu = ReadingMenu {
                    title: Some("history.menu.addTitle"),
                    current: None,
                    exclude: &exclude,
                    pairs: true,
                    none: None,
                    add: false,
                };
                this.history_open_readings(menu, Command::AddLane, "history-add-lane", window, cx);
            }))
            .into_any_element()
    }

    /// The line where a dragged lane or reading would go.
    fn history_drop_mark(&self) -> Option<AnyElement> {
        let p = self.palette;
        let target = match &self.history.drag {
            Some(Drag::Reading {
                target: Some(Drop::Insert(index)),
                moved: true,
                ..
            }) => (*index, false),
            Some(Drag::Lane {
                target: Some(Drop::Insert(index)),
                moved: true,
                ..
            }) => (*index, true),
            _ => return None,
        };
        let geometry = self.history.geometry.borrow();
        let area = geometry.area;
        let lanes = &geometry.lanes;
        let y = if target.0 == 0 {
            lanes.first().map_or(area.top(), |(_, lane, _)| lane.top())
        } else if target.0 >= lanes.len() {
            lanes
                .last()
                .map_or(area.top(), |(_, lane, _)| lane.bottom())
        } else {
            lanes[target.0].1.top() - px(GAP_Y / 2.)
        };
        let top = y - area.top();
        Some(
            div()
                .absolute()
                .left(px(GUTTER))
                .right(px(GUTTER))
                .top(top - px(1.))
                .h(px(2.))
                .bg(p.accent)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(-11.))
                        .flex()
                        .items_center()
                        .gap_1()
                        .px(px(6.))
                        .h(px(20.))
                        .rounded(px(5.))
                        .bg(p.accent)
                        .text_color(p.on_accent)
                        .text_size(theme::font(11.))
                        .child(label(if target.1 {
                            "history.drop.move"
                        } else {
                            "history.drop.lane"
                        })),
                )
                .into_any_element(),
        )
    }

    /// What follows the pointer while a reading or a lane is dragged.
    fn history_ghost(&self) -> Option<AnyElement> {
        let p = self.palette;
        let (name, color, at) = match &self.history.drag {
            Some(Drag::Reading {
                id,
                at,
                moved: true,
                ..
            }) => {
                let reading = self.history_reading(id)?;
                (reading.long.clone(), reading.color, *at)
            }
            Some(Drag::Lane {
                id,
                at,
                moved: true,
                ..
            }) => {
                let lane = self.history.lanes.iter().find(|l| l.id == *id)?;
                let reading = self.history_reading(&lane.reading)?;
                (reading.long.clone(), reading.color, *at)
            }
            _ => return None,
        };
        Some(
            deferred(
                anchored()
                    .position(point(at.x + px(14.), at.y + px(12.)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .px(px(8.))
                            .h(px(24.))
                            .rounded(px(5.))
                            .border_1()
                            .border_color(p.stroke_strong)
                            .bg(widgets::solid(p.flyout))
                            .text_size(theme::font(12.))
                            .child(div().size(px(8.)).rounded_full().bg(color))
                            .child(name),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The card by the pointer: when, the lane's readings then, or why nothing
    /// was recorded.
    fn history_tip(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.palette;
        let hover = self.history.hover?;
        if self.history.drag.is_some() {
            return None;
        }
        let points = self.history.points.as_ref()?;
        let lane = self.history.lanes.iter().find(|l| l.id == hover.lane)?;
        let mut rows: Vec<AnyElement> = Vec::new();
        let head = |time: String, kind: Option<String>| {
            div()
                .flex()
                .items_baseline()
                .gap_2()
                .child(div().font_weight(theme::LABEL_WEIGHT).child(time))
                .when_some(kind, |d, kind| {
                    d.child(
                        div()
                            .text_size(theme::font(11.))
                            .text_color(p.faint)
                            .child(kind),
                    )
                })
                .into_any_element()
        };
        let bits = self.history_bits();
        match points.index_at(hover.at) {
            None => {
                let gap = points.gap_at(hover.at);
                let offsets = self.history_offsets_differ();
                rows.push(head(
                    with_offset(moment_label(hover.at, false), hover.at, offsets),
                    None,
                ));
                rows.push(
                    div()
                        .text_color(p.muted)
                        .child(match gap {
                            Some(gap) => tr!("history.gap.tip", reason = label(gap.reason.key())),
                            None => label("history.noRecordAt"),
                        })
                        .into_any_element(),
                );
            }
            Some(index) => {
                let start = points.ts[index];
                let length = points.span[index];
                let (time, kind) = if length <= FRAME {
                    (
                        moment_label(start, true),
                        Some(label(if length < FRAME {
                            "history.tip.second"
                        } else {
                            "history.tip.frame"
                        })),
                    )
                } else {
                    (
                        range_label(start, start + length, length < MINUTE),
                        Some(label("history.tip.average")),
                    )
                };
                rows.push(head(
                    with_offset(time, start, self.history_offsets_differ()),
                    kind,
                ));
                let mut readings = vec![self.history_reading(&lane.reading)];
                readings.push(
                    lane.overlay
                        .as_deref()
                        .and_then(|id| self.history_reading(id)),
                );
                for reading in readings.into_iter().flatten() {
                    let value = reading
                        .value(points, index)
                        .map(|v| reading.format(v, bits))
                        .unwrap_or_else(|| label("format.missing"));
                    rows.push(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(8.))
                                    .flex_shrink_0()
                                    .rounded(px(2.))
                                    .bg(reading.color),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(p.muted)
                                    .child(reading.long.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .font_weight(theme::LABEL_WEIGHT)
                                    .font_features(theme::tabular())
                                    .child(value),
                            )
                            .into_any_element(),
                    );
                    if let (Some((a, b)), Some((na, nb))) =
                        (reading.part_values(points, index), &reading.parts)
                    {
                        rows.push(
                            div()
                                .pl(px(16.))
                                .truncate()
                                .text_size(theme::font(11.))
                                .text_color(p.faint)
                                .child(format!(
                                    "{na} {} · {nb} {}",
                                    reading.format(a, bits),
                                    reading.format(b, bits)
                                ))
                                .into_any_element(),
                        );
                    }
                    if length > FRAME
                        && let Some(peak) = reading.peak_value(points, index)
                    {
                        rows.push(
                            div()
                                .pl(px(16.))
                                .truncate()
                                .text_size(theme::font(11.))
                                .text_color(p.faint)
                                .child(tr!(
                                    "history.metric.peak",
                                    value = reading.format(peak, bits)
                                ))
                                .into_any_element(),
                        );
                    }
                }
                // The processes busiest by the lane's reading then: the frame's, or
                // the bucket's averages. Their rows stay while fewer are known,
                // and a long name ends in an ellipsis.
                let key = self.history_trend_of(lane);
                let ranked = self.history_tip_ranking(key);
                let live = values(&self.snapshot["processes"]);
                let mut list = div()
                    .mt(px(3.))
                    .pt(px(6.))
                    .border_t_1()
                    .border_color(p.stroke)
                    .flex()
                    .flex_col()
                    .gap(px(4.));
                for slot in 0..TIP_PROCESSES {
                    let row = div().flex().items_center().gap_2();
                    list = list.child(match ranked.get(slot) {
                        Some((name, value)) => {
                            let icon_of = live
                                .iter()
                                .find(|process| text(process, "name").eq_ignore_ascii_case(name));
                            row.child(self.process_icon(icon_of, 16., cx))
                                .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(p.muted)
                                        .font_features(theme::tabular())
                                        .child(table::format_process(key, *value, bits)),
                                )
                        }
                        None => row
                            .child(div().size(px(16.)).flex_shrink_0())
                            .child("\u{a0}"),
                    });
                }
                rows.push(list.into_any_element());
            }
        }
        rows.push(
            div()
                .mt(px(3.))
                .pt(px(6.))
                .border_t_1()
                .border_color(p.stroke)
                .text_size(theme::font(11.))
                .text_color(p.faint)
                .child(label("history.tip.hint"))
                .into_any_element(),
        );
        // Beside the pointer, on its left where the right has no room: at a
        // fixed width, it turns over at one place and keeps its size.
        let flip = hover.position.x + px(TIP_GAP + TIP_WIDTH)
            > window.viewport_size().width - px(TIP_MARGIN);
        Some(
            deferred(
                anchored()
                    .anchor(if flip {
                        gpui_kit::gpui::Anchor::TopRight
                    } else {
                        gpui_kit::gpui::Anchor::TopLeft
                    })
                    .position(point(
                        if flip {
                            hover.position.x - px(TIP_GAP)
                        } else {
                            hover.position.x + px(TIP_GAP)
                        },
                        hover.position.y + px(16.),
                    ))
                    .snap_to_window_with_margin(px(TIP_MARGIN))
                    .child(
                        div()
                            .w(px(TIP_WIDTH))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(p.stroke_strong)
                            .bg(widgets::solid(p.tip))
                            .text_size(theme::font(12.))
                            .line_height(relative(1.4))
                            .children(rows),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// A lane's title row: its grip, reading, value, overlay and remove button.
    #[allow(clippy::too_many_arguments)]
    fn history_lane_header(
        &self,
        lane: &Lane,
        presence: f32,
        multi: bool,
        axes: &Axes,
        read_index: Option<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let id = lane.id;
        let bits = self.history_bits();
        let points = self.history.points.as_ref();
        let reading = self.history_reading(&lane.reading);
        let active = multi && self.history.active == id;
        let value_size = theme::font(13.);
        let mut row = div()
            .flex_none()
            .h(px(HEADER * presence))
            .opacity(presence)
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(if multi { 0. } else { GUTTER - 8. }).max(px(0.)))
            .pr(px(GUTTER - 8.))
            // The active lane (of several) is the one a reading's click shows in.
            .rounded(px(6.))
            .when(active, |d| d.bg(p.hover));
        if multi {
            row = row.child(
                div()
                    .id(("history-grip", id))
                    .w(px(GUTTER - 8.))
                    .flex_shrink_0()
                    .flex()
                    .justify_end()
                    .cursor_grab()
                    .child(icon("GripIcon", p.faint).size(px(14.)))
                    .tooltip(|window, cx| widgets::tooltip(label("history.lane.grip"), window, cx))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.history.drag = Some(Drag::Lane {
                                id,
                                start: event.position,
                                at: event.position,
                                moved: false,
                                target: None,
                            });
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            );
        }
        let (name, color) = reading.map_or_else(
            || (lane.reading.clone(), p.muted),
            |r| (r.long.clone(), r.color),
        );
        let anchor = format!("history-lane-name-{id}");
        row = row.child(
            div()
                .id(("history-lane-name", id))
                .relative()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(6.))
                .h(px(24.))
                .rounded(px(5.))
                .min_w_0()
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover_strong))
                .child(div().size(px(8.)).rounded(px(2.)).bg(color).flex_shrink_0())
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(theme::font(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.ink)
                        .child(name),
                )
                .child(icon("ChevronDown", p.faint).size(px(13.)))
                .child(layout::anchor_probe(
                    anchor.clone(),
                    self.ui.anchors.clone(),
                ))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.history.active = id;
                    let current = this
                        .history
                        .lanes
                        .iter()
                        .find(|l| l.id == id)
                        .map(|l| l.reading.clone());
                    let menu = ReadingMenu {
                        title: None,
                        current: current.as_deref(),
                        exclude: &[],
                        pairs: true,
                        none: None,
                        add: true,
                    };
                    let make = move |reading| Command::Reading(id, reading);
                    this.history_open_readings(menu, make, &anchor, window, cx);
                })),
        );
        if let Some(reading) = reading {
            let width = slot_width(
                &reading.widest_samples(bits),
                value_size,
                FontWeight::SEMIBOLD,
                window,
                cx,
            );
            let value = points
                .zip(read_index)
                .and_then(|(points, index)| reading.value(points, index))
                .map(|v| reading.format(v, bits))
                .unwrap_or_else(|| label("format.missing"));
            row = row.child(
                div()
                    .flex_shrink_0()
                    .min_w(width)
                    .text_right()
                    .text_size(value_size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_features(theme::tabular())
                    .child(value),
            );
            // A pair's parts, or the highest sample of a bucket.
            if let Some((first, second)) = &reading.parts {
                let small = theme::font(12.);
                let part_width = slot_width(
                    &reading.widest_samples(bits),
                    small,
                    FontWeight::NORMAL,
                    window,
                    cx,
                );
                let parts = points
                    .zip(read_index)
                    .and_then(|(points, index)| reading.part_values(points, index));
                for (index, name) in [first, second].into_iter().enumerate() {
                    let text = parts
                        .map(|(a, b)| reading.format(if index == 0 { a } else { b }, bits))
                        .unwrap_or_else(|| label("format.missing"));
                    row = row.child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .text_size(small)
                            .text_color(p.muted)
                            .child(charts::swatch(
                                if index == 0 {
                                    reading.color
                                } else {
                                    reading.color.opacity(0.5)
                                },
                                false,
                                px(12.),
                            ))
                            .child(name.clone())
                            .child(
                                div()
                                    .min_w(part_width)
                                    .text_right()
                                    .font_features(theme::tabular())
                                    .text_color(p.ink)
                                    .child(text),
                            ),
                    );
                }
            } else if reading.shows_peak() {
                let small = theme::font(12.);
                let peak = points.zip(read_index).and_then(|(points, index)| {
                    (points.span[index] > FRAME)
                        .then(|| reading.peak_value(points, index))
                        .flatten()
                });
                let samples = reading
                    .widest_samples(bits)
                    .into_iter()
                    .map(|v| tr!("history.metric.peak", value = v))
                    .collect::<Vec<_>>();
                let width = slot_width(&samples, small, FontWeight::NORMAL, window, cx);
                row = row.child(
                    div()
                        .flex_shrink_0()
                        .min_w(width)
                        .text_size(small)
                        .text_color(p.faint)
                        .font_features(theme::tabular())
                        .child(peak.map_or(String::new(), |v| {
                            tr!("history.metric.peak", value = reading.format(v, bits))
                        })),
                );
            }
            // The overlay on the right axis, or a button adding one.
            if !reading.is_pair() {
                row = row.child(self.history_overlay_chip(lane, axes, read_index, window, cx));
            }
            // The process laid over this lane, and a way to take it off.
            if let Some(name) = self
                .history
                .curve
                .clone()
                .filter(|_| reading.process.is_some())
            {
                row = row.child(
                    div()
                        .id(("history-curve", id))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .pl(px(8.))
                        .pr(px(2.))
                        .h(px(22.))
                        .rounded(px(11.))
                        .border_1()
                        .border_color(p.stroke_strong)
                        .text_size(theme::font(12.))
                        .child(charts::swatch(p.ink.opacity(0.6), true, px(12.)))
                        .child(div().text_color(p.muted).child(name))
                        .child(
                            div()
                                .id(("history-curve-remove", id))
                                .size(px(18.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(move |s| s.bg(p.hover))
                                .child(icon("CloseIcon", p.muted).size(px(12.)))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.history_command(Command::Curve(None), window, cx)
                                })),
                        ),
                );
            }
        }
        row = row.child(div().flex_1());
        if multi {
            row = row.child(
                div()
                    .id(("history-lane-remove", id))
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(p.hover))
                    .child(icon("CloseIcon", p.muted).size(px(14.)))
                    .tooltip(|window, cx| {
                        widgets::tooltip(label("history.lane.remove"), window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.history_command(Command::RemoveLane(id), window, cx)
                    })),
            );
        }
        row.into_any_element()
    }

    /// The lane's second reading on the right axis, as a dashed chip that
    /// changes it (its × takes it off), or a quiet button adding one.
    fn history_overlay_chip(
        &self,
        lane: &Lane,
        axes: &Axes,
        read_index: Option<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let id = lane.id;
        let bits = self.history_bits();
        let primary = lane.reading.clone();
        let anchor = format!("history-overlay-{id}");
        let probe = layout::anchor_probe(anchor.clone(), self.ui.anchors.clone());
        let Some(overlay) = lane
            .overlay
            .as_deref()
            .and_then(|o| self.history_reading(o))
        else {
            return div()
                .id(("history-overlay-add", id))
                .relative()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(8.))
                .h(px(22.))
                .rounded(px(11.))
                .text_size(theme::font(12.))
                .text_color(p.faint)
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover_strong).text_color(p.ink))
                .child(icon("PlusIcon", p.faint).size(px(12.)))
                .child(label("history.overlay.add"))
                .child(probe)
                .tooltip(|window, cx| {
                    widgets::tooltip(label("history.overlay.addTitle"), window, cx)
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    let menu = ReadingMenu {
                        title: Some("history.menu.overlayTitle"),
                        current: None,
                        exclude: std::slice::from_ref(&primary),
                        pairs: false,
                        none: None,
                        add: false,
                    };
                    let make = move |reading| Command::Overlay(id, Some(reading));
                    this.history_open_readings(menu, make, &anchor, window, cx);
                }))
                .into_any_element();
        };
        let size = theme::font(12.5);
        // Without the privileged service it has no records: it says so instead.
        let missing = overlay.service && !overlay.recorded;
        let reading = if missing {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .text_color(p.faint)
                .child(label("history.menu.needsService"))
                .child(icon("ShieldAlertIcon", p.io).size(px(12.)))
        } else {
            let value = self
                .history
                .points
                .as_ref()
                .zip(read_index)
                .and_then(|(points, index)| overlay.value(points, index))
                .map(|v| overlay.format(v, bits))
                .unwrap_or_else(|| label("format.missing"));
            let width = slot_width(
                &overlay.widest_samples(bits),
                size,
                FontWeight::SEMIBOLD,
                window,
                cx,
            );
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .min_w(width)
                        .text_right()
                        .text_color(p.ink)
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_features(theme::tabular())
                        .child(value),
                )
                .child(
                    div()
                        .text_size(theme::font(11.))
                        .text_color(p.faint)
                        .child(label(if axes.shared {
                            "history.overlay.shared"
                        } else {
                            "history.overlay.right"
                        })),
                )
        };
        let current = overlay.id.clone();
        div()
            .id(("history-overlay", id))
            .relative()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .pl(px(9.))
            .pr(px(3.))
            .h(px(24.))
            .rounded(px(12.))
            .border_1()
            .border_dashed()
            .border_color(overlay.color.opacity(0.7))
            .text_size(size)
            .text_color(p.muted)
            .cursor_pointer()
            .hover(move |s| s.bg(p.hover))
            .child(charts::swatch(overlay.color, false, px(10.)))
            .child(overlay.long.clone())
            .child(reading)
            .child(
                div()
                    .id(("history-overlay-remove", id))
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .hover(move |s| s.bg(p.hover_strong))
                    .child(icon("CloseIcon", p.faint).size(px(12.)))
                    .tooltip(|window, cx| {
                        widgets::tooltip(label("history.overlay.remove"), window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.history_command(Command::Overlay(id, None), window, cx)
                    })),
            )
            .child(probe)
            .tooltip(|window, cx| widgets::tooltip(label("history.overlay.change"), window, cx))
            .on_click(cx.listener(move |this, _, window, cx| {
                let menu = ReadingMenu {
                    title: Some("history.menu.overlayTitle"),
                    current: Some(&current),
                    exclude: std::slice::from_ref(&primary),
                    pairs: false,
                    none: Some(Command::Overlay(id, None)),
                    add: false,
                };
                let make = move |reading| Command::Overlay(id, Some(reading));
                this.history_open_readings(menu, make, &anchor, window, cx);
            }))
            .into_any_element()
    }
}
