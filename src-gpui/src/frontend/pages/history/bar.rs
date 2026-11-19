//! The reading tabs over the chart, and the menu of every reading. A tab shows
//! its reading's value at the moment the page describes, in a slot as wide as
//! the reading's widest value, so that the tabs never move while values
//! change. Clicked, a tab shows its reading in the active lane; its + adds a
//! lane; dragged onto a lane it overlays it there, between lanes it adds one.
use super::lanes::slot_width;
use super::readings::{Group, tab_ids};
use super::state::Drag;
use super::*;

impl Desktop {
    /// Every reading by group (each group a submenu), for `make` to turn the
    /// chosen one into a command. `pairs`: also readings drawn as two lines.
    pub(super) fn history_reading_menu(
        &self,
        current: Option<&str>,
        exclude: &[String],
        pairs: bool,
        make: impl Fn(String) -> Command,
    ) -> Vec<MenuItem> {
        let mut items = Vec::new();
        for group in Group::ALL {
            let children = self
                .history
                .readings
                .iter()
                .filter(|r| r.group == group)
                .filter(|r| !exclude.contains(&r.id))
                .filter(|r| pairs || !r.is_pair())
                .filter(|r| r.recorded || r.service || r.group != Group::Battery)
                .map(|r| {
                    let mut item =
                        MenuItem::new(r.long.clone(), Action::History(make(r.id.clone())))
                            .checked(current == Some(r.id.as_str()));
                    if r.service && !r.recorded {
                        item = item.detail(label("history.menu.needsService"));
                    } else if !r.recorded {
                        item = item.detail(label("history.menu.notRecorded"));
                    }
                    item
                })
                .collect::<Vec<_>>();
            if children.is_empty() {
                continue;
            }
            items.push(MenuItem::new(group.label(), Action::CloseOverlay).submenu(children));
        }
        items
    }

    pub(super) fn history_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let bits = self.history_bits();
        let points = self.history.points.as_ref();
        let index = self.history_read_index();
        let lanes = self.history.live_lanes().collect::<Vec<_>>();
        let multi = lanes.len() > 1;
        let active = self.history.active_lane().reading.clone();
        let value_size = theme::font(13.);
        let mut row = div()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.));
        let ids = tab_ids(&self.history.readings);
        for id in &ids {
            let Some(reading) = self.history_reading(id) else {
                continue;
            };
            let shown = lanes.iter().any(|lane| lane.reading == *id);
            let overlaid = lanes
                .iter()
                .any(|lane| lane.overlay.as_deref() == Some(id.as_str()));
            let current = multi && active == *id;
            let width = slot_width(
                &reading.widest_samples(bits),
                value_size,
                FontWeight::SEMIBOLD,
                window,
                cx,
            );
            let value = points
                .zip(index)
                .and_then(|(points, index)| reading.value(points, index))
                .map(|v| reading.format(v, bits))
                .unwrap_or_else(|| label("format.missing"));
            let reading_id = id.clone();
            let add_id = id.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("history-tab-{id}")))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .h(px(32.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(if current {
                        reading.color
                    } else if shown {
                        p.stroke_strong
                    } else {
                        p.stroke
                    })
                    .when(overlaid && !shown, |d| d.border_dashed())
                    .bg(if shown { p.control } else { p.layer })
                    .cursor_pointer()
                    .hover(move |s| s.bg(p.hover_strong))
                    .when(!reading.recorded, |d| d.opacity(0.6))
                    .child(
                        div()
                            .size(px(8.))
                            .rounded_full()
                            .bg(reading.color)
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .text_size(theme::font(12.))
                            .text_color(p.muted)
                            .whitespace_nowrap()
                            .child(reading.name.clone()),
                    )
                    .child(
                        div()
                            .min_w(width)
                            .text_right()
                            .text_size(value_size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .font_features(theme::tabular())
                            .whitespace_nowrap()
                            .child(value),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("history-tab-add-{id}")))
                            .size(px(22.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .hover(move |s| s.bg(p.hover))
                            .child(icon("PlusIcon", p.faint).size(px(12.)))
                            .tooltip(|window, cx| {
                                widgets::tooltip(label("history.tab.add"), window, cx)
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.history_command(Command::AddLane(add_id.clone()), window, cx)
                            })),
                    )
                    .tooltip(|window, cx| widgets::tooltip(label("history.tab.title"), window, cx))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.history.drag = Some(Drag::Reading {
                                id: reading_id.clone(),
                                start: event.position,
                                at: event.position,
                                moved: false,
                                target: None,
                            });
                            cx.notify();
                        }),
                    ),
            );
        }
        let exclude = ids.clone();
        row = row.child(
            div()
                .id("history-tab-more")
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(10.))
                .h(px(32.))
                .rounded(px(6.))
                .text_size(theme::font(12.))
                .text_color(p.muted)
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover).text_color(p.ink))
                .child(label("history.tab.more"))
                .child(icon("ChevronDown", p.muted).size(px(13.)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    let current = this.history.active_lane().reading.clone();
                    let items =
                        this.history_reading_menu(Some(&current), &exclude, true, Command::Show);
                    this.open_popup(items, window, cx);
                })),
        );
        row.into_any_element()
    }
}
