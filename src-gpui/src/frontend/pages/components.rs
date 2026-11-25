//! Components: the parts of Argus that stay on the desktop outside its
//! window: the tray, the desktop monitor, the taskbar monitor and, as it
//! arrives, the lookout. Each available component has a section of its
//! settings; a planned one is listed with what it will be. A list of the
//! sections on the left follows the scroll (`section_rail`).
use super::section_rail::RailEntry;
use super::*;
use crate::frontend::monitors::{
    Kind, absent_name, arrange, arranged, device_kind, devices, main_gpu_key, metric_color,
    metric_key, with_kinds, with_metrics,
};
use crate::frontend::reorder::{Flow, Lift, Lifted, Unseen};
use crate::frontend::shift;
use crate::frontend::widgets::Tone;
use crate::settings::{
    DeskLayout, DeskStyle, DesktopMonitor, MONITOR_OPACITIES, Metric, Reading, TaskbarMonitor,
    TrayIcon,
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
        key: "lookout",
        glyph: "NavTopIcon",
        available: false,
    },
];

/// The width of the device names beside the options.
const NAME_COLUMN: f32 = 240.;
/// An option to check, and the space between options across and down.
const OPTION: f32 = 32.;
const OPTION_GAP: f32 = 4.;
const OPTION_LINE_GAP: f32 = 2.;
/// What a block of options reaches past the table on either side: its grip
/// in the column of the card's icon on the left, room for its outline on
/// the right.
const BLOCK_START: f32 = 36.;
const BLOCK_END: f32 = 8.;
/// The space above and below a block's rows: a line between two blocks
/// sits in the middle of twice this.
const BLOCK_PAD: f32 = 8.;

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
    /// The kinds of reading in another order, by key (`monitors::KINDS`).
    Kinds(Vec<String>),
    /// A kind's metrics in another order, by name (`metric_key`).
    Metrics(Vec<String>),
    /// The display whose taskbar it sits on, by its key; empty for the
    /// primary display.
    Display(String),
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
    /// Its options are its metrics, which can be dragged into another
    /// order, rather than a device each (network adapters).
    pub sortable: bool,
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
    /// Its metric's name (`metric_key`), what it is dragged by.
    pub metric: &'static str,
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
        let look: String = TrayIcon::from(text(settings, "trayIcon").to_owned()).into();
        let icon = self.select(
            "setting-trayIcon",
            label("components.tray.icon.title"),
            looks
                .iter()
                .map(|value| label(&format!("components.tray.icon.{value}")))
                .collect(),
            looks.iter().position(|value| *value == look),
            false,
            move |this, index, window, cx| {
                this.request(
                    "set_tray",
                    json!({ "icon": looks[index] }),
                    Some("settings"),
                    window,
                    cx,
                );
            },
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
            "components.tray.card.title",
            settings["trayCard"].as_bool().unwrap_or(true),
            "set_tray",
            "card",
            window,
            cx,
        );
        let close = self.setting_toggle(
            "closeToTray",
            "settings.startup.closeToTray.title",
            boolean(settings, "closeToTray"),
            "set_close_to_tray",
            "value",
            window,
            cx,
        );
        let interval = self.background_card("components", "trayInterval", "TimerIcon", "tray", cx);
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
            interval,
        ]
    }

    /// The rows of options for one kind of reading, from the sample: one row
    /// for the CPU, the memory and the rest; for GPUs, disks and network
    /// adapters one row for the only device, or with several a row for the
    /// default (all of them, or the main GPU) and one for each, and a row
    /// for each device chosen before that is away now.
    fn option_rows(&self, kind: &Kind, chosen: &[Reading]) -> Vec<OptionRow> {
        let (group, metrics) = (kind.key, kind.metrics.as_slice());
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
    /// the readings with it added or taken away, as saved, in the order
    /// `order` arranges.
    fn toggled(
        &self,
        chosen: &[Reading],
        order: &[Metric],
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
        let next = arrange(&self.snapshot, order, next)
            .iter()
            .map(Reading::text)
            .collect::<Vec<_>>();
        (reading, on, next)
    }

    /// The readings the taskbar monitor's panel offers, as the components
    /// page offers them and in the order arranged there: per kind, one set
    /// of options, or with several devices of a kind a tab per device (the
    /// GPUs, the disks); network adapters, with one reading each, are
    /// options of their own.
    pub(in crate::frontend) fn taskbar_panel_kinds(&self) -> Vec<PanelKind> {
        let monitor: TaskbarMonitor =
            serde_json::from_value(self.data["settings"]["taskbarMonitor"].clone())
                .unwrap_or_default();
        let (chosen, order) = (&monitor.metrics, &monitor.order);
        let option = |row: &OptionRow, metric: Metric, text: String, tip: String| {
            let (reading, on, readings) = self.toggled(chosen, order, row, metric);
            PanelOption {
                key: reading.text(),
                metric: metric_key(metric),
                text,
                tip,
                on,
                readings,
            }
        };
        let metric_name =
            |metric: Metric| label(&format!("components.metric.{}.option", metric_key(metric)));
        let mut kinds = Vec::new();
        for kind in arranged(order) {
            let group = kind.key;
            let rows = self.option_rows(&kind, chosen);
            let sortable = !(rows.len() > 1 && rows.iter().all(|row| row.metrics.len() == 1));
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
                tone: metric_color(kind.metrics[0], self.palette),
                devices,
                sortable,
            });
        }
        kinds
    }

    /// What the control the tests name by `key` does: "style:curves",
    /// "reading:<reading>" (as saved), "kinds:<kind>,<kind>…" or
    /// "metrics:<metric>,<metric>…" (as dragged into that order),
    /// "display:<display>" (empty for the primary one), "settings" or
    /// "hide".
    pub(in crate::frontend) fn taskbar_panel_choice(&self, key: &str) -> Option<TaskbarChoice> {
        let list = |keys: &str| keys.split(',').map(str::to_owned).collect::<Vec<_>>();
        if let Some(keys) = key.strip_prefix("kinds:") {
            return Some(TaskbarChoice::Kinds(list(keys)));
        }
        if let Some(keys) = key.strip_prefix("metrics:") {
            return Some(TaskbarChoice::Metrics(list(keys)));
        }
        if let Some(display) = key.strip_prefix("display:") {
            return Some(TaskbarChoice::Display(display.to_owned()));
        }
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
    /// description: a block per kind of reading (the CPU, the memory, the
    /// GPUs …) with a row per device in it, its name and what tells it apart
    /// on the left, its options in fixed columns on the right. A click adds
    /// or removes one; checking changes only its box. A block dragged by its
    /// grip or its names, or an option dragged, takes another place, and the
    /// monitor shows its readings in that order (`order`, as saved).
    #[allow(clippy::too_many_arguments)]
    fn metric_card(
        &self,
        id: &'static str,
        settings_key: &'static str,
        command: &'static str,
        chosen: &[Reading],
        order: &[Metric],
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let reorder = &self.ui.readings_order;
        let now = Instant::now();
        let lift = reorder.lifted(id);
        // Let go where nothing takes the drop: it ends all the same.
        if lift.is_some() && !cx.has_active_drag() {
            cx.defer_in(window, move |this, window, cx| {
                this.finish_readings_drag(id, settings_key, command, window, cx)
            });
        }
        if reorder.moving(now) {
            window.request_animation_frame();
        }
        // The kinds, and each one's options, as the drag has arranged them.
        let mut kinds = arranged(order);
        if let Some(lift) = &lift {
            if lift.list == id {
                kinds = lift
                    .order
                    .iter()
                    .filter_map(|key| kinds.iter().find(|kind| kind.key == *key).cloned())
                    .collect();
            } else if let Some(kind) = kinds
                .iter_mut()
                .find(|kind| lift.list == format!("{id}/{}", kind.key))
            {
                kind.metrics = metrics_named(&lift.order);
            }
        }
        let kind_keys = kinds
            .iter()
            .map(|kind| kind.key.to_owned())
            .collect::<Vec<_>>();
        let desktop = cx.entity().downgrade();
        let animate = !cx.reduce_motion();
        let test = self.options.test;
        // What tests take hold of.
        let anchor = |name: String| {
            let anchors = self.ui.anchors.clone();
            let name = SharedString::from(name);
            canvas(
                move |bounds, _, _| {
                    anchors.borrow_mut().insert(name.clone(), bounds);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        };
        let record = |list: &str, part: &str, key: &str| {
            canvas(reorder.recorder(list, part, key), |_, _, _, _| {})
                .absolute()
                .inset_0()
        };
        // An option as it looks: its box and its name.
        let face = |on: bool, name: String| {
            div()
                .h(px(OPTION))
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .rounded(px(4.))
                .text_size(theme::font(13.))
                .text_color(p.ink)
                .child(self.check_box(on, false))
                .child(div().min_w_0().truncate().child(name))
        };
        let name_cell = |row: &OptionRow| {
            div()
                .w(px(NAME_COLUMN))
                .flex_shrink_0()
                .min_h(px(OPTION))
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
        };
        let grid = || {
            div()
                .flex_1()
                .min_w_0()
                .grid()
                .grid_cols(3)
                .gap_x(px(OPTION_GAP))
                .gap_y(px(OPTION_LINE_GAP))
        };
        let option_name =
            |metric: Metric| label(&format!("components.metric.{}.option", metric_key(metric)));
        // An option to check and to drag along its row.
        let option = |kind: &Kind, row: &OptionRow, metric: Metric| {
            let (_, on, next) = self.toggled(chosen, order, row, metric);
            let key = metric_key(metric);
            let list = format!("{id}/{}", kind.key);
            let lifted = lift
                .as_ref()
                .is_some_and(|lift| lift.list == list && lift.key == key);
            let offset = reorder.offset(&list, &row.id, key, now);
            if lifted && lift.as_ref().is_some_and(|lift| lift.part == row.id) {
                // Its place in the row it was taken from, tinted, to drop it into.
                return div()
                    .relative()
                    .h(px(OPTION))
                    .rounded(px(4.))
                    .bg(p.accent.opacity(0.08))
                    .child(record(&list, &row.id, key))
                    .into_any_element();
            }
            let name = option_name(metric);
            let anchor_name = format!("{id}-{}-{key}", row.id);
            let keys = row
                .metrics
                .iter()
                .map(|metric| metric_key(*metric).to_owned())
                .collect::<Vec<_>>();
            let part = row.id.clone();
            let desktop = desktop.clone();
            let element = Button::new(SharedString::from(anchor_name.clone()))
                .relative()
                .h(px(OPTION))
                .min_w_0()
                .flex()
                .items_center()
                .justify_start()
                .gap(px(8.))
                .px(px(8.))
                .rounded(px(4.))
                .text_size(theme::font(13.))
                .line_height(theme::line_height())
                .text_color(p.ink)
                .role(Role::CheckBox)
                .aria_toggled(if on { Toggled::True } else { Toggled::False })
                .accessibility_label(format!("{} {name}", row.title))
                .when(!lifted, |option| {
                    option
                        .hover(move |style| style.bg(p.hover))
                        .active(move |style| style.bg(p.press))
                })
                .child(self.check_box(on, false))
                .child(div().min_w_0().truncate().child(name))
                .child(record(&list, &row.id, key))
                .when(test, |option| option.child(anchor(anchor_name)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        this.ui.readings_order.press(event.position);
                        cx.stop_propagation();
                    }),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.choose_metrics(settings_key, command, next.clone(), window, cx);
                }))
                .when(row.metrics.len() > 1, |option| {
                    option.on_drag(Lifted, move |_, _, window, cx| {
                        let pointer = window.mouse_position();
                        let flow = Flow::Lines {
                            gap: OPTION_GAP,
                            line_gap: OPTION_LINE_GAP,
                        };
                        let _ = desktop.update(cx, |this, cx| {
                            this.ui.readings_order.take(
                                &list,
                                &part,
                                key,
                                flow,
                                keys.clone(),
                                pointer,
                            );
                            cx.notify();
                        });
                        cx.new(|_| Unseen)
                    })
                });
            shift::shifted(element, offset, 1.).into_any_element()
        };
        let row_element = |kind: &Kind, row: &OptionRow| {
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
                    name_cell(row)
                        .id(SharedString::from(format!("{id}-{}-name", row.id)))
                        .cursor_grab()
                        .tooltip(move |window, cx| widgets::tooltip(full.clone(), window, cx)),
                )
                .child(
                    grid()
                        .relative()
                        .child(record(&format!("{id}/{}", kind.key), &row.id, ""))
                        .children(row.metrics.iter().map(|metric| option(kind, row, *metric))),
                )
        };
        // A block as it looks, taking no input: drawn under the pointer
        // while it is dragged.
        let block_face = |rows: &[OptionRow]| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .children(rows.iter().map(|row| {
                    div()
                        .flex()
                        .items_start()
                        .gap(px(12.))
                        .child(name_cell(row))
                        .child(grid().children(row.metrics.iter().map(|metric| {
                            let (_, on, _) = self.toggled(chosen, order, row, *metric);
                            face(on, option_name(*metric))
                        })))
                }))
        };
        let grip = |color: Hsla| {
            div()
                .absolute()
                .left_0()
                .top(px(BLOCK_PAD))
                .w(px(20.))
                .h(px(OPTION))
                .flex()
                .items_center()
                .justify_center()
                .child(icon("GripIcon", color).size(px(14.)))
        };
        let block_lifted = lift.as_ref().filter(|lift| lift.list == id);
        let mut raised = None;
        let blocks = kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                let rows = self.option_rows(kind, chosen);
                let lifted = block_lifted.is_some_and(|lift| lift.key == kind.key);
                let group = SharedString::from(format!("{id}-block-{}", kind.key));
                let block = div()
                    .id(group.clone())
                    .group(group.clone())
                    .relative()
                    .ml(px(-BLOCK_START))
                    .mr(px(-BLOCK_END))
                    .pl(px(BLOCK_START))
                    .pr(px(BLOCK_END))
                    .py(px(BLOCK_PAD))
                    .rounded(px(6.))
                    .child(record(id, "", kind.key))
                    .when(test, |block| block.child(anchor(group.to_string())));
                if lifted {
                    // Its place, tinted, to drop it into; the block itself
                    // rises under the pointer.
                    if let Some((lift, body)) = block_lifted.zip(reorder.bounds(id, "", "")) {
                        let at = lift.origin() - body.origin;
                        raised = Some(
                            div()
                                .absolute()
                                .left(at.x)
                                .top(at.y)
                                .w(lift.start.size.width)
                                // Its content where the slot's is, inside its border.
                                .pl(px(BLOCK_START - 1.))
                                .pr(px(BLOCK_END - 1.))
                                .py(px(BLOCK_PAD - 1.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(p.accent.opacity(0.6))
                                .bg(widgets::solid(p.flyout))
                                .cursor_grabbing()
                                .child(grip(p.muted).left(px(-1.)).top(px(BLOCK_PAD - 1.)))
                                .child(block_face(&rows)),
                        );
                    }
                    return block
                        .bg(p.accent.opacity(0.08))
                        .child(block_face(&rows).invisible())
                        .into_any_element();
                }
                let keys = kind_keys.clone();
                let key = kind.key;
                let desktop = desktop.clone();
                let block = block
                    // A line between two blocks.
                    .when(index > 0, |block| {
                        block.child(
                            div()
                                .absolute()
                                .top_0()
                                .left(px(BLOCK_START))
                                .right(px(BLOCK_END))
                                .h(px(1.))
                                .bg(p.stroke),
                        )
                    })
                    .child(grip(p.faint).cursor_grab().when(test, |grip| {
                        grip.child(anchor(format!("{id}-grip-{}", kind.key)))
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .children(rows.iter().map(|row| row_element(kind, row))),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, _| {
                            this.ui.readings_order.press(event.position);
                        }),
                    )
                    .on_drag(Lifted, move |_, _, window, cx| {
                        let pointer = window.mouse_position();
                        let _ = desktop.update(cx, |this, cx| {
                            this.ui.readings_order.take(
                                id,
                                "",
                                key,
                                Flow::Stack { gap: 0. },
                                keys.clone(),
                                pointer,
                            );
                            cx.notify();
                        });
                        cx.new(|_| Unseen)
                    });
                shift::shifted(block, reorder.offset(id, "", kind.key, now), 1.).into_any_element()
            })
            .collect::<Vec<_>>();
        // An option dragged: drawn under the pointer, over the others.
        let raised = raised.or_else(|| {
            let lift = lift.as_ref().filter(|lift| lift.list != id)?;
            let body = reorder.bounds(id, "", "")?;
            let metric = *metrics_named(std::slice::from_ref(&lift.key)).first()?;
            let kind = kinds.iter().find(|kind| kind.metrics.contains(&metric))?;
            let row = self
                .option_rows(kind, chosen)
                .into_iter()
                .find(|row| row.id == lift.part)?;
            let (_, on, _) = self.toggled(chosen, order, &row, metric);
            let at = lift.origin() - body.origin;
            Some(
                face(on, option_name(metric))
                    .absolute()
                    .left(at.x)
                    .top(at.y)
                    .w(lift.start.size.width)
                    .px(px(7.))
                    .border_1()
                    .border_color(p.accent.opacity(0.6))
                    .bg(widgets::solid(p.flyout))
                    .cursor_grabbing(),
            )
        });
        let body = div()
            .relative()
            .flex()
            .flex_col()
            .child(record(id, "", ""))
            .children(blocks)
            .children(raised)
            // A drag of this card's blocks or options follows the pointer.
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<Lifted>, _, cx| {
                    let reorder = &this.ui.readings_order;
                    if reorder.lifted(id).is_some() {
                        reorder.follow(event.event.position, animate, Instant::now());
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(move |this, _: &Lifted, window, cx| {
                this.finish_readings_drag(id, settings_key, command, window, cx);
            }))
            .into_any_element();
        self.setting_with_body(
            id,
            "ListIcon",
            "components.metrics.title",
            "components.metrics.desc",
            body,
        )
    }

    /// Lets go of a block or an option dragged on `card`: it glides into
    /// its place, and the monitor shows its readings in the order arranged.
    fn finish_readings_drag(
        &mut self,
        card: &str,
        settings_key: &'static str,
        command: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reorder = &self.ui.readings_order;
        if reorder.lifted(card).is_none() {
            return;
        }
        let released = reorder.release(!cx.reduce_motion(), Instant::now());
        if let Some(lift) = released
            && let Some(order) = lift.moved()
        {
            let saved = saved_order(&self.data["settings"][settings_key]["order"]);
            let arranged = arranged_order(&lift, card, &saved, order);
            self.arrange_readings(settings_key, command, arranged, window, cx);
        }
        cx.notify();
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
        let mut toggle = |id: &str, title: &str, value: bool, field: &str| {
            self.setting_toggle(id, title, value, command, field, window, cx)
        };
        let enabled = toggle(
            "desktopMonitor",
            "components.desktop.title",
            monitor.enabled,
            "enabled",
        );
        let topmost = toggle(
            "desktopTopmost",
            "components.desktop.topmost.title",
            monitor.topmost,
            "topmost",
        );
        let locked = toggle(
            "desktopLocked",
            "components.desktop.locked.title",
            monitor.locked,
            "locked",
        );
        let through = toggle(
            "desktopClickThrough",
            "components.desktop.clickThrough.title",
            monitor.click_through,
            "clickThrough",
        );
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
                &monitor.order,
                window,
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
            self.setting(
                "desktopTopmost",
                self.glyph("PinIcon"),
                label("components.desktop.topmost.title"),
                None,
                None,
                vec![topmost],
            ),
            self.setting(
                "desktopLocked",
                self.glyph("LockIcon"),
                label("components.desktop.locked.title"),
                None,
                None,
                vec![locked],
            ),
            self.setting_text(
                "desktopClickThrough",
                "PointerIcon",
                "components.desktop.clickThrough.title",
                "components.desktop.clickThrough.desc",
                vec![through],
            ),
            self.setting(
                "desktopOpacity",
                self.glyph("EyeIcon"),
                label("components.desktop.opacity.title"),
                None,
                None,
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
                    .child(style_caption(name, on, p))
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
        let body = div()
            .flex()
            .flex_wrap()
            .gap(px(STYLE_GAP))
            .children(cards)
            .into_any_element();
        self.setting_card(
            "desktopStyle",
            self.glyph("ThemeIcon"),
            label("components.desktop.style.title"),
            None,
            None,
            Vec::new(),
            Some(body),
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
        self.change_monitor("desktopMonitor", "set_desktop_monitor", change, window, cx);
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
            "components.taskbar.title",
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
        let choices = display_choices(&monitor.display, true);
        let chosen = choices.iter().position(|(key, _)| *key == monitor.display);
        let keys = choices
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let display = self.select(
            "components-taskbar-display",
            label("components.taskbar.display.title"),
            choices.into_iter().map(|(_, name)| name).collect(),
            chosen,
            false,
            move |this, index, window, cx| {
                if let Some(key) = keys.get(index) {
                    this.taskbar_choice(TaskbarChoice::Display(key.clone()), window, cx);
                }
            },
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
                &monitor.order,
                window,
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
            self.setting_text(
                "taskbarDisplay",
                "DisplayIcon",
                "components.taskbar.display.title",
                "components.taskbar.display.desc",
                vec![display],
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
                    .child(style_caption(name, on, p))
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
        let body = div()
            .flex()
            .flex_wrap()
            .gap(px(STYLE_GAP))
            .child(pair(cards))
            .child(pair(second))
            .into_any_element();
        self.setting_with_body(
            "taskbarStyle",
            "PulseIcon",
            "components.taskbar.style.title",
            "components.taskbar.style.desc",
            body,
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

fn style_caption(name: String, on: bool, p: Palette) -> Div {
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
    div()
        .min_h(px(20.))
        .px(px(2.))
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(theme::font(13.))
        .line_height(theme::line_height())
        .text_color(p.ink)
        .child(mark)
        .child(div().min_w_0().truncate().child(name))
}

/// The displays the taskbar monitor can sit on, as offered: the primary
/// display (an empty key), whichever that is, then each display by its
/// monitor's name, and its size when `sized`; one chosen and away now
/// follows, so that the choice shows. Alike names are numbered.
pub(in crate::frontend) fn display_choices(chosen: &str, sized: bool) -> Vec<(String, String)> {
    let displays = crate::shell::displays::displays();
    let names = displays
        .iter()
        .enumerate()
        .map(|(index, display)| {
            if display.name.is_empty() {
                tr!("components.taskbar.display.unnamed", index = index + 1)
            } else {
                display.name.clone()
            }
        })
        .collect::<Vec<_>>();
    let mut choices = vec![(String::new(), label("components.taskbar.display.primary"))];
    for (index, display) in displays.iter().enumerate() {
        let alike = names.iter().filter(|name| **name == names[index]).count();
        let name = if alike > 1 {
            let nth = names[..index]
                .iter()
                .filter(|name| **name == names[index])
                .count();
            format!("{} {}", names[index], nth + 1)
        } else {
            names[index].clone()
        };
        let text = if sized {
            tr!(
                "components.taskbar.display.item",
                name = name,
                width = display.width(),
                height = display.height()
            )
        } else {
            name
        };
        choices.push((display.key.clone(), text));
    }
    if !chosen.is_empty() && !displays.iter().any(|display| display.key == chosen) {
        choices.push((
            chosen.to_owned(),
            label("components.taskbar.display.absent"),
        ));
    }
    choices
}

/// The metrics `keys` name (`metric_key`), in turn; names this version
/// does not know are left out.
pub(in crate::frontend) fn metrics_named(keys: &[String]) -> Vec<Metric> {
    keys.iter()
        .filter_map(|key| {
            Metric::ALL
                .into_iter()
                .find(|metric| metric_key(*metric) == key.as_str())
        })
        .collect()
}

/// A monitor's order as saved; metrics this version does not know are
/// left out.
pub(in crate::frontend) fn saved_order(value: &Value) -> Vec<Metric> {
    values(value)
        .iter()
        .filter_map(|name| serde_json::from_value(name.clone()).ok())
        .collect()
}

/// `saved` (a monitor's order) as a drag on `card` arranged it: its kinds
/// (the drag's list is the card) or one kind's metrics, in `keys`' order.
fn arranged_order(lift: &Lift, card: &str, saved: &[Metric], keys: &[String]) -> Vec<Metric> {
    if lift.list == card {
        let keys = keys.iter().map(String::as_str).collect::<Vec<_>>();
        with_kinds(saved, &keys)
    } else {
        with_metrics(saved, &metrics_named(keys))
    }
}

#[cfg(test)]
mod tests {
    use super::{COMPONENTS, SECTIONS, metrics_named};
    use crate::frontend::monitors::{KINDS, metric_key};
    use crate::settings::Metric;

    #[test]
    fn the_kinds_offer_every_metric_once_in_order() {
        let offered = KINDS
            .iter()
            .flat_map(|(_, metrics)| metrics.iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(offered, Metric::ALL);
        let names = Metric::ALL
            .iter()
            .map(|metric| metric_key(*metric).to_owned())
            .chain(["fps".to_owned()])
            .collect::<Vec<_>>();
        assert_eq!(metrics_named(&names), Metric::ALL);
    }

    #[test]
    fn the_tray_and_the_monitors_are_available_and_the_lookout_is_planned() {
        let keys = COMPONENTS.map(|component| (component.key, component.available));
        assert_eq!(
            keys,
            [
                ("tray", true),
                ("desktop", true),
                ("taskbar", true),
                ("lookout", false)
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
