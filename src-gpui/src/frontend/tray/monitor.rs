//! The taskbar monitor's panel: the style, a card per style with the
//! monitor's look in it; with several displays, the display whose taskbar
//! it sits on; and the readings it shows, a row per kind of device, checked
//! in place. A kind dragged by its name, or a reading
//! dragged, takes another place, and the monitor shows its readings in that
//! order. The panel stays open while they change; each change shows on the
//! taskbar at once and is saved.
use super::*;
use crate::frontend::monitors::KINDS;
use crate::frontend::pages::components::{PanelKind, PanelOption, TASKBAR_STYLES};
use crate::frontend::reorder::{Flow, Lift, Lifted, Unseen};
use crate::frontend::shift;

/// The width of the kinds' names beside their options.
const KIND_COLUMN: f32 = 60.;
/// A style card's preview, logical pixels: half the panel's content less
/// the gap, the card's padding and its border.
const PREVIEW: [f32; 2] = [163., 38.];
/// The space a kind's row reaches past the options on either side and
/// keeps above and below them, for its tint while dragged; and the space
/// between rows, which with it makes 10.
const ROW_BLEED: f32 = 6.;
const ROW_PAD: f32 = 3.;
const ROW_GAP: f32 = 4.;
const CHIP_GAP: f32 = 6.;
/// The list of kinds as dragged (`reorder`); a kind's options are the list
/// `chips/<kind>`.
const KIND_LIST: &str = "kinds";

fn chip_list(kind: &str) -> String {
    format!("chips/{kind}")
}

/// A kind's place in `KINDS`: what its controls are told apart by,
/// wherever it is arranged.
fn stable(kind: &str) -> usize {
    KINDS.iter().position(|(key, _)| *key == kind).unwrap_or(0)
}

/// Where tests and the drag find a part of the panel.
fn record(
    order: &crate::frontend::reorder::Reorder,
    list: &str,
    part: &str,
    key: &str,
) -> impl IntoElement {
    canvas(order.recorder(list, part, key), |_, _, _, _| {})
        .absolute()
        .inset_0()
}

impl Flyout {
    pub(super) fn monitor_panel(&self, p: Palette, window: &Window, cx: &mut Context<Self>) -> Div {
        let now = Instant::now();
        let lift = self.order.current();
        // Let go where nothing takes the drop: it ends all the same.
        if lift.is_some() && !cx.has_active_drag() {
            cx.defer_in(window, |this, window, cx| this.finish_order(window, cx));
        }
        if self.order.moving(now) {
            window.request_animation_frame();
        }
        let caption = |text: String| {
            div()
                .text_size(theme::font(12.))
                .font_weight(theme::LABEL_WEIGHT)
                .text_color(p.muted)
                .child(text)
        };
        let styles = TASKBAR_STYLES
            .iter()
            .enumerate()
            .map(|(index, style)| self.style_card(index, style, p, cx))
            .collect::<Vec<_>>();
        // The kinds, and each one's options, as the drag has arranged them.
        let mut kinds = self.monitor.kinds.clone();
        if let Some(lift) = &lift {
            arrange_panel(&mut kinds, lift);
        }
        let rows = kinds
            .iter()
            .map(|kind| self.kind_row(kind, &kinds, lift.as_ref(), now, p, cx))
            .collect::<Vec<_>>();
        let raised = lift.as_ref().and_then(|lift| self.raised(&kinds, lift, p));
        let animate = !cx.reduce_motion();
        let readings = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .child(record(&self.order, KIND_LIST, "", ""))
            .children(rows)
            .children(raised)
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<Lifted>, _, cx| {
                    if this.order.dragging() {
                        this.order
                            .follow(event.event.position, animate, Instant::now());
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(|this, _: &Lifted, window, cx| this.finish_order(window, cx)));
        let displays = (!self.monitor.displays.is_empty()).then(|| self.display_choice(p, cx));
        let mut footer_button =
            |id: &'static str, glyph: &str, name: String, choice: TaskbarChoice| {
                self.pressable(id, name.clone(), Ask::Taskbar(choice), p.hover, cx)
                    .h(px(30.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(theme::font(12.5))
                    .child(icon(glyph, p.muted).size(px(15.)))
                    .child(name)
            };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px(px(14.))
                    .pt(px(12.))
                    .pb(px(12. - ROW_PAD))
                    .child(caption(label("components.taskbar.style.title")).mb(px(8.)))
                    .child(div().grid().grid_cols(2).gap(px(6.)).children(styles))
                    .when_some(displays, |panel, displays| {
                        panel
                            .child(
                                caption(label("components.taskbar.display.title"))
                                    .mt(px(14.))
                                    .mb(px(8.)),
                            )
                            .child(div().flex().child(displays))
                    })
                    .child(
                        caption(label("components.metrics.title"))
                            .mt(px(14.))
                            .mb(px(8. - ROW_PAD)),
                    )
                    .child(readings),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(8.))
                    .py(px(6.))
                    .border_t_1()
                    .border_color(p.stroke)
                    .bg(p.layer)
                    .child(footer_button(
                        "monitor-settings",
                        "SettingsIcon",
                        label("components.taskbar.panel.settings"),
                        TaskbarChoice::Settings,
                    ))
                    .child(footer_button(
                        "monitor-hide",
                        "PowerIcon",
                        label("components.taskbar.panel.hide"),
                        TaskbarChoice::Hide,
                    )),
            )
    }

