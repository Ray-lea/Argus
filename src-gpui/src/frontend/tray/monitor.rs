//! The taskbar monitor's panel: the style, a card per style with the
//! monitor's look in it, and the readings it shows, a row per kind of
//! device, checked in place. It stays open while they change; each change
//! shows on the taskbar at once and is saved.
use super::*;
use crate::frontend::pages::components::{PanelKind, PanelOption, TASKBAR_STYLES};

/// The width of the kinds' names beside their options.
const KIND_COLUMN: f32 = 60.;
/// A style card's preview, logical pixels: half the panel's content less
/// the gap, the card's padding and its border.
const PREVIEW: [f32; 2] = [163., 38.];

impl Flyout {
    pub(super) fn monitor_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
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
        let kinds = self
            .monitor
            .kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| self.kind_row(index, kind, p, cx))
            .collect::<Vec<_>>();
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
                    .pb(px(12.))
                    .child(caption(label("components.taskbar.style.title")).mb(px(8.)))
                    .child(div().grid().grid_cols(2).gap(px(6.)).children(styles))
                    .child(
                        caption(label("components.metrics.title"))
                            .mt(px(14.))
                            .mb(px(8.)),
                    )
                    .child(div().flex().flex_col().gap(px(10.)).children(kinds)),
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

    /// A kind of reading: its name in its device's colour, and its options;
    /// with several devices, a tab per device above them.
    fn kind_row(
        &self,
        index: usize,
        kind: &PanelKind,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let last = kind.devices.len().saturating_sub(1);
        let at = self.tabs.get(kind.key).copied().unwrap_or(0).min(last);
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
                        .id(("monitor-tab", index * 64 + tab))
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
        let options = kind.devices.get(at).map(|device| {
            device
                .options
                .iter()
                .enumerate()
                .map(|(option, choice)| {
                    self.option_chip(index * 64 + at * 8 + option, choice, kind.tone, p, cx)
                })
                .collect::<Vec<_>>()
        });
        let tip = kind.tip.clone();
        div()
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .id(("monitor-kind", index))
                    .w(px(KIND_COLUMN))
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
                    .when(!tip.is_empty(), |name| {
                        name.tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
                    }),
            )
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
                            .gap(px(6.))
                            .children(options.unwrap_or_default()),
                    ),
            )
            .into_any_element()
    }

    /// A reading to show or not: checked in its device's colour, which it
    /// has on the taskbar.
    fn option_chip(
        &self,
        id: usize,
        option: &PanelOption,
        tone: Hsla,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
        let tip = option.tip.clone();
        self.pressable(
            ("monitor-option", id),
            option.text.clone(),
            Ask::Taskbar(TaskbarChoice::Readings(option.readings.clone())),
            if on { tone.opacity(0.24) } else { p.hover },
            cx,
        )
        .role(Role::CheckBox)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .h(px(28.))
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
        .when(!tip.is_empty(), |chip| {
            chip.tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
        })
        .into_any_element()
    }
}
