//! The lanes stacked in the chart card, with the dividers between them and
//! the time axis under them, and what is drawn over them: the drop mark,
//! the notice, the tip and the ghost of what is dragged.
use super::*;

impl Desktop {
    pub(super) fn history_lanes(
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
                .flow_windows
                .filter(|_| fading.is_none())
                .map(|(lanes, _)| {
                    let origin = lanes.start.at(lanes.sample_end);
                    let overscan = (lanes.sample_end - origin) / 8.;
                    let lines = ticks(
                        (origin - overscan).floor() as i64,
                        (lanes.sample_end + overscan).ceil() as i64,
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
}
