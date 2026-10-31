use super::*;
use charts::{ChartHeight, GIB, KIB, Line, Look, MIB, MetricFormat, Scale};
use gpui_kit::base::ElementExt;
mod cpu_motion;
mod engines;
mod telemetry;

/// Device list geometry (the interface's `.dev` rows).
const ROW_HEIGHT: f32 = 58.;
const ROW_PADDING_LEFT: f32 = 12.;
const ROW_PADDING: f32 = 8.;
const PREVIEW_WIDTH: f32 = 76.;
const PREVIEW_GAP: f32 = 10.;
/// The selection marker follows the actual wrapped names, including when
/// the rail is resized or the user changes the base font size.
fn row_height(title: &str, width: Pixels, window: &Window, cx: &App) -> Pixels {
    let line_height = |size: Pixels| {
        window.pixel_snap(theme::line_height().to_pixels(size.into(), window.rem_size()))
    };
    let size = theme::font(13.);
    let title_line = line_height(size);
    let run = theme::text_run(title, theme::LABEL_WEIGHT, cx);
    let height = window
        .text_system()
        .shape_text(title.to_owned().into(), size, &[run], Some(width), None)
        .map_or(title_line, |lines| {
            lines.iter().map(|line| line.size(title_line).height).sum()
        });
    px(ROW_HEIGHT).max(height + line_height(theme::font(12.)) + px(2. * ROW_PADDING + 2.))
}
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
/// Widest a figure's explanation runs before it wraps.
const HINT_WIDTH: f32 = 420.;
/// The CPU's figures and specs go side by side from this width each.
const CPU_STATS_MIN: f32 = 320.;
/// Smallest chart heights before the page scrolls (main, half, small).
const MAIN_MIN: f32 = 120.;
const HALF_MIN: f32 = 80.;
const SMALL_MIN: f32 = 64.;
/// A detail card at most this tall (its content box) draws the chart
/// captions inside the frames: `@container perf (max-height: 640px)`.
const SHORT_CARD: f32 = 640.;

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

