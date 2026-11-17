//! The monitors outside the main window, fed by the desktop's samples: the
//! desktop monitor, a small window of live readings drawn here (its window
//! as the system sees it is `shell::desk`), and the taskbar monitor, whose
//! readings the shell draws on the taskbar (`shell::taskbar`). Each shows
//! the metrics its settings name, in their order.
use super::*;
use crate::settings::{DesktopMonitor, Metric, TaskbarMonitor};
use crate::shell::{TaskbarCell, TaskbarGroup};
use windows::Win32::Foundation::HWND;

/// From this percent or temperature (°C) a reading turns amber, as on the
/// tray icon.
const HIGH: f64 = 80.;
/// The desktop monitor's width, logical pixels.
const WIDTH: f32 = 248.;
/// How far inside the work area's top-right corner it first opens.
const MARGIN: f32 = 24.;
/// Off every display: where the window is laid out before it first shows.
const AWAY: i32 = -32000;

/// A metric's name in settings and texts (`components.metric.*`).
pub(in crate::frontend) fn metric_key(metric: Metric) -> &'static str {
    match metric {
        Metric::Cpu => "cpu",
        Metric::CpuTemperature => "cpuTemperature",
        Metric::Memory => "memory",
        Metric::Gpu => "gpu",
        Metric::GpuTemperature => "gpuTemperature",
        Metric::GpuMemory => "gpuMemory",
        Metric::Disk => "disk",
        Metric::Network => "network",
    }
}

/// One value of a reading.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::frontend) struct Part {
    /// What a rate is (an arrow, read or write); empty for a single value.
    pub mark: String,
    pub text: String,
    /// The widest text the value takes, so that what holds it keeps its
    /// width.
    pub widest: String,
}

/// A metric's reading.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::frontend) struct Gauge {
    pub metric: Metric,
    /// One value, or the two rates of a pair (received and sent; read and
    /// written).
    pub parts: Vec<Part>,
    /// How far a bar fills, 0–1, for a reading with a scale.
    pub fraction: Option<f64>,
    pub high: bool,
}

/// The graphics card the GPU readings follow: the one with the most
/// dedicated memory, the discrete card beside a processor's graphics, so
/// that the readings keep to one card.
fn main_gpu(hardware: &Value) -> Option<&Value> {
    values(&hardware["gpus"])
        .iter()
        .fold(None, |main, gpu| match main {
            Some(main)
                if number(&gpu["info"], "dedicatedTotal")
                    <= number(&main["info"], "dedicatedTotal") =>
            {
                Some(main)
            }
            _ => Some(gpu),
        })
}

