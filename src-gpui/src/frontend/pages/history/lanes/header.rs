//! A lane's title row: its grip, reading, value, overlay and remove button.
use super::*;

impl Desktop {
    /// A lane's title row: its grip, reading, value, overlay and remove button.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn history_lane_header(
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
