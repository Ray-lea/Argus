//! Components: the parts of Argus that stay on the desktop outside its
//! window: the tray, the desktop monitor, the taskbar monitor and, as it
//! arrives, the island. Each available component has a section of its
//! settings; a planned one is listed with what it will be. A list of the
//! sections on the left follows the scroll (`section_rail`).
use super::section_rail::RailEntry;
use super::*;
use crate::frontend::monitors::{
    absent_name, arrange, device_kind, devices, main_gpu_key, metric_color, metric_key,
};
use crate::frontend::widgets::Tone;
use crate::settings::{
    DeskLayout, DeskStyle, DesktopMonitor, MONITOR_OPACITIES, Metric, Reading, TaskbarMonitor,
};

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

/// The width of the device names beside the options.
const NAME_COLUMN: f32 = 240.;

/// A row of options: a device, the default one of its kind, or the CPU, the
/// memory and the rest.
struct OptionRow {
    /// Its place in the card's element ids and test anchors.
    id: String,
    title: String,
    detail: String,
    /// The device its readings name; none for the default.
    device: Option<String>,
    /// The device's own key, when the row holds the readings without one
    /// (the main GPU): readings that name it are checked here too.
    alias: Option<String>,
    metrics: Vec<Metric>,
}

/// The metrics on offer, a section of rows per kind of device, in the order of
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

/// The page's sections, in order: one per available component, then the
/// planned ones.
pub(in crate::frontend) const SECTIONS: [&str; 4] = ["tray", "desktop", "taskbar", "planned"];

/// The height of a style's preview: the taskbar's, as the previews are cut.
const STYLE_PREVIEW: f32 = 40.;
/// The least width of a style card in a row of four; with less room the
/// cards go two by two.
const STYLE_CARD: f32 = 150.;
const STYLE_GAP: f32 = 6.;
/// The height of a desktop monitor style's preview: its first group and a
/// part of the next.
const DESK_PREVIEW: f32 = 128.;

/// The taskbar monitor's styles, as saved, in the order offered.
pub(in crate::frontend) const TASKBAR_STYLES: [&str; 4] =
    ["labels", "curves", "capsules", "figures"];

/// What a control of the taskbar monitor's panel does.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::frontend) enum TaskbarChoice {
    Style(&'static str),
    /// The readings with one added or taken away, as saved.
    Readings(Vec<String>),
    /// The components page in the window.
    Settings,
    Hide,
}

/// A kind of reading in the taskbar monitor's panel: the CPU, the memory,
/// the GPUs, the disks, the network adapters or the rest.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::frontend) struct PanelKind {
    pub key: &'static str,
    pub title: String,
    /// What tells its only device apart, for its tip.
    pub tip: String,
    pub tone: Hsla,
    /// One with no tab, or a tab per device of several.
    pub devices: Vec<PanelDevice>,
}

/// A device's options, or all of a kind's when it has no tabs.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::frontend) struct PanelDevice {
    pub tab: String,
    pub tip: String,
    /// Some of its readings are chosen: its tab carries a dot.
    pub chosen: bool,
    pub options: Vec<PanelOption>,
}

