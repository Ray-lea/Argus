//! A lane: its plot with the curves, gaps and marks, its axis labels and
//! its title row (`header`).
use super::curves::history_curve;
use super::*;

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

impl Desktop {
    pub(super) fn history_lane(
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
        let nudge = self.history.flow_windows.map(|_| Nudge {
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
                    None => self
                        .history
                        .flow_windows
                        .map_or((from, None), |(lanes, _)| lanes.raster()),
                },
                clock_lines.cloned(),
            );
            if let Some((_, _, alpha)) = fade {
                plot = plot.child(div().absolute().inset_0().opacity(alpha).child(curve));
            } else if let Some((lanes, _)) = self.history.flow_windows {
                // The plot fills the box: its bounds are the box's.
                let bounds = Rc::new(Cell::new(Bounds::default()));
                self.history.glides.push(Glide {
                    plot: curve.clone(),
                    bounds: bounds.clone(),
                    start: lanes.start,
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
}