    /// Lets go of a kind or a reading dragged in the panel: it glides into
    /// its place, the panel keeps the order arranged, and the monitor
    /// follows it.
    pub(super) fn finish_order(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lift) = self.order.release(!cx.reduce_motion(), Instant::now()) else {
            return;
        };
        if let Some(keys) = lift.moved() {
            let keys = keys.to_vec();
            // Shown at once in its new order, before the settings come back.
            arrange_panel(&mut self.monitor.kinds, &lift);
            let choice = if lift.list == KIND_LIST {
                TaskbarChoice::Kinds(keys)
            } else {
                TaskbarChoice::Metrics(keys)
            };
            self.ask(Ask::Taskbar(choice), window, cx);
        }
        cx.notify();
    }

    /// Where a part of the panel the drag knows was last drawn, in the
    /// panel's window: a kind (`kinds`, its key), the name it is dragged by
    /// (`kinds`, `name`, its key) or a reading of the kind's device tab
    /// shown (`chips/<kind>`, the tab, its metric).
    pub(super) fn item_bounds(&self, list: &str, part: &str, key: &str) -> Option<Bounds<Pixels>> {
        self.order.bounds(list, part, key)
    }

    /// A style to choose: the monitor's look in it on the taskbar's colour,
    /// and its name beside a radio mark.
    fn style_card(
        &self,
        index: usize,
        style: &'static str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let on = self.monitor.style == style;
        let name = label(&format!("components.taskbar.style.{style}"));
        let preview = self.monitor.previews.get(index).cloned().flatten();
        let mark = div()
            .size(px(14.))
            .flex_shrink_0()
            .rounded_full()
            .border_1()
            .border_color(if on { p.accent } else { p.stroke_strong })
            .flex()
            .items_center()
            .justify_center()
            .when(on, |mark| {
                mark.child(div().size(px(6.)).rounded_full().bg(p.accent))
            });
        self.pressable(
            ("monitor-style", index),
            name.clone(),
            Ask::Taskbar(TaskbarChoice::Style(style)),
            if on { p.accent.opacity(0.12) } else { p.hover },
            cx,
        )
        .role(Role::RadioButton)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .p(px(4.))
        .rounded(px(7.))
        .border_1()
        .border_color(if on { p.accent } else { p.stroke_strong })
        .when(on, |card| card.bg(p.accent.opacity(0.08)))
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(
            div()
                .h(px(PREVIEW[1]))
                .rounded(px(4.))
                .bg(self.monitor.ground)
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_center()
                .children(preview.map(|image| {
                    let size = image.size(0);
                    let scale = self.monitor.scale.max(1.);
                    let width = u32::from(size.width) as f32 / scale;
                    let height = u32::from(size.height) as f32 / scale;
                    let fit = (PREVIEW[0] / width).min(PREVIEW[1] / height).min(1.);
                    img(image).w(px(width * fit)).h(px(height * fit))
                })),
        )
        .child(
            div()
                .h(px(18.))
                .px(px(2.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(theme::font(12.5))
                .child(mark)
                .child(name),
        )
        .into_any_element()
    }

    /// The displays to choose from, as tabs: the primary display, then each
    /// by its monitor's name. Choosing one moves the monitor there and
    /// closes the panel, which it leaves behind.
    fn display_choice(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_wrap()
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(6.))
            .bg(p.control_face)
            .border_1()
            .border_color(p.stroke)
            .children(
                self.monitor
                    .displays
                    .iter()
                    .enumerate()
                    .map(|(index, (key, name))| {
                        let current = *key == self.monitor.display;
                        self.pressable(
                            ("monitor-display", index),
                            name.clone(),
                            Ask::Taskbar(TaskbarChoice::Display(key.clone())),
                            if current { p.control_checked } else { p.hover },
                            cx,
                        )
                        .role(Role::RadioButton)
                        .aria_toggled(if current {
                            Toggled::True
                        } else {
                            Toggled::False
                        })
                        .h(px(24.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .text_size(theme::font(12.))
                        .text_color(if current { p.ink } else { p.muted })
                        .when(current, |tab| tab.bg(p.control_checked))
                        .child(div().max_w(px(140.)).truncate().child(name.clone()))
                    }),
            )
    }

    /// The device tab of `kind` shown.
    fn tab(&self, kind: &PanelKind) -> usize {
        let last = kind.devices.len().saturating_sub(1);
        self.tabs.get(kind.key).copied().unwrap_or(0).min(last)
    }

    /// A kind of reading: its name in its device's colour, to drag it by,
    /// and its options; with several devices, a tab per device above them.
    /// Dragged, its place stays as a tinted slot.
    fn kind_row(
        &self,
        kind: &PanelKind,
        kinds: &[PanelKind],
        lift: Option<&Lift>,
        now: Instant,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = stable(kind.key);
        let at = self.tab(kind);
        let row = div()
            .relative()
            .mx(px(-ROW_BLEED))
            .px(px(ROW_BLEED))
            .py(px(ROW_PAD))
            .rounded(px(6.))
            .child(record(&self.order, KIND_LIST, "", kind.key));
        if lift.is_some_and(|lift| lift.list == KIND_LIST && lift.key == kind.key) {
            return row
                .bg(p.accent.opacity(0.08))
                .child(self.kind_face(kind, at, p).invisible())
                .into_any_element();
        }
        let tabs = (kind.devices.len() > 1).then(|| {
            div()
                .flex()
                .gap(px(2.))
                .p(px(2.))
                .rounded(px(6.))
                .bg(p.control_face)
                .border_1()
                .border_color(p.stroke)
                .children(kind.devices.iter().enumerate().map(|(tab, device)| {
                    let key = kind.key;
                    let current = tab == at;
                    let tip = device.tip.clone();
                    div()
                        .id(("monitor-tab", id * 64 + tab))
                        .role(Role::Tab)
                        .aria_label(device.tab.clone())
                        .aria_toggled(if current {
                            Toggled::True
                        } else {
                            Toggled::False
                        })
                        .relative()
                        .h(px(22.))
                        .px(px(8.))
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .text_size(theme::font(12.))
                        .text_color(if current { p.ink } else { p.muted })
                        .when(current, |tab| tab.bg(p.control_checked))
                        .when(!current, |tab| tab.hover(move |s| s.bg(p.hover)))
                        .child(div().max_w(px(120.)).truncate().child(device.tab.clone()))
                        .when(device.chosen, |tab| {
                            tab.child(
                                div()
                                    .absolute()
                                    .top(px(3.))
                                    .right(px(2.))
                                    .size(px(4.))
                                    .rounded_full()
                                    .bg(kind.tone),
                            )
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.tabs.insert(key, tab);
                            cx.notify();
                        }))
                        .when(!tip.is_empty(), |tab| {
                            tab.tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
                        })
                }))
        });
        let list = chip_list(kind.key);
        let part = at.to_string();
        let options = kind.devices.get(at).map(|device| {
            let keys = device
                .options
                .iter()
                .map(|option| option.metric.to_owned())
                .collect::<Vec<_>>();
            let sortable = kind.sortable && keys.len() > 1;
            device
                .options
                .iter()
                .map(|choice| {
                    let lifted = lift.is_some_and(|lift| {
                        lift.list == list && lift.part == part && lift.key == choice.metric
                    });
                    let chip = if lifted {
                        div()
                            .relative()
                            .rounded(px(6.))
                            .bg(p.accent.opacity(0.08))
                            .child(chip_face(div(), choice, kind.tone, p).invisible())
                            .child(record(&self.order, &list, &part, choice.metric))
                            .into_any_element()
                    } else {
                        let drag = sortable.then(|| (list.clone(), part.clone(), keys.clone()));
                        self.option_chip(choice, kind.tone, drag, p, cx)
                    };
                    let offset = self.order.offset(&list, &part, choice.metric, now);
                    shift::shifted(chip, offset, 1.).into_any_element()
                })
                .collect::<Vec<_>>()
        });
        let keys = kinds
            .iter()
            .map(|kind| kind.key.to_owned())
            .collect::<Vec<_>>();
        let key = kind.key;
        let tip = kind.tip.clone();
        let flyout = cx.entity().downgrade();
        let name = kind_name(div().id(("monitor-kind", id)), kind)
            .relative()
            .child(record(&self.order, KIND_LIST, "name", kind.key))
            .cursor_grab()
            .when(!tip.is_empty(), |name| {
                name.tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| this.order.press(event.position)),
            )
            .on_drag(Lifted, move |_, _, window, cx| {
                let pointer = window.mouse_position();
                let _ = flyout.update(cx, |this, cx| {
                    let gap = ROW_GAP;
                    this.order.take(
                        KIND_LIST,
                        "",
                        key,
                        Flow::Stack { gap },
                        keys.clone(),
                        pointer,
                    );
                    cx.notify();
                });
                cx.new(|_| Unseen)
            });
        let row = row.child(
            div().flex().gap(px(8.)).child(name).child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(6.))
                    .children(tabs)
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_wrap()
                            .gap(px(CHIP_GAP))
                            .child(record(&self.order, &list, &part, ""))
                            .children(options.unwrap_or_default()),
                    ),
            ),
        );
        shift::shifted(row, self.order.offset(KIND_LIST, "", kind.key, now), 1.).into_any_element()
    }

    /// A kind as it looks, taking no input: its name, its tabs and the
    /// options of the tab `at`.
    fn kind_face(&self, kind: &PanelKind, at: usize, p: Palette) -> Div {
        let tabs = (kind.devices.len() > 1).then(|| {
            div()
                .flex()
                .gap(px(2.))
                .p(px(2.))
                .rounded(px(6.))
                .bg(p.control_face)
                .border_1()
                .border_color(p.stroke)
                .children(kind.devices.iter().enumerate().map(|(tab, device)| {
                    let current = tab == at;
                    div()
                        .h(px(22.))
                        .px(px(8.))
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .text_size(theme::font(12.))
                        .text_color(if current { p.ink } else { p.muted })
                        .when(current, |tab| tab.bg(p.control_checked))
                        .child(div().max_w(px(120.)).truncate().child(device.tab.clone()))
                }))
        });
        let options = kind.devices.get(at).map(|device| {
            device
                .options
                .iter()
                .map(|choice| chip_face(div(), choice, kind.tone, p))
                .collect::<Vec<_>>()
        });
        div()
            .flex()
            .gap(px(8.))
            .child(kind_name(div(), kind))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(6.))
                    .children(tabs)
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(CHIP_GAP))
                            .children(options.unwrap_or_default()),
                    ),
            )
    }

    /// The kind or the reading dragged, drawn under the pointer over the
    /// others, where the list of kinds is.
    fn raised(&self, kinds: &[PanelKind], lift: &Lift, p: Palette) -> Option<AnyElement> {
        let list = self.order.bounds(KIND_LIST, "", "")?;
        let at = lift.origin() - list.origin;
        // On the panel's own colour, as the chips' and rows' colours are
        // see-through.
        let raised = div()
            .absolute()
            .left(at.x)
            .top(at.y)
            .w(lift.start.size.width)
            .rounded(px(6.))
            .bg(widgets::solid(p.flyout))
            .cursor_grabbing();
        if lift.list == KIND_LIST {
            let kind = kinds.iter().find(|kind| kind.key == lift.key)?;
            // Its content where the slot's is, inside its border.
            return Some(
                raised
                    .px(px(ROW_BLEED - 1.))
                    .py(px(ROW_PAD - 1.))
                    .border_1()
                    .border_color(p.accent.opacity(0.6))
                    .child(self.kind_face(kind, self.tab(kind), p))
                    .into_any_element(),
            );
        }
        let kind = kinds.iter().find(|kind| chip_list(kind.key) == lift.list)?;
        let device = kind.devices.get(lift.part.parse::<usize>().ok()?)?;
        let choice = device
            .options
            .iter()
            .find(|option| option.metric == lift.key)?;
        Some(
            raised
                .child(chip_face(div(), choice, kind.tone, p).border_color(p.accent.opacity(0.6)))
                .into_any_element(),
        )
    }

    /// A reading to show or not: checked in its device's colour, which it
    /// has on the taskbar. `drag` names its list, the device tab and the
    /// list's order when it can be dragged to another place.
    fn option_chip(
        &self,
        option: &PanelOption,
        tone: Hsla,
        drag: Option<(String, String, Vec<String>)>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let on = option.on;
        let tip = option.tip.clone();
        let key = option.metric;
        let flyout = cx.entity().downgrade();
        let chip = self
            .pressable(
                SharedString::from(format!("monitor-option-{}", option.key)),
                option.text.clone(),
                Ask::Taskbar(TaskbarChoice::Readings(option.readings.clone())),
                if on { tone.opacity(0.24) } else { p.hover },
                cx,
            )
            .role(Role::CheckBox)
            .aria_toggled(if on { Toggled::True } else { Toggled::False })
            .relative();
        let recorded = drag
            .as_ref()
            .map(|(list, part, _)| record(&self.order, list, part, key));
        chip_face(chip, option, tone, p)
            .children(recorded)
            .when(!tip.is_empty(), |chip| {
                chip.tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
            })
            .when_some(drag, |chip, (list, part, keys)| {
                chip.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, _| {
                        this.order.press(event.position)
                    }),
                )
                .on_drag(Lifted, move |_, _, window, cx| {
                    let pointer = window.mouse_position();
                    let _ = flyout.update(cx, |this, cx| {
                        let flow = Flow::Lines {
                            gap: CHIP_GAP,
                            line_gap: CHIP_GAP,
                        };
                        this.order
                            .take(&list, &part, key, flow, keys.clone(), pointer);
                        cx.notify();
                    });
                    cx.new(|_| Unseen)
                })
            })
            .into_any_element()
    }
}

