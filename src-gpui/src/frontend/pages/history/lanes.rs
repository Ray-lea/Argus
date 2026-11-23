//! The chart card: lanes of readings, each with its title, axes and plot; the
//! time axis under them; the cursor, selection and drop marks over them; and
//! the navigator below.
use super::axes::{Axes, AxisEase, Eased, Mark, crossfade_marks, marks};
use super::bar::ReadingMenu;
use super::readings::Reading;
use super::series::{BANDED, Part, Points, STEP_CORNER, tick_step, ticks};
use super::state::{Drag, Drop, Glide, Hover, Lane};
use super::transition::OverlayFade;
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

/// The chart card's height that gives `lanes` lanes room to read: about
/// 128 px each, with the axis and the navigator.
pub(super) fn comfortable_height(lanes: f32) -> f32 {
    lanes * 128. + (lanes - 1.).max(0.) * GAP_Y + AXIS + NAV + 2. * PAD + 8.
}

/// A lane's curves fading out on a range switch: drawn on the axes and in
/// the buckets of the window shown before it (`Crossfade::from`), with the
/// points it showed (`HistoryState::previous`).
pub(super) struct Leaving {
    axes: Axes,
    bucket: i64,
    window: (f64, f64),
    alpha: f32,
}

/// What a lane is drawn with besides its own settings.
pub(super) struct LaneDraw<'a> {
    /// How far it has come in (or gone), 0 to 1.
    presence: f32,
    /// Whether there are other lanes.
    multi: bool,
    /// Its axes as they ease, and their rows.
    eased: &'a Eased,
    /// The points drawn.
    points: Option<&'a Points>,
    /// A range switch: the curves fading out, and how opaque the new ones are.
    fade: Option<(&'a Points, &'a Leaving, f32)>,
    /// The overlay coming in or going, and how opaque it is.
    overlay_fade: Option<&'a (OverlayFade, f32)>,
    /// The bucket width asked for.
    bucket: i64,
    /// The window shown.
    window: (f64, f64),
    /// The plot's width as laid out last: measured before the lanes record
    /// their places anew (`history_plot_width` falls back to an estimate from
    /// the window while they do, and marks placed by it stood off their times).
    plot_width: f32,
    /// The clock lines' times, and those drawn into the curves' textures
    /// while the window glides.
    ticks: &'a Arc<Vec<i64>>,
    clock_lines: Option<&'a (Arc<Vec<f64>>, Hsla)>,
    /// The point the readings in the title show.
    read_index: Option<usize>,
}

