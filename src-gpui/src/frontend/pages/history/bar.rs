//! The reading tabs over the chart, and the menu of every reading. A tab shows
//! its reading's value at the moment the page describes, in a slot as wide as
//! the reading's widest value, so that the tabs never move while values
//! change. Clicked, a tab shows its reading in the active lane; its + adds a
//! lane; dragged onto a lane it overlays it there, between lanes it adds one.
use super::lanes::slot_width;
use super::readings::{Group, tab_ids};
use super::state::Drag;
use super::*;

/// What a menu of readings is for.
pub(super) struct ReadingMenu<'a> {
    /// Its title (a locale key), if it needs one.
    pub(super) title: Option<&'static str>,
    /// The reading shown checked.
    pub(super) current: Option<&'a str>,
    /// Readings left out.
    pub(super) exclude: &'a [String],
    /// Whether readings drawn as two lines (disk, network) may be chosen.
    pub(super) pairs: bool,
    /// A first choice of none, checked while `current` is none.
    pub(super) none: Option<Command>,
    /// Whether each reading's + adds a lane for it.
    pub(super) add: bool,
}

impl Desktop {
    /// Every reading, grouped (the groups side by side), each with its colour
    /// and whether it needs the privileged service, for `make` to turn the
    /// chosen one into a command.
    pub(super) fn history_reading_menu(
        &self,
        menu: ReadingMenu<'_>,
        make: impl Fn(String) -> Command,
    ) -> Vec<MenuItem> {
        let p = self.palette;
        let mut items = Vec::new();
        if let Some(title) = menu.title {
            items.push(MenuItem::title(label(title)));
        }
        if let Some(none) = menu.none {
            items.push(
                MenuItem::new(label("history.menu.none"), Action::History(none))
                    .checked(menu.current.is_none()),
            );
        }
        for group in Group::ALL {
            let readings = self
                .history
                .readings
                .iter()
                .filter(|r| r.group == group)
                .filter(|r| !menu.exclude.contains(&r.id))
                .filter(|r| menu.pairs || !r.is_pair())
                .filter(|r| r.recorded || r.service || r.group != Group::Battery)
                .collect::<Vec<_>>();
            if readings.is_empty() {
                continue;
            }
            items.push(MenuItem::heading(group.label()));
            for r in readings {
                let mut item = MenuItem::new(r.long.clone(), Action::History(make(r.id.clone())))
                    .checked(menu.current == Some(r.id.as_str()))
                    .swatch(r.color);
                if r.service && r.recorded {
                    item = item.badge("ShieldCheckIcon", p.faint);
                } else if r.service {
                    item = item
                        .badge("ShieldAlertIcon", p.io)
                        .detail(label("history.menu.needsService"));
                } else if !r.recorded {
                    item = item.detail(label("history.menu.notRecorded"));
                }
                if menu.add {
                    item = item.side(
                        "PlusIcon",
                        Action::History(Command::AddLane(r.id.clone())),
                        label("history.tab.add"),
                    );
                }
                items.push(item);
            }
        }
        if menu.add {
            items.push(MenuItem::note(label("history.menu.foot")));
        }
        items
    }

    /// Opens a menu of readings under the control recorded as `anchor`.
    pub(super) fn history_open_readings(
        &mut self,
        menu: ReadingMenu<'_>,
        make: impl Fn(String) -> Command,
        anchor: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = self.history_reading_menu(menu, make);
        self.open_anchored(items, anchor, layout::Side::Under, window, cx);
        self.menu_columns(2);
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
            .gap(px(4.));
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
            let group = SharedString::from(format!("history-tab-{id}"));
            // Shown in a lane: tinted in its colour. The active lane's reading
            // (with several lanes) is ringed, an overlaid one outlined in dashes.
            // The border is always there, so that no state moves the tabs.
            let border = if current {
                reading.color.opacity(0.6)
            } else if overlaid {
                reading.color.opacity(0.7)
            } else {
                gpui_kit::gpui::transparent_black()
            };
            row = row.child(
                div()
                    .id(group.clone())
                    .group(group.clone())
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .h(px(30.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(border)
                    .when(overlaid && !current, |d| d.border_dashed())
                    .text_color(if shown { p.ink } else { p.muted })
                    .when(shown, |d| d.bg(reading.color.opacity(0.15)))
                    .when(!shown, |d| d.hover(move |s| s.bg(p.hover_strong)))
                    .cursor_pointer()
                    .child(
                        div()
                            .size(px(8.))
                            .rounded(px(2.))
                            .bg(reading.color)
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .text_size(theme::font(13.))
                            .whitespace_nowrap()
                            .child(reading.name.clone()),
                    )
                    .child(
                        div()
                            .min_w(width)
                            .text_right()
                            .text_size(value_size)
                            .when(reading.recorded, |d| {
                                d.text_color(p.ink).font_weight(FontWeight::SEMIBOLD)
                            })
                            .when(!reading.recorded, |d| d.text_color(p.faint))
                            .font_features(theme::tabular())
                            .whitespace_nowrap()
                            .child(value),
                    )
                    .child(
                        // Shown on hover, its room kept so that the tabs stay put.
                        div()
                            .id(SharedString::from(format!("history-tab-add-{id}")))
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .opacity(0.)
                            .group_hover(group, |s| s.opacity(1.))
                            .hover(move |s| s.bg(p.hover_strong))
                            .child(icon("PlusIcon", p.muted).size(px(12.)))
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
                .relative()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(10.))
                .h(px(30.))
                .rounded(px(6.))
                .text_size(theme::font(13.))
                .text_color(p.muted)
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover_strong).text_color(p.ink))
                .child(label("history.tab.more"))
                .child(icon("ChevronDown", p.muted).size(px(13.)))
                .child(layout::anchor_probe(
                    "history-tab-more",
                    self.ui.anchors.clone(),
                ))
                .on_click(cx.listener(move |this, _, window, cx| {
                    let current = this.history.active_lane().reading.clone();
                    let menu = ReadingMenu {
                        title: Some("history.menu.showTitle"),
                        current: Some(&current),
                        exclude: &exclude,
                        pairs: true,
                        none: None,
                        add: true,
                    };
                    this.history_open_readings(menu, Command::Show, "history-tab-more", window, cx);
                })),
        );
        row.into_any_element()
    }
}
