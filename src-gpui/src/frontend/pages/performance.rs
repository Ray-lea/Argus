use super::*;
use charts::{ChartHeight, GIB, KIB, Line, Look, MIB, MetricFormat, Scale};
use gpui_kit::component::scroll::{ScrollableElement, ScrollbarAxis};
mod engines;
mod telemetry;

/// Device list geometry (the original's `.dev` rows).
const ROW_HEIGHT: f32 = 58.;
const ROW_GAP: f32 = 4.;
const SIDE_DEFAULT: f32 = 248.;
const SIDE_MIN: f32 = 200.;
const SIDE_MAX: f32 = 440.;
/// Space between the device list and the detail card.
const PAGE_GAP: f32 = 12.;
/// Live figures: `repeat(auto-fill, minmax(120px, 1fr))` with 14 × 20 gaps.
const STAT_MIN: f32 = 120.;
const STAT_GAP_X: f32 = 20.;
const STAT_GAP_Y: f32 = 14.;
/// Gap between the figures and the specs beside them.
const STATS_GAP: f32 = 28.;
/// The CPU's figures and specs go side by side from this width each.
const CPU_STATS_MIN: f32 = 320.;
/// How long the CPU charts take to settle in after another graph or core type.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(200);
/// Smallest chart heights before the page scrolls (main, half, small).
const MAIN_MIN: f32 = 120.;
const HALF_MIN: f32 = 80.;
const SMALL_MIN: f32 = 64.;

#[derive(Clone)]
pub(super) struct Device {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) model: String,
    pub(super) color: Hsla,
    pub(super) kind: &'static str,
    pub(super) index: usize,
}
pub(super) fn available_devices(snapshot: &Value, p: Palette) -> Vec<Device> {
    let hw = &snapshot["hardware"];
    let mut out = vec![
        Device {
            id: "cpu".into(),
            title: label("common.resource.cpu"),
            model: text(&hw["cpu"], "name").into(),
            color: p.cpu,
            kind: "cpu",
            index: 0,
        },
        Device {
            id: "memory".into(),
            title: label("common.resource.memory"),
            model: theme::memory(number(&snapshot["system"], "memTotal")),
            color: p.memory,
            kind: "memory",
            index: 0,
        },
    ];
    let batteries = values(&hw["batteries"]).len();
    for (kind, collection, prefix, color) in [
        ("disk", "disks", "disk", p.io),
        ("network", "nics", "nic", p.network),
        ("gpu", "gpus", "gpu", p.gpu),
        ("battery", "batteries", "bat", p.battery),
    ] {
        for (index, item) in values(&hw[collection]).iter().enumerate() {
            let info = &item["info"];
            let identity = if kind == "disk" {
                info["index"].to_string()
            } else {
                text(info, "id").into()
            };
            let title = match kind {
                "disk" => disk_title(info),
                "gpu" => format!("{} {index}", label("common.resource.gpu")),
                "battery" if batteries > 1 => {
                    tr!("performance.battery.titleNumbered", id = text(info, "id"))
                }
                "battery" => label("performance.battery.title"),
                _ => text(info, "name").into(),
            };
            let model = if kind == "disk" {
                text(info, "model")
            } else if kind == "network" {
                text(info, "description")
            } else {
                text(info, "name")
            };
            out.push(Device {
                id: format!("{prefix}:{identity}"),
                title,
                model: model.into(),
                color,
                kind,
                index,
            });
        }
    }
    out
}

// ---- Formatting (lib/format.ts) ------------------------------------------------