/// A lane's curve: `series` on the left axis of `axes`, shown in the window
/// `shown` and drawn for the window from `drawn.0`, as long as `drawn.1`
/// (none: the window shown).
fn history_curve(
    id: String,
    series: Vec<charts::Series>,
    axes: &Axes,
    (from, to): (f64, f64),
    drawn: (f64, Option<f64>),
    ticks: Option<(Arc<Vec<f64>>, Hsla)>,
) -> charts::TimePlot {
    charts::TimePlot {
        id: id.into(),
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
        ticks,
        geometry_start: drawn.0,
        measured_width: Rc::new(Cell::new(1.)),
        live_clock: None,
        image_surface: Default::default(),
        dash_phases: Default::default(),
        rescale: Default::default(),
        readout: Default::default(),
        captions: None,
        redraw_alone: true,
        raster_span: drawn.1,
        corner: STEP_CORNER,
    }
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

    /// The series a lane draws: the band's top (CPU's highest samples), the
    /// reading (a pair's second part as a dim line), the overlay on its axis.
    fn history_lane_series(
        &self,
        lane: &Lane,
        // The overlay drawn (one going included) and how opaque it is.
        overlay: Option<(&str, f32)>,
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
                .source(source("band"))
                .layer(true),
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
        if let Some((overlay, alpha)) =
            overlay.and_then(|(id, alpha)| Some((self.history_reading(id)?, alpha)))
            && let Some(right) = axes.right
        {
            let left = axes.left;
            // A pair's parts as in a lane of its own, the second dimmer.
            let mut lines = vec![(
                overlay.main_key(points).to_owned(),
                overlay.color.opacity(alpha),
                "overlay",
            )];
            if overlay.is_pair() && points.has(&overlay.keys[1]) {
                lines.push((
                    overlay.keys[1].clone(),
                    overlay.color.opacity(0.5 * alpha),
                    "overlay-second",
                ));
            }
            for (key, color, part) in lines {
                series.push(
                    charts::Series::new(
                        String::new(),
                        points.line(&key, Part::Mean, end, |v| onto(v, right, left)),
                        color,
                        format,
                    )
                    .fill(0.)
                    .source(charts::hash_of(("history", lane.id, &overlay.id, part)))
                    // Coming and going with the overlay, it leaves the lane's
                    // curve on screen while its texture is drawn.
                    .layer(true),
                );
            }
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
        // Its share of the page; lanes that do not fit there scroll inside.
        (content * self.history.split).max(260.)
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
        // The points drawn: spliced with those they replaced while the window
        // eases from them, for the lanes and the navigator alike.
        let mut drawn = self.history.drawn_points();
        // Following a range in buckets, the curves end at the newest complete
        // bucket, and one that completes is drawn on (`reveal_to`).
        self.history.reveal_moving = false;
        if self.history.follow
            && self.history.fading.is_none()
            && !self.history_flowing(cx)
            && let Some(points) = drawn.as_mut().filter(|points| points.bucket > SECOND)
        {
            let motion = self.history_motion(cx);
            let (cutoff, moving) =
                self.history
                    .reveal_to(points.settled, points.bucket, Instant::now(), motion);
            *points = points.until(cutoff);
            self.history.reveal_moving = moving;
        }
        let lanes = self.history_lanes(shown, drawn.as_ref(), window, cx);
        let navigator = self.history_navigator(drawn.as_ref(), window, cx);
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
        points: Option<&Points>,
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
        // A range switch cross-fades the curves drawn for the window before
        // it into those drawn for the window it goes to: no clock lines in
        // their textures then (those are drawn for one window).
        let fading = self.history.fading;
        let leaving_points = fading.and(self.history.previous.clone());
        let arriving_points = fading.and(self.history.points.clone());
        let view = self.history.view;
        let clock_lines =
            self.history
                .flow_origin
                .filter(|_| fading.is_none())
                .map(|(origin, _)| {
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
        let motion = self.history_motion(cx);
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
        // The lanes share the card's height by their weights down to their
        // narrowest (`MIN_LANE`); more than fit scroll there, under the time
        // axis and the navigator that stay put. The wheel scrolls them over
        // their titles and axes; over the plots it zooms.
        let lanes_scroll = self.history.lanes_scroll.clone();
        // A lane dragged by its grip follows the pointer over the others,
        // drawn last; its place is a slot, and the others glide aside.
        let now = Instant::now();
        let lift = self.history.lanes_order.lifted(state::LANES);
        let mut lanes = lanes;
        if let Some(lift) = &lift {
            lanes.sort_by_key(|lane| {
                lift.order
                    .iter()
                    .position(|key| *key == lane.id.to_string())
                    .unwrap_or(usize::MAX)
            });
        }
        let mut least = 0.;
        let mut stack = div().relative().flex().flex_col().child(
            canvas(
                self.history.lanes_order.recorder(state::LANES, "", ""),
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        );
        let mut lifted = None;
        let mut previous: Option<f32> = None;
        let mut axes_moving = false;
        self.history
            .axes
            .retain(|id, _| lanes.iter().any(|lane| lane.id == *id));
        for (index, lane) in lanes.iter().enumerate() {
            let presence = lane.presence();
            if let Some(before) = previous {
                let presence = presence.min(before);
                least += GAP_Y * presence;
                stack = stack.child(self.history_divider(index - 1, presence, cx));
            }
            previous = Some(presence);
            least += state::MIN_LANE * presence;
            let plot_height = measured.get(&lane.id).copied().unwrap_or(120.);
            // On a range switch the new curves stand on the axes of the window
            // they are drawn for, the old ones on those of theirs; the rows
            // cross-fade, and the axes then start from the new ones.
            let target = match fading {
                Some(_) => {
                    self.history_axes(lane, arriving_points.as_ref(), view, bucket, plot_height)
                }
                None => self.history_axes(lane, points, window_ms, bucket, plot_height),
            };
            let leaving = fading.zip(leaving_points.as_ref()).map(|(fade, old)| {
                let window = (fade.from.0.floor() as i64, fade.from.1.ceil() as i64);
                let old_bucket = series::bucket_for(window.1 - window.0, width);
                Leaving {
                    axes: self.history_axes(lane, Some(old), window, old_bucket, plot_height),
                    bucket: old_bucket,
                    window: fade.from,
                    alpha: 1. - fade.alpha,
                }
            });
            // An overlay that comes in or goes fades (`OverlayFade`); the right
            // axis one goes with is kept for its fading curve.
            let seen = self
                .history
                .overlays_seen
                .insert(lane.id, lane.overlay.clone());
            if let Some(was) = seen
                && was != lane.overlay
            {
                let fade = match (&was, &lane.overlay) {
                    _ if !motion => None,
                    (_, Some(id)) => Some(OverlayFade {
                        id: id.clone(),
                        right: None,
                        coming: true,
                        started: now,
                    }),
                    (Some(id), None) => Some(OverlayFade {
                        id: id.clone(),
                        right: self
                            .history
                            .axes
                            .get(&lane.id)
                            .and_then(|ease| ease.target().right),
                        coming: false,
                        started: now,
                    }),
                    (None, None) => None,
                };
                match fade {
                    Some(fade) => {
                        self.history.overlay_fades.insert(lane.id, fade);
                    }
                    None => {
                        self.history.overlay_fades.remove(&lane.id);
                    }
                }
            }
            let overlay_fade = self
                .history
                .overlay_fades
                .get(&lane.id)
                .and_then(|fade| fade.alpha(now).map(|alpha| (fade.clone(), alpha)));
            if overlay_fade.is_none() {
                self.history.overlay_fades.remove(&lane.id);
            }
            axes_moving |= overlay_fade.is_some();
            let readings = (lane.reading.clone(), lane.overlay.clone());
            let mut eased = self
                .history
                .axes
                .entry(lane.id)
                .or_insert_with(|| AxisEase::new(target, readings.clone()))
                .toward(target, readings, now, motion && fading.is_none());
            if let (Some(fade), Some(old)) = (fading, &leaving) {
                eased.left = crossfade_marks(
                    (old.axes.left, old.axes.rows),
                    (target.left, target.rows),
                    fade.alpha,
                );
                if let (Some(was), Some(will)) = (old.axes.right, target.right) {
                    eased.right =
                        crossfade_marks((was, old.axes.rows), (will, target.rows), fade.alpha);
                }
            }
            axes_moving |= eased.moving || fading.is_some();
            let fade = fading
                .and(leaving_points.as_ref())
                .zip(leaving.as_ref())
                .map(|(old, leaving)| (old, leaving, fading.map_or(1., |fade| fade.alpha)));
            let draw = LaneDraw {
                presence,
                multi,
                eased: &eased,
                points: if fading.is_some() {
                    arriving_points.as_ref()
                } else {
                    points
                },
                fade,
                overlay_fade: overlay_fade.as_ref(),
                bucket,
                window: (from, to),
                plot_width: width,
                ticks: &tick_list,
                clock_lines: clock_lines.as_ref(),
                read_index,
            };
            let element = self.history_lane(lane, draw, window, cx);
            let key = lane.id.to_string();
            match &lift {
                Some(lift) if lift.key == key => {
                    // Its place, to drop it into, as tall as it was.
                    stack = stack.child(
                        div()
                            .relative()
                            .flex_none()
                            .h(lift.start.size.height)
                            .rounded(px(6.))
                            .border_1()
                            .border_dashed()
                            .border_color(p.accent.opacity(0.5))
                            .bg(p.accent.opacity(0.06))
                            .child(
                                canvas(
                                    self.history.lanes_order.recorder(state::LANES, "", &key),
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .inset_0(),
                            ),
                    );
                    lifted = Some((element, lift.clone()));
                }
                _ => {
                    let offset = self.history.lanes_order.offset(state::LANES, "", &key, now);
                    stack = stack.child(crate::frontend::shift::shifted(element, offset, 1.));
                }
            }
        }
        self.history.axes_moving = axes_moving;
        if let Some((element, lift)) = lifted
            && let Some(part) = self.history.lanes_order.bounds(state::LANES, "", "")
        {
            // Up and down only, within the lanes.
            let room = (part.size.height - lift.start.size.height).max(px(0.));
            let top = (lift.origin().y - part.top()).clamp(px(0.), room);
            stack = stack.child(
                div()
                    .absolute()
                    .left(lift.start.left() - part.left())
                    .top(top)
                    .w(lift.start.size.width)
                    .h(lift.start.size.height)
                    .flex()
                    .flex_col()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.accent.opacity(0.6))
                    .bg(p.chart_surface)
                    .cursor_grabbing()
                    .child(element),
            );
        }
        column = column.child(
            div()
                .id("history-lanes")
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                // Clipping at the card's edge rather than the lanes': a long
                // axis label reaches into the card's padding.
                .mx(px(-PAD))
                .px(px(PAD))
                .track_scroll(&lanes_scroll)
                .overflow_y_scroll()
                .child(stack.flex_1().min_h(px(least)))
                .child(scrollbar::scrollbar(
                    "history-lanes-scrollbar",
                    &lanes_scroll,
                    p.dark,
                )),
        );
        // The time axis.
        let axis = div()
            .flex_none()
            .h(px(AXIS) + theme::text_growth(11., 1.4))
            .flex()
            .child(div().w(px(GUTTER)).flex_shrink_0())
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

    fn history_lane(
        &mut self,
        lane: &Lane,
        draw: LaneDraw,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let LaneDraw {
            presence,
            multi,
            eased,
            points,
            fade,
            overlay_fade,
            bucket,
            window: (from, to),
            plot_width,
            ticks: tick_list,
            clock_lines,
            read_index,
        } = draw;
        let p = self.palette;
        let id = lane.id;
        let span = (to - from).max(1.);
        let reading = self.history_reading(&lane.reading).cloned();
        // The overlay drawn: the lane's, or one going, on the axis it went with.
        let going = overlay_fade.filter(|(fade, _)| !fade.coming);
        let overlay_id = going.map_or(lane.overlay.clone(), |(fade, _)| Some(fade.id.clone()));
        let overlay_alpha = overlay_fade.map_or(1., |(_, alpha)| *alpha);
        let overlay = overlay_id
            .as_deref()
            .and_then(|id| self.history_reading(id))
            .cloned();
        let drawn_axes = Axes {
            right: eased.axes.right.or(going.and_then(|(fade, _)| fade.right)),
            ..eased.axes
        };
        let axes = &drawn_axes;
        let going_marks = going
            .and_then(|(fade, _)| fade.right)
            .filter(|_| eased.axes.right.is_none())
            .map(|right| marks(right, (right, eased.axes.rows), None, 1.));
        let bits = self.history_bits();
        let header =
            self.history_lane_header(lane, presence, multi, &eased.axes, read_index, window, cx);
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
        // The rows, sliding with the curve and crossfading while the axis
        // eases to a new range.
        for mark in &eased.left {
            plot = plot.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(mark.at))
                    .when(mark.at > 0.999, |d| d.mt(px(-1.)))
                    .h(px(1.))
                    .bg(p.stroke.opacity(mark.alpha)),
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
            width: plot_width,
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
        // Gaps the line breaks for: hatched, named where there is room. On a
        // range switch those of the window before it fade out with its curves.
        let hatched = points
            .map(|points| (points, fade.map_or(1., |(_, _, alpha)| alpha)))
            .into_iter()
            .chain(fade.map(|(old, leaving, _)| (old, leaving.alpha)));
        for (points, alpha) in hatched {
            for gap in points.shown_gaps() {
                let a = ((gap.from as f64 - from) / span).clamp(0., 1.) as f32;
                let b = ((gap.to as f64 - from) / span).clamp(0., 1.) as f32;
                if b - a <= 0. {
                    continue;
                }
                let wide = (b - a) * plot_width > 58.;
                plot = plot.child(band(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .when(alpha < 1., |d| d.opacity(alpha))
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
        let generation = self.history.drawn_generation();
        if let (Some((old, leaving, _)), Some(_)) = (fade, &reading) {
            // The window's curves before the switch, as drawn for it, fading out.
            let series = self.history_lane_series(
                lane,
                lane.overlay.as_deref().map(|id| (id, 1.)),
                old,
                &leaving.axes,
                leaving.bucket,
            );
            let curve = history_curve(
                format!("history-lane-{id}-{}", generation.wrapping_sub(1)),
                series,
                &leaving.axes,
                (from, to),
                (leaving.window.0, Some(leaving.window.1 - leaving.window.0)),
                None,
            );
            plot = plot.child(
                div()
                    .absolute()
                    .inset_0()
                    .opacity(leaving.alpha)
                    .child(curve),
            );
        }
        if let (Some(points), Some(_)) = (&points, &reading) {
            let series = self.history_lane_series(
                lane,
                overlay_id.as_deref().map(|id| (id, overlay_alpha)),
                points,
                axes,
                bucket,
            );
            let (view_from, view_to) = self.history.view;
            let curve = history_curve(
                format!("history-lane-{id}-{generation}"),
                series,
                axes,
                (from, to),
                match fade {
                    // Drawn once for the window the switch goes to.
                    Some(_) => (view_from as f64, Some((view_to - view_from) as f64)),
                    // While the window glides, the texture stays the one drawn
                    // for the newest reading's window, moved under the window on
                    // screen.
                    None => (
                        self.history.flow_origin.map_or(from, |(lanes, _)| lanes),
                        None,
                    ),
                },
                clock_lines.cloned(),
            );
            if let Some((_, _, alpha)) = fade {
                plot = plot.child(div().absolute().inset_0().opacity(alpha).child(curve));
            } else if self.history.flow_origin.is_some() {
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
                plot = plot.child(curve);
            } else {
                plot = plot.child(curve);
            }
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
        // The pinned moment in the lane it was pinned in (else the first).
        let pinned_here = match self.history.pin_lane {
            Some(lane) if self.history.live_lanes().any(|l| l.id == lane) => lane == id,
            _ => first,
        };
        if let Some(pin) = self.history.pin.filter(|_| pinned_here) {
            let f = ((pin as f64 - from) / span) as f32;
            if (0. ..=1.).contains(&f) {
                plot = plot.child(place(
                    nudge,
                    div().absolute().top_0().bottom_0().w(px(1.5)).bg(p.accent),
                    f,
                    -0.75,
                ));
                // Its mark: pointed at, it clears the cursor.
                {
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
        // The cursor: a line where the pointer is, through a dot on each curve
        // at the value its step holds there. The time under the pointer is
        // read again with every draw, so the line keeps still on the pointer's
        // device pixel while the curves glide under it, its dots taking the
        // values that pass.
        let cursor = self
            .history
            .hover
            .filter(|hover| hover.lane == id)
            .and_then(|hover| {
                let scale = window.scale_factor();
                let x = ((hover.at as f64 - from) / span) as f32 * plot_width;
                let x = (x * scale).round() / scale;
                (0. ..plot_width).contains(&x).then_some((hover.at, x))
            });
        if let Some((at, x)) = cursor {
            plot = plot.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(x))
                    .w(px(1.))
                    .bg(p.ink.opacity(0.4)),
            );
            if let Some(points) = &points
                && let Some(index) = points.index_at(at)
            {
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
                    match overlay.part_values(points, index) {
                        Some((first, second)) => {
                            dots.push((Some(first), overlay.color, right));
                            dots.push((Some(second), overlay.color.opacity(0.6), right));
                        }
                        None => dots.push((overlay.value(points, index), overlay.color, right)),
                    }
                }
                for (value, color, (low, high)) in dots {
                    let Some(value) = value else { continue };
                    let y = (1. - ((value - low) / (high - low).max(f64::EPSILON)).clamp(0., 1.))
                        as f32;
                    // Centred on the line.
                    plot = plot.child(
                        div()
                            .absolute()
                            .left(px(x - 4.))
                            .top(relative(y))
                            .mt(px(-4.5))
                            .size(px(9.))
                            .rounded_full()
                            .border_2()
                            .border_color(p.tip)
                            .bg(color),
                    );
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
            .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                // Only its own: leaving for the next lane can come after that
                // lane took the pointer.
                if !*hovered
                    && this
                        .history
                        .hover
                        .is_some_and(|hover| hover.lane == lane_id)
                    && this.history.drag.is_none()
                {
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
                    // About the pointer, following or not: the window holds
                    // still there (the live button resumes following).
                    let following = this.history.follow;
                    if this
                        .history
                        .zoom_at(1.0018f64.powf(-f64::from(dy)), at, state::FAST)
                        && following
                    {
                        this.history_toast("history.toast.paused");
                    }
                }
                this.load_history(window, cx);
                cx.notify();
            }));
        // The axes' labels either side.
        let label_column = |marks: &[Mark], reading: Option<&Reading>, color: Hsla, right: bool| {
            let mut column = div().relative().w(px(GUTTER)).flex_shrink_0().h_full();
            if let Some(reading) = reading {
                for mark in marks {
                    let text = reading.axis_label(mark.value, mark.top, bits);
                    // Centred on its line; the lowest just above it, so it
                    // stays in the lane and clear of the time axis below (a
                    // row easing to the bottom moves up as it comes).
                    let lift = -8. - 8. * ((mark.at - 0.94) / 0.06).clamp(0., 1.);
                    column = column.child(
                        div()
                            .absolute()
                            .top(relative(mark.at))
                            .mt(px(lift))
                            .h(px(16.))
                            .when(!right, |d| d.right(px(6.)))
                            .when(right, |d| d.left(px(6.)))
                            .flex()
                            .items_center()
                            .text_size(theme::font(11.))
                            .text_color(color.opacity(mark.alpha))
                            .whitespace_nowrap()
                            .child(text),
                    );
                }
            }
            column
        };
        let left_labels = label_column(&eased.left, reading.as_ref(), p.faint, false);
        // The right side labels the overlay's axis, fading with it; without a
        // second axis it repeats the left one's labels, in the same unit.
        let right_marks: Vec<Mark> = match (&overlay, going_marks) {
            (Some(_), Some(going)) => going,
            (Some(_), None) if !axes.shared && !eased.right.is_empty() => eased.right.clone(),
            _ => Vec::new(),
        };
        let right_labels = if right_marks.is_empty() {
            label_column(&eased.left, reading.as_ref(), p.faint, true)
        } else {
            let faded: Vec<Mark> = right_marks
                .iter()
                .map(|mark| Mark {
                    alpha: mark.alpha * overlay_alpha,
                    ..*mark
                })
                .collect();
            label_column(
                &faded,
                overlay.as_ref(),
                overlay.as_ref().map_or(p.faint, |o| o.color),
                true,
            )
        };
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
            .min_h(px(state::MIN_LANE * presence))
            // Clipped only while it comes or goes: settled, a long axis label
            // reaches past the gutter into the card's padding.
            .when(presence < 1., |d| d.overflow_hidden())
            .flex()
            .flex_col()
            // Where it is, for a drag arranging the lanes.
            .child(
                canvas(
                    self.history
                        .lanes_order
                        .recorder(state::LANES, "", &id.to_string()),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
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

    /// The button at the end of the reading tabs adding a lane.
    pub(super) fn history_add_lane(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        div()
            .id("history-add-lane")
            .relative()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(10.))
            .h(px(30.))
            .rounded(px(6.))
            .text_size(theme::font(13.))
            .text_color(p.muted)
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(move |s| s.bg(p.hover_strong).text_color(p.ink))
            .child(icon("PlusIcon", p.muted).size(px(13.)))
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
            // A lane dragged shows its own place (`history_lanes`).
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
            // A lane dragged is drawn under the pointer itself.
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
                            this.history.lanes_order.press(event.position);
                            this.history.drag = Some(Drag::Lane {
                                id,
                                start: event.position,
                                moved: false,
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
                    points
                        .aggregated(index)
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
            // The overlay on the right axis, or a button adding one: a pair
            // (read and write, sent and received) takes one as any reading.
            row = row.child(self.history_overlay_chip(lane, axes, read_index, window, cx));
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
                        pairs: true,
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
                    pairs: true,
                    none: Some(Command::Overlay(id, None)),
                    add: false,
                };
                let make = move |reading| Command::Overlay(id, Some(reading));
                this.history_open_readings(menu, make, &anchor, window, cx);
            }))
            .into_any_element()
    }
}
