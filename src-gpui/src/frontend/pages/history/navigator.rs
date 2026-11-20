//! The navigator under the lanes: the range (a pill opening a grid of the
//! ranges, `history_ranges`), and
//! a track with the active lane's reading over the whole range, the window
//! drawn on it as a box to drag, and the range's ends. Dragging only ever
//! pauses: the range is held as it was when the drag began, and returning to
//! the latest is up to the live button, End or a right click on the lanes.
use super::series::{Part, Points, STEP_CORNER, min_span};
use super::state::{Brush, Drag, Glide, PRESETS, Range};
use super::*;

/// How near a brush edge a press takes that edge.
const EDGE: f32 = 7.;

impl Desktop {
    fn history_range_name(&self) -> String {
        if let Some(name) = self.history.range_name {
            return label(name);
        }
        match self.history.range {
            Range::Last(length) => match PRESETS.iter().find(|(_, l)| *l == length) {
                Some((id, _)) => label(&format!("history.range.{id}")),
                None => duration_label(length),
            },
            Range::All => label("history.range.all"),
            Range::Fixed(..) => label("history.range.custom"),
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
                    .on_click(cx.listener(|this, _, _, cx| this.history_open_ranges(cx))),
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
            let curve = charts::TimePlot {
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
                geometry_start: self
                    .history
                    .flow_origin
                    .map_or(a as f64, |(_, navigator)| navigator),
                measured_width: Rc::new(Cell::new(1.)),
                live_clock: None,
                image_surface: Default::default(),
                dash_phases: Default::default(),
                rescale: Default::default(),
                readout: Default::default(),
                captions: None,
                redraw_alone: true,
                corner: STEP_CORNER,
            };
            let mut track = div()
                .absolute()
                .top(px(3.))
                .bottom(px(3.))
                .left_0()
                .right_0();
            if self.history.flow_origin.is_some() {
                // The plot fills the box: its bounds are the box's.
                let bounds = Rc::new(Cell::new(Bounds::default()));
                self.history.glides.push(Glide {
                    plot: curve.clone(),
                    bounds: bounds.clone(),
                });
                track = track.child(
                    canvas(move |laid, _, _| bounds.set(laid), |_, _, _, _| {})
                        .absolute()
                        .inset_0(),
                );
            }
            strip = strip.child(track.child(curve));
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

/// The ranges the pill offers, four to a row: the presets, all of it, and
/// "Custom…" opening the dialog.
const QUICK: [&str; 8] = ["5m", "1h", "6h", "1d", "7d", "30d", "all", "custom"];
const QUICK_COLUMNS: usize = 4;

impl Desktop {
    /// The quick range in force: a preset or all of it, else "Custom…"
    /// (today, yesterday or ends of its own).
    fn history_quick_current(&self) -> usize {
        let id = self
            .history
            .range
            .id()
            .filter(|_| self.history.range_name.is_none());
        QUICK
            .iter()
            .position(|quick| Some(*quick) == id)
            .unwrap_or(QUICK.len() - 1)
    }

    /// Opens the ranges over the pill, the keys on the one in force.
    pub(super) fn history_open_ranges(&mut self, cx: &mut Context<Self>) {
        self.history.ranges = Some(self.history_quick_current());
        cx.notify();
    }

    pub(super) fn history_close_ranges(&mut self, cx: &mut Context<Self>) {
        if self.history.ranges.take().is_some() {
            cx.notify();
        }
    }

    fn history_choose_range(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.history.ranges = None;
        match QUICK.get(index) {
            Some(&"custom") => self.history_command(Command::Picker, window, cx),
            Some(id) => self.history_command(Command::Range(id), window, cx),
            None => cx.notify(),
        }
    }

    /// The keys while the ranges are open: arrows move through them, Enter
    /// or Space takes one, Esc closes them.
    pub(super) fn history_ranges_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(at) = self.history.ranges else {
            return false;
        };
        let last = QUICK.len() - 1;
        let next = match key {
            "escape" => {
                self.history_close_ranges(cx);
                return true;
            }
            "enter" | "space" => {
                self.history_choose_range(at, window, cx);
                return true;
            }
            "left" => at.saturating_sub(1),
            "right" => (at + 1).min(last),
            "up" => at.checked_sub(QUICK_COLUMNS).unwrap_or(at),
            "down" => (at + QUICK_COLUMNS).min(last),
            _ => return false,
        };
        self.history.ranges = Some(next);
        cx.notify();
        true
    }

    /// The ranges, over the pill (under it where there is no room above).
    pub(super) fn history_ranges(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let keyed = self.history.ranges?;
        let p = self.palette;
        let current = self.history_quick_current();
        let viewport = window.viewport_size();
        let margin = px(8.);
        let pill = self
            .ui
            .anchors
            .borrow()
            .get("history-range")
            .copied()
            .unwrap_or_default();
        let names = QUICK
            .iter()
            .map(|id| {
                label(&if *id == "custom" {
                    "history.range.customMenu".to_owned()
                } else {
                    format!("history.range.{id}")
                })
            })
            .collect::<Vec<_>>();
        // Every cell as wide as the widest name (the custom one with its icon).
        let cell = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let text = f32::from(theme::text_width(name, theme::font(13.), window, cx));
                text + if index == QUICK.len() - 1 { 18. } else { 0. }
            })
            .fold(0., f32::max)
            + 24.;
        let gap = 4.;
        let width = cell * QUICK_COLUMNS as f32 + gap * (QUICK_COLUMNS - 1) as f32 + 2. * 5. + 2.;
        let tall = 2. * 30. + gap + 2. * 5. + 2.;
        let above = pill.top() - px(6. + tall) >= margin;
        let left = pill
            .left()
            .min(viewport.width - px(width) - margin)
            .max(margin);
        let mut grid = div().grid().grid_cols(QUICK_COLUMNS as u16).gap(px(gap));
        for (index, name) in names.into_iter().enumerate() {
            let on = index == current;
            let custom = index == QUICK.len() - 1;
            grid = grid.child(
                div()
                    .id(("history-quick-range", index))
                    .h(px(30.))
                    .w(px(cell))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(5.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(if index == keyed {
                        p.accent.opacity(0.6)
                    } else {
                        gpui_kit::gpui::transparent_black()
                    })
                    .text_size(theme::font(13.))
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .when(on, |d| {
                        d.bg(p.accent.opacity(0.16))
                            .text_color(p.accent)
                            .font_weight(theme::LABEL_WEIGHT)
                    })
                    .when(!on, |d| d.text_color(p.ink).hover(move |s| s.bg(p.hover)))
                    .when(custom, |d| {
                        d.child(
                            icon("CalendarRangeIcon", if on { p.accent } else { p.muted })
                                .size(px(13.)),
                        )
                    })
                    .child(name)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.history_choose_range(index, window, cx)
                    })),
            );
        }
        let panel = div()
            .id("history-quick-ranges")
            .absolute()
            .left(left)
            .when(above, |d| d.bottom(viewport.height - pill.top() + px(6.)))
            .when(!above, |d| d.top(pill.bottom() + px(6.)))
            .p(px(5.))
            .border_1()
            .border_color(p.stroke_strong)
            .rounded(px(8.))
            .bg(widgets::solid(p.flyout))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(grid);
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("history-quick-ranges-layer")
                        .w(viewport.width)
                        .h(viewport.height)
                        .occlude()
                        // A press anywhere else closes them.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.history_close_ranges(cx)),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, _, _, cx| this.history_close_ranges(cx)),
                        )
                        .on_scroll(|_, _, cx| cx.stop_propagation())
                        .child(panel),
                ),
            )
            .with_priority(gpui_kit::base::POPUP_PRIORITY - 1)
            .into_any_element(),
        )
    }
}