/// The readings of `metrics` in `snapshot`; rates in bits when `bits`.
pub(in crate::frontend) fn gauges(snapshot: &Value, metrics: &[Metric], bits: bool) -> Vec<Gauge> {
    let system = &snapshot["system"];
    let hardware = &snapshot["hardware"];
    let missing = label("format.missing");
    let percent = |metric: Metric, value: Option<f64>| {
        let value = value.filter(|value| value.is_finite());
        Gauge {
            metric,
            parts: vec![Part {
                mark: String::new(),
                text: value.map_or_else(|| missing.clone(), |v| format!("{}%", v.round())),
                widest: "100%".into(),
            }],
            fraction: value.map(|v| (v / 100.).clamp(0., 1.)),
            high: value.is_some_and(|v| v.round() >= HIGH),
        }
    };
    let celsius = |metric: Metric, value: Option<f64>| {
        let value = value.filter(|value| value.is_finite() && *value > 0.);
        Gauge {
            metric,
            parts: vec![Part {
                mark: String::new(),
                text: value.map_or_else(
                    || missing.clone(),
                    |v| crate::regional::number!("{v:.0} °C"),
                ),
                widest: crate::regional::number!("{:.0} °C", 100.),
            }],
            fraction: value.map(|v| (v / 100.).clamp(0., 1.)),
            high: value.is_some_and(|v| v.round() >= HIGH),
        }
    };
    let rate = |value: Option<f64>, network: bool| match value {
        None => missing.clone(),
        Some(value) if value < 1. => if network && bits { "0 Kbps" } else { "0 KB/s" }.into(),
        Some(value) if network => theme::network(value, bits),
        Some(value) => theme::bytes(value, true),
    };
    let widest_rate = |network: bool| {
        if network && bits {
            crate::regional::number!("{:.1} Mbps", 999.9)
        } else {
            crate::regional::number!("{:.1} MB/s", 999.9)
        }
    };
    let pair = |metric: Metric, marks: [String; 2], rates: [Option<f64>; 2], network: bool| Gauge {
        metric,
        parts: marks
            .into_iter()
            .zip(rates)
            .map(|(mark, value)| Part {
                mark,
                text: rate(value, network),
                widest: widest_rate(network),
            })
            .collect(),
        fraction: None,
        high: false,
    };
    let sum = |devices: &Value, key: &str| -> Option<f64> {
        let devices = values(devices);
        if devices.is_empty() {
            return None;
        }
        devices
            .iter()
            .try_fold(0., |sum, device| Some(sum + device[key].as_f64()?))
    };
    let gpu = main_gpu(hardware);
    metrics
        .iter()
        .map(|metric| match metric {
            Metric::Cpu => percent(*metric, system["cpu"].as_f64()),
            Metric::CpuTemperature => celsius(*metric, model::cpu_temperature(hardware)),
            Metric::Memory => percent(
                *metric,
                system["memUsed"].as_f64().and_then(|used| {
                    let total = number(system, "memTotal");
                    (total > 0.).then(|| used / total * 100.)
                }),
            ),
            Metric::Gpu => percent(*metric, gpu.and_then(|gpu| gpu["util"].as_f64())),
            Metric::GpuTemperature => {
                celsius(*metric, gpu.and_then(|gpu| gpu["temperature"].as_f64()))
            }
            Metric::GpuMemory => percent(
                *metric,
                gpu.and_then(|gpu| {
                    let total = number(&gpu["info"], "dedicatedTotal");
                    (total > 0.).then(|| number(gpu, "dedicatedUsed") / total * 100.)
                }),
            ),
            Metric::Disk => pair(
                *metric,
                [
                    label("components.metric.read"),
                    label("components.metric.write"),
                ],
                [
                    sum(&hardware["disks"], "read"),
                    sum(&hardware["disks"], "write"),
                ],
                false,
            ),
            Metric::Network => pair(
                *metric,
                ["↓".into(), "↑".into()],
                [sum(&hardware["nics"], "rx"), sum(&hardware["nics"], "tx")],
                true,
            ),
        })
        .collect()
}

/// The taskbar's cells for `gauges`: a single value under the metric's
/// short name, a pair of rates under their marks.
fn taskbar_groups(gauges: &[Gauge]) -> Vec<TaskbarGroup> {
    gauges
        .iter()
        .map(|gauge| match gauge.parts.as_slice() {
            [part] => vec![TaskbarCell {
                label: label(&format!(
                    "components.metric.{}.short",
                    metric_key(gauge.metric)
                )),
                value: part.text.clone(),
                widest: part.widest.clone(),
                high: gauge.high,
            }],
            parts => parts
                .iter()
                .map(|part| TaskbarCell {
                    label: part.mark.clone(),
                    value: part.text.clone(),
                    widest: part.widest.clone(),
                    high: false,
                })
                .collect(),
        })
        .collect()
}

/// The colour of a metric's bar: its device's.
fn metric_color(metric: Metric, p: Palette) -> Hsla {
    match metric {
        Metric::Cpu | Metric::CpuTemperature => p.cpu,
        Metric::Memory => p.memory,
        Metric::Gpu | Metric::GpuTemperature | Metric::GpuMemory => p.gpu,
        Metric::Disk => p.io,
        Metric::Network => p.network,
    }
}

/// The desktop monitor's window, once made.
#[derive(Clone)]
pub(in crate::frontend) struct Open {
    window: AnyWindowHandle,
    view: Entity<DeskMonitor>,
}

