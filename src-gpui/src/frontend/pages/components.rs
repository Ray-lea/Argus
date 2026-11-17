//! Components: the parts of Argus that stay on the desktop outside its
//! window: the tray, the desktop monitor, the taskbar monitor and, as it
//! arrives, the island. Each available component has a section of its
//! settings; a planned one is listed with what it will be.
use super::*;
use crate::frontend::monitors::metric_key;
use crate::frontend::widgets::Tone;
use crate::settings::{DesktopMonitor, MONITOR_OPACITIES, Metric, TaskbarMonitor};

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
        available: true,
    },
    Component {
        key: "taskbar",
        glyph: "GaugeIcon",
        available: true,
    },
    Component {
        key: "island",
        glyph: "NavTopIcon",
        available: false,
    },
];

/// The metrics on offer, a row of options per device, in the order of
/// `Metric::ALL`.
const METRIC_GROUPS: [(&str, &[Metric]); 6] = [
    (
        "cpu",
        &[
            Metric::Cpu,
            Metric::CpuTemperature,
            Metric::CpuClock,
            Metric::CpuPower,
        ],
    ),
    ("memory", &[Metric::Memory, Metric::MemoryUsed]),
    (
        "gpu",
        &[
            Metric::Gpu,
            Metric::GpuTemperature,
            Metric::GpuMemory,
            Metric::GpuClock,
            Metric::GpuPower,
            Metric::GpuFan,
        ],
    ),
    (
        "disk",
        &[Metric::Disk, Metric::DiskActive, Metric::DiskTemperature],
    ),
    ("network", &[Metric::Network]),
    ("other", &[Metric::Battery, Metric::Uptime]),
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
        let desktop = self.desktop_cards(&settings, window, cx);
        let taskbar = self.taskbar_cards(&settings, window, cx);
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
                    .child(section(false, "desktop", desktop))
                    .child(section(false, "taskbar", taskbar))
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
        let looks = ["logo", "load", "share", "number"];
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

    /// A monitor's metrics, checked in a grid under its card's description,
    /// a row of options per device; a click adds or removes one, and they
    /// keep the order offered.
    fn metric_card(
        &self,
        id: &str,
        settings_key: &'static str,
        command: &'static str,
        chosen: &[Metric],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let option = |metric: &Metric| {
            let on = chosen.contains(metric);
            let next = Metric::ALL
                .iter()
                .filter(|other| chosen.contains(other) != (*other == metric))
                .map(|other| metric_key(*other))
                .collect::<Vec<_>>();
            let key = metric_key(*metric);
            let name = label(&format!("components.metric.{key}.option"));
            let anchor = SharedString::from(format!("{id}-{key}"));
            let anchors = self.ui.anchors.clone();
            Button::new(anchor.clone())
                .relative()
                .when(self.options.test, |option| {
                    option.child(
                        canvas(
                            move |bounds, _, _| {
                                anchors.borrow_mut().insert(anchor.clone(), bounds);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                })
                .h(px(32.))
                .min_w_0()
                .flex()
                .items_center()
                .justify_start()
                .gap(px(8.))
                .px(px(8.))
                .rounded(px(4.))
                .text_size(theme::font(13.))
                .text_color(p.ink)
                .role(Role::CheckBox)
                .aria_toggled(if on { Toggled::True } else { Toggled::False })
                .accessibility_label(name.clone())
                .hover(move |style| style.bg(p.hover))
                .active(move |style| style.bg(p.press))
                .child(self.check_box(on, false))
                .child(div().min_w_0().truncate().child(name))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.choose_metrics(settings_key, command, next.clone(), window, cx);
                }))
                .into_any_element()
        };
        // A fixed grid: checking an option changes only its box, never the
        // place or size of the others or of the card.
        let rows = METRIC_GROUPS.iter().map(|(group, metrics)| {
            div()
                .flex()
                .items_start()
                .child(
                    div()
                        .w(px(64.))
                        .h(px(32.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .text_size(theme::font(13.))
                        .text_color(p.muted)
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .child(label(&format!("components.metrics.group.{group}"))),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .grid()
                        .grid_cols(4)
                        .gap_x(px(4.))
                        .gap_y(px(2.))
                        .children(metrics.iter().map(option)),
                )
        });
        let desc = div()
            .child(label("components.metrics.desc"))
            .child(
                div()
                    .mt(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .text_color(p.ink)
                    .children(rows),
            )
            .into_any_element();
        self.setting(
            id,
            self.glyph("ListIcon"),
            label("components.metrics.title"),
            Some(desc),
            None,
            Vec::new(),
        )
    }

    /// The desktop monitor: on or off, its metrics, how it sits among the
    /// other windows and how it answers the pointer.
    fn desktop_cards(
        &mut self,
        settings: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let monitor: DesktopMonitor =
            serde_json::from_value(settings["desktopMonitor"].clone()).unwrap_or_default();
        let command = "set_desktop_monitor";
        let mut toggle = |id: &str, value: bool, field: &str| {
            self.setting_toggle(id, value, command, field, window, cx)
        };
        let enabled = toggle("desktopMonitor", monitor.enabled, "enabled");
        let topmost = toggle("desktopTopmost", monitor.topmost, "topmost");
        let locked = toggle("desktopLocked", monitor.locked, "locked");
        let through = toggle("desktopClickThrough", monitor.click_through, "clickThrough");
        let opacity = self.select(
            "components-desktop-opacity",
            label("components.desktop.opacity.title"),
            MONITOR_OPACITIES
                .iter()
                .map(|value| format!("{value}%"))
                .collect(),
            MONITOR_OPACITIES
                .iter()
                .position(|value| *value == monitor.opacity),
            false,
            move |this, index, window, cx| {
                this.request(
                    command,
                    json!({ "opacity": MONITOR_OPACITIES[index] }),
                    Some("settings"),
                    window,
                    cx,
                );
            },
            cx,
        );
        vec![
            self.setting_text(
                "desktopMonitor",
                "ComputerIcon",
                "components.desktop.title",
                "components.desktop.desc",
                vec![enabled],
            ),
            self.metric_card(
                "desktopMetrics",
                "desktopMonitor",
                command,
                &monitor.metrics,
                cx,
            ),
            self.setting_text(
                "desktopTopmost",
                "PinIcon",
                "components.desktop.topmost.title",
                "components.desktop.topmost.desc",
                vec![topmost],
            ),
            self.setting_text(
                "desktopLocked",
                "LockIcon",
                "components.desktop.locked.title",
                "components.desktop.locked.desc",
                vec![locked],
            ),
            self.setting_text(
                "desktopClickThrough",
                "PointerIcon",
                "components.desktop.clickThrough.title",
                "components.desktop.clickThrough.desc",
                vec![through],
            ),
            self.setting_text(
                "desktopOpacity",
                "EyeIcon",
                "components.desktop.opacity.title",
                "components.desktop.opacity.desc",
                vec![opacity],
            ),
        ]
    }

    /// The taskbar monitor: on or off, its metrics and where it sits.
    fn taskbar_cards(
        &mut self,
        settings: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let monitor: TaskbarMonitor =
            serde_json::from_value(settings["taskbarMonitor"].clone()).unwrap_or_default();
        let command = "set_taskbar_monitor";
        let enabled = self.setting_toggle(
            "taskbarMonitor",
            monitor.enabled,
            command,
            "enabled",
            window,
            cx,
        );
        let place = text(&settings["taskbarMonitor"], "place");
        let place = self.segments(
            "setting-taskbarPlace",
            ["tray", "start"]
                .iter()
                .map(|value| {
                    (
                        label(&format!("components.taskbar.place.{value}")),
                        None,
                        Action::Invoke(command.into(), json!({ "place": value })),
                        place == *value || (place.is_empty() && *value == "tray"),
                    )
                })
                .collect(),
            cx,
        );
        vec![
            self.setting_text(
                "taskbarMonitor",
                "GaugeIcon",
                "components.taskbar.title",
                "components.taskbar.desc",
                vec![enabled],
            ),
            self.metric_card(
                "taskbarMetrics",
                "taskbarMonitor",
                command,
                &monitor.metrics,
                cx,
            ),
            self.setting_text(
                "taskbarPlace",
                "FitWidthIcon",
                "components.taskbar.place.title",
                "components.taskbar.place.desc",
                vec![place],
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
    use super::{COMPONENTS, METRIC_GROUPS};
    use crate::settings::Metric;

    #[test]
    fn the_metric_rows_offer_every_metric_once_in_order() {
        let offered = METRIC_GROUPS
            .iter()
            .flat_map(|(_, metrics)| metrics.iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(offered, Metric::ALL);
    }

    #[test]
    fn the_tray_and_the_monitors_are_available_and_the_island_is_planned() {
        let keys = COMPONENTS.map(|component| (component.key, component.available));
        assert_eq!(
            keys,
            [
                ("tray", true),
                ("desktop", true),
                ("taskbar", true),
                ("island", false)
            ]
        );
    }
}