// ---- Formatting ------------------------------------------------

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
/// The CPU's one power figure and the label naming its source: the package,
/// or the cores where the package has no reading.
fn cpu_power(power: &Value) -> Option<(f64, &'static str)> {
    [
        ("package", "performance.cpu.packagePower"),
        ("cores", "performance.cpu.corePower"),
    ]
    .into_iter()
    .find_map(|(field, source)| power[field].as_f64().map(|watts| (watts, source)))
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
/// view places them at once, while later resizes and layout changes animate.
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
            self.nav_width + 8.
        } else {
            48.
        };
        f32::from(window.viewport_size().width) - navigation - 2. * PAGE_GAP
    }
    /// The detail card's content height (the interface's `100cqh`), once laid
    /// out: the card fills the page's height whatever it holds, so in this
    /// viewport it is the viewport's less what surrounds the card.
    fn perf_card_height(&self, window: &Window) -> Option<Pixels> {
        self.perf_card_chrome
            .get()
            .map(|chrome| window.viewport_size().height - chrome)
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
        let title_width = px(side_width
            - scrollbar::SIZE
            - 2.
            - ROW_PADDING_LEFT
            - ROW_PADDING
            - PREVIEW_GAP
            - PREVIEW_WIDTH);
        let heights: Vec<_> = devices
            .iter()
            .map(|device| row_height(&device.title, title_width, window, cx))
            .collect();
        let selected_height = heights
            .get(selected_index)
            .copied()
            .unwrap_or(px(ROW_HEIGHT));
        // The selected device: a raised surface with an accent pill, sliding between rows.
        let mark_top = gpui_kit::base::motion::transition(
            "perf-device-mark",
            heights[..selected_index].iter().copied().sum::<Pixels>()
                + px(ROW_GAP) * selected_index as f32,
            gpui_kit::base::motion::Transition::new(std::time::Duration::from_millis(250)).easing(
                gpui_kit::base::motion::Easing::cubic_bezier(0.33, 0., 0.1, 1.).unwrap_or_default(),
            ),
            window,
            cx,
        );
        let device_list = window
            .use_keyed_state("devices-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        // The rows leave the scrollbar's width free, whether or not it shows
        // (`.dev` is `100cqw - var(--scrollbar-size)` wide).
        let mut list = div()
            .id("devices")
            .track_scroll(&device_list)
            .relative()
            .w(px(side_width))
            .flex_shrink_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .pr(px(scrollbar::SIZE))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right(px(scrollbar::SIZE))
                    .top(mark_top)
                    .h(selected_height)
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(6.))
                    .bg(p.layer_strong)
                    .child(
                        div()
                            .absolute()
                            .left(px(-1.))
                            .top((selected_height - px(2. + 18.)) / 2.)
                            .w(px(3.))
                            .h(px(18.))
                            .rounded(px(2.))
                            .bg(p.accent),
                    ),
            );
        for (device, height) in devices.iter().zip(heights) {
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
                    .line_height(theme::line_height())
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
                    .h(height)
                    .w_full()
                    .justify_start()
                    .pl(px(ROW_PADDING_LEFT))
                    .pr(px(ROW_PADDING))
                    .py(px(ROW_PADDING))
                    .gap(px(PREVIEW_GAP))
                    .border_1()
                    .border_color(gpui_kit::gpui::transparent_black())
                    .rounded(px(6.))
                    .child(
                        div()
                            .w(title_width)
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(theme::LABEL_WEIGHT)
                                    .child(device.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(theme::font(12.))
                                    .text_color(p.muted)
                                    .truncate()
                                    // `.dev-sub` collapsed runs of spaces, as HTML does.
                                    .child(
                                        self.device_sub(device)
                                            .split_whitespace()
                                            .collect::<Vec<_>>()
                                            .join(" "),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .w(px(PREVIEW_WIDTH))
                            .h(px(40.))
                            .flex_shrink_0()
                            .border_1()
                            .border_color(device.color.opacity(0.45))
                            .rounded(px(3.))
                            .overflow_hidden()
                            .child(div().size_full().bg(p.chart_surface).child(graph)),
                    ),
            );
        }
        // A newly chosen device starts at the top and arrives from its direction in
        // the list, fading in (250 ms, decelerating). Only the offset and opacity
        // change: an animation wrapper keyed by each arrival would drop the
        // state of everything inside, the charts' drawn curves included, and
        // they would pop in again. Both apply only when drawing (the interface's
        // transform), so the view is laid out in place and its scroll range
        // never grows by the offset.
        let arrival = window.use_keyed_state("perf-device-arrival", cx, |_, _| {
            (String::new(), 0usize, 0f32, None::<Instant>)
        });
        let (previous_id, previous_index, mut direction, mut started) = arrival.read(cx).clone();
        let current_id = selected.as_ref().map(|d| d.id.clone()).unwrap_or_default();
        if previous_id != current_id {
            if !previous_id.is_empty() {
                direction = (selected_index as f32 - previous_index as f32).signum();
                started = Some(Instant::now());
                self.device_scroll.set_offset(point(px(0.), px(0.)));
            }
            arrival.update(cx, |state, _| {
                *state = (current_id, selected_index, direction, started)
            });
        }
        let progress = started
            .filter(|_| !cx.reduce_motion())
            .map(|at| (at.elapsed().as_secs_f32() / 0.25).min(1.))
            .unwrap_or(1.);
        let arriving = if progress < 1. {
            window.request_animation_frame();
            theme::ease_out(progress)
        } else {
            1.
        };
        let offset = point(px(0.), px(direction * 16. * (1. - arriving)));
        // The chart layer's curves move and fade with the view.
        self.chart_fade.set(arriving);
        self.chart_shift.set(offset);
        // A short card (the interface's size container query on the card)
        // takes the chart captions into the frames.
        let short = self
            .perf_card_height(window)
            .is_some_and(|card| card <= px(SHORT_CARD));
        self.chart_captions_inside.set(short);
        let content = selected
            .map(|device| self.device_view(&device, window, cx))
            .unwrap_or_else(|| div().into_any_element());
        self.chart_captions_inside.set(false);
        self.chart_fade.set(1.);
        self.chart_shift.set(Point::default());
        let content = super::super::shift::shifted(
            div().flex_1().flex().flex_col().child(content),
            offset,
            arriving,
        )
        .into_any_element();
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
        let chrome = self.perf_card_chrome.clone();
        let owner = cx.entity().downgrade();
        div()
            .size_full()
            .min_h_0()
            .px(px(PAGE_GAP))
            .flex()
            // The interface body line height; GPUI's default is taller.
            .line_height(relative(1.4))
            // The card is as tall as the page; its content box is inside the
            // border. What surrounds it changes rarely (text size, navigation,
            // toolbar rows): the views sized by it are built again then.
            .on_prepaint(move |bounds, window, cx| {
                let card = bounds.size.height - px(2.);
                let measured = Some(window.viewport_size().height - card);
                if chrome.replace(measured) != measured {
                    window.defer(cx, move |_, cx| {
                        let _ = owner.update(cx, |_, cx| cx.notify());
                    });
                }
            })
            .child(list.child(scrollbar::scrollbar(
                "devices-scrollbar",
                &device_list,
                p.dark,
            )))
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
                    // The gutter is kept (`scrollbar-gutter: stable`).
                    .pr(px(scrollbar::SIZE))
                    .child(content)
                    // Clear of the card's rounded corners (`.perf-scroll` track margin).
                    .child(
                        scrollbar::scrollbar("device-scrollbar", &self.device_scroll, p.dark)
                            .border(1.)
                            .track_margin(8.),
                    ),
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
        // `.perf-head`: the model on the title's baseline.
        div()
            .flex()
            .flex_shrink_0()
            .items_baseline()
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
                            .text_size(theme::font_dimension("pageTitleSize"))
                            .font_weight(theme::LABEL_WEIGHT)
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
                        .text_size(theme::font(15.))
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
                        .text_size(theme::font(13.))
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
                    .text_size(theme::font(12.))
                    .text_color(self.palette.muted)
                    .truncate()
                    .child(title.into()),
            )
            .child(
                div()
                    .font_family(theme::display_font(cx))
                    .text_size(theme::font_dimension("metricSize"))
                    .whitespace_nowrap()
                    .child(value),
            )
            .when_some(note, |d, note| {
                d.child(
                    div()
                        .mt(px(1.))
                        .text_size(theme::font(12.))
                        .text_color(self.palette.faint)
                        .child(note),
                )
            })
            .into_any_element()
    }
    /// A figure explained on hover, as the interface's `title` does. The
    /// explanation wraps like a system tooltip instead of running off the window.
    fn hinted(&self, id: &'static str, hint: Option<String>, stat: AnyElement) -> AnyElement {
        let hint = hint.map(SharedString::from);
        match hint {
            Some(hint) => div()
                .id(id)
                .min_w_0()
                .child(stat)
                .tooltip(move |window, cx| {
                    widgets::wrapping_tooltip(hint.clone(), px(HINT_WIDTH), window, cx)
                })
                .into_any_element(),
            None => stat,
        }
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
                        .text_size(theme::font(13.))
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
            .text_size(theme::font(12.))
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
                            .text_size(theme::font(14.))
                            .font_weight(theme::LABEL_WEIGHT)
                            .child(title)
                            .when_some(note, |d, note| {
                                d.child(
                                    div()
                                        .ml(px(6.))
                                        .text_size(theme::font(12.))
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
                            .text_size(theme::font(12.))
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
        out.push(self.cpu_graphs(logical, hybrid, &filter, window, cx));
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
        // One power figure, its source named on hover. Graphics power belongs
        // to the GPU view, where its source is named.
        if let Some((watts, source)) = cpu_power(&hw["power"]) {
            let stat = self.stat(
                label("performance.cpu.power"),
                charts::watts(watts),
                Some("W".into()),
                None,
                cx,
            );
            stats.push(self.hinted("cpu-stat-power", Some(label(source)), stat));
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
                d.tooltip(move |window, cx| widgets::tooltip(source.clone(), window, cx))
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
    /// The CPU chart area: the overall graph or the logical processors' grid.
    /// Another graph cross-fades in from a slight zoom (`cpu_motion`): the
    /// outgoing one stays drawn over it where it was, fading out quickly. The
    /// area is laid out once in its new arrangement; the zoom and fades are
    /// live transforms, so their frames replay the page's paint rather than
    /// render it, and the chart layer draws the curves inside them.
    fn cpu_graphs(
        &self,
        logical: bool,
        hybrid: bool,
        filter: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let animate = !cx.reduce_motion();
        let motions = self.live_motions.clone();
        let switch = window.use_keyed_state("cpu-view-switch", cx, |_, _| {
            cpu_motion::ViewSwitch::default()
        });
        let cores = window.use_keyed_state("cpu-core-motion", cx, |_, _| {
            cpu_motion::CoreMotion::default()
        });
        let drawn = cores.read(cx).drawn(&motions);
        let done = self.render_page_later(cx);
        let leaving = switch.update(cx, |switch, _| {
            switch.update(logical, animate, &motions, || drawn, done)
        });
        let (area, ghost, ghost_cards) = {
            let switch = switch.read(cx);
            (
                switch.area.clone(),
                switch.ghost.clone(),
                switch.cards.clone(),
            )
        };
        let lives = |transform: &LiveTransform| charts::Live {
            transform: transform.clone(),
            anchor: Rc::new(Cell::new(Point::default())),
        };
        let incoming = leaving.map(|_| lives(&area));
        if let Some(live) = &incoming {
            self.chart_lives.borrow_mut().push(live.clone());
        }
        let graphs = if logical {
            let (graphs, placed) = self.core_grid(hybrid, filter, &cores, window, cx);
            // A grid shown for the first time places its cards once it knows
            // its size, a frame later: the switch begins when they are drawn.
            if placed {
                let done = self.render_page_later(cx);
                switch.update(cx, |switch, _| switch.ready(&motions, done));
            } else {
                switch.update(cx, |switch, _| switch.wait());
            }
            graphs
        } else {
            cores.update(cx, |cores, _| cores.reset(&motions));
            self.cpu_overall_chart(cx)
        };
        if incoming.is_some() {
            self.chart_lives.borrow_mut().pop();
        }
        let graphs = div()
            .w_full()
            .min_w_0()
            .flex_1()
            .flex()
            .flex_col()
            .child(graphs);
        let graphs = match incoming {
            Some(live) => super::super::shift::live(
                graphs,
                live.transform,
                super::super::shift::Anchor::Center,
                live.anchor,
            )
            .into_any_element(),
            None => graphs.into_any_element(),
        };
        let leaving = leaving.map(|was_logical| {
            let live = lives(&ghost);
            self.chart_lives.borrow_mut().push(live.clone());
            let graphs = if was_logical {
                self.core_layout(hybrid, &ghost_cards, None, None, px(0.), false, cx)
            } else {
                self.cpu_overall_chart(cx)
            };
            self.chart_lives.borrow_mut().pop();
            div()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .child(super::super::shift::live(
                    div().size_full().flex().flex_col().child(graphs),
                    live.transform,
                    super::super::shift::Anchor::TopLeft,
                    live.anchor,
                ))
        });
        div()
            .relative()
            .w_full()
            .min_w_0()
            .flex_1()
            .flex()
            .flex_col()
            .child(graphs)
            .children(leaving)
            .into_any_element()
    }
    fn cpu_overall_chart(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
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
    }
    /// The logical processors' grid: rows stretch to the height the view leaves it,
    /// and the column count follows the area. Another core type or another
    /// size of the grid rearranges the cards (`cpu_motion`): they are laid out
    /// where they go, and drawn moving there with live transforms. Also
    /// returns whether it placed its cards (not before it knows its size).
    fn core_grid(
        &self,
        hybrid: bool,
        filter: &str,
        cores: &Entity<cpu_motion::CoreMotion>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, bool) {
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
        let measuring = cores.clone();
        let measure = canvas(
            move |bounds, window, cx| {
                grid_size.set(bounds.size);
                let previous = here_state.update(cx, |size, _| size.replace(bounds.size));
                if bounds.size != measured {
                    // Laid out at a size it does not have: again at its own.
                    let owner = owner.clone();
                    window.defer(cx, move |_, cx| {
                        let _ = owner.update(cx, |_, cx| cx.notify());
                    });
                } else if previous.is_none() {
                    // A first display laid out at the size the grid last had,
                    // which it has: no second render (it would hold up the
                    // transition to this graph), and later changes animate.
                    measuring.update(cx, |cores, _| cores.measured());
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
        .into_any_element();
        // Never laid out before: the cards wait one frame for the real size
        // rather than appearing in a guessed arrangement.
        let place = here.is_some() || measured.height > px(0.);
        let targets = visible
            .iter()
            .enumerate()
            .filter(|_| place)
            .map(|(ordinal, index)| {
                (
                    *index,
                    Bounds::new(
                        point(
                            (width + gap) * (ordinal % columns) as f32,
                            (height + gap) * (ordinal / columns) as f32,
                        ),
                        size(width, height),
                    ),
                )
            })
            .collect::<Vec<_>>();
        let animate = !cx.reduce_motion();
        let enter_scale = theme::design()["layout"]["coreEnterScale"]
            .as_f64()
            .expect("core enter scale") as f32;
        let motions = self.live_motions.clone();
        let done = self.render_page_later(cx);
        let moving = cores.update(cx, |cores, _| {
            cores.update(
                filter,
                &targets,
                here.is_some(),
                animate,
                header,
                enter_scale,
                &motions,
                done,
            )
        });
        let cards = match &moving {
            Some(moving) => moving
                .cards
                .iter()
                .map(|card| {
                    (
                        card.index,
                        cpu_motion::Card {
                            bounds: card.layout,
                            presence: 1.,
                        },
                    )
                })
                .collect(),
            None => targets
                .iter()
                .map(|(index, bounds)| {
                    (
                        *index,
                        cpu_motion::Card {
                            bounds: *bounds,
                            presence: 1.,
                        },
                    )
                })
                .collect::<Vec<_>>(),
        };
        let grid = self.core_layout(
            hybrid,
            &cards,
            moving.as_deref(),
            Some(measure),
            (fit_height + gap) * rows as f32 - gap,
            true,
            cx,
        );
        (grid, place)
    }
    /// The grid holding `cards`, with the legend below. While the cards
    /// rearrange (`moving`), each is drawn inside its live transforms;
    /// `interactive` cards carry their tooltips (not those of a grid fading out).
    #[allow(clippy::too_many_arguments)]
    fn core_layout(
        &self,
        hybrid: bool,
        cards: &[(usize, cpu_motion::Card)],
        moving: Option<&cpu_motion::Rearrangement>,
        measure: Option<AnyElement>,
        min_height: Pixels,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let core_temperatures =
            values(&self.snapshot["hardware"]["cpuSensors"]["coreTemperatures"])
                .iter()
                .filter_map(|core| {
                    Some((
                        (core["group"].as_u64()?, core["processor"].as_u64()?),
                        core["celsius"].as_f64()?,
                    ))
                })
                .collect::<HashMap<_, _>>();
        let mut grid = div()
            .relative()
            .w_full()
            .flex_1()
            .min_h(min_height)
            .children(measure);
        for (index, card) in cards {
            let lives = moving.and_then(|moving| {
                moving
                    .cards
                    .iter()
                    .find(|moving| moving.index == *index)
                    .map(|moving| moving.lives.clone())
            });
            grid = grid.child(self.core_card(
                *index,
                *card,
                lives,
                hybrid,
                interactive,
                &core_temperatures,
                cx,
            ));
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
    /// A logical processor's card at `card`'s bounds, drawn faded and shrunk
    /// toward its centre by its presence (a grid fading out keeps the cards as
    /// they were drawn). A card that `lives` move is drawn inside them: the
    /// card zooms and fades, its caption's two ends follow the card's top
    /// corners and its chart frame stretches to the card, its curve with it.
    #[allow(clippy::too_many_arguments)]
    fn core_card(
        &self,
        index: usize,
        card: cpu_motion::Card,
        lives: Option<cpu_motion::CardLives>,
        hybrid: bool,
        interactive: bool,
        core_temperatures: &HashMap<(u64, u64), f64>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::super::shift::{Anchor, live};
        let p = self.palette;
        let hw = &self.snapshot["hardware"];
        let threads = values(&hw["cpu"]["threads"]);
        let value = &self.snapshot["system"]["cores"][index];
        let header = theme::dimension("coreHeaderHeight");
        let enter_scale = theme::design()["layout"]["coreEnterScale"]
            .as_f64()
            .expect("core enter scale") as f32;
        let bounds = card.bounds;
        let key = format!("system.cores.{index}");
        let eff = hybrid
            && threads
                .get(index)
                .is_some_and(|thread| boolean(thread, "efficient"));
        let color = if eff { p.cpu_e } else { p.cpu };
        let anchored = |transform: &LiveTransform| charts::Live {
            transform: transform.clone(),
            anchor: Rc::new(Cell::new(Point::default())),
        };
        // A card drawn part way in or out (a grid fading out keeps it so).
        let presence = (card.presence < 1.).then(|| {
            LiveTransform::new(LiveTransformValue::zoom(
                enter_scale + (1. - enter_scale) * card.presence,
                card.presence,
            ))
        });
        let whole = lives
            .as_ref()
            .map(|lives| anchored(&lives.card))
            .or_else(|| presence.as_ref().map(anchored));
        let frame = lives.as_ref().map(|lives| anchored(&lives.frame));
        let pushed = whole
            .iter()
            .chain(frame.iter())
            .cloned()
            .collect::<Vec<_>>();
        self.chart_lives.borrow_mut().extend(pushed.iter().cloned());
        let chart = self.chart_view(
            &format!("cpu-{index}"),
            &[Line::new(key, color)],
            Some(Scale::Fixed(100.)),
            (bounds.size.height - header).max(px(1.)),
            Look {
                frame: true,
                ..Look::default()
            },
            cx,
        );
        {
            let mut stack = self.chart_lives.borrow_mut();
            let kept = stack.len() - pushed.len();
            stack.truncate(kept);
        }
        let chart = match frame {
            Some(frame) => {
                live(chart, frame.transform, Anchor::TopLeft, frame.anchor).into_any_element()
            }
            None => chart,
        };
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
        let name = div()
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
                        .text_size(theme::font(10.))
                        .font_weight(FontWeight::BOLD)
                        .line_height(px(15.))
                        .text_center()
                        .child(if eff { "E" } else { "P" }),
                )
            })
            .child(
                div()
                    .text_size(theme::font(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(index.to_string()),
            )
            .when(mhz > 0., |d| {
                d.child(
                    div()
                        .ml(px(2.))
                        .min_w_0()
                        .truncate()
                        .text_size(theme::font(11.))
                        .text_color(p.muted)
                        .child(ghz(mhz)),
                )
            });
        let load = div()
            .flex_none()
            .flex()
            .items_baseline()
            .text_size(theme::font(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .map(|d| match value.as_f64() {
                Some(load) => d.child(format!("{}", load.round())).child(
                    div()
                        .ml(px(1.))
                        .text_size(theme::font(10.5))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(p.muted)
                        .child("%"),
                ),
                None => d.child(missing()),
            });
        let (name, load) = match &lives {
            Some(lives) => (
                live(
                    name,
                    lives.left.clone(),
                    Anchor::TopLeft,
                    Rc::new(Cell::new(Point::default())),
                )
                .into_any_element(),
                live(
                    load,
                    lives.right.clone(),
                    Anchor::TopLeft,
                    Rc::new(Cell::new(Point::default())),
                )
                .into_any_element(),
            ),
            None => (name.into_any_element(), load.into_any_element()),
        };
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
            .child(name)
            .child(load);
        let cell = div()
            .absolute()
            .left(bounds.origin.x)
            .top(bounds.origin.y)
            .w(bounds.size.width)
            .h(bounds.size.height)
            // A card gliding from a larger size draws its frame past its bounds.
            .when(lives.is_none(), |d| d.overflow_hidden())
            .flex()
            .flex_col()
            .child(head)
            .child(chart);
        let cell = if interactive {
            cell.id(("core-cell", index))
                .tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
                .into_any_element()
        } else {
            cell.into_any_element()
        };
        match whole {
            Some(whole) => {
                live(cell, whole.transform, Anchor::Center, whole.anchor).into_any_element()
            }
            None => cell,
        }
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
                                widgets::tooltip(label(description), window, cx)
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
            .text_size(theme::font(12.))
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
            // Separate the description fields with a single space.
            .join(" ");
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
                        .text_size(theme::font(12.))
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

#[cfg(test)]
mod tests {
    use super::cpu_power;
    use serde_json::json;

    #[test]
    fn the_cpu_shows_one_power_figure() {
        let both = json!({"package": 88.4, "cores": 73.1, "graphics": 2.0});
        assert_eq!(
            cpu_power(&both),
            Some((88.4, "performance.cpu.packagePower"))
        );
        let cores = json!({"package": null, "cores": 12.5});
        assert_eq!(cpu_power(&cores), Some((12.5, "performance.cpu.corePower")));
        // A valid zero is a reading; graphics power is not the CPU's.
        assert_eq!(
            cpu_power(&json!({"package": 0.0})),
            Some((0.0, "performance.cpu.packagePower"))
        );
        assert_eq!(cpu_power(&json!({"package": null, "graphics": 3.0})), None);
        assert_eq!(cpu_power(&serde_json::Value::Null), None);
    }
}
