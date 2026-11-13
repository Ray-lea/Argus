//! Components: the tray and, as they arrive, the other parts of Argus that
//! stay on the desktop outside its window (a desktop monitor, a taskbar
//! monitor, the island). Each available component has a section of its
//! settings; a planned one is listed with what it will be.
use super::*;
use crate::frontend::widgets::Tone;

/// A component of the page: its key (`components.*` texts), its icon and
/// whether it is available yet. Settings of a component that ships go in a
/// section of its own, before the planned ones.
pub(in crate::frontend) struct Component {
    pub key: &'static str,
    pub glyph: &'static str,
    pub available: bool,
}

pub(in crate::frontend) const COMPONENTS: [Component; 4] = [
    Component {
        key: "tray",
        glyph: "TrayIcon",
        available: true,
    },
    Component {
        key: "desktop",
        glyph: "ComputerIcon",
        available: false,
    },
    Component {
        key: "taskbar",
        glyph: "GaugeIcon",
        available: false,
    },
    Component {
        key: "island",
        glyph: "NavTopIcon",
        available: false,
    },
];

/// The column of cards, as wide as the settings' cards.
const CONTENT: f32 = 780.;

impl Desktop {
    pub(in crate::frontend) fn components_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.ui.located.is_some() {
            window.request_animation_frame();
        }
        let p = self.palette;
        let settings = self.data["settings"].clone();
        let tray = self.tray_cards(&settings, window, cx);
        let planned = COMPONENTS
            .iter()
            .filter(|component| !component.available)
            .map(|component| {
                self.setting(
                    component.key,
                    self.glyph(component.glyph),
                    label(&format!("components.planned.{}.title", component.key)),
                    Some(
                        label(&format!("components.planned.{}.desc", component.key))
                            .into_any_element(),
                    ),
                    None,
                    vec![self.state_pill(label("components.planned.state"), "off")],
                )
            })
            .collect::<Vec<_>>();
        let section = |first: bool, key: &str, cards: Vec<AnyElement>| {
            div()
                .id(SharedString::from(format!("components-{key}")))
                .w_full()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .gap(px(4.))
                .child(
                    div()
                        .mt(px(if first { 6. } else { 28. }))
                        .mb(px(6.))
                        .text_size(theme::font(14.))
                        .font_weight(theme::LABEL_WEIGHT)
                        .text_color(p.ink)
                        .child(label(&format!("components.sections.{key}"))),
                )
                .children(cards)
        };
        let body = div()
            .id("components-content")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.components_scroll)
            .flex()
            .flex_col()
            .items_center()
            .pl(px(4.))
            .pr(px(4. + scrollbar::SIZE))
            .pt(px(4.))
            .pb(px(32.))
            .child(
                div()
                    .w_full()
                    .max_w(px(CONTENT))
                    .flex()
                    .flex_col()
                    .child(section(true, "tray", tray))
                    .child(section(false, "planned", planned)),
            )
            .child(scrollbar::scrollbar(
                "components-scrollbar",
                &self.components_scroll,
                p.dark,
            ));
        div()
            .size_full()
            .min_h_0()
            .px(px(12.))
            .child(body)
            .into_any_element()
    }

    /// The tray's look, its hover card, what a right click opens, and how
    /// Argus stays in the tray.
    fn tray_cards(
        &mut self,
        settings: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let looks = ["logo", "load", "number"];
        let look = text(settings, "trayIcon");
        let icon = self.segments(
            "setting-trayIcon",
            looks
                .iter()
                .map(|value| {
                    (
                        label(&format!("components.tray.icon.{value}")),
                        None,
                        Action::Invoke("set_tray".into(), json!({ "icon": value })),
                        look == *value || (look.is_empty() && *value == "load"),
                    )
                })
                .collect(),
            cx,
        );
        let menus = ["panel", "menu"];
        let menu = text(settings, "trayMenu");
        let menu = self.segments(
            "setting-trayMenu",
            menus
                .iter()
                .map(|value| {
                    (
                        label(&format!("components.tray.menu.{value}")),
                        None,
                        Action::Invoke("set_tray".into(), json!({ "menu": value })),
                        menu == *value || (menu.is_empty() && *value == "panel"),
                    )
                })
                .collect(),
            cx,
        );
        let card = self.setting_toggle(
            "trayCard",
            settings["trayCard"].as_bool().unwrap_or(true),
            "set_tray",
            "card",
            window,
            cx,
        );
        let close = self.setting_toggle(
            "closeToTray",
            boolean(settings, "closeToTray"),
            "set_close_to_tray",
            "value",
            window,
            cx,
        );
        let interval = self.background_select("components", "tray", cx);
        vec![
            self.setting_text(
                "trayIcon",
                "TrayIcon",
                "components.tray.icon.title",
                "components.tray.icon.desc",
                vec![icon],
            ),
            self.setting_text(
                "trayCard",
                "PointerIcon",
                "components.tray.card.title",
                "components.tray.card.desc",
                vec![card],
            ),
            self.setting_text(
                "trayMenu",
                "DashboardIcon",
                "components.tray.menu.title",
                "components.tray.menu.desc",
                vec![menu],
            ),
            self.setting_text(
                "closeToTray",
                "CloseIcon",
                "settings.startup.closeToTray.title",
                "settings.startup.closeToTray.desc",
                vec![close],
            ),
            self.tray_page_setting(cx),
            self.setting_text(
                "trayInterval",
                "TimerIcon",
                "settings.data.tray.title",
                "settings.data.tray.desc",
                vec![interval],
            ),
        ]
    }

    /// Where Settings points to the tray's settings on this page.
    pub(in crate::frontend) fn components_link(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self
            .btn(
                "open-components",
                label("components.tray.moved.open"),
                None,
                Tone::Plain,
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.act(Action::Page("components".into()), window, cx)
            }))
            .into_any_element();
        self.setting_text(
            "trayMoved",
            "ComponentsIcon",
            "components.tray.moved.title",
            "components.tray.moved.desc",
            vec![open],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::COMPONENTS;

    #[test]
    fn the_tray_is_available_and_the_reserved_components_are_planned() {
        let keys = COMPONENTS.map(|component| (component.key, component.available));
        assert_eq!(
            keys,
            [
                ("tray", true),
                ("desktop", false),
                ("taskbar", false),
                ("island", false)
            ]
        );
    }
}