/// The monitors the desktop keeps.
#[derive(Default)]
pub(in crate::frontend) struct Monitors {
    desk: Option<Open>,
    /// Metrics chosen on the components page while a settings change was on
    /// its way: per monitor's settings key, its command and the latest
    /// choice, sent once that change has come back.
    queued: Vec<(&'static str, &'static str, Vec<&'static str>)>,
}

pub(in crate::frontend) struct DeskMonitor {
    gauges: Vec<Gauge>,
    settings: DesktopMonitor,
    hwnd: HWND,
    /// The content's height as last laid out, logical pixels.
    measured: Rc<Cell<f32>>,
    /// The height and scale the window was last given; none until it first
    /// showed.
    placed: Option<(f32, f32)>,
}

fn hwnd_of(window: &Window) -> HWND {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => HWND(handle.hwnd.get() as _),
        _ => HWND::default(),
    }
}

impl DeskMonitor {
    fn new(settings: DesktopMonitor, gauges: Vec<Gauge>, window: &mut Window) -> Self {
        let hwnd = hwnd_of(window);
        unsafe {
            use windows::Win32::Graphics::Dwm::{
                DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
            };
            let round = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const round).cast(),
                size_of_val(&round) as u32,
            );
        }
        crate::shell::desk::install(hwnd);
        Self {
            gauges,
            settings,
            hwnd,
            measured: Rc::new(Cell::new(0.)),
            placed: None,
        }
    }

    fn set(
        &mut self,
        settings: DesktopMonitor,
        gauges: Vec<Gauge>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let restyled = settings != self.settings;
        self.settings = settings;
        if gauges != self.gauges {
            self.gauges = gauges;
            cx.notify();
        }
        if restyled && let Some((height, _)) = self.placed {
            self.place(height, window, cx);
        }
    }

    fn set_gauges(&mut self, gauges: Vec<Gauge>, cx: &mut Context<Self>) {
        if gauges != self.gauges {
            self.gauges = gauges;
            cx.notify();
        }
    }

    /// Gives the window `height` logical pixels where it is (or where it
    /// opens, the first time) and its settings.
    fn place(&mut self, height: f32, window: &mut Window, cx: &mut Context<Self>) {
        let scale = window.scale_factor();
        let (width, tall) = (
            (WIDTH * scale).round() as i32,
            (height * scale).round() as i32,
        );
        let (x, y) = match self.placed {
            Some(_) => crate::shell::desk::position(self.hwnd),
            None => crate::shell::desk::origin(
                self.settings.position,
                width,
                tall,
                (MARGIN * scale).round() as i32,
            ),
        };
        crate::shell::desk::show(self.hwnd, &self.settings, x, y, width, tall);
        self.placed = Some((height, scale));
        crate::shell::set_desktop_monitor_shown(true);
        window.bounds_changed(cx);
        window.refresh();
    }

    /// The content was laid out: the window follows its height, and its
    /// scale when it moved to another display.
    fn settle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let measured = self.measured.get();
        if measured <= 0. {
            return;
        }
        let scale = window.scale_factor();
        if self.placed.is_none_or(|(height, placed)| {
            (height - measured).abs() > 0.5 || (placed - scale).abs() > 0.001
        }) {
            self.place(measured, window, cx);
        }
    }

    fn row(&self, gauge: &Gauge, p: Palette) -> AnyElement {
        let name = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_color(p.muted)
            .child(label(&format!(
                "components.metric.{}.name",
                metric_key(gauge.metric)
            )));
        match gauge.parts.as_slice() {
            [part] => {
                let color = if gauge.high {
                    p.heat
                } else {
                    metric_color(gauge.metric, p)
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div().flex().items_center().gap(px(8.)).child(name).child(
                            div()
                                .flex_shrink_0()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if gauge.high { p.heat } else { p.ink })
                                .child(part.text.clone()),
                        ),
                    )
                    .child(
                        div().h(px(4.)).rounded(px(2.)).bg(p.stroke_strong).child(
                            div()
                                .h_full()
                                .w(relative(gauge.fraction.unwrap_or(0.) as f32))
                                .rounded(px(2.))
                                .bg(color),
                        ),
                    )
                    .into_any_element()
            }
            parts => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(name)
                .children(parts.iter().map(|part| {
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(div().text_color(p.muted).child(part.mark.clone()))
                        .child(
                            div()
                                .min_w(theme::font(12.) * 5.2)
                                .text_right()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(part.text.clone()),
                        )
                }))
                .into_any_element(),
        }
    }
}