fn missing() -> String {
    label("format.missing")
}
fn ghz(mhz: f64) -> String {
    format!("{:.2} GHz", mhz / 1000.)
}
fn gb(bytes: f64) -> String {
    format!("{:.1}", bytes / GIB)
}
fn fmt_cache(bytes: f64) -> String {
    if bytes >= MIB {
        format!("{:.1} MB", bytes / MIB)
    } else {
        format!("{} KB", (bytes / KIB).round())
    }
}
/// Capacity in binary units, the way Windows reports drive sizes.
fn fmt_capacity(bytes: f64) -> String {
    if bytes >= 1024. * GIB {
        format!("{:.1} TB", bytes / (1024. * GIB))
    } else {
        format!("{} GB", (bytes / GIB).round())
    }
}
fn link_speed(bits: f64) -> String {
    if bits >= 1e9 {
        let value = format!("{:.1}", bits / 1e9);
        format!("{} Gbps", value.trim_end_matches(".0"))
    } else {
        format!("{} Mbps", (bits / 1e6).round())
    }
}
/// A figure split into number and unit: "912.0" + "MB".
fn split(value: String) -> (String, Option<String>) {
    match value.split_once(' ') {
        Some((number, unit)) => (number.into(), Some(unit.into())),
        None => (value, None),
    }
}
fn duration(seconds: f64) -> String {
    let hours = (seconds / 3600.).floor();
    let minutes = ((seconds % 3600.) / 60.).round();
    if hours > 0. {
        tr!(
            "performance.battery.hoursMinutes",
            hours = hours,
            minutes = minutes
        )
    } else {
        tr!("performance.battery.minutes", minutes = minutes)
    }
}
fn disk_title(info: &Value) -> String {
    let volumes = values(&info["volumes"])
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if volumes.is_empty() {
        tr!("performance.disk.title", index = info["index"])
    } else {
        tr!(
            "performance.disk.titleVolumes",
            index = info["index"],
            volumes = volumes.join(" ")
        )
    }
}
/// A disk's medium and bus, e.g. "SSD (NVMe)".
fn disk_kind(info: &Value) -> String {
    let bus = match text(info, "bus") {
        "nvme" => "NVMe",
        "usb" => "USB",
        "sata" => "SATA",
        _ => "",
    };
    let media = match info["ssd"].as_bool() {
        Some(true) => "SSD".into(),
        Some(false) => "HDD".into(),
        None if boolean(info, "removable") => label("performance.disk.removableMedia"),
        None => String::new(),
    };
    [
        media,
        if bus.is_empty() {
            String::new()
        } else {
            format!("({bus})")
        },
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}
/// A die reading takes priority; ACPI remains an explicitly labelled fallback.
fn cpu_temperature(hw: &Value) -> Option<f64> {
    let sensors = &hw["cpuSensors"];
    sensors["packageCelsius"]
        .as_f64()
        .or_else(|| {
            values(&sensors["coreTemperatures"])
                .iter()
                .filter_map(|core| core["celsius"].as_f64())
                .reduce(f64::max)
        })
        .or_else(|| {
            values(&hw["thermal"])
                .iter()
                .filter_map(|zone| zone["celsius"].as_f64())
                .reduce(f64::max)
        })
}
fn has_die_temperature(hw: &Value) -> bool {
    let sensors = &hw["cpuSensors"];
    sensors["packageCelsius"].is_number() || !values(&sensors["coreTemperatures"]).is_empty()
}
/// The timeline series behind the CPU temperature figure.
fn temperature_key(hw: &Value) -> Option<String> {
    let sensors = &hw["cpuSensors"];
    if sensors["packageCelsius"].is_number() {
        return Some("hardware.cpuSensors.packageCelsius".into());
    }
    let hottest = |list: &[Value]| {
        list.iter()
            .enumerate()
            .filter_map(|(i, item)| item["celsius"].as_f64().map(|c| (i, c)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    if let Some(index) = hottest(values(&sensors["coreTemperatures"])) {
        return Some(format!(
            "hardware.cpuSensors.coreTemperatures.{index}.celsius"
        ));
    }
    hottest(values(&hw["thermal"])).map(|index| format!("hardware.thermal.{index}.celsius"))
}
/// Memory parts that do not overlap; standby and the rest are unknown without the page lists.
fn memory_composition(sys: &Value) -> (f64, Option<f64>, Option<f64>, Option<f64>) {
    let total = number(sys, "memTotal").max(0.);
    let lists = &sys["memoryLists"];
    if !lists.is_object() {
        return (number(sys, "memUsed").clamp(0., total), None, None, None);
    }
    let free = number(lists, "free").clamp(0., total);
    let standby = number(lists, "standby").clamp(0., total - free);
    let modified = number(lists, "modified").clamp(0., total - free - standby);
    (
        total - free - standby - modified,
        Some(modified),
        Some(standby),
        Some(free),
    )
}
fn battery_state(state: &str) -> String {
    label(&format!("performance.battery.state.{state}"))
}
fn chemistry(code: &str) -> String {
    let id = match code.to_uppercase().as_str() {
        "LION" | "LI-I" => "liIon",
        "LIP" | "LIPO" => "liPoly",
        "PBAC" => "leadAcid",
        "NICD" => "niCd",
        "NIMH" => "niMh",
        "RAM" => "alkaline",
        _ if code.is_empty() => return missing(),
        _ => return code.into(),
    };
    label(&format!("performance.battery.chemistries.{id}"))
}
fn watt_hours(milliwatt_hours: &Value) -> String {
    milliwatt_hours
        .as_f64()
        .filter(|v| *v > 0.)
        .map(|v| format!("{:.1} Wh", v / 1000.))
        .unwrap_or_else(missing)
}
fn or_missing(value: &str) -> String {
    if value.is_empty() {
        missing()
    } else {
        value.into()
    }
}
/// How a grid's cards move. They glide only once this display has been laid
/// out at its real size (`measured`): entering the page or switching device or
/// view places them at once, while later resizes and filters animate.
fn settle_policy(
    id: &'static str,
    measured: bool,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::base::motion::Transition {
    let settled = window.use_keyed_state(id, cx, |_, _| false);
    if *settled.read(cx) {
        return theme::transition();
    }
    if measured {
        settled.update(cx, |settled, _| *settled = true);
    }
    gpui_kit::base::motion::Transition::new(std::time::Duration::ZERO)
}
/// The opacity of a view that settles in once `key` changes, as the original's
/// CPU charts do: from nothing to full over [`SETTLE`], decelerating. A first
/// display (entering the page, another device) shows at once.
fn settle_fade(id: &'static str, key: String, window: &mut Window, cx: &mut App) -> f32 {
    let state = window.use_keyed_state(id, cx, |_, _| (key.clone(), None::<Instant>));
    let (shown, mut started) = state.read(cx).clone();
    if shown != key {
        started = Some(Instant::now());
        state.update(cx, |state, _| *state = (key, started));
    }
    let Some(started) = started.filter(|_| !cx.reduce_motion()) else {
        return 1.;
    };
    let progress = started.elapsed().as_secs_f32() / SETTLE.as_secs_f32();
    if progress >= 1. {
        return 1.;
    }
    window.request_animation_frame();
    theme::ease_out(progress)
}
/// Splits text such as "5<u>d</u> 14:00:00" into plain and small runs.
fn rich_runs(template: &str) -> Vec<(String, bool)> {
    let mut runs = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("<u>") {
        if start > 0 {
            runs.push((rest[..start].to_owned(), false));
        }
        let tail = &rest[start + 3..];
        let end = tail.find("</u>").unwrap_or(tail.len());
        runs.push((tail[..end].to_owned(), true));
        rest = tail.get(end + 4..).unwrap_or("");
    }
    if !rest.is_empty() {
        runs.push((rest.to_owned(), false));
    }
    runs
}

impl Desktop {
    fn net_bits(&self) -> bool {
        self.prefs.get::<String>("netUnit", "bytes".into()) == "bits"
    }
    /// The device list's width: at most half the page, so the detail never gets squeezed.
    fn side_width(&self, page: f32) -> f32 {
        self.perf_sidebar_width
            .min(SIDE_MAX)
            .min(page / 2.)
            .max(SIDE_MIN)
            .round()
    }
    fn page_width(&self, window: &Window) -> f32 {
        let navigation = if self.nav_top {
            0.
        } else if self.nav_expanded {
            192.
        } else {
            48.
        };
        f32::from(window.viewport_size().width) - navigation - 2. * PAGE_GAP
    }
    /// The detail card's content width: the page less the list, gap, border, padding and scrollbar.
    fn detail_width(&self, window: &Window) -> f32 {
        let page = self.page_width(window);
        (page - self.side_width(page) - PAGE_GAP - 2. - 40. - 10.).max(160.)
    }
    fn device_sub(&self, device: &Device) -> String {
        let hw = &self.snapshot["hardware"];
        let sys = &self.snapshot["system"];
        match device.kind {
            "cpu" => {
                let load = format!("{}%", number(sys, "cpu").round());
                match hw["cpuMhz"].as_f64().filter(|v| *v > 0.) {
                    Some(mhz) => format!("{load}  {}", ghz(mhz)),
                    None => load,
                }
            }
            "memory" => tr!(
                "performance.memory.sub",
                used = gb(number(sys, "memUsed")),
                total = gb(number(sys, "memTotal")),
                percent = (number(sys, "memUsed") / number(sys, "memTotal").max(1.) * 100.).round()
            ),
            "disk" => {
                let disk = &hw["disks"][device.index];
                let mut parts = vec![format!("{}%", number(disk, "active").round())];
                let kind = disk_kind(&disk["info"]);
                if !kind.is_empty() {
                    parts.insert(0, kind);
                }
                if let Some(celsius) = disk["temperature"].as_f64() {
                    parts.push(format!("{} °C", celsius.round()));
                }
                parts.join("  ")
            }
            "network" => {
                let nic = &hw["nics"][device.index];
                let bits = self.net_bits();
                tr!(
                    "performance.network.sub",
                    rx = charts::net_rate(number(nic, "rx"), bits),
                    tx = charts::net_rate(number(nic, "tx"), bits)
                )
            }
            "gpu" => {
                let gpu = &hw["gpus"][device.index];
                let mut parts = vec![format!("{}%", number(gpu, "util").round())];
                if let Some(celsius) = gpu["temperature"].as_f64() {
                    parts.push(format!("{} °C", celsius.round()));
                }
                if let Some(power) = gpu["boardPower"].as_f64() {
                    parts.push(format!("{} W", charts::watts(power)));
                }
                parts.join("  ")
            }
            "battery" => {
                let battery = &hw["batteries"][device.index];
                let percent = format!("{}%", number(battery, "percent").round());
                let state = text(battery, "state");
                if state == "discharging" {
                    match battery["remaining"].as_f64().filter(|v| *v > 0.) {
                        Some(seconds) => format!(
                            "{percent}  {}",
                            tr!("performance.battery.timeLeft", time = duration(seconds))
                        ),
                        None => format!("{percent}  {}", label("performance.battery.onBattery")),
                    }
                } else {
                    format!("{percent}  {}", battery_state(state))
                }
            }
            _ => device.model.clone(),
        }
    }
    /// A device's preview chart and its scale.
    fn device_lines(&self, device: &Device) -> (Vec<Line>, Scale) {
        let c = device.color;
        match device.kind {
            "cpu" => (vec![Line::new("system.cpu", c)], Scale::Fixed(100.)),
            "memory" => (
                vec![Line::new("system.memUsed", c)],
                Scale::Fixed(number(&self.snapshot["system"], "memTotal").max(1.)),
            ),
            "disk" => (
                vec![Line::new(
                    format!("hardware.disks.{}.active", device.index),
                    c,
                )],
                Scale::Fixed(100.),
            ),
            "network" => (
                vec![
                    Line::new(format!("hardware.nics.{}.rx", device.index), c),
                    Line::new(format!("hardware.nics.{}.tx", device.index), c).dashed(),
                ],
                Scale::Rate {
                    floor: 16. * KIB,
                    bits: self.net_bits(),
                },
            ),
            "gpu" => (
                vec![Line::new(format!("hardware.gpus.{}.util", device.index), c)],
                Scale::Fixed(100.),
            ),
            _ => (
                vec![Line::new(
                    format!("hardware.batteries.{}.percent", device.index),
                    c,
                )],
                Scale::Fixed(100.),
            ),
        }
    }
    pub(in crate::frontend) fn performance_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let devices = available_devices(&self.snapshot, self.palette);
        let selected_index = devices
            .iter()
            .position(|d| d.id == self.device)
            .unwrap_or(0);
        let selected = devices.get(selected_index).cloned();
        let p = self.palette;
        let page = self.page_width(window);
        let side_width = self.side_width(page);
        // The selected device: a raised surface with an accent pill, sliding between rows.
        let mark_top = gpui_kit::base::motion::transition(
            "perf-device-mark",
            px(selected_index as f32 * (ROW_HEIGHT + ROW_GAP)),
            gpui_kit::base::motion::Transition::new(std::time::Duration::from_millis(250)).easing(
                gpui_kit::base::motion::Easing::cubic_bezier(0.33, 0., 0.1, 1.).unwrap_or_default(),
            ),
            window,
            cx,
        );
        let mut list = div()
            .id("devices")
            .relative()
            .w(px(side_width))
            .flex_shrink_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .pr_1()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_1()
                    .top(mark_top)
                    .h(px(ROW_HEIGHT))
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(6.))
                    .bg(p.layer_strong)
                    .child(
                        div()
                            .absolute()
                            .left(px(-1.))
                            .top(px((ROW_HEIGHT - 2. - 18.) / 2.))
                            .w(px(3.))
                            .h(px(18.))
                            .rounded(px(2.))
                            .bg(p.accent),
                    ),
            );
        for device in &devices {
            let (lines, scale) = self.device_lines(device);
            let graph = self.chart_view(
                &format!("device-preview-{}", device.id),
                &lines,
                Some(scale),
                px(38.),
                Look::default(),
                cx,
            );
            let current = selected.as_ref().is_some_and(|s| s.id == device.id);
            list = list.child(
                Button::new(SharedString::from(format!("device-{}", device.id)))
                    .accessibility_label(device.title.clone())
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .text_color(p.ink)
                    .when(!current, |row| row.hover(move |style| style.bg(p.hover)))
                    .on_click(cx.listener({
                        let id = device.id.clone();
                        move |this, _, window, cx| this.act(Action::Device(id.clone()), window, cx)
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .relative()
                    .h(px(ROW_HEIGHT))
                    .min_h(px(ROW_HEIGHT))
                    .w_full()
                    .justify_start()
                    .pl(px(12.))
                    .pr(px(8.))
                    .py(px(8.))
                    .gap(px(10.))
                    .border_1()
                    .border_color(gpui_kit::gpui::transparent_black())
                    .rounded(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(device.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(p.muted)
                                    .truncate()
                                    .child(self.device_sub(device)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(76.))
                            .h(px(40.))
                            .flex_shrink_0()
                            .border_1()
                            .border_color(device.color.opacity(0.45))
                            .rounded(px(3.))
                            .overflow_hidden()
                            .child(graph),
                    ),
            );
        }
        // A newly chosen device starts at the top and arrives from its direction in the list.
        let arrival = window.use_keyed_state("perf-device-arrival", cx, |_, _| {
            (String::new(), 0usize, 0f32, 0u64)
        });
        let (previous_id, previous_index, mut direction, mut epoch) = arrival.read(cx).clone();
        let current_id = selected.as_ref().map(|d| d.id.clone()).unwrap_or_default();
        if previous_id != current_id {
            if !previous_id.is_empty() {
                direction = (selected_index as f32 - previous_index as f32).signum();
                epoch += 1;
                self.device_scroll.set_offset(point(px(0.), px(0.)));
            }
            arrival.update(cx, |state, _| {
                *state = (current_id, selected_index, direction, epoch)
            });
        }
        let content = selected
            .map(|device| self.device_view(&device, window, cx))
            .unwrap_or_else(|| div().into_any_element());
        let content = if epoch > 0 && !cx.reduce_motion() {
            div()
                .relative()
                .flex_1()
                .flex()
                .flex_col()
                .child(content)
                .with_animation(
                    ("perf-device-enter", epoch),
                    Animation::new(std::time::Duration::from_millis(250))
                        .with_easing(gpui_kit::base::animation::ease_out_cubic),
                    move |element, progress| element.top(px(direction * 16. * (1. - progress))),
                )
                .into_any_element()
        } else {
            content
        };
        let splitter = div()
            .id("perf-sidebar-splitter")
            .group("perf-splitter")
            .w(px(PAGE_GAP))
            .h_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_col_resize()
            .child(
                div()
                    .w(px(2.))
                    .h(px(24.))
                    .rounded(px(1.))
                    .invisible()
                    .group_hover("perf-splitter", |s| s.visible())
                    .bg(p.stroke_strong),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if event.click_count >= 2 {
                        this.perf_sidebar_width = SIDE_DEFAULT;
                        this.prefs.set("perfSideWidth", SIDE_DEFAULT);
                        this.drag = None;
                    } else {
                        let page = this.page_width(window);
                        this.drag =
                            Some(("perf-sidebar".into(), event.position, this.side_width(page)));
                    }
                    cx.notify();
                }),
            );
        div()
            .size_full()
            .min_h_0()
            .px(px(PAGE_GAP))
            .flex()
            // The original page's body line height; GPUI's default is taller.
            .line_height(relative(1.4))
            .child(list)
            .child(splitter)
            .child(
                div()
                    .id("device-content")
                    .track_scroll(&self.device_scroll)
                    .flex_1()
                    .min_w_0()
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(8.))
                    .bg(p.layer)
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(content)
                    .scrollbar(&self.device_scroll, ScrollbarAxis::Vertical),
            )
            .into_any_element()
    }

    // ---- Shared building blocks ------------------------------------------------

    /// A view's title, the controls that follow it, and the model on the right.
    fn perf_head(
        &self,
        title: String,
        model: Option<String>,
        controls: Option<AnyElement>,
        cx: &App,
    ) -> AnyElement {
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap_4()
            .mb(px(14.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .font_family(theme::display_font(cx))
                            .text_size(theme::dimension("pageTitleSize"))
                            .font_weight(FontWeight::SEMIBOLD)
                            .whitespace_nowrap()
                            .child(title),
                    )
                    .children(controls),
            )
            .when_some(model.filter(|m| !m.is_empty()), |d, model| {
                d.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(15.))
                        .text_color(self.palette.muted)
                        .text_right()
                        .child(model),
                )
            })
            .into_any_element()
    }
    /// A live figure: label, large value with a small unit, and an optional note in another unit.
    fn stat(
        &self,
        title: impl Into<SharedString>,
        value: impl Into<SharedString>,
        unit: Option<String>,
        note: Option<String>,
        cx: &App,
    ) -> AnyElement {
        let value = div()
            .flex()
            .items_baseline()
            .child(value.into())
            .when_some(unit, |d, unit| {
                d.child(
                    div()
                        .ml(px(2.))
                        .text_size(px(13.))
                        .text_color(self.palette.muted)
                        .child(unit),
                )
            })
            .into_any_element();
        self.stat_element(title, value, note, cx)
    }
    fn stat_element(
        &self,
        title: impl Into<SharedString>,
        value: AnyElement,
        note: Option<String>,
        cx: &App,
    ) -> AnyElement {
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(self.palette.muted)
                    .truncate()
                    .child(title.into()),
            )
            .child(
                div()
                    .font_family(theme::display_font(cx))
                    .text_size(theme::dimension("metricSize"))
                    .whitespace_nowrap()
                    .child(value),
            )
            .when_some(note, |d, note| {
                d.child(
                    div()
                        .mt(px(1.))
                        .text_size(px(12.))
                        .text_color(self.palette.faint)
                        .child(note),
                )
            })
            .into_any_element()
    }
    /// A figure split into number and unit.
    fn stat_parts(&self, title: String, value: String, cx: &App) -> AnyElement {
        let (number, unit) = split(value);
        self.stat(title, number, unit, None, cx)
    }
    /// Uptime to the second: days, then hh:mm:ss, which keeps its width as it ticks.
    fn uptime_value(&self, milliseconds: f64) -> AnyElement {
        let seconds = (milliseconds / 1000.).floor() as u64;
        let days = seconds / 86400;
        let clock = format!(
            "{:02}:{:02}:{:02}",
            seconds % 86400 / 3600,
            seconds % 3600 / 60,
            seconds % 60
        );
        if days == 0 {
            return div().child(clock).into_any_element();
        }
        let text = tr!("format.uptimeExact", days = days, clock = clock);
        div()
            .flex()
            .items_baseline()
            .children(rich_runs(&text).into_iter().map(|(run, small)| {
                if small {
                    div()
                        .ml(px(2.))
                        .text_size(px(13.))
                        .text_color(self.palette.muted)
                        .child(run)
                } else {
                    div().whitespace_nowrap().child(run.replace(' ', "\u{a0}"))
                }
            }))
            .into_any_element()
    }
    /// Labels and values in two columns: the labels as wide as the longest, the values the rest.
    fn info_list(&self, rows: Vec<(String, AnyElement)>) -> AnyElement {
        div()
            .grid()
            .grid_cols_max_content(2)
            .gap_x(px(20.))
            .gap_y(px(6.))
            .items_start()
            .children(rows.into_iter().flat_map(|(name, value)| {
                [
                    div()
                        .text_color(self.palette.muted)
                        .whitespace_nowrap()
                        .child(name)
                        .into_any_element(),
                    div().min_w_0().child(value).into_any_element(),
                ]
            }))
            .into_any_element()
    }
    /// Live figures beside the specs, the CPU's stacking when the card is narrow.
    fn perf_stats(
        &self,
        stats: Vec<AnyElement>,
        info: AnyElement,
        cpu: bool,
        window: &Window,
    ) -> AnyElement {
        let width = self.detail_width(window);
        let (stacked, stats_width) = if cpu {
            if width >= 2. * CPU_STATS_MIN + STATS_GAP {
                (false, (width - STATS_GAP) / 2.)
            } else {
                (true, width)
            }
        } else {
            (false, (width - STATS_GAP) * 1.1 / 2.1)
        };
        let columns = ((stats_width + STAT_GAP_X) / (STAT_MIN + STAT_GAP_X))
            .floor()
            .max(1.) as u16;
        let grid = div()
            .min_w_0()
            .grid()
            .grid_cols(columns)
            .gap_x(px(STAT_GAP_X))
            .gap_y(px(STAT_GAP_Y))
            .content_start()
            .children(stats);
        let info = div().min_w_0().child(info);
        div()
            .mt(px(20.))
            .flex_shrink_0()
            .flex()
            .gap(px(STATS_GAP))
            .map(|d| {
                if stacked {
                    d.flex_col().child(grid.w_full()).child(info.w_full())
                } else {
                    d.items_start()
                        .child(
                            grid.flex_grow(if cpu { 1. } else { 1.1 })
                                .flex_shrink(1.)
                                .flex_basis(px(0.)),
                        )
                        .child(info.flex_grow(1.).flex_shrink(1.).flex_basis(px(0.)))
                }
            })
            .into_any_element()
    }
    fn perf_note(&self, note: String) -> AnyElement {
        div()
            .mt(px(18.))
            .flex_shrink_0()
            .max_w(px(640.))
            .text_size(px(12.))
            .text_color(self.palette.faint)
            .child(note)
            .into_any_element()
    }
    /// A details card's title (with an optional note), a one-line summary on the right, and its body.
    fn card(
        &self,
        title: String,
        note: Option<String>,
        summary: Option<String>,
        alert: bool,
        body: impl IntoElement,
    ) -> AnyElement {
        let p = self.palette;
        div()
            .min_w_0()
            .pt(px(14.))
            .px(px(16.))
            .pb(px(16.))
            .border_1()
            .border_color(if alert {
                p.danger.opacity(0.45)
            } else {
                p.stroke
            })
            .rounded(px(8.))
            .bg(p.layer)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_4()
                    .mb(px(12.))
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_baseline()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title)
                            .when_some(note, |d, note| {
                                d.child(
                                    div()
                                        .ml(px(6.))
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::NORMAL)
                                        .text_color(p.muted)
                                        .child(note),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_right()
                            .text_size(px(12.))
                            .text_color(if alert { p.danger } else { p.muted })
                            .child(summary.unwrap_or_default()),
                    ),
            )
            .child(body)
            .into_any_element()
    }
    /// The toolbar above a view's charts: right-aligned controls.
    fn perf_toolbar(&self, controls: Vec<AnyElement>) -> AnyElement {
        div()
            .flex()
            .flex_shrink_0()
            .justify_end()
            .gap_2()
            .mt(px(-6.))
            .mb(px(8.))
            .children(controls)
            .into_any_element()
    }

    // ---- Detail views ------------------------------------------------------------

    fn device_view(
        &self,
        device: &Device,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let details = device.kind == "cpu" && self.cpu_page == "details";
        let body = div()
            .flex()
            .flex_col()
            .pt(px(16.))
            .px(px(20.))
            .pb(px(20.))
            .when(!details, |d| d.flex_1().flex_shrink_0());
        let content = match device.kind {
            "cpu" => self.cpu_view(window, cx),
            "memory" => self.memory_view(window, cx),
            "disk" => self.disk_view(device, window, cx),
            "network" => self.nic_view(device, window, cx),
            "gpu" => self.gpu_view(device, window, cx),
            "battery" => self.battery_view(device, window, cx),
            _ => Vec::new(),
        };
        body.children(content).into_any_element()
    }
    fn cpu_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.palette;
        let hw = &self.snapshot["hardware"];
        let sys = &self.snapshot["system"];
        let cpu = &hw["cpu"];
        let pages = self.segments(
            "cpu-page",
            ["overview", "details"]
                .into_iter()
                .map(|page| {
                    (
                        label(&format!("performance.cpu.page.{page}")),
                        None,
                        Action::CpuMode(if page == "details" {
                            "details".into()
                        } else {
                            self.cpu_mode.clone()
                        }),
                        self.cpu_page == page,
                    )
                })
                .collect(),
            cx,
        );
        let mut out = vec![self.perf_head(
            label("common.resource.cpu"),
            Some(text(cpu, "name").into()),
            Some(pages),
            cx,
        )];
        if self.cpu_page == "details" {
            out.push(self.cpu_details(hw, window, cx));
            return out;
        }
        let hybrid = number(cpu, "performanceCores") > 0. && number(cpu, "efficiencyCores") > 0.;
        let filter: String = if hybrid {
            self.prefs.get("coreFilter", "all".into())
        } else {
            "all".into()
        };
        let logical = self.cpu_mode == "logical";
        let mut controls = Vec::new();
        if hybrid && logical {
            controls.push(
                self.segments(
                    "core-filter",
                    ["all", "p", "e"]
                        .into_iter()
                        .map(|choice| {
                            (
                                label(&if choice == "all" {
                                    "performance.cpu.filterAll".into()
                                } else {
                                    format!("performance.cpu.filterShort.{choice}")
                                }),
                                None,
                                Action::Preference("coreFilter".into(), json!(choice)),
                                choice == filter,
                            )
                        })
                        .collect(),
                    cx,
                ),
            );
        }
        controls.push(
            self.segments(
                "cpu-mode",
                ["overall", "logical"]
                    .into_iter()
                    .map(|mode| {
                        (
                            label(&format!("performance.cpu.{mode}")),
                            None,
                            Action::CpuMode(mode.into()),
                            self.cpu_mode == mode,
                        )
                    })
                    .collect(),
                cx,
            ),
        );
        out.push(self.perf_toolbar(controls));
        // Another graph, or core type among the processors, swaps the charts at
        // once and fades them in, as the original's do. Its slight scale is left
        // out: every curve would be redrawn on each frame of it.
        let fade = settle_fade(
            "cpu-chart-settle",
            if logical {
                format!("logical/{filter}")
            } else {
                "overall".into()
            },
            window,
            cx,
        );
        self.chart_fade.set(fade);
        let graphs = if logical {
            self.core_grid(hybrid, &filter, window, cx)
        } else {
            self.chart_view(
                "cpu-overall-plot",
                &[
                    Line::new("system.cpu", p.cpu).label(label("performance.cpu.total")),
                    Line::new("system.cpuKernel", p.cpu)
                        .dashed()
                        .fill(0.18)
                        .label(label("performance.cpu.kernel")),
                ],
                Some(Scale::Fixed(100.)),
                ChartHeight::Fill,
                Look::boxed(label("performance.cpu.utilizationChart"))
                    .top("100%")
                    .format(MetricFormat::RoundPercent)
                    .min_height(px(MAIN_MIN))
                    .legend(vec![
                        (label("performance.cpu.total"), false),
                        (label("performance.cpu.kernelTime"), true),
                    ]),
                cx,
            )
        };
        self.chart_fade.set(1.);
        out.push(
            div()
                .w_full()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .opacity(fade)
                .child(graphs)
                .into_any_element(),
        );
        // Live figures.
        let mut stats = vec![
            self.stat(
                label("performance.cpu.utilization"),
                format!("{}", number(sys, "cpu").round()),
                Some("%".into()),
                None,
                cx,
            ),
            self.stat(
                label("performance.cpu.speed"),
                format!("{:.2}", number(hw, "cpuMhz") / 1000.),
                Some("GHz".into()),
                None,
                cx,
            ),
        ];
        if hybrid {
            for (efficient, title) in [
                (false, "performance.cpu.pCoreSpeed"),
                (true, "performance.cpu.eCoreSpeed"),
            ] {
                let clocks = values(&cpu["threads"])
                    .iter()
                    .enumerate()
                    .filter(|(_, thread)| boolean(thread, "efficient") == efficient)
                    .filter_map(|(i, _)| hw["coreMhz"][i].as_f64())
                    .filter(|mhz| *mhz > 0.)
                    .collect::<Vec<_>>();
                if !clocks.is_empty() {
                    stats.push(self.stat(
                        label(title),
                        format!(
                            "{:.2}",
                            clocks.iter().sum::<f64>() / clocks.len() as f64 / 1000.
                        ),
                        Some("GHz".into()),
                        None,
                        cx,
                    ));
                }
            }
        }
        if let Some(celsius) = cpu_temperature(hw) {
            stats.push(self.stat(
                label("common.resource.temperature"),
                format!("{}", celsius.round()),
                Some("°C".into()),
                None,
                cx,
            ));
        }
        for (field, title) in [
            ("package", "performance.cpu.packagePower"),
            ("cores", "performance.cpu.corePower"),
            ("graphics", "performance.cpu.graphicsPower"),
        ] {
            if let Some(watts) = hw["power"][field].as_f64() {
                stats.push(self.stat(
                    label(title),
                    charts::watts(watts),
                    Some("W".into()),
                    None,
                    cx,
                ));
            }
        }
        for (field, title) in [
            ("processCount", "performance.cpu.processes"),
            ("threadCount", "performance.cpu.threads"),
            ("handleCount", "performance.cpu.handles"),
        ] {
            stats.push(self.stat(
                label(title),
                theme::count(number(sys, field)),
                None,
                None,
                cx,
            ));
        }
        stats.push(self.stat_element(
            label("performance.cpu.uptime"),
            self.uptime_value(number(sys, "uptimeMs")),
            None,
            cx,
        ));
        // Specs.
        let base_source = match text(cpu, "baseSource") {
            "cpuid" => Some(label("performance.cpu.baseSourceCpuid")),
            "" => None,
            _ => Some(label("performance.cpu.baseSourceWindows")),
        };
        let base = number(cpu, "baseMhz");
        let base_value = div()
            .id("cpu-base-speed")
            .child(if base > 0. { ghz(base) } else { missing() })
            .when_some(base_source, |d, source| {
                d.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(source.clone()).build(window, cx)
                })
            })
            .into_any_element();
        let plain = |value: String| div().child(value).into_any_element();
        let cores = if hybrid {
            tr!(
                "performance.cpu.coresHybrid",
                cores = cpu["cores"],
                performance = cpu["performanceCores"],
                efficiency = cpu["efficiencyCores"]
            )
        } else {
            cpu["cores"].to_string()
        };
        let info = self.info_list(vec![
            (label("performance.cpu.baseSpeed"), base_value),
            (
                label("performance.cpu.sockets"),
                plain(cpu["sockets"].to_string()),
            ),
            (label("performance.cpu.cores"), plain(cores)),
            (
                label("performance.cpu.logicalProcessors"),
                plain(cpu["logical"].to_string()),
            ),
            (
                label("performance.cpu.virtualization"),
                plain(label(if boolean(cpu, "virtualization") {
                    "common.enabled"
                } else {
                    "performance.cpu.notEnabled"
                })),
            ),
            (
                tr!("performance.cpu.cache", level = 1),
                plain(fmt_cache(number(cpu, "l1"))),
            ),
            (
                tr!("performance.cpu.cache", level = 2),
                plain(fmt_cache(number(cpu, "l2"))),
            ),
            (
                tr!("performance.cpu.cache", level = 3),
                plain(fmt_cache(number(cpu, "l3"))),
            ),
        ]);
        out.push(self.perf_stats(stats, info, true, window));
        if hw["cpuSensors"].is_null() && values(&hw["thermal"]).is_empty() {
            out.push(self.perf_note(label("performance.cpu.thermalMissing")));
        }
        out
    }
    /// The logical processors' grid: rows stretch to the height the view leaves it,
    /// and the column count follows the area.
    fn core_grid(
        &self,
        hybrid: bool,
        filter: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let hw = &self.snapshot["hardware"];
        let sys = &self.snapshot["system"];
        let cpu = &hw["cpu"];
        let core_values = values(&sys["cores"]);
        let threads = values(&cpu["threads"]);
        let efficient = |index: usize| {
            hybrid
                && threads
                    .get(index)
                    .is_some_and(|thread| boolean(thread, "efficient"))
        };
        let visible = (0..core_values.len())
            .filter(|index| filter == "all" || efficient(*index) == (filter == "e"))
            .collect::<Vec<_>>();
        let gap = theme::dimension("coreGap");
        let cell_width = theme::dimension("coreMinWidth");
        let cell_height = theme::dimension("coreMinHeight");
        let header = theme::dimension("coreHeaderHeight");
        // Below the preferred card height the cards flatten to this before the view scrolls.
        let fit_height = theme::dimension("coreFitHeight");
        // The grid's size in this display; until it is known, the size it last had.
        let here_state = window.use_keyed_state("cpu-core-grid-size", cx, |_, _| None);
        let here: Option<Size<Pixels>> = *here_state.read(cx);
        let measured = here.unwrap_or_else(|| self.core_grid_size.get());
        let policy = settle_policy("cpu-core-settled", here.is_some(), window, cx);
        // Another core type is a new arrangement rather than a move: the cards
        // take their places at once while the view fades in (see `settle_fade`).
        let shown_filter = window.use_keyed_state("cpu-core-filter", cx, |_, _| filter.to_owned());
        let policy = if *shown_filter.read(cx) != filter {
            shown_filter.update(cx, |shown, _| *shown = filter.to_owned());
            gpui_kit::base::motion::Transition::new(std::time::Duration::ZERO)
        } else {
            policy
        };
        let available = if measured.width > px(0.) {
            measured.width
        } else {
            px(self.detail_width(window))
        };
        let previous = window.use_keyed_state("cpu-core-columns", cx, |_, _| (0usize, 0usize));
        let (previous_count, previous_columns) = *previous.read(cx);
        let columns = model::core_grid_columns(
            &model::CoreGrid {
                count: visible.len(),
                width: f32::from(available),
                height: f32::from(measured.height),
                cell_width: f32::from(cell_width),
                cell_height: f32::from(cell_height),
                header_height: f32::from(header),
                gap: f32::from(gap),
            },
            if previous_count == visible.len() {
                previous_columns
            } else {
                0
            },
        );
        previous.update(cx, |state, _| *state = (visible.len(), columns));
        let rows = visible.len().div_ceil(columns).max(1);
        let width = (available - gap * (columns - 1) as f32) / columns as f32;
        let height = ((measured.height - gap * (rows - 1) as f32) / rows as f32).max(fit_height);
        let grid_size = self.core_grid_size.clone();
        let owner = cx.entity().downgrade();
        let mut grid = div()
            .relative()
            .w_full()
            .flex_1()
            .min_h((fit_height + gap) * rows as f32 - gap)
            .child(
                canvas(
                    move |bounds, window, cx| {
                        grid_size.set(bounds.size);
                        let changed = here_state.update(cx, |size, _| size.replace(bounds.size))
                            != Some(bounds.size);
                        if changed {
                            let owner = owner.clone();
                            window.defer(cx, move |_, cx| {
                                let _ = owner.update(cx, |_, cx| cx.notify());
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        // Never laid out before: the cards wait one frame for the real size
        // rather than appearing in a guessed arrangement.
        let place = here.is_some() || measured.height > px(0.);
        let core_temperatures = values(&hw["cpuSensors"]["coreTemperatures"])
            .iter()
            .filter_map(|core| {
                Some((
                    (core["group"].as_u64()?, core["processor"].as_u64()?),
                    core["celsius"].as_f64()?,
                ))
            })
            .collect::<HashMap<_, _>>();
        let initial_scale = theme::design()["layout"]["coreEnterScale"]
            .as_f64()
            .expect("core enter scale") as f32;
        for (index, value) in core_values.iter().enumerate().filter(|_| place) {
            let ordinal = visible.iter().position(|i| *i == index);
            let key = format!("system.cores.{index}");
            let opacity = gpui_kit::base::motion::transition(
                format!("core-opacity-{index}"),
                if ordinal.is_some() { 1f32 } else { 0. },
                policy.clone(),
                window,
                cx,
            );
            if let Some(ordinal) = ordinal {
                self.grid_bounds.borrow_mut().insert(
                    key.clone(),
                    Bounds::new(
                        point(
                            (width + gap) * (ordinal % columns) as f32,
                            (height + gap) * (ordinal / columns) as f32,
                        ),
                        size(width, height),
                    ),
                );
            }
            let Some(target) = self.grid_bounds.borrow().get(&key).copied() else {
                continue;
            };
            let mut bounds = gpui_kit::base::motion::transition(
                format!("core-bounds-{index}"),
                target,
                policy.clone(),
                window,
                cx,
            );
            if opacity <= 0.001 {
                continue;
            }
            let scale = initial_scale + (1. - initial_scale) * opacity;
            let inset_x = bounds.size.width * ((1. - scale) / 2.);
            let inset_y = bounds.size.height * ((1. - scale) / 2.);
            bounds.origin += point(inset_x, inset_y);
            bounds.size = size(bounds.size.width * scale, bounds.size.height * scale);
            let eff = efficient(index);
            let color = if eff { p.cpu_e } else { p.cpu };
            let chart = self.chart_view(
                &format!("cpu-{index}"),
                &[Line::new(key.clone(), color)],
                Some(Scale::Fixed(100.)),
                (bounds.size.height - header).max(px(1.)),
                Look {
                    frame: true,
                    ..Look::default()
                },
                cx,
            );
            let thread = threads.get(index);
            let core = thread
                .and_then(|t| t["core"].as_u64())
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".into());
            let mut tip = match (hybrid, eff) {
                (true, true) => tr!("performance.cpu.coreTipE", index = index, core = core),
                (true, false) => tr!("performance.cpu.coreTipP", index = index, core = core),
                _ => tr!("performance.cpu.coreTip", index = index, core = core),
            };
            if let Some(celsius) = thread.and_then(|t| {
                core_temperatures
                    .get(&(t["group"].as_u64()?, t["processor"].as_u64()?))
                    .copied()
            }) {
                tip.push_str(&format!(
                    "\n{}: {} °C",
                    label("common.resource.temperature"),
                    celsius.round()
                ));
            }
            let mhz = number(&hw["coreMhz"], &index.to_string())
                .max(hw["coreMhz"][index].as_f64().unwrap_or(0.));
            let head = div()
                .h(header)
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px(px(1.))
                .pb(px(3.))
                .whitespace_nowrap()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .min_w_0()
                        .when(hybrid, |d| {
                            d.child(
                                div()
                                    .flex_none()
                                    .min_w(px(15.))
                                    .px(px(3.))
                                    .rounded(px(4.))
                                    .bg(color.opacity(0.16))
                                    .text_color(color.blend(p.ink.opacity(0.3)))
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::BOLD)
                                    .line_height(px(15.))
                                    .text_center()
                                    .child(if eff { "E" } else { "P" }),
                            )
                        })
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(index.to_string()),
                        )
                        .when(mhz > 0., |d| {
                            d.child(
                                div()
                                    .ml(px(2.))
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(p.muted)
                                    .child(ghz(mhz)),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_baseline()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .map(|d| match value.as_f64() {
                            Some(load) => d.child(format!("{}", load.round())).child(
                                div()
                                    .ml(px(1.))
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(p.muted)
                                    .child("%"),
                            ),
                            None => d.child(missing()),
                        }),
                );
            grid = grid.child(
                div()
                    .id(("core-cell", index))
                    .absolute()
                    .left(bounds.origin.x)
                    .top(bounds.origin.y)
                    .w(bounds.size.width)
                    .h(bounds.size.height)
                    .opacity(opacity)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(head)
                    .child(chart)
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                    }),
            );
        }
        div()
            .flex_1()
            .flex()
            .flex_col()
            .child(grid)
            .when(hybrid, |d| {
                d.child(self.legend(vec![
                    (label("performance.cpu.coreKind.p"), p.cpu, false),
                    (label("performance.cpu.coreKind.e"), p.cpu_e, false),
                ]))
            })
            .into_any_element()
    }
    fn memory_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.palette;
        let sys = &self.snapshot["system"];
        let memory = &self.snapshot["hardware"]["memory"];
        let total = number(sys, "memTotal");
        let installed = number(memory, "installed");
        let model = format!(
            "{:.0} GB{}",
            (if installed > 0. { installed } else { total }) / GIB,
            memory["kind"]
                .as_str()
                .map(|kind| format!(" {kind}"))
                .unwrap_or_default()
        );
        let (used, modified, standby, free) = memory_composition(sys);
        let fraction = |bytes: f64| (bytes / total.max(1.)).clamp(0., 1.) as f32;
        let segment = |start: f64, length: f64, color: Hsla| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(fraction(start)))
                .w(relative(fraction(length)))
                .bg(color)
        };
        let reading = |bytes: Option<f64>| bytes.map(theme::memory).unwrap_or_else(missing);
        let bar = div()
            .relative()
            .flex_shrink_0()
            .h(px(28.))
            .mt(px(14.))
            .overflow_hidden()
            .border_1()
            .border_color(p.memory.opacity(0.45))
            .rounded(px(4.))
            .child(segment(0., used, p.memory.opacity(0.55)))
            .child(segment(used, modified.unwrap_or(0.), p.memory.opacity(0.9)))
            .child(segment(
                used + modified.unwrap_or(0.),
                standby.unwrap_or(0.),
                p.memory.opacity(0.2),
            ));
        let legend_item =
            |id: &'static str, swatch: Div, description: &'static str, text: String| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        swatch
                            .id(id)
                            .size(px(10.))
                            .rounded(px(2.))
                            .flex_shrink_0()
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(label(description))
                                    .build(window, cx)
                            }),
                    )
                    .child(text)
            };
        let legend = div()
            .flex()
            .flex_wrap()
            .flex_shrink_0()
            .gap_x(px(18.))
            .mt(px(6.))
            .text_size(px(12.))
            .text_color(p.muted)
            .child(legend_item(
                "memory-legend-used",
                div().bg(p.memory.opacity(0.55)),
                "performance.memory.inUseDescription",
                tr!(
                    "performance.memory.legendInUse",
                    value = theme::memory(used)
                ),
            ))
            .child(legend_item(
                "memory-legend-modified",
                div().bg(p.memory.opacity(0.9)),
                "performance.memory.modifiedDescription",
                tr!(
                    "performance.memory.legendModified",
                    value = reading(modified)
                ),
            ))
            .child(legend_item(
                "memory-legend-standby",
                div().bg(p.memory.opacity(0.2)),
                "performance.memory.standbyDescription",
                tr!("performance.memory.legendStandby", value = reading(standby)),
            ))
            .child(legend_item(
                "memory-legend-free",
                div().border_1().border_color(p.stroke_strong),
                "performance.memory.freeDescription",
                tr!("performance.memory.legendFree", value = reading(free)),
            ));
        let gb_stat =
            |key: &str, bytes: f64| self.stat(label(key), gb(bytes), Some("GB".into()), None, cx);
        let stats = vec![
            gb_stat("performance.memory.inUse", used),
            gb_stat("performance.memory.available", number(sys, "memAvailable")),
            self.stat(
                label("performance.memory.committed"),
                format!(
                    "{} / {}",
                    gb(number(sys, "commitUsed")),
                    gb(number(sys, "commitLimit"))
                ),
                Some("GB".into()),
                None,
                cx,
            ),
            gb_stat("performance.memory.cached", number(sys, "cached")),
            self.stat_parts(
                label("performance.memory.pagedPool"),
                theme::memory(number(sys, "kernelPaged")),
                cx,
            ),
            self.stat_parts(
                label("performance.memory.nonPagedPool"),
                theme::memory(number(sys, "kernelNonpaged")),
                cx,
            ),
        ];
        let plain = |value: String| div().child(value).into_any_element();
        let reserved = (installed - total).max(0.);
        let form_factor = memory["formFactor"].as_str();
        let mut rows = vec![
            (
                label("performance.memory.speed"),
                plain(
                    memory["speed"]
                        .as_f64()
                        .filter(|v| *v > 0.)
                        .map(|v| format!("{v} MT/s"))
                        .unwrap_or_else(missing),
                ),
            ),
            (
                label("performance.memory.slotsUsed"),
                plain(if number(memory, "slotsTotal") > 0. {
                    format!("{} / {}", memory["slotsUsed"], memory["slotsTotal"])
                } else {
                    missing()
                }),
            ),
            (
                label("performance.memory.formFactor"),
                plain(match form_factor {
                    Some("onboard") => label("performance.memory.onboard"),
                    Some(value) => value.into(),
                    None => missing(),
                }),
            ),
            (
                label("performance.memory.type"),
                plain(
                    memory["kind"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(missing),
                ),
            ),
        ];
        if reserved > 0. {
            rows.push((
                label("performance.memory.reserved"),
                plain(format!("{:.0} MB", reserved / MIB)),
            ));
        }
        vec![
            self.perf_head(label("common.resource.memory"), Some(model), None, cx),
            self.chart_view(
                "memory-usage",
                &[Line::new("system.memUsed", p.memory)],
                Some(Scale::Fixed(total.max(1.))),
                ChartHeight::Fill,
                Look::boxed(label("performance.memory.usage"))
                    .top(format!("{} GB", gb(total)))
                    .format(MetricFormat::Gigabytes)
                    .min_height(px(MAIN_MIN)),
                cx,
            ),
            bar.into_any_element(),
            legend.into_any_element(),
            self.perf_stats(stats, self.info_list(rows), false, window),
        ]
    }
    fn disk_view(
        &self,
        device: &Device,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = self.palette;
        let disk = &self.snapshot["hardware"]["disks"][device.index];
        let info = &disk["info"];
        let prefix = format!("hardware.disks.{}", device.index);
        let mut stats = vec![
            self.stat(
                label("performance.disk.activeTime"),
                format!("{}", number(disk, "active").round()),
                Some("%".into()),
                None,
                cx,
            ),
            self.stat(
                label("performance.disk.responseTime"),
                format!("{:.1}", number(disk, "responseMs")),
                Some(label("performance.disk.ms")),
                None,
                cx,
            ),
            self.stat_parts(
                label("performance.disk.readSpeed"),
                charts::byte_rate(number(disk, "read")),
                cx,
            ),
            self.stat_parts(
                label("performance.disk.writeSpeed"),
                charts::byte_rate(number(disk, "write")),
                cx,
            ),
        ];
        if let Some(celsius) = disk["temperature"].as_f64().filter(|v| *v != 0.) {
            stats.push(self.stat(
                label("common.resource.temperature"),
                format!("{}", celsius.round()),
                Some("°C".into()),
                None,
                cx,
            ));
        }
        let plain = |value: String| div().child(value).into_any_element();
        let volumes = values(&info["volumes"])
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("  ");
        let kind = disk_kind(info);
        let mut rows = vec![
            (
                label("performance.disk.capacity"),
                plain(
                    info["size"]
                        .as_f64()
                        .filter(|v| *v > 0.)
                        .map(fmt_capacity)
                        .unwrap_or_else(missing),
                ),
            ),
            (
                label("performance.disk.type"),
                plain(if kind.is_empty() { missing() } else { kind }),
            ),
            (
                label("performance.disk.volumes"),
                plain(if volumes.is_empty() {
                    label("common.none")
                } else {
                    volumes
                }),
            ),
        ];
        if boolean(info, "removable") {
            rows.push((
                label("performance.disk.removable"),
                plain(label("performance.disk.yes")),
            ));
        }
        if let Some(warning) = disk["temperatureWarning"].as_f64().filter(|v| *v > 0.) {
            rows.push((
                label("performance.disk.warningTemperature"),
                plain(format!("{warning} °C")),
            ));
        }
        vec![
            self.perf_head(device.title.clone(), Some(device.model.clone()), None, cx),
            self.chart_view(
                "disk-active",
                &[Line::new(format!("{prefix}.active"), p.io)],
                Some(Scale::Fixed(100.)),
                ChartHeight::Fill,
                Look::boxed(label("performance.disk.activeTime"))
                    .top("100%")
                    .format(MetricFormat::RoundPercent)
                    .min_height(px(HALF_MIN)),
                cx,
            ),
            div()
                .mt(px(18.))
                .flex_1()
                .flex()
                .flex_col()
                .child(
                    self.chart_view(
                        "disk-rate",
                        &[
                            Line::new(format!("{prefix}.read"), p.io)
                                .label(label("performance.disk.read")),
                            Line::new(format!("{prefix}.write"), p.io)
                                .dashed()
                                .label(label("performance.disk.write")),
                        ],
                        Some(Scale::Rate {
                            floor: MIB,
                            bits: false,
                        }),
                        ChartHeight::Fill,
                        Look::boxed(label("performance.disk.transferRate"))
                            .format(MetricFormat::ByteRate)
                            .min_height(px(HALF_MIN))
                            .legend(vec![
                                (label("performance.disk.read"), false),
                                (label("performance.disk.write"), true),
                            ]),
                        cx,
                    ),
                )
                .into_any_element(),
            self.perf_stats(stats, self.info_list(rows), false, window),
        ]
    }
    /// Addresses past the first three fold behind a toggle.
    fn addresses(
        &self,
        id: String,
        list: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const SHOWN: usize = 3;
        if list.is_empty() {
            return div().child(missing()).into_any_element();
        }
        let state = window.use_keyed_state(SharedString::from(id.clone()), cx, |_, _| false);
        let all = *state.read(cx);
        let total = list.len();
        let p = self.palette;
        div()
            .flex()
            .flex_col()
            .children(
                list.into_iter()
                    .take(if all { total } else { SHOWN })
                    .map(|address| div().child(address)),
            )
            .when(total > SHOWN, |d| {
                let owner = cx.entity().downgrade();
                d.child(
                    div()
                        .id(SharedString::from(format!("{id}-toggle")))
                        .flex()
                        .items_center()
                        .gap_1()
                        .mt(px(2.))
                        .text_size(px(12.))
                        .text_color(p.accent)
                        .cursor_pointer()
                        .child(if all {
                            label("performance.network.showFewer")
                        } else {
                            tr!("performance.network.showAll", count = total)
                        })
                        .child(
                            icon(if all { "ChevronUp" } else { "ChevronDown" }, p.accent)
                                .size(px(12.)),
                        )
                        .on_click(move |_, _, cx| {
                            state.update(cx, |all, _| *all = !*all);
                            let _ = owner.update(cx, |_, cx| cx.notify());
                        }),
                )
            })
            .into_any_element()
    }
    fn nic_view(
        &self,
        device: &Device,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = self.palette;
        let nic = &self.snapshot["hardware"]["nics"][device.index];
        let info = &nic["info"];
        let prefix = format!("hardware.nics.{}", device.index);
        let bits = self.net_bits();
        let rate_stat = |key: &str, bytes: f64| {
            let (value, unit) = split(charts::net_rate(bytes, bits));
            self.stat(
                label(key),
                value,
                unit,
                Some(charts::net_rate(bytes, !bits)),
                cx,
            )
        };
        let stats = vec![
            rate_stat("performance.network.receive", number(nic, "rx")),
            rate_stat("performance.network.send", number(nic, "tx")),
        ];
        let plain = |value: String| div().child(value).into_any_element();
        let addresses = |list: &Value| {
            values(list)
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect::<Vec<_>>()
        };
        let speed = number(info, "linkSpeed");
        let ipv4 = self.addresses(
            format!("nic-{}-ipv4", text(info, "id")),
            addresses(&info["ipv4"]),
            window,
            cx,
        );
        let ipv6 = self.addresses(
            format!("nic-{}-ipv6", text(info, "id")),
            addresses(&info["ipv6"]),
            window,
            cx,
        );
        let rows = vec![
            (
                label("performance.network.connectionType"),
                plain(label(&format!(
                    "performance.network.kind.{}",
                    match text(info, "kind") {
                        "" => "other",
                        kind => kind,
                    }
                ))),
            ),
            (
                label("performance.network.linkSpeed"),
                plain(if speed > 0. {
                    tr!(
                        "performance.withAlt",
                        value = link_speed(speed),
                        alt = charts::byte_rate(speed / 8.)
                    )
                } else {
                    missing()
                }),
            ),
            (label("performance.network.ipv4"), ipv4),
            (label("performance.network.ipv6"), ipv6),
            (
                label("performance.network.mac"),
                plain(
                    info["mac"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(missing),
                ),
            ),
        ];
        vec![
            self.perf_head(device.title.clone(), Some(device.model.clone()), None, cx),
            self.chart_view(
                "network-throughput",
                &[
                    Line::new(format!("{prefix}.rx"), p.network)
                        .label(label("performance.network.receive")),
                    Line::new(format!("{prefix}.tx"), p.network)
                        .dashed()
                        .label(label("performance.network.send")),
                ],
                Some(Scale::Rate {
                    floor: 64. * KIB,
                    bits,
                }),
                ChartHeight::Fill,
                Look::boxed(label("performance.network.throughput"))
                    .format(MetricFormat::NetworkAlt(bits))
                    .min_height(px(MAIN_MIN))
                    .legend(vec![
                        (label("performance.network.receive"), false),
                        (label("performance.network.send"), true),
                    ]),
                cx,
            ),
            self.perf_stats(stats, self.info_list(rows), false, window),
        ]
    }
    fn battery_view(
        &self,
        device: &Device,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = self.palette;
        let battery = &self.snapshot["hardware"]["batteries"][device.index];
        let info = &battery["info"];
        let prefix = format!("hardware.batteries.{}", device.index);
        let state = text(battery, "state");
        let flow = label(if state == "charging" {
            "performance.battery.chargeRate"
        } else {
            "performance.battery.dischargeRate"
        });
        let health = info["fullCapacity"]
            .as_f64()
            .zip(info["designedCapacity"].as_f64())
            .filter(|(full, design)| *full > 0. && *design > 0.)
            .map(|(full, design)| full / design * 100.);
        let mut stats = vec![
            self.stat(
                label("performance.battery.level"),
                format!("{}", number(battery, "percent").round()),
                Some("%".into()),
                None,
                cx,
            ),
            self.stat(label("common.status"), battery_state(state), None, None, cx),
        ];
        if let Some(rate) = battery["rate"].as_f64() {
            stats.push(self.stat(
                flow.clone(),
                charts::watts(rate.abs() / 1000.),
                Some("W".into()),
                None,
                cx,
            ));
        }
        if let Some(voltage) = battery["voltage"].as_f64().filter(|v| *v > 0.) {
            stats.push(self.stat(
                label("performance.battery.voltage"),
                format!("{:.2}", voltage / 1000.),
                Some("V".into()),
                None,
                cx,
            ));
        }
        if let Some(remaining) = battery["remaining"].as_f64().filter(|v| *v > 0.) {
            stats.push(self.stat(
                label("performance.battery.remaining"),
                duration(remaining),
                None,
                None,
                cx,
            ));
        }
        if let Some(health) = health {
            stats.push(self.stat(
                label("performance.battery.health"),
                format!("{}", health.round()),
                Some("%".into()),
                None,
                cx,
            ));
        }
        if let Some(cycles) = info["cycleCount"].as_f64().filter(|v| *v > 0.) {
            stats.push(self.stat(
                label("performance.battery.cycles"),
                format!("{cycles}"),
                None,
                None,
                cx,
            ));
        }
        if let Some(celsius) = battery["temperature"].as_f64().filter(|v| *v != 0.) {
            stats.push(self.stat(
                label("common.resource.temperature"),
                format!("{}", celsius.round()),
                Some("°C".into()),
                None,
                cx,
            ));
        }
        let plain = |value: String| div().child(value).into_any_element();
        let mut rows = vec![
            (
                label("performance.battery.capacity"),
                plain(watt_hours(&battery["capacity"])),
            ),
            (
                label("performance.battery.fullCapacity"),
                plain(watt_hours(&info["fullCapacity"])),
            ),
            (
                label("performance.battery.designCapacity"),
                plain(watt_hours(&info["designedCapacity"])),
            ),
            (
                label("performance.battery.chemistry"),
                plain(chemistry(text(info, "chemistry"))),
            ),
            (
                label("performance.battery.manufacturer"),
                plain(or_missing(text(info, "manufacturer"))),
            ),
            (
                label("performance.battery.model"),
                plain(or_missing(text(info, "name"))),
            ),
            (
                label("performance.battery.serial"),
                plain(or_missing(text(info, "serial"))),
            ),
        ];
        if let Some(date) = info["manufactured"].as_str().filter(|d| !d.is_empty()) {
            rows.push((
                label("performance.battery.manufactured"),
                plain(date.into()),
            ));
        }
        let model = [text(info, "manufacturer"), text(info, "name")]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let mut out = vec![
            self.perf_head(label("performance.battery.title"), Some(model), None, cx),
            self.chart_view(
                "battery-level",
                &[Line::new(format!("{prefix}.percent"), p.battery)],
                Some(Scale::Fixed(100.)),
                ChartHeight::Fill,
                Look::boxed(label("performance.battery.level"))
                    .top("100%")
                    .format(MetricFormat::RoundPercent)
                    .min_height(px(HALF_MIN)),
                cx,
            ),
        ];
        if battery["rate"].is_number() {
            out.push(
                div()
                    .mt(px(18.))
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        self.chart_view(
                            "battery-rate",
                            &[
                                Line::new(format!("{prefix}.rate"), p.battery)
                                    .magnitude(1. / 1000.),
                            ],
                            Some(Scale::Round {
                                floor: 5.,
                                unit: "W",
                            }),
                            ChartHeight::Fill,
                            Look::boxed(flow)
                                .format(MetricFormat::Watts)
                                .min_height(px(HALF_MIN)),
                            cx,
                        ),
                    )
                    .into_any_element(),
            );
        }
        out.push(self.perf_stats(stats, self.info_list(rows), false, window));
        if let Some(health) = health.filter(|h| *h < 80.) {
            out.push(self.perf_note(tr!(
                "performance.battery.wornNote",
                percent = health.round()
            )));
        }
        out
    }
}