/// `kinds` as `lift` has arranged them so far: the kinds, or one kind's
/// options in each of its device tabs.
fn arrange_panel(kinds: &mut [PanelKind], lift: &Lift) {
    let place = |key: &str| lift.order.iter().position(|ordered| ordered == key);
    if lift.list == KIND_LIST {
        kinds.sort_by_key(|kind| place(kind.key));
    } else if let Some(kind) = kinds
        .iter_mut()
        .find(|kind| chip_list(kind.key) == lift.list)
    {
        for device in &mut kind.devices {
            device.options.sort_by_key(|option| place(option.metric));
        }
    }
}

/// A kind's name, its device's colour beside it.
fn kind_name<E: Styled + ParentElement>(base: E, kind: &PanelKind) -> E {
    base.w(px(KIND_COLUMN))
        .flex_shrink_0()
        .h(px(28.))
        .flex()
        .items_center()
        .gap(px(7.))
        .child(
            div()
                .size(px(7.))
                .flex_shrink_0()
                .rounded_full()
                .bg(kind.tone),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .child(kind.title.clone()),
        )
}

/// A reading's chip as it looks: a box checked in its device's colour, and
/// its name.
fn chip_face<E: Styled + ParentElement>(
    base: E,
    option: &PanelOption,
    tone: Hsla,
    p: Palette,
) -> E {
    let on = option.on;
    let check = div()
        .size(px(14.))
        .flex_shrink_0()
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .map(|check| {
            if on {
                check
                    .bg(tone)
                    .child(icon("CheckIcon", widgets::solid(p.flyout)).size(px(11.)))
            } else {
                check.border_1().border_color(p.stroke_strong)
            }
        });
    base.h(px(28.))
        .pl(px(7.))
        .pr(px(9.))
        .rounded(px(6.))
        .border_1()
        .border_color(if on {
            tone.opacity(0.5)
        } else {
            p.stroke_strong
        })
        .bg(if on {
            tone.opacity(0.15)
        } else {
            p.control_face
        })
        .flex()
        .items_center()
        .gap(px(5.))
        .text_size(theme::font(12.5))
        .child(check)
        .child(option.text.clone())
}