impl Render for DeskMonitor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let measured = self.measured.clone();
        let owner = cx.entity().downgrade();
        let scale = window.scale_factor();
        let placed = self.placed;
        let rows = self
            .gauges
            .iter()
            .map(|gauge| self.row(gauge, p))
            .collect::<Vec<_>>();
        div()
            .id("desktop-monitor")
            .role(Role::Group)
            .aria_label(label("components.desktop.title"))
            .size_full()
            .overflow_hidden()
            .bg(widgets::solid(p.flyout))
            .text_color(p.ink)
            .text_size(theme::font(12.))
            .line_height(theme::line_height())
            .font_features(theme::tabular())
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(12.))
                    .children(rows)
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                let height = f32::from(bounds.size.height);
                                let moved = placed.is_none_or(|(placed, at)| {
                                    (placed - height).abs() > 0.5 || (at - scale).abs() > 0.001
                                });
                                if (measured.replace(height) - height).abs() > 0.5 || moved {
                                    let owner = owner.clone();
                                    window.defer(cx, move |window, cx| {
                                        let _ =
                                            owner.update(cx, |this, cx| this.settle(window, cx));
                                    });
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
    }
}

impl Desktop {
    fn monitor_settings<T: serde::de::DeserializeOwned + Default>(&self, key: &str) -> T {
        serde_json::from_value(self.data["settings"][key].clone()).unwrap_or_default()
    }

    fn bits(&self) -> bool {
        self.prefs.get::<String>("netUnit", "bytes".into()) == "bits"
    }

