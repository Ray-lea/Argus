//! The navigator under the lanes: the range (a pill dropping the presets), and
//! a track with the active lane's reading over the whole range, the window
//! drawn on it as a box to drag, and the range's ends. Dragging only ever
//! pauses: the range is held as it was when the drag began, and returning to
//! the latest is up to the live button, End or a right click on the lanes.
use super::series::{Part, Points, min_span};
use super::state::{Brush, Drag, PRESETS, Range};
use super::*;

/// How near a brush edge a press takes that edge.
const EDGE: f32 = 7.;

impl Desktop {
    fn history_range_name(&self) -> String {
        match self.history.range {
            Range::Last(length) => match PRESETS.iter().find(|(_, l)| *l == length) {
                Some((id, _)) => label(&format!("history.range.{id}")),
                None => duration_label(length),
            },
            Range::All => label("history.range.all"),
        }
    }

    pub(super) fn history_navigator(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let geometry = self.history.geometry.clone();
        let now = self.history_now();
        let (a, b) = self.history.extent;
        let length = (b - a).max(1);
        let follow = self.history.follow;
        let checked = self.history.range.id();
        let pill = div()
            .relative()
            .flex_shrink_0()
            .child(
                div()
                    .id("history-range")
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .h(px(30.))
                    .rounded(px(15.))
                    .border_1()
                    .border_color(p.stroke_strong)
                    .bg(p.control)
                    .text_size(theme::font(13.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(p.hover_strong))
                    .child(self.history_range_name())
                    .child(icon("ChevronDown", p.muted).size(px(14.)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let mut items = PRESETS
                            .iter()
                            .map(|(id, _)| {
                                MenuItem::new(
                                    label(&format!("history.range.{id}")),
                                    Action::History(Command::Range(id)),
                                )
                                .checked(checked == Some(*id))
                            })
                            .collect::<Vec<_>>();
                        items.push(
                            MenuItem::new(
                                label("history.range.all"),
                                Action::History(Command::Range("all")),
                            )
                            .checked(checked == Some("all")),
                        );
                        this.open_anchored(items, "history-range", layout::Side::Above, window, cx);
                    })),
            )
            .child(layout::anchor_probe(
                "history-range",
                self.ui.anchors.clone(),
            ));
        // The track: the reading over the range, the window as a box.
        let reading = self
            .history_reading(&self.history.active_lane().reading.clone())
            .cloned();
        let source = self
            .history
            .points
            .as_ref()
            .filter(|points| points.from <= a && points.to >= b)
            .or(self.history.overview.as_ref());
        let mut strip = div()
            .id("history-track")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .cursor_pointer()
            .child({
                let geometry = geometry.clone();
                canvas(
                    move |bounds, _, _| geometry.borrow_mut().track = bounds,
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
            });
        if let (Some(reading), Some(points)) = (&reading, source) {
            let key = navigator_key(reading, points);
            let end = self.history_clock();
            let top = points.highest(&key, Part::Mean, a, b, end).unwrap_or(0.);
            let (low, high) = match reading.axis {
                readings::Axis::Percent => (0., series::nice_top(top, true).max(10.)),
                _ => {
                    let (low, high) = reading.axis_range(points, a, b, end, false);
                    (low.min(0.), high)
                }
            };
            strip = strip.child(
                div()
                    .absolute()
                    .top(px(3.))
                    .bottom(px(3.))
                    .left_0()
                    .right_0()
                    .child(charts::TimePlot {
                        id: "history-navigator".into(),
                        series: Arc::new(vec![
                            charts::Series::new(
                                String::new(),
                                points.line(&key, Part::Mean, end, |v| v),
                                reading.color.opacity(0.75),
                                charts::MetricFormat::Percent,
                            )
                            .fill(0.35)
                            .source(charts::hash_of(("history-navigator", &reading.id))),
                        ]),
                        start: a as f64,
                        end: b as f64,
                        max: high,
                        min: low,
                        interactive: false,
                        grid: false,
                        fill: true,
                        background_separate: false,
                        smooth: false,
                        ticks: None,
                        geometry_start: a as f64,
                        measured_width: Rc::new(Cell::new(1.)),
                        live_clock: None,
                        image_surface: Default::default(),
                        dash_phases: Default::default(),
                        rescale: Default::default(),
                        readout: Default::default(),
                        captions: None,
                    }),
            );
        }
        let (va, vb) = self.history.shown_now();
        let fa = ((va - a as f64) / length as f64).clamp(0., 1.) as f32;
        let fb = ((vb - a as f64) / length as f64).clamp(0., 1.) as f32;
        let shade = p.background.opacity(if p.dark { 0.55 } else { 0.45 });
        strip = strip
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(relative(fa))
                    .bg(shade),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(fb))
                    .right_0()
                    .bg(shade),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(fa))
                    .w(relative((fb - fa).max(0.)))
                    .min_w(px(4.))
                    .border(px(1.5))
                    .border_color(p.accent)
                    .rounded(px(5.))
                    .child(
                        div()
                            .absolute()
                            .left(px(-2.5))
                            .top(relative(0.5))
                            .mt(px(-5.))
                            .w(px(3.))
                            .h(px(10.))
                            .rounded(px(1.5))
                            .bg(p.accent),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(-2.5))
                            .top(relative(0.5))
                            .mt(px(-5.))
                            .w(px(3.))
                            .h(px(10.))
                            .rounded(px(1.5))
                            .bg(p.accent),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if event.click_count >= 2 {
                        this.history.reset_zoom();
                        this.history.drag = None;
                        this.load_history(window, cx);
                        cx.notify();
                        return;
                    }
                    this.history_brush_down(event.position);
                    cx.stop_propagation();
                    cx.notify();
                }),
            );
        let left = edge_label(a, length, now);
        let right = if follow {
            label("history.now")
        } else {
            edge_label(b, length, now)
        };
        let edge = |text: String| {
            div()
                .flex_shrink_0()
                .text_size(theme::font(11.))
                .text_color(p.faint)
                .font_features(theme::tabular())
                .whitespace_nowrap()
                .child(text)
        };
        let track = div()
            .flex_1()
            .min_w_0()
            .h(px(30.))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(10.))
            .pr(px(if follow { 10. } else { 4. }))
            .rounded(px(15.))
            .border_1()
            .border_color(p.stroke)
            .bg(p.chart_surface)
            .child(edge(left))
            .child(strip)
            .child(edge(right))
            .when(!follow, |d| {
                d.child(
                    div()
                        .id("history-track-latest")
                        .size(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .hover(move |s| s.bg(p.hover))
                        .child(icon("ArrowRightIcon", p.accent).size(px(13.)))
                        .tooltip(|window, cx| {
                            widgets::tooltip(label("history.live.offTitle"), window, cx)
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.history_command(Command::Latest, window, cx)
                        })),
                )
            });
        let _ = window;
        div()
            .flex_none()
            .h(px(32.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(pill)
            .child(track)
            .into_any_element()
    }

    /// A press on the track: an edge of the box resizes it, inside moves it,
    /// elsewhere centres it there and moves it from there on.
    fn history_brush_down(&mut self, at: Point<Pixels>) {
        let track = self.history.geometry.borrow().track;
        let width = f32::from(track.size.width).max(1.);
        let x = f32::from(at.x - track.left());
        let extent = self.history.extent;
        let length = (extent.1 - extent.0).max(1) as f64;
        let x_of = |t: f64| ((t - extent.0 as f64) / length) as f32 * width;
        let time_of = |x: f32| extent.0 + (f64::from(x / width) * length).round() as i64;
        let (va, vb) = self.history.shown_now();
        let (x0, x1) = (x_of(va), x_of(vb));
        let mode = if (x - x0).abs() < EDGE {
            Brush::Start
        } else if (x - x1).abs() < EDGE {
            Brush::End
        } else if x > x0 && x < x1 {
            Brush::Move {
                grab: time_of(x) - va.round() as i64,
            }
        } else {
            let span = self.history.span();
            let middle = time_of(x);
            let from = (middle - span / 2).clamp(extent.0, (extent.1 - span).max(extent.0));
            self.history.set_window(from, from + span, state::SLOW);
            Brush::Move {
                grab: middle - from,
            }
        };
        self.history.drag = Some(Drag::Brush { mode, extent });
    }

    /// The box follows the pointer over the range held since the press. The
    /// start edge alone keeps following (only the width changes); anything
    /// else pauses where the box is.
    pub(super) fn history_brush_move(
        &mut self,
        mode: Brush,
        extent: (i64, i64),
        at: Point<Pixels>,
    ) {
        let track = self.history.geometry.borrow().track;
        let width = f32::from(track.size.width).max(1.);
        let length = (extent.1 - extent.0).max(1);
        let f = (f32::from(at.x - track.left()) / width).clamp(0., 1.);
        let t = extent.0 + (f64::from(f) * length as f64).round() as i64;
        let narrowest = min_span(length);
        let (a, b) = self.history.view;
        let span = b - a;
        match mode {
            Brush::Start if self.history.follow => {
                self.history.set_span(
                    (b - t.min(b - narrowest)).max(narrowest),
                    std::time::Duration::ZERO,
                );
            }
            Brush::Start => {
                self.history.extent = extent;
                self.history
                    .set_window(t.min(b - narrowest), b, std::time::Duration::ZERO);
            }
            Brush::End => {
                self.history.extent = extent;
                self.history
                    .set_window(a, t.max(a + narrowest), std::time::Duration::ZERO);
            }
            Brush::Move { grab } => {
                self.history.extent = extent;
                let from = (t - grab).clamp(extent.0, (extent.1 - span).max(extent.0));
                self.history
                    .set_window(from, from + span, std::time::Duration::ZERO);
            }
        }
    }
}

/// The channel the navigator draws for a reading: a pair's first part, or
/// the combined channel of older records.
fn navigator_key(reading: &readings::Reading, points: &Points) -> String {
    let first = &reading.keys[0];
    match &reading.fallback {
        Some(fallback) if !points.has(first) && points.has(fallback) => fallback.clone(),
        _ => first.clone(),
    }
}