/// A reading to check: its key (`Reading::text`), its name and tip,
/// whether it shows, and the readings with it added or taken away.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::frontend) struct PanelOption {
    pub key: String,
    pub text: String,
    pub tip: String,
    pub on: bool,
    pub readings: Vec<String>,
}

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
        let section = |key: &str, cards: Vec<AnyElement>| {
            div()
                .id(SharedString::from(format!("components-{key}")))
                .w_full()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .gap(px(4.))
                .child(
                    div()
                        .mt(px(if key == SECTIONS[0] { 6. } else { 28. }))
                        .mb(px(6.))
                        .text_size(theme::font(14.))
                        .font_weight(theme::LABEL_WEIGHT)
                        .text_color(p.ink)
                        .child(label(&format!("components.sections.{key}"))),
                )
                .children(cards)
                .into_any_element()
        };
        let sections = vec![
            section("tray", tray),
            section("desktop", desktop),
            section("taskbar", taskbar),
            section("planned", planned),
        ];
        let entries = SECTIONS
            .map(|key| RailEntry {
                key,
                name: label(&format!("components.sections.{key}")),
                icon: COMPONENTS
                    .iter()
                    .find(|component| component.key == key)
                    .map_or("ComponentsIcon", |component| component.glyph),
                dot: None,
            })
            .into();
        self.section_page(
            "components",
            |this| &mut this.components_rail,
            entries,
            sections,
            window,
            cx,
        )
    }

    /// Opens the page at a section (`SECTIONS`), as the tray's and the
    /// monitors' settings entries do.
    pub(in crate::frontend) fn open_components(
        &mut self,
        section: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = SECTIONS.iter().position(|key| *key == section) {
            self.components_rail.open_at(index);
        }
        if self.page != "components" {
            self.act(Action::Page("components".into()), window, cx);
        }
        cx.notify();
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

    /// The rows of options for one kind of reading, from the sample: one row
    /// for the CPU, the memory and the rest; for GPUs, disks and network
    /// adapters one row for the only device, or with several a row for the
    /// default (all of them, or the main GPU) and one for each, and a row
    /// for each device chosen before that is away now.
    fn option_rows(&self, section: usize, chosen: &[Reading]) -> Vec<OptionRow> {
        let (group, metrics) = METRIC_GROUPS[section];
        let snapshot = &self.snapshot;
        let hardware = &snapshot["hardware"];
        let group_title = label(&format!("components.metrics.group.{group}"));
        let row = |id: String, title: String, detail: String, device: Option<String>| OptionRow {
            id,
            title,
            detail,
            device,
            alias: None,
            metrics: metrics.to_vec(),
        };
        let Some(kind) = metrics.first().and_then(|metric| device_kind(*metric)) else {
            let detail = match group {
                "cpu" => text(&hardware["cpu"], "name").trim().to_owned(),
                "memory" => {
                    let total = number(&snapshot["system"], "memTotal");
                    if total > 0. {
                        theme::bytes(total, false)
                    } else {
                        String::new()
                    }
                }
                _ => String::new(),
            };
            let mut only = row(group.into(), group_title, detail, None);
            // The battery is offered where there is one, or once chosen.
            if group == "other"
                && values(&hardware["batteries"]).is_empty()
                && !chosen
                    .iter()
                    .any(|reading| reading.metric == Metric::Battery)
            {
                only.metrics.retain(|metric| *metric != Metric::Battery);
            }
            return vec![only];
        };
        let devices = devices(snapshot, kind);
        let mut rows = match devices.as_slice() {
            [] => vec![row(group.into(), group_title, String::new(), None)],
            [only] => {
                let detail = if kind == "gpu" {
                    only.title.clone()
                } else {
                    [only.title.as_str(), only.detail.as_str()]
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ")
                };
                vec![row(group.into(), group_title, detail, None)]
            }
            many if kind == "gpu" => {
                // The main GPU's row holds the readings without a device,
                // which follow it; the others name theirs.
                let main = main_gpu_key(snapshot, many);
                let mut rows = Vec::new();
                for (index, device) in many.iter().enumerate() {
                    let is_main = main.as_deref() == Some(device.key.as_str());
                    let detail = if is_main {
                        tr!("components.metrics.gpuMain", memory = device.detail.clone())
                    } else {
                        device.detail.clone()
                    };
                    let mut gpu = row(
                        format!("gpu-{index}"),
                        device.title.clone(),
                        detail,
                        (!is_main).then(|| device.key.clone()),
                    );
                    if is_main {
                        gpu.alias = Some(device.key.clone());
                        rows.insert(0, gpu);
                    } else {
                        rows.push(gpu);
                    }
                }
                rows
            }
            many => std::iter::once(row(
                group.into(),
                label(&format!("components.metrics.all.{group}")),
                label(&format!("components.metrics.all.{group}Detail")),
                None,
            ))
            .chain(many.iter().enumerate().map(|(index, device)| {
                row(
                    format!("{group}-{index}"),
                    device.title.clone(),
                    device.detail.clone(),
                    Some(device.key.clone()),
                )
            }))
            .collect(),
        };
        // Devices chosen before and away now, so that they can be unchosen.
        let mut away: Vec<&str> = Vec::new();
        for reading in chosen {
            if let Some(key) = reading.device.as_deref()
                && device_kind(reading.metric) == Some(kind)
                && !devices.iter().any(|device| device.key == key)
                && !away.contains(&key)
            {
                away.push(key);
            }
        }
        for (index, key) in away.into_iter().enumerate() {
            rows.push(row(
                format!("{group}-away-{index}"),
                absent_name(key),
                label("components.metrics.absent"),
                Some(key.to_owned()),
            ));
        }
        rows
    }

    /// `metric` of `row`: its reading, whether it is among `chosen`, and
    /// the readings with it added or taken away, as saved.
    fn toggled(
        &self,
        chosen: &[Reading],
        row: &OptionRow,
        metric: Metric,
    ) -> (Reading, bool, Vec<String>) {
        let reading = Reading {
            metric,
            device: row.device.clone(),
        };
        let alias = row.alias.clone().map(|device| Reading {
            metric,
            device: Some(device),
        });
        let on =
            chosen.contains(&reading) || alias.as_ref().is_some_and(|alias| chosen.contains(alias));
        let mut next = chosen
            .iter()
            .filter(|other| **other != reading && Some(*other) != alias.as_ref())
            .cloned()
            .collect::<Vec<_>>();
        if !on {
            next.push(reading.clone());
        }
        let next = arrange(&self.snapshot, next)
            .iter()
            .map(Reading::text)
            .collect::<Vec<_>>();
        (reading, on, next)
    }

    /// The readings the taskbar monitor's panel offers, as the components
    /// page offers them: per kind, one set of options, or with several
    /// devices of a kind a tab per device (the GPUs, the disks); network
    /// adapters, with one reading each, are options of their own.
    pub(in crate::frontend) fn taskbar_panel_kinds(&self) -> Vec<PanelKind> {
        let monitor: TaskbarMonitor =
            serde_json::from_value(self.data["settings"]["taskbarMonitor"].clone())
                .unwrap_or_default();
        let chosen = &monitor.metrics;
        let option = |row: &OptionRow, metric: Metric, text: String, tip: String| {
            let (reading, on, readings) = self.toggled(chosen, row, metric);
            PanelOption {
                key: reading.text(),
                text,
                tip,
                on,
                readings,
            }
        };
        let metric_name =
            |metric: Metric| label(&format!("components.metric.{}.option", metric_key(metric)));
        let mut kinds = Vec::new();
        for (section, (group, metrics)) in METRIC_GROUPS.iter().enumerate() {
            let rows = self.option_rows(section, chosen);
            let devices = match rows.as_slice() {
                [] => continue,
                [row] => vec![PanelDevice {
                    tab: String::new(),
                    tip: String::new(),
                    chosen: false,
                    options: row
                        .metrics
                        .iter()
                        .map(|metric| option(row, *metric, metric_name(*metric), String::new()))
                        .collect(),
                }],
                rows if rows.iter().all(|row| row.metrics.len() == 1) => vec![PanelDevice {
                    tab: String::new(),
                    tip: String::new(),
                    chosen: false,
                    options: rows
                        .iter()
                        .map(|row| {
                            option(row, row.metrics[0], row.title.clone(), row.detail.clone())
                        })
                        .collect(),
                }],
                rows => rows
                    .iter()
                    .map(|row| {
                        let options = row
                            .metrics
                            .iter()
                            .map(|metric| option(row, *metric, metric_name(*metric), String::new()))
                            .collect::<Vec<_>>();
                        PanelDevice {
                            tab: row.title.clone(),
                            tip: row.detail.clone(),
                            chosen: options.iter().any(|option| option.on),
                            options,
                        }
                    })
                    .collect(),
            };
            kinds.push(PanelKind {
                key: group,
                title: label(&format!("components.metrics.group.{group}")),
                tip: match rows.as_slice() {
                    [row] => row.detail.clone(),
                    _ => String::new(),
                },
                tone: metric_color(metrics[0], self.palette),
                devices,
            });
        }
        kinds
    }

    /// What the control the tests name by `key` does: "style:curves",
    /// "reading:<reading>" (as saved), "settings" or "hide".
    pub(in crate::frontend) fn taskbar_panel_choice(&self, key: &str) -> Option<TaskbarChoice> {
        if let Some(style) = key.strip_prefix("style:") {
            return TASKBAR_STYLES
                .iter()
                .find(|value| **value == style)
                .map(|value| TaskbarChoice::Style(value));
        }
        if let Some(reading) = key.strip_prefix("reading:") {
            return self
                .taskbar_panel_kinds()
                .into_iter()
                .flat_map(|kind| kind.devices)
                .flat_map(|device| device.options)
                .find(|option| option.key == reading)
                .map(|option| TaskbarChoice::Readings(option.readings));
        }
        match key {
            "settings" => Some(TaskbarChoice::Settings),
            "hide" => Some(TaskbarChoice::Hide),
            _ => None,
        }
    }

    /// A monitor's readings, checked in a table under its card's
    /// description: a row per device (or for the CPU, the memory and the
    /// rest), its name and what tells it apart on the left, its options in
    /// fixed columns on the right, kinds of device apart. A click adds or
    /// removes one; checking changes only its box.
    fn metric_card(
        &self,
        id: &str,
        settings_key: &'static str,
        command: &'static str,
        chosen: &[Reading],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let option = |row: &OptionRow, metric: Metric| {
            let (_, on, next) = self.toggled(chosen, row, metric);
            let key = metric_key(metric);
            let name = label(&format!("components.metric.{key}.option"));
            let anchor = SharedString::from(format!("{id}-{}-{key}", row.id));
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
                .accessibility_label(format!("{} {name}", row.title))
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
        let row_element = |row: &OptionRow| {
            let full = if row.detail.is_empty() {
                row.title.clone()
            } else {
                format!("{}\n{}", row.title, row.detail)
            };
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-{}-name", row.id)))
                        .w(px(NAME_COLUMN))
                        .flex_shrink_0()
                        .min_h(px(32.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(
                            div()
                                .truncate()
                                .text_size(theme::font(13.))
                                .text_color(p.ink)
                                .child(row.title.clone()),
                        )
                        .when(!row.detail.is_empty(), |cell| {
                            cell.child(
                                div()
                                    .truncate()
                                    .text_size(theme::font(12.))
                                    .text_color(p.muted)
                                    .child(row.detail.clone()),
                            )
                        })
                        .tooltip(move |window, cx| widgets::tooltip(full.clone(), window, cx)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .grid()
                        .grid_cols(3)
                        .gap_x(px(4.))
                        .gap_y(px(2.))
                        .children(row.metrics.iter().map(|metric| option(row, *metric))),
                )
                .into_any_element()
        };
        let sections = (0..METRIC_GROUPS.len()).map(|section| {
            let rows = self.option_rows(section, chosen);
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .when(section > 0, |group| {
                    group
                        .pt(px(8.))
                        .mt(px(8.))
                        .border_t_1()
                        .border_color(p.stroke)
                })
                .children(rows.iter().map(row_element))
        });
        let desc = div()
            .child(label("components.metrics.desc"))
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .flex_col()
                    .text_color(p.ink)
                    .children(sections),
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
        let choices = |key: &str, values: &[&str], chosen: &str| {
            values
                .iter()
                .map(|value| {
                    (
                        label(&format!("components.desktop.{key}.{value}")),
                        None,
                        Action::Invoke(command.into(), json!({ key: value })),
                        *value == chosen,
                    )
                })
                .collect::<Vec<_>>()
        };
        let layout = self.segments(
            "setting-desktopLayout",
            choices("layout", &["panel", "strip"], &String::from(monitor.layout)),
            cx,
        );
        let panel = monitor.layout == DeskLayout::Panel;
        let style = panel.then(|| self.desk_style_card(monitor.style, window, cx));
        // Gauges are tiles: set apart already.
        let grouping = (panel && monitor.style != DeskStyle::Gauges).then(|| {
            let segments = self.segments(
                "setting-desktopGrouping",
                choices(
                    "grouping",
                    &["cards", "stripes", "rules"],
                    &String::from(monitor.grouping),
                ),
                cx,
            );
            self.setting_text(
                "desktopGrouping",
                "AppsIcon",
                "components.desktop.grouping.title",
                "components.desktop.grouping.desc",
                vec![segments],
            )
        });
        let mut cards = vec![
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
                "desktopLayout",
                "PanelIcon",
                "components.desktop.layout.title",
                "components.desktop.layout.desc",
                vec![layout],
            ),
        ];
        cards.extend(style);
        cards.extend(grouping);
        cards.extend([
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
        ]);
        cards
    }

    /// The desktop monitor's panel styles: a card each with the panel's
    /// first two groups in that style, as wide as the card, and its name
    /// beside a radio mark; a click chooses one.
    fn desk_style_card(
        &mut self,
        chosen: DeskStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let (test, anchors) = (self.options.test, self.ui.anchors.clone());
        let cards = [DeskStyle::List, DeskStyle::Curves, DeskStyle::Gauges]
            .into_iter()
            .map(|style| {
                let on = style == chosen;
                let key = String::from(style);
                let name = label(&format!("components.desktop.style.{key}"));
                let anchor = SharedString::from(format!("desktopStyle-{key}"));
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
                Button::new(anchor.clone())
                    .relative()
                    .flex_1()
                    .min_w(px(180.))
                    .flex_col()
                    .items_stretch()
                    .justify_start()
                    .gap(px(6.))
                    .p(px(4.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(if on { p.accent } else { p.stroke_strong })
                    .when(on, |card| card.bg(p.accent.opacity(0.08)))
                    .hover(move |card| card.bg(if on { p.accent.opacity(0.12) } else { p.hover }))
                    .active(move |card| card.bg(p.press))
                    .role(Role::RadioButton)
                    .aria_toggled(if on { Toggled::True } else { Toggled::False })
                    .accessibility_label(name.clone())
                    .child(
                        div()
                            .h(px(DESK_PREVIEW))
                            .rounded(px(4.))
                            .bg(p.layer)
                            .overflow_hidden()
                            .p(px(8.))
                            .child(self.desk_preview(style, window, cx)),
                    )
                    .child(
                        div()
                            .h(px(20.))
                            .px(px(2.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(theme::font(13.))
                            .text_color(p.ink)
                            .child(mark)
                            .child(div().min_w_0().truncate().child(name)),
                    )
                    // Where tests take hold of it.
                    .when(test, |card| {
                        let anchors = anchors.clone();
                        card.child(
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
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.desk_look(json!({ "style": String::from(style) }), window, cx);
                    }))
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let desc = div()
            .child(label("components.desktop.style.desc"))
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .flex_wrap()
                    .gap(px(STYLE_GAP))
                    .children(cards),
            )
            .into_any_element();
        self.setting(
            "desktopStyle",
            self.glyph("ThemeIcon"),
            label("components.desktop.style.title"),
            Some(desc),
            None,
            Vec::new(),
        )
    }

    /// A change to the desktop monitor's look: shown at once, on this page
    /// and by the monitor, and saved.
    pub(in crate::frontend) fn desk_look(
        &mut self,
        change: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(monitor) = self
            .data
            .get_mut("settings")
            .and_then(|settings| settings.get_mut("desktopMonitor"))
            && let (Some(monitor), Some(change)) = (monitor.as_object_mut(), change.as_object())
        {
            for (key, value) in change {
                monitor.insert(key.clone(), value.clone());
            }
        }
        self.request("set_desktop_monitor", change, Some("settings"), window, cx);
        self.sync_monitors(cx);
        cx.notify();
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
        let style = text(&settings["taskbarMonitor"], "style").to_owned();
        let style = self.style_card(&style, window, cx);
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
            style,
            self.setting_text(
                "taskbarPlace",
                "FitWidthIcon",
                "components.taskbar.place.title",
                "components.taskbar.place.desc",
                vec![place],
            ),
        ]
    }

    /// The taskbar monitor's styles as its panel offers them: a card each
    /// with the monitor's look in it on the taskbar's colour and its name
    /// beside a radio mark; a click chooses one. Four in a row where the
    /// card has the room, else two by two: the cards go in pairs, and the
    /// second pair wraps.
    fn style_card(
        &mut self,
        chosen: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let (previews, ground) = self.page_previews(window);
        let (test, anchors) = (self.options.test, self.ui.anchors.clone());
        let mut cards = TASKBAR_STYLES
            .iter()
            .enumerate()
            .map(|(index, style)| {
                let style: &'static str = style;
                let on = chosen == style || (chosen.is_empty() && index == 0);
                let name = label(&format!("components.taskbar.style.{style}"));
                let anchor = SharedString::from(format!("taskbarStyle-{style}"));
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
                // Scaled down to fit the card, never up, and on whole pixels
                // at its own size.
                let preview = previews.get(index).cloned().flatten().map(|image| {
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let scale = window.scale_factor();
                            let size = image.size(0);
                            let width = u32::from(size.width) as f32 / scale;
                            let height = u32::from(size.height) as f32 / scale;
                            let fit = (f32::from(bounds.size.width) / width)
                                .min(f32::from(bounds.size.height) / height)
                                .min(1.);
                            let size = gpui_kit::size(px(width * fit), px(height * fit));
                            let origin = bounds.center() - point(size.width / 2., size.height / 2.);
                            let at = Bounds::new(
                                point(window.pixel_snap(origin.x), window.pixel_snap(origin.y)),
                                size,
                            );
                            let _ = window.paint_image(at, at, Corners::default(), image, 0, false);
                        },
                    )
                    .size_full()
                });
                Button::new(anchor.clone())
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .flex_col()
                    .items_stretch()
                    .justify_start()
                    .gap(px(6.))
                    .p(px(4.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(if on { p.accent } else { p.stroke_strong })
                    .when(on, |card| card.bg(p.accent.opacity(0.08)))
                    .hover(move |card| card.bg(if on { p.accent.opacity(0.12) } else { p.hover }))
                    .active(move |card| card.bg(p.press))
                    .role(Role::RadioButton)
                    .aria_toggled(if on { Toggled::True } else { Toggled::False })
                    .accessibility_label(name.clone())
                    .child(
                        div()
                            .h(px(STYLE_PREVIEW))
                            .rounded(px(4.))
                            .bg(ground)
                            .overflow_hidden()
                            .children(preview),
                    )
                    .child(
                        div()
                            .h(px(20.))
                            .px(px(2.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(theme::font(13.))
                            .text_color(p.ink)
                            .child(mark)
                            .child(div().min_w_0().truncate().child(name)),
                    )
                    // Where tests take hold of it.
                    .when(test, |card| {
                        let anchors = anchors.clone();
                        card.child(
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
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.taskbar_choice(TaskbarChoice::Style(style), window, cx);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let second = cards.split_off(2);
        let pair = |cards: Vec<AnyElement>| {
            div()
                .flex()
                .gap(px(STYLE_GAP))
                .flex_grow(1.)
                .flex_basis(px(2. * STYLE_CARD + STYLE_GAP))
                .min_w_0()
                .children(cards)
        };
        let desc = div()
            .child(label("components.taskbar.style.desc"))
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .flex_wrap()
                    .gap(px(STYLE_GAP))
                    .child(pair(cards))
                    .child(pair(second)),
            )
            .into_any_element();
        self.setting(
            "taskbarStyle",
            self.glyph("PulseIcon"),
            label("components.taskbar.style.title"),
            Some(desc),
            None,
            Vec::new(),
        )
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
    use super::{COMPONENTS, METRIC_GROUPS, SECTIONS};
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

    #[test]
    fn each_available_component_has_a_section_before_the_planned_ones() {
        let available = COMPONENTS
            .iter()
            .filter(|component| component.available)
            .map(|component| component.key)
            .chain(["planned"])
            .collect::<Vec<_>>();
        assert_eq!(available, SECTIONS);
    }
}