    /// Shows, rearranges or closes the monitors as the settings now say.
    pub(in crate::frontend) fn sync_monitors(&mut self, cx: &mut Context<Self>) {
        if !self.data.contains_key("settings") {
            return;
        }
        let desktop: DesktopMonitor = self.monitor_settings("desktopMonitor");
        let wanted = desktop.enabled && !desktop.metrics.is_empty();
        let gauges = gauges(&self.snapshot, &desktop.metrics, self.bits());
        match (wanted, self.monitors.desk.clone()) {
            (true, Some(Open { window, view })) => {
                let _ = window.update(cx, |_, window, cx| {
                    view.update(cx, |view, cx| view.set(desktop, gauges, window, cx))
                });
            }
            (true, None) => self.open_desk_monitor(desktop, gauges, cx),
            (false, Some(Open { window, .. })) => {
                self.monitors.desk = None;
                crate::shell::set_desktop_monitor_shown(false);
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
            (false, None) => {}
        }
        self.refresh_taskbar_monitor();
    }

    fn open_desk_monitor(
        &mut self,
        settings: DesktopMonitor,
        gauges: Vec<Gauge>,
        cx: &mut Context<Self>,
    ) {
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(label("components.desktop.title").into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            focus: false,
            show: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|_| DeskMonitor::new(settings, gauges, window))
        });
        match opened {
            Ok((window, view)) => {
                // Laid out off screen first; it shows at its place once its
                // height is known.
                let _ = window.update(cx, |_, window, cx| {
                    view.update(cx, |view, cx| {
                        crate::shell::desk::show(
                            view.hwnd,
                            &view.settings,
                            AWAY,
                            AWAY,
                            (WIDTH * window.scale_factor()).round() as i32,
                            (120. * window.scale_factor()).round() as i32,
                        );
                        window.bounds_changed(cx);
                        window.refresh();
                    })
                });
                self.monitors.desk = Some(Open { window, view });
            }
            Err(error) => argus_core::diagnostics::warn(
                "desktop monitor creation",
                argus_core::diagnostics::error_chain(error.as_ref()),
            ),
        }
    }

    fn refresh_taskbar_monitor(&self) {
        let taskbar: TaskbarMonitor = self.monitor_settings("taskbarMonitor");
        if taskbar.enabled {
            crate::shell::set_taskbar_readings(taskbar_groups(&gauges(
                &self.snapshot,
                &taskbar.metrics,
                self.bits(),
            )));
        }
    }

    /// A monitor's metrics chosen on the components page: shown at once,
    /// in the page and by the monitor, and saved. Settings changes go one
    /// at a time; a choice made while one is on its way is saved after it,
    /// the latest choice winning, so that quick clicks all count.
    pub(in crate::frontend) fn choose_metrics(
        &mut self,
        key: &'static str,
        command: &'static str,
        metrics: Vec<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(monitor) = self
            .data
            .get_mut("settings")
            .and_then(|settings| settings.get_mut(key))
        {
            monitor["metrics"] = json!(metrics);
        }
        self.monitors.queued.retain(|(queued, ..)| *queued != key);
        if self
            .pending
            .values()
            .any(|pending| pending.cache.as_deref() == Some("settings"))
        {
            self.monitors.queued.push((key, command, metrics));
        } else {
            self.request(
                command,
                json!({ "metrics": metrics }),
                Some("settings"),
                window,
                cx,
            );
        }
        self.sync_monitors(cx);
        cx.notify();
    }

    /// A settings change came back: choices made meanwhile stay shown over
    /// the settings it returned, and the next of them is sent.
    pub(in crate::frontend) fn send_queued_metrics(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.monitors.queued.is_empty() {
            return;
        }
        for (key, _, metrics) in &self.monitors.queued {
            if let Some(monitor) = self
                .data
                .get_mut("settings")
                .and_then(|settings| settings.get_mut(*key))
            {
                monitor["metrics"] = json!(metrics);
            }
        }
        let (_, command, metrics) = self.monitors.queued.remove(0);
        self.request(
            command,
            json!({ "metrics": metrics }),
            Some("settings"),
            window,
            cx,
        );
    }

    /// The theme or language changed: the desktop monitor draws again, and
    /// the taskbar monitor's names follow.
    pub(in crate::frontend) fn restyle_monitors(&mut self, cx: &mut Context<Self>) {
        if let Some(Open { view, .. }) = self.monitors.desk.clone() {
            view.update(cx, |_, cx| cx.notify());
        }
        self.refresh_taskbar_monitor();
    }

    /// A new sample: the monitors that show follow it.
    pub(in crate::frontend) fn refresh_monitors(&mut self, cx: &mut Context<Self>) {
        if let Some(Open { view, .. }) = self.monitors.desk.clone() {
            let metrics = view.read(cx).settings.metrics.clone();
            let gauges = gauges(&self.snapshot, &metrics, self.bits());
            view.update(cx, |view, cx| view.set_gauges(gauges, cx));
        }
        self.refresh_taskbar_monitor();
    }

    /// The desktop monitor was dragged, or an item of its menu chosen.
    pub(in crate::frontend) fn desktop_monitor_event(
        &mut self,
        event: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current: DesktopMonitor = self.monitor_settings("desktopMonitor");
        let change = if let Some(position) = event["moved"].as_array() {
            json!({ "position": position })
        } else {
            match event["menu"].as_str() {
                Some("topmost") => json!({ "topmost": !current.topmost }),
                Some("locked") => json!({ "locked": !current.locked }),
                Some("hide") => json!({ "enabled": false }),
                Some("settings") => {
                    self.act(Action::Page("components".into()), window, cx);
                    crate::shell::show_main(&self.host);
                    return;
                }
                _ => return,
            }
        };
        self.request("set_desktop_monitor", change, Some("settings"), window, cx);
    }

    /// What the tests read of the monitors (`inspectLayout.monitors`).
    pub(in crate::frontend) fn monitors_test_state(&self, cx: &App) -> Value {
        let desk = self.monitors.desk.as_ref().map(|open| {
            let view = open.view.read(cx);
            let (x, y) = crate::shell::desk::position(view.hwnd);
            json!({
                "visible": unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(view.hwnd).as_bool() },
                "placed": view.placed.is_some(),
                "position": [x, y],
                "rows": view.gauges.iter().map(|gauge| json!({
                    "metric": metric_key(gauge.metric),
                    "values": gauge.parts.iter().map(|part| part.text.clone()).collect::<Vec<_>>(),
                    "high": gauge.high,
                })).collect::<Vec<_>>(),
            })
        });
        json!({
            "desktop": desk,
            "taskbar": crate::shell::taskbar_monitor_rect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Gauge, gauges, main_gpu, taskbar_groups};
    use crate::settings::Metric;
    use serde_json::json;

    fn snapshot() -> serde_json::Value {
        json!({
            "system": {"cpu": 83.6, "memUsed": 4294967296u64, "memTotal": 17179869184u64},
            "hardware": {
                "cpuSensors": {"packageCelsius": 61.2},
                "gpus": [
                    {"info": {"dedicatedTotal": 134217728u64}, "util": 90.0, "temperature": 50.0, "dedicatedUsed": 0},
                    {"info": {"dedicatedTotal": 8589934592u64}, "util": 12.0, "temperature": 79.6, "dedicatedUsed": 2147483648u64}
                ],
                "disks": [{"read": 2097152.0, "write": 0.0}, {"read": 1048576.0, "write": 512.0}],
                "nics": [{"rx": 2048.0, "tx": 0.5}]
            }
        })
    }

    fn texts(gauge: &Gauge) -> Vec<String> {
        gauge.parts.iter().map(|part| part.text.clone()).collect()
    }

    #[test]
    fn readings_follow_the_sample_and_turn_high_from_80() {
        let all = gauges(&snapshot(), &Metric::ALL, false);
        assert_eq!(all.len(), Metric::ALL.len());
        let by = |metric| all.iter().find(|gauge| gauge.metric == metric).unwrap();
        assert_eq!(texts(by(Metric::Cpu)), ["84%"]);
        assert!(by(Metric::Cpu).high);
        assert_eq!(by(Metric::Memory).fraction, Some(0.25));
        assert!(!by(Metric::Memory).high);
        // The GPU readings follow the card with the most dedicated memory.
        assert_eq!(texts(by(Metric::Gpu)), ["12%"]);
        assert_eq!(texts(by(Metric::GpuMemory)), ["25%"]);
        assert!(by(Metric::GpuTemperature).high);
        assert!((by(Metric::CpuTemperature).fraction.unwrap() - 0.612).abs() < 1e-9);
        // Rates add up over every disk and adapter; under a byte is idle.
        assert_eq!(texts(by(Metric::Disk))[1], "1 KB/s");
        assert_eq!(texts(by(Metric::Network)), ["2 KB/s", "0 KB/s"]);
        assert_eq!(
            texts(&gauges(&snapshot(), &[Metric::Network], true)[0])[1],
            "0 Kbps"
        );
    }

    #[test]
    fn readings_without_data_stay_unknown() {
        let empty = gauges(&serde_json::Value::Null, &Metric::ALL, false);
        assert!(
            empty
                .iter()
                .all(|gauge| gauge.fraction.is_none() && !gauge.high)
        );
        let missing = empty[0].parts[0].text.clone();
        assert!(
            empty
                .iter()
                .all(|gauge| gauge.parts.iter().all(|part| part.text == missing))
        );
        assert!(main_gpu(&json!({})).is_none());
    }

    #[test]
    fn the_taskbar_shows_single_values_by_name_and_rates_by_mark() {
        let groups = taskbar_groups(&gauges(&snapshot(), &[Metric::Cpu, Metric::Network], false));
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 1);
        assert!(groups[0][0].high);
        assert_eq!(groups[0][0].widest, "100%");
        assert_eq!(
            groups[1]
                .iter()
                .map(|cell| cell.label.as_str())
                .collect::<Vec<_>>(),
            ["↓", "↑"]
        );
    }
}
