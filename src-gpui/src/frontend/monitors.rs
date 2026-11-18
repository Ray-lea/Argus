//! The monitors outside the main window, fed by the desktop's samples: the
//! desktop monitor, a small window of live readings drawn here (its window
//! as the system sees it is `shell::desk`), and the taskbar monitor, whose
//! readings the shell draws on the taskbar (`shell::taskbar`). Each shows
//! the readings its settings name, in their order: a metric of the
//! default device, or of one GPU, disk or network adapter of several.
use super::pages::components::{TASKBAR_STYLES, TaskbarChoice};
use super::pages::performance::rail;
use super::*;
use crate::settings::{DesktopMonitor, Metric, Reading, TaskbarMonitor, TaskbarStyle};
use crate::shell::{TaskbarCell, TaskbarGroup, TaskbarTone};
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
        Metric::CpuClock => "cpuClock",
        Metric::CpuPower => "cpuPower",
        Metric::Memory => "memory",
        Metric::MemoryUsed => "memoryUsed",
        Metric::Gpu => "gpu",
        Metric::GpuTemperature => "gpuTemperature",
        Metric::GpuMemory => "gpuMemory",
        Metric::GpuClock => "gpuClock",
        Metric::GpuPower => "gpuPower",
        Metric::GpuFan => "gpuFan",
        Metric::Disk => "disk",
        Metric::DiskActive => "diskActive",
        Metric::DiskTemperature => "diskTemperature",
        Metric::Network => "network",
        Metric::Battery => "battery",
        Metric::Uptime => "uptime",
    }
}

/// The kind of device a metric reads when it reads one of several, as the
/// performance page names it (`rail::stable_key`).
pub(in crate::frontend) fn device_kind(metric: Metric) -> Option<&'static str> {
    match metric {
        Metric::Gpu
        | Metric::GpuTemperature
        | Metric::GpuMemory
        | Metric::GpuClock
        | Metric::GpuPower
        | Metric::GpuFan => Some("gpu"),
        Metric::Disk | Metric::DiskActive | Metric::DiskTemperature => Some("disk"),
        Metric::Network => Some("network"),
        _ => None,
    }
}

/// The metrics that show two rates rather than one value.
fn paired(metric: Metric) -> bool {
    matches!(metric, Metric::Disk | Metric::Network)
}

/// A device the monitors can follow, as the sample lists it.
#[derive(Clone, Debug)]
pub(in crate::frontend) struct Device<'a> {
    /// What its readings are saved with: what it reports of itself, which
    /// outlasts restarts and replugging (`rail::stable_key`).
    pub key: String,
    /// Its name in the components page: its drive letters, its
    /// connection's name, its GPU's name.
    pub title: String,
    /// What tells it apart further: model and size, adapter, memory.
    pub detail: String,
    /// Its name before its readings on the desktop monitor.
    pub name: String,
    /// Before its readings on the taskbar: `C:`, `GPU2`, `WLAN`.
    pub short: String,
    pub sample: &'a Value,
}

/// A disk's drive letters, or its number when it has none.
fn disk_title(disk: &Value) -> String {
    let volumes = values(&disk["info"]["volumes"])
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if volumes.is_empty() {
        tr!(
            "components.metrics.diskNumber",
            index = disk["info"]["index"].as_u64().unwrap_or(0)
        )
    } else {
        volumes.join(" ")
    }
}

/// A GPU's name without its maker's marks, where a model number stays:
/// "RTX 4060 Laptop GPU", "UHD Graphics 770", "Radeon Graphics".
fn gpu_name(name: &str) -> String {
    let mut name = name
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace("(tm)", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for maker in ["NVIDIA ", "GeForce ", "AMD ", "Radeon ", "Intel "] {
        if let Some(rest) = name.strip_prefix(maker)
            && rest.chars().any(|c| c.is_ascii_digit())
        {
            name = rest.to_owned();
        }
    }
    name
}

/// At most `count` characters of `text`, an ellipsis standing for the rest.
fn clipped(text: &str, count: usize) -> String {
    if text.chars().count() <= count {
        text.to_owned()
    } else {
        text.chars()
            .take(count.saturating_sub(1))
            .chain(Some('…'))
            .collect()
    }
}

/// The devices of `kind` in the sample, in its order.
pub(in crate::frontend) fn devices<'a>(snapshot: &'a Value, kind: &str) -> Vec<Device<'a>> {
    let list = values(
        &snapshot["hardware"][match kind {
            "gpu" => "gpus",
            "disk" => "disks",
            _ => "nics",
        }],
    );
    let keys = rail::distinct(
        list.iter()
            .map(|sample| rail::stable_key(kind, &sample["info"]))
            .collect(),
    );
    let many = list.len() > 1;
    list.iter()
        .zip(keys)
        .enumerate()
        .map(|(index, (sample, key))| {
            let info = &sample["info"];
            match kind {
                "gpu" => Device {
                    key,
                    title: text(info, "name").to_owned(),
                    detail: tr!(
                        "components.metrics.gpuMemory",
                        memory = theme::bytes(number(info, "dedicatedTotal"), false)
                    ),
                    name: gpu_name(text(info, "name")),
                    short: if many {
                        format!("GPU{}", index + 1)
                    } else {
                        "GPU".into()
                    },
                    sample,
                },
                "disk" => {
                    let title = disk_title(sample);
                    let size = number(info, "size");
                    Device {
                        key,
                        detail: [
                            text(info, "model").trim().to_owned(),
                            if size > 0. {
                                theme::bytes(size, false)
                            } else {
                                String::new()
                            },
                        ]
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · "),
                        name: title.clone(),
                        short: values(&info["volumes"])
                            .first()
                            .and_then(Value::as_str)
                            .map_or_else(
                                || format!("#{}", info["index"].as_u64().unwrap_or(0)),
                                str::to_owned,
                            ),
                        title,
                        sample,
                    }
                }
                _ => Device {
                    key,
                    title: text(info, "name").to_owned(),
                    detail: text(info, "description").to_owned(),
                    name: text(info, "name").to_owned(),
                    short: clipped(text(info, "name"), 6),
                    sample,
                },
            }
        })
        .collect()
}

/// The name a device's key gives it while it is away: a disk's model, a
/// connection's name, a GPU's name.
pub(in crate::frontend) fn absent_name(key: &str) -> String {
    let key = key.rsplit_once('#').map_or(key, |(key, _)| key);
    let Some((kind, rest)) = key.split_once(':') else {
        return key.to_owned();
    };
    let parts = rest.split('|').collect::<Vec<_>>();
    let name = match kind {
        "disk" | "nic" => parts.get(1),
        _ => parts.first(),
    };
    name.filter(|name| !name.is_empty())
        .map_or_else(|| key.to_owned(), |name| (*name).to_owned())
}

/// Where a metric's row sits on the components page: by device kind.
fn section(metric: Metric) -> usize {
    match metric {
        Metric::Cpu | Metric::CpuTemperature | Metric::CpuClock | Metric::CpuPower => 0,
        Metric::Memory | Metric::MemoryUsed => 1,
        Metric::Battery | Metric::Uptime => 5,
        metric => match device_kind(metric) {
            Some("gpu") => 2,
            Some("disk") => 3,
            _ => 4,
        },
    }
}

/// `readings` in the order the monitors show them: by device kind as the
/// components page offers them; within a kind the default device's first,
/// then each device's in the sample's order, those of a device that is away
/// last; each device's metrics in the order offered.
pub(in crate::frontend) fn arrange(snapshot: &Value, mut readings: Vec<Reading>) -> Vec<Reading> {
    let lists = ["gpu", "disk", "network"].map(|kind| (kind, devices(snapshot, kind)));
    let rank = |reading: &Reading| match (&reading.device, device_kind(reading.metric)) {
        (Some(key), Some(kind)) => lists
            .iter()
            .find(|(listed, _)| *listed == kind)
            .and_then(|(_, list)| list.iter().position(|device| device.key == *key))
            .map_or(usize::MAX, |position| position + 1),
        _ => 0,
    };
    let order = |metric: Metric| Metric::ALL.iter().position(|offered| *offered == metric);
    readings.sort_by_key(|reading| {
        (
            section(reading.metric),
            rank(reading),
            order(reading.metric),
        )
    });
    readings
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

/// A reading as the monitors show it.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::frontend) struct Gauge {
    pub reading: Reading,
    /// Its name on the desktop monitor: the metric's, or the device's with
    /// the metric's.
    pub name: String,
    /// Before a single value on the taskbar, and before the marks of a
    /// device's rates there.
    pub short: String,
    /// One value, or the two rates of a pair (received and sent; read and
    /// written).
    pub parts: Vec<Part>,
    /// How far its bar fills, 0–1; none while the reading is unknown.
    pub fraction: Option<f64>,
    /// The reading has a scale, and a bar on the desktop monitor (shown
    /// empty while it is unknown, so that the window keeps its height).
    pub bar: bool,
    pub high: bool,
    /// Where its recent values are kept, for a curve.
    pub trend: Option<Trend>,
}

/// The graphics card the GPU readings without a device follow: the one
/// with the most dedicated memory, the discrete card beside a processor's
/// graphics, so that the readings keep to one card.
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

/// The key of the GPU readings without a device follow, among `gpus`.
pub(in crate::frontend) fn main_gpu_key(snapshot: &Value, gpus: &[Device]) -> Option<String> {
    let main = main_gpu(&snapshot["hardware"])?;
    gpus.iter()
        .find(|gpu| std::ptr::eq(gpu.sample, main))
        .map(|gpu| gpu.key.clone())
}

/// What a reading shows, before its names.
struct Shown {
    parts: Vec<Part>,
    fraction: Option<f64>,
    bar: bool,
    high: bool,
}

/// The readings `readings` name in `snapshot`; rates in bits when `bits`.
pub(in crate::frontend) fn gauges(
    snapshot: &Value,
    readings: &[Reading],
    bits: bool,
) -> Vec<Gauge> {
    let system = &snapshot["system"];
    let hardware = &snapshot["hardware"];
    let missing = label("format.missing");
    let finite = |value: Option<f64>| value.filter(|value| value.is_finite());
    // One value; with a bar when the reading has a scale.
    let single =
        |text: Option<String>, widest: String, fraction: Option<f64>, bar: bool, high: bool| {
            Shown {
                parts: vec![Part {
                    mark: String::new(),
                    text: text.unwrap_or_else(|| missing.clone()),
                    widest,
                }],
                fraction: fraction.map(|fraction| fraction.clamp(0., 1.)),
                bar,
                high,
            }
        };
    let percent = |value: Option<f64>| {
        let value = finite(value);
        single(
            value.map(|v| format!("{}%", v.round())),
            "100%".into(),
            value.map(|v| v / 100.),
            true,
            value.is_some_and(|v| v.round() >= HIGH),
        )
    };
    let celsius = |value: Option<f64>, high: f64| {
        let value = finite(value).filter(|value| *value > 0.);
        single(
            value.map(|v| crate::regional::number!("{v:.0} °C")),
            crate::regional::number!("{:.0} °C", 100.),
            value.map(|v| v / 100.),
            true,
            value.is_some_and(|v| v.round() >= high),
        )
    };
    let gigahertz = |mhz: Option<f64>| {
        let mhz = finite(mhz).filter(|mhz| *mhz > 0.);
        single(
            mhz.map(|mhz| crate::regional::number!("{:.2} GHz", mhz / 1000.)),
            crate::regional::number!("{:.2} GHz", 8.88),
            None,
            false,
            false,
        )
    };
    let watts = |value: Option<f64>| {
        let value = finite(value).filter(|value| *value >= 0.);
        single(
            value.map(|v| format!("{} W", charts::watts(v))),
            format!("{} W", charts::watts(888.)),
            None,
            false,
            false,
        )
    };
    let gigabytes = |bytes: f64| crate::regional::number!("{:.1} GB", bytes / 1073741824.);
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
    let pair = |marks: [String; 2], rates: [Option<f64>; 2], network: bool| Shown {
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
        bar: false,
        high: false,
    };
    // A rate of every device listed; unknown with none, or one unknown.
    let sum = |list: &[&Value], key: &str| -> Option<f64> {
        if list.is_empty() {
            return None;
        }
        list.iter()
            .try_fold(0., |sum, device| Some(sum + device[key].as_f64()?))
    };
    let gpus = devices(snapshot, "gpu");
    let disks = devices(snapshot, "disk");
    let nics = devices(snapshot, "network");
    let all_disks = values(&hardware["disks"]).iter().collect::<Vec<_>>();
    let all_nics = values(&hardware["nics"]).iter().collect::<Vec<_>>();
    readings
        .iter()
        .map(|reading| {
            let metric = reading.metric;
            let key = metric_key(metric);
            // The device named, if the metric reads one: found in the
            // sample, or away.
            let device =
                reading
                    .device
                    .as_deref()
                    .zip(device_kind(metric))
                    .map(|(wanted, kind)| {
                        let list = match kind {
                            "gpu" => &gpus,
                            "disk" => &disks,
                            _ => &nics,
                        };
                        (wanted, list.iter().find(|device| device.key == wanted))
                    });
            let (name, short) = match &device {
                // A pair of rates goes by its marks alone.
                None if paired(metric) => (
                    label(&format!("components.metric.{key}.name")),
                    String::new(),
                ),
                None => (
                    label(&format!("components.metric.{key}.name")),
                    label(&format!("components.metric.{key}.short")),
                ),
                Some((wanted, found)) => {
                    let device_name = found.map_or_else(|| absent_name(wanted), |d| d.name.clone());
                    let name = if paired(metric) {
                        device_name
                    } else {
                        tr!(
                            "components.metric.onDevice",
                            device = device_name,
                            metric = label(&format!("components.metric.{key}.option"))
                        )
                    };
                    let short = found.map(|d| d.short.clone()).unwrap_or_default();
                    let short = if short.is_empty() && !paired(metric) {
                        label(&format!("components.metric.{key}.short"))
                    } else {
                        short
                    };
                    (name, short)
                }
            };
            let gpu = match &device {
                None => main_gpu(hardware),
                Some((_, found)) => found.map(|device| device.sample),
            };
            let (disk_list, nic_list): (Vec<&Value>, Vec<&Value>) = match &device {
                None => (all_disks.clone(), all_nics.clone()),
                Some((_, found)) => {
                    let one = found
                        .map(|device| device.sample)
                        .into_iter()
                        .collect::<Vec<_>>();
                    (one.clone(), one)
                }
            };
            let hottest = disk_list
                .iter()
                .filter(|disk| disk["temperature"].as_f64().is_some_and(|t| t > 0.))
                .max_by(|a, b| number(a, "temperature").total_cmp(&number(b, "temperature")));
            let shown = match metric {
                Metric::Cpu => percent(system["cpu"].as_f64()),
                Metric::CpuTemperature => celsius(model::cpu_temperature(hardware), HIGH),
                Metric::CpuClock => gigahertz(hardware["cpuMhz"].as_f64()),
                Metric::CpuPower => watts(
                    hardware["power"]["package"]
                        .as_f64()
                        .or_else(|| hardware["power"]["cores"].as_f64()),
                ),
                Metric::Memory => percent(system["memUsed"].as_f64().and_then(|used| {
                    let total = number(system, "memTotal");
                    (total > 0.).then(|| used / total * 100.)
                })),
                Metric::MemoryUsed => {
                    let total = number(system, "memTotal");
                    let used = finite(system["memUsed"].as_f64()).filter(|_| total > 0.);
                    single(
                        used.map(gigabytes),
                        // Used memory never shows more digits than the total.
                        gigabytes(total.max(1073741824. * 8.88)),
                        used.map(|used| used / total),
                        true,
                        used.is_some_and(|used| (used / total * 100.).round() >= HIGH),
                    )
                }
                Metric::Gpu => percent(gpu.and_then(|gpu| gpu["util"].as_f64())),
                Metric::GpuTemperature => {
                    celsius(gpu.and_then(|gpu| gpu["temperature"].as_f64()), HIGH)
                }
                Metric::GpuMemory => percent(gpu.and_then(|gpu| {
                    let total = number(&gpu["info"], "dedicatedTotal");
                    (total > 0.).then(|| number(gpu, "dedicatedUsed") / total * 100.)
                })),
                Metric::GpuClock => gigahertz(gpu.and_then(|gpu| gpu["clockMhz"].as_f64())),
                Metric::GpuPower => match gpu {
                    // The board's power where the driver measures it, as the
                    // performance page shows it; otherwise the share of its
                    // budget.
                    Some(gpu) if gpu["boardPower"].is_null() && gpu["power"].as_f64().is_some() => {
                        let share = finite(gpu["power"].as_f64()).filter(|share| *share > 0.);
                        single(
                            share.map(|share| crate::regional::number!("{share:.0}% TDP")),
                            crate::regional::number!("{:.0}% TDP", 100.),
                            None,
                            false,
                            false,
                        )
                    }
                    gpu => watts(gpu.and_then(|gpu| gpu["boardPower"].as_f64())),
                },
                Metric::GpuFan => {
                    let rpm = gpu
                        .and_then(|gpu| gpu["fanRpm"].as_u64())
                        .filter(|rpm| *rpm > 0);
                    single(
                        rpm.map(|rpm| format!("{rpm} RPM")),
                        "8888 RPM".into(),
                        None,
                        false,
                        false,
                    )
                }
                Metric::Disk => pair(
                    [
                        label("components.metric.read"),
                        label("components.metric.write"),
                    ],
                    [sum(&disk_list, "read"), sum(&disk_list, "write")],
                    false,
                ),
                // The busiest disk's.
                Metric::DiskActive => percent(
                    disk_list
                        .iter()
                        .filter_map(|disk| disk["active"].as_f64())
                        .reduce(f64::max),
                ),
                // The hottest disk's, amber from the warning temperature it
                // reports, or from 70 °C.
                Metric::DiskTemperature => celsius(
                    hottest.and_then(|disk| disk["temperature"].as_f64()),
                    hottest
                        .and_then(|disk| disk["temperatureWarning"].as_f64())
                        .filter(|warning| *warning > 0.)
                        .unwrap_or(70.),
                ),
                Metric::Network => pair(
                    ["↓".into(), "↑".into()],
                    [sum(&nic_list, "rx"), sum(&nic_list, "tx")],
                    true,
                ),
                // Low is what needs attention: amber at 20 % and below, on
                // battery power.
                Metric::Battery => {
                    let battery = values(&hardware["batteries"]).first();
                    let charge = battery.and_then(|battery| finite(battery["percent"].as_f64()));
                    let plugged = battery.is_some_and(|battery| boolean(battery, "pluggedIn"));
                    single(
                        charge.map(|v| format!("{}%", v.round())),
                        "100%".into(),
                        charge.map(|v| v / 100.),
                        true,
                        charge.is_some_and(|v| v.round() <= 20. && !plugged),
                    )
                }
                Metric::Uptime => single(
                    system["uptimeMs"]
                        .as_f64()
                        .filter(|ms| *ms > 0.)
                        .map(|ms| super::layout::status_uptime(ms / 1000.)),
                    super::layout::status_uptime(88. * 86400. + 88. * 3600.),
                    None,
                    false,
                    false,
                ),
            };
            // Where the timeline keeps the reading's recent values.
            let of = |sample: &Value, field: &str, kind: &str| {
                let identity = if kind == "disks" {
                    sample["info"]["index"].to_string()
                } else {
                    text(&sample["info"], "id").to_owned()
                };
                format!("hardware.{kind}.{identity}.{field}")
            };
            let gpu_trend = |field: &str, scale: Scale| {
                gpu.map(|gpu| Trend::new(vec![of(gpu, field, "gpus")], Combine::Sum, scale))
            };
            let trend = match metric {
                Metric::Cpu => Some(Trend::one("system.cpu", Scale::Percent)),
                Metric::CpuTemperature => {
                    Some(Trend::one(model::CPU_TEMPERATURE_SERIES, Scale::Percent))
                }
                Metric::CpuClock => Some(Trend::one("hardware.cpuMhz", Scale::Span)),
                Metric::CpuPower => Some(Trend::one(
                    if hardware["power"]["package"].is_number() {
                        "hardware.power.package"
                    } else {
                        "hardware.power.cores"
                    },
                    Scale::Span,
                )),
                Metric::Memory | Metric::MemoryUsed => Some(Trend::one(
                    "system.memUsed",
                    Scale::Of(number(system, "memTotal")),
                )),
                Metric::Gpu => gpu_trend("util", Scale::Percent),
                Metric::GpuTemperature => gpu_trend("temperature", Scale::Percent),
                Metric::GpuMemory => gpu.and_then(|gpu| {
                    gpu_trend(
                        "dedicatedUsed",
                        Scale::Of(number(&gpu["info"], "dedicatedTotal")),
                    )
                }),
                Metric::GpuClock => gpu_trend("clockMhz", Scale::Span),
                Metric::GpuPower => match gpu {
                    Some(gpu) if gpu["boardPower"].is_null() => gpu_trend("power", Scale::Percent),
                    _ => gpu_trend("boardPower", Scale::Span),
                },
                Metric::GpuFan => gpu_trend("fanRpm", Scale::Span),
                Metric::Disk => Some(match &device {
                    None => Trend::one("totals.disk", Scale::Peak(RATE_FLOOR)),
                    Some(_) => Trend::new(
                        disk_list
                            .iter()
                            .flat_map(|disk| {
                                [of(disk, "read", "disks"), of(disk, "write", "disks")]
                            })
                            .collect(),
                        Combine::Sum,
                        Scale::Peak(RATE_FLOOR),
                    ),
                }),
                Metric::DiskActive => Some(Trend::new(
                    disk_list
                        .iter()
                        .map(|disk| of(disk, "active", "disks"))
                        .collect(),
                    Combine::Max,
                    Scale::Percent,
                )),
                Metric::DiskTemperature => hottest.map(|disk| {
                    Trend::new(
                        vec![of(disk, "temperature", "disks")],
                        Combine::Sum,
                        Scale::Percent,
                    )
                }),
                Metric::Network => Some(match &device {
                    None => Trend::one("totals.network", Scale::Peak(RATE_FLOOR)),
                    Some(_) => Trend::new(
                        nic_list
                            .iter()
                            .flat_map(|nic| [of(nic, "rx", "nics"), of(nic, "tx", "nics")])
                            .collect(),
                        Combine::Sum,
                        Scale::Peak(RATE_FLOOR),
                    ),
                }),
                Metric::Battery => values(&hardware["batteries"]).first().map(|battery| {
                    Trend::new(
                        vec![of(battery, "percent", "batteries")],
                        Combine::Sum,
                        Scale::Percent,
                    )
                }),
                Metric::Uptime => None,
            };
            Gauge {
                reading: reading.clone(),
                name,
                short,
                parts: shown.parts,
                fraction: shown.fraction,
                bar: shown.bar,
                high: shown.high,
                trend,
            }
        })
        .collect()
}

/// How a trend's series add up at each sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::frontend) enum Combine {
    Sum,
    Max,
}

/// What a trend's values are measured against: a hundred percent (or
/// degrees), a total, the largest value it shows but no less than a floor
/// (so that an idle rate stays low), or the span between its lowest and
/// highest values (a clock, a power or a fan, which seldom drop to nothing:
/// measured from zero their curves would lie flat along the top).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::frontend) enum Scale {
    Percent,
    Of(f64),
    Peak(f64),
    Span,
}

/// Where the timeline keeps a reading's recent values, for its curve on the
/// taskbar: one series, or several added up or the largest of them taken.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::frontend) struct Trend {
    pub keys: Vec<String>,
    pub combine: Combine,
    pub scale: Scale,
}

impl Trend {
    fn new(keys: Vec<String>, combine: Combine, scale: Scale) -> Self {
        Self {
            keys,
            combine,
            scale,
        }
    }

    fn one(key: &str, scale: Scale) -> Self {
        Self::new(vec![key.into()], Combine::Sum, scale)
    }
}

/// An idle disk's or adapter's curve stays low below this rate, 1 MB/s.
const RATE_FLOOR: f64 = 1048576.;
/// How many samples a curve shows.
const CURVE: usize = 40;

/// `trend`'s last values, 0–1 and oldest first; an unknown one repeats the
/// one before it. Empty when the timeline keeps none of its series.
fn history(timeline: &model::Timeline, trend: &Trend) -> Vec<f32> {
    // Newest first, so that series of different lengths line up at the end.
    let series = trend
        .keys
        .iter()
        .filter_map(|key| timeline.samples.get(key))
        .map(|list| {
            list.iter()
                .rev()
                .take(CURVE)
                .map(|(_, value)| *value)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let length = series.iter().map(Vec::len).max().unwrap_or(0);
    let mut values = (0..length)
        .map(|index| {
            let known = series
                .iter()
                .filter_map(|list| list.get(index).copied().flatten());
            match trend.combine {
                Combine::Sum => known.reduce(|a, b| a + b),
                Combine::Max => known.reduce(f64::max),
            }
        })
        .collect::<Vec<_>>();
    values.reverse();
    let known = values.iter().flatten().copied();
    let (low, high) = known
        .clone()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), v| {
            (low.min(v), high.max(v))
        });
    // Where 0 and 1 of the curve lie.
    let (bottom, top) = match trend.scale {
        Scale::Percent => (0., 100.),
        Scale::Of(total) => (0., total),
        Scale::Peak(floor) => (0., known.fold(floor, f64::max)),
        // A span too small to see (a fixed clock) lies flat in the middle.
        Scale::Span if high - low > high.abs() * 0.01 => {
            let margin = (high - low) * 0.25;
            (low - margin, high + margin)
        }
        Scale::Span => (low - 1., low + 1.),
    };
    if top.is_nan() || bottom.is_nan() || top <= bottom {
        return Vec::new();
    }
    let mut last = None;
    values
        .iter()
        .map(|value| {
            if value.is_some() {
                last = *value;
            }
            last.map_or(0., |last| ((last - bottom) / (top - bottom)).clamp(0., 1.)) as f32
        })
        .collect()
}

/// The colour a metric's readings take on the taskbar: its device's.
fn tone(metric: Metric) -> TaskbarTone {
    match metric {
        Metric::Cpu | Metric::CpuTemperature | Metric::CpuClock | Metric::CpuPower => {
            TaskbarTone::Cpu
        }
        Metric::Memory | Metric::MemoryUsed => TaskbarTone::Memory,
        Metric::Gpu
        | Metric::GpuTemperature
        | Metric::GpuMemory
        | Metric::GpuClock
        | Metric::GpuPower
        | Metric::GpuFan => TaskbarTone::Gpu,
        Metric::Disk | Metric::DiskActive | Metric::DiskTemperature => TaskbarTone::Disk,
        Metric::Network => TaskbarTone::Network,
        Metric::Battery => TaskbarTone::Battery,
        Metric::Uptime => TaskbarTone::Plain,
    }
}

/// The taskbar's cells for `gauges`: a single value under its short name,
/// a pair of rates under their marks, after the short name of the device
/// they are of; each with its device's colour and, from `timeline`, its
/// recent values for a curve.
fn taskbar_groups(gauges: &[Gauge], timeline: Option<&model::Timeline>) -> Vec<TaskbarGroup> {
    gauges
        .iter()
        .map(|gauge| TaskbarGroup {
            tone: tone(gauge.reading.metric),
            history: timeline
                .zip(gauge.trend.as_ref())
                .map(|(timeline, trend)| history(timeline, trend))
                .unwrap_or_default(),
            cells: match gauge.parts.as_slice() {
                [part] => vec![TaskbarCell {
                    label: gauge.short.clone(),
                    value: part.text.clone(),
                    widest: part.widest.clone(),
                    high: gauge.high,
                }],
                parts => parts
                    .iter()
                    .map(|part| TaskbarCell {
                        label: format!("{}{}", gauge.short, part.mark),
                        value: part.text.clone(),
                        widest: part.widest.clone(),
                        high: false,
                    })
                    .collect(),
            },
        })
        .collect()
}

/// The colour of a metric's bar: its device's.
pub(in crate::frontend) fn metric_color(metric: Metric, p: Palette) -> Hsla {
    match metric {
        Metric::Cpu | Metric::CpuTemperature | Metric::CpuClock | Metric::CpuPower => p.cpu,
        Metric::Memory | Metric::MemoryUsed => p.memory,
        Metric::Gpu
        | Metric::GpuTemperature
        | Metric::GpuMemory
        | Metric::GpuClock
        | Metric::GpuPower
        | Metric::GpuFan => p.gpu,
        Metric::Disk | Metric::DiskActive | Metric::DiskTemperature => p.io,
        Metric::Network => p.network,
        Metric::Battery => p.battery,
        Metric::Uptime => p.muted,
    }
}

/// The desktop monitor's window, once made.
#[derive(Clone)]
pub(in crate::frontend) struct Open {
    window: AnyWindowHandle,
    view: Entity<DeskMonitor>,
}

/// What the taskbar monitor's panel shows (`tray::Kind::Monitor`).
#[derive(Clone, Default)]
pub(in crate::frontend) struct TaskbarPanel {
    /// The chosen style, as saved.
    pub style: String,
    /// The monitor's look in each style (`TASKBAR_STYLES`), where it could
    /// be drawn.
    pub previews: Vec<Option<Arc<RenderImage>>>,
    /// The taskbar's colour, under the previews.
    pub ground: Hsla,
    /// The display scale the previews were drawn at.
    pub scale: f32,
    pub kinds: Vec<super::pages::components::PanelKind>,
}

impl PartialEq for TaskbarPanel {
    fn eq(&self, other: &Self) -> bool {
        self.style == other.style
            && self.ground == other.ground
            && self.scale == other.scale
            && self.kinds == other.kinds
            && self.previews.len() == other.previews.len()
            && self
                .previews
                .iter()
                .zip(&other.previews)
                .all(|(a, b)| match (a, b) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                })
    }
}

/// The monitors the desktop keeps.
#[derive(Default)]
pub(in crate::frontend) struct Monitors {
    desk: Option<Open>,
    /// Metrics chosen on the components page while a settings change was on
    /// its way: per monitor's settings key, its command and the latest
    /// choice, sent once that change has come back.
    queued: Vec<(&'static str, &'static str, Vec<String>)>,
    /// The taskbar monitor's look in each style, for its panel and the
    /// components page, and what they were drawn for: the latest first, at
    /// most two (the panel's display and the main window's may differ in
    /// scale).
    previews: Vec<(u64, Vec<Option<Arc<RenderImage>>>)>,
    /// The previews the components page last showed in the main window.
    page_previews: Vec<Option<Arc<RenderImage>>>,
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
            .child(gauge.name.clone());
        match gauge.parts.as_slice() {
            [part] => {
                let color = if gauge.high {
                    p.heat
                } else {
                    metric_color(gauge.reading.metric, p)
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
                    .when(gauge.bar, |row| {
                        row.child(
                            div().h(px(4.)).rounded(px(2.)).bg(p.stroke_strong).child(
                                div()
                                    .h_full()
                                    .w(relative(gauge.fraction.unwrap_or(0.) as f32))
                                    .rounded(px(2.))
                                    .bg(color),
                            ),
                        )
                    })
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
        // The taskbar monitor's panel follows its settings.
        self.refresh_tray(cx);
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
            // Only curves need the readings' recent values.
            let timeline = (taskbar.style == TaskbarStyle::Curves).then_some(&self.timeline);
            crate::shell::set_taskbar_readings(taskbar_groups(
                &gauges(&self.snapshot, &taskbar.metrics, self.bits()),
                timeline,
            ));
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
        metrics: Vec<String>,
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
                    self.open_components("desktop", window, cx);
                    crate::shell::show_main(&self.host);
                    return;
                }
                _ => return,
            }
        };
        self.request("set_desktop_monitor", change, Some("settings"), window, cx);
    }

    /// A right click on the taskbar monitor: its panel opens over it, or
    /// closes when open (`panel`, where the shell would put it).
    pub(in crate::frontend) fn taskbar_monitor_event(
        &mut self,
        event: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(anchor) = event.get("panel") {
            self.tray_flyout(super::tray::Kind::Monitor, anchor, window, cx);
        }
    }

    /// The taskbar monitor's look in each style (`TASKBAR_STYLES`) at
    /// `scale`, where it could be drawn, and the taskbar's colour under it.
    /// They are drawn again only when what they show changed: the first two
    /// readings, the taskbar's colours and the display's scale, not their
    /// values.
    fn taskbar_previews(&mut self, scale: f32) -> (Vec<Option<Arc<RenderImage>>>, Hsla) {
        let monitor: TaskbarMonitor = self.monitor_settings("taskbarMonitor");
        let light = crate::shell::taskbar_light();
        let mut shown = monitor.metrics.iter().take(2).cloned().collect::<Vec<_>>();
        if shown.is_empty() {
            shown = vec![Reading::parse("cpu"), Reading::parse("memory")]
                .into_iter()
                .flatten()
                .collect();
        }
        let key = super::charts::hash_of((
            shown.iter().map(Reading::text).collect::<Vec<_>>(),
            light,
            scale.to_bits(),
        ));
        let cache = &mut self.monitors.previews;
        match cache.iter().position(|(drawn, _)| *drawn == key) {
            Some(index) => {
                let entry = cache.remove(index);
                cache.insert(0, entry);
            }
            None => {
                let groups = taskbar_groups(
                    &gauges(&self.snapshot, &shown, self.bits()),
                    Some(&self.timeline),
                );
                let previews = TASKBAR_STYLES
                    .iter()
                    .map(|style| {
                        let style = TaskbarStyle::from(style.to_string());
                        let (width, height, pixels) =
                            crate::shell::taskbar_preview(&groups, style, light, scale)?;
                        let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
                        Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
                    })
                    .collect();
                let cache = &mut self.monitors.previews;
                cache.insert(0, (key, previews));
                cache.truncate(2);
            }
        }
        let [red, green, blue, _] = crate::shell::taskbar_color(light).to_le_bytes();
        let ground = Rgba {
            r: f32::from(red) / 255.,
            g: f32::from(green) / 255.,
            b: f32::from(blue) / 255.,
            a: 1.,
        };
        (self.monitors.previews[0].1.clone(), ground.into())
    }

    /// What the taskbar monitor's panel shows.
    pub(in crate::frontend) fn taskbar_panel(&mut self, scale: f32) -> TaskbarPanel {
        let (previews, ground) = self.taskbar_previews(scale);
        TaskbarPanel {
            style: String::from(
                self.monitor_settings::<TaskbarMonitor>("taskbarMonitor")
                    .style,
            ),
            previews,
            ground,
            scale,
            kinds: self.taskbar_panel_kinds(),
        }
    }

    /// The previews for the components page in the main window, and the
    /// taskbar's colour under them; those it no longer shows leave the
    /// window's atlas.
    pub(in crate::frontend) fn page_previews(
        &mut self,
        window: &mut Window,
    ) -> (Vec<Option<Arc<RenderImage>>>, Hsla) {
        let (previews, ground) = self.taskbar_previews(window.scale_factor());
        let old = std::mem::replace(&mut self.monitors.page_previews, previews.clone());
        for image in old.into_iter().flatten() {
            if !previews
                .iter()
                .flatten()
                .any(|new| Arc::ptr_eq(new, &image))
            {
                let _ = window.drop_image(image);
            }
        }
        (previews, ground)
    }

    /// A control of the taskbar monitor's panel, as if used, by the key the
    /// tests name it by (`taskbarPanel`); false when there is none.
    pub(in crate::frontend) fn choose_taskbar_item(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(choice) = self.taskbar_panel_choice(key) else {
            return false;
        };
        self.taskbar_choice(choice, window, cx);
        true
    }

    /// A control of the taskbar monitor's panel was used. A style shows at
    /// once in the panel, before the settings come back.
    pub(in crate::frontend) fn taskbar_choice(
        &mut self,
        choice: TaskbarChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = "set_taskbar_monitor";
        match choice {
            TaskbarChoice::Style(style) => {
                if let Some(monitor) = self
                    .data
                    .get_mut("settings")
                    .and_then(|settings| settings.get_mut("taskbarMonitor"))
                {
                    monitor["style"] = json!(style);
                }
                self.request(
                    command,
                    json!({ "style": style }),
                    Some("settings"),
                    window,
                    cx,
                );
                self.refresh_tray(cx);
            }
            TaskbarChoice::Readings(next) => {
                self.choose_metrics("taskbarMonitor", command, next, window, cx)
            }
            TaskbarChoice::Settings => {
                self.open_components("taskbar", window, cx);
                crate::shell::show_main(&self.host);
            }
            TaskbarChoice::Hide => self.request(
                command,
                json!({ "enabled": false }),
                Some("settings"),
                window,
                cx,
            ),
        }
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
                    "metric": gauge.reading.text(),
                    "values": gauge.parts.iter().map(|part| part.text.clone()).collect::<Vec<_>>(),
                    "high": gauge.high,
                })).collect::<Vec<_>>(),
            })
        });
        json!({
            "desktop": desk,
            "taskbar": crate::shell::taskbar_monitor_rect(),
            "taskbarStyle": self.data["settings"]["taskbarMonitor"]["style"],
            "taskbarPanel": self.taskbar_panel_kinds().iter().map(|kind| json!({
                "kind": kind.key,
                "devices": kind.devices.iter().map(|device| json!({
                    "tab": device.tab,
                    "chosen": device.chosen,
                    "options": device.options.iter().map(|option| json!({"key": option.key, "on": option.on})).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Device, Gauge, absent_name, arrange, devices, gauges, gpu_name, main_gpu, main_gpu_key,
        taskbar_groups,
    };
    use crate::frontend::theme::label;
    use crate::settings::{Metric, Reading};
    use crate::shell::TaskbarTone;
    use serde_json::json;

    fn snapshot() -> serde_json::Value {
        json!({
            "system": {"cpu": 83.6, "memUsed": 4294967296u64, "memTotal": 17179869184u64, "uptimeMs": 90061000u64},
            "hardware": {
                "cpuMhz": 3420,
                "power": {"cores": 21.5},
                "cpuSensors": {"packageCelsius": 61.2},
                "gpus": [
                    {"info": {"dedicatedTotal": 134217728u64}, "util": 90.0, "temperature": 50.0, "dedicatedUsed": 0, "power": 40.0},
                    {"info": {"dedicatedTotal": 8589934592u64}, "util": 12.0, "temperature": 79.6, "dedicatedUsed": 2147483648u64,
                     "clockMhz": 1850.0, "boardPower": 120.4, "fanRpm": 1500}
                ],
                "disks": [
                    {"read": 2097152.0, "write": 0.0, "active": 12.0, "temperature": 41.0},
                    {"read": 1048576.0, "write": 512.0, "active": 63.4, "temperature": 58.0, "temperatureWarning": 55.0}
                ],
                "nics": [{"rx": 2048.0, "tx": 0.5}],
                "batteries": [{"percent": 18.2, "pluggedIn": false}]
            }
        })
    }

    /// Readings of the default devices.
    fn plain(metrics: &[Metric]) -> Vec<Reading> {
        metrics.iter().copied().map(Reading::from).collect()
    }

    fn texts(gauge: &Gauge) -> Vec<String> {
        gauge.parts.iter().map(|part| part.text.clone()).collect()
    }

    #[test]
    fn readings_follow_the_sample_and_turn_high_from_80() {
        let all = gauges(&snapshot(), &plain(&Metric::ALL), false);
        assert_eq!(all.len(), Metric::ALL.len());
        let by = |metric| {
            all.iter()
                .find(|gauge| gauge.reading.metric == metric)
                .unwrap()
        };
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
            texts(&gauges(&snapshot(), &plain(&[Metric::Network]), true)[0])[1],
            "0 Kbps"
        );
        // Clock, power and fan show without a bar; the CPU's cores stand in
        // for a package with no figure.
        let no_bar = [
            Metric::CpuClock,
            Metric::CpuPower,
            Metric::GpuClock,
            Metric::GpuPower,
            Metric::GpuFan,
            Metric::Uptime,
        ];
        assert!(
            no_bar
                .iter()
                .all(|metric| !by(*metric).bar && by(*metric).fraction.is_none())
        );
        assert!(texts(by(Metric::CpuClock))[0].ends_with(" GHz"));
        assert!(texts(by(Metric::CpuPower))[0].ends_with(" W"));
        assert!(texts(by(Metric::GpuPower))[0].ends_with(" W"));
        assert_eq!(texts(by(Metric::GpuFan)), ["1500 RPM"]);
        assert_ne!(texts(by(Metric::Uptime))[0], label("format.missing"));
        // Memory in use as an amount, with its share as the bar.
        assert!(texts(by(Metric::MemoryUsed))[0].ends_with(" GB"));
        assert_eq!(by(Metric::MemoryUsed).fraction, Some(0.25));
        // The busiest disk's active time; the hottest disk, amber from the
        // warning it reports.
        assert_eq!(texts(by(Metric::DiskActive)), ["63%"]);
        assert!(by(Metric::DiskTemperature).high);
        // A low battery on battery power is what turns amber.
        assert_eq!(texts(by(Metric::Battery)), ["18%"]);
        assert!(by(Metric::Battery).high);
        let plugged = json!({"hardware": {"batteries": [{"percent": 18.0, "pluggedIn": true}]}});
        assert!(!gauges(&plugged, &plain(&[Metric::Battery]), false)[0].high);
        // Without a measured board power, the share of the power budget.
        let budget =
            json!({"hardware": {"gpus": [{"info": {"dedicatedTotal": 1}, "power": 40.0}]}});
        assert!(
            texts(&gauges(&budget, &plain(&[Metric::GpuPower]), false)[0])[0].ends_with("% TDP")
        );
    }

    #[test]
    fn readings_without_data_stay_unknown() {
        let empty = gauges(&serde_json::Value::Null, &plain(&Metric::ALL), false);
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
        let groups = taskbar_groups(
            &gauges(&snapshot(), &plain(&[Metric::Cpu, Metric::Network]), false),
            None,
        );
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].cells.len(), 1);
        assert!(groups[0].cells[0].high);
        assert_eq!(groups[0].cells[0].widest, "100%");
        assert_eq!(groups[0].tone, TaskbarTone::Cpu);
        assert_eq!(groups[1].tone, TaskbarTone::Network);
        // Without a timeline there is no curve.
        assert!(groups[0].history.is_empty());
        assert_eq!(
            groups[1]
                .cells
                .iter()
                .map(|cell| cell.label.as_str())
                .collect::<Vec<_>>(),
            ["↓", "↑"]
        );
    }

    fn several() -> serde_json::Value {
        json!({
            "hardware": {
                "gpus": [
                    {"info": {"name": "NVIDIA GeForce RTX 4060 Laptop GPU", "dedicatedTotal": 8589934592u64}, "util": 40.0},
                    {"info": {"name": "Intel(R) UHD Graphics", "dedicatedTotal": 134217728u64}, "util": 7.0}
                ],
                "disks": [
                    {"info": {"index": 0, "bus": "nvme", "model": "Samsung SSD 990 PRO 2TB", "size": 2000000000000u64, "volumes": ["C:"]},
                     "read": 1048576.0, "write": 0.0, "active": 20.0, "temperature": 45.0},
                    {"info": {"index": 1, "bus": "sata", "model": "WDC WD40", "size": 4000000000000u64, "volumes": []},
                     "read": 0.0, "write": 2097152.0, "active": 60.0, "temperature": 35.0}
                ],
                "nics": [
                    {"info": {"name": "WLAN", "description": "Intel Wi-Fi 6E AX211"}, "rx": 4096.0, "tx": 1024.0},
                    {"info": {"name": "vEthernet (WSL)", "description": "Hyper-V Virtual Ethernet Adapter"}, "rx": 2048.0, "tx": 0.0}
                ]
            }
        })
    }

    fn of(metric: Metric, device: &Device) -> Reading {
        Reading {
            metric,
            device: Some(device.key.clone()),
        }
    }

    #[test]
    fn devices_are_told_apart_by_letters_names_and_models() {
        let snapshot = several();
        let disks = devices(&snapshot, "disk");
        assert_eq!(
            (disks[0].title.as_str(), disks[0].short.as_str()),
            ("C:", "C:")
        );
        assert!(disks[0].detail.starts_with("Samsung SSD 990 PRO 2TB · "));
        // A disk without letters goes by its number.
        assert_eq!(disks[1].short, "#1");
        let gpus = devices(&snapshot, "gpu");
        assert_eq!(gpus[0].name, "RTX 4060 Laptop GPU");
        assert_eq!(gpus[1].name, "Intel UHD Graphics");
        assert_eq!(
            (gpus[0].short.as_str(), gpus[1].short.as_str()),
            ("GPU1", "GPU2")
        );
        assert_eq!(
            main_gpu_key(&snapshot, &gpus).as_deref(),
            Some(gpus[0].key.as_str())
        );
        let nics = devices(&snapshot, "network");
        assert_eq!(nics[1].short, "vEthe…");
        assert_eq!(gpu_name("AMD Radeon(TM) Graphics"), "AMD Radeon Graphics");
        assert_eq!(gpu_name("AMD Radeon RX 7900 XTX"), "RX 7900 XTX");
        // A device away is named after its key.
        assert_eq!(absent_name("disk:usb|Portable SSD|1000#1"), "Portable SSD");
        assert_eq!(absent_name("nic:Intel Wi-Fi|WLAN"), "WLAN");
        assert_eq!(
            absent_name("gpu:NVIDIA GeForce RTX 4060"),
            "NVIDIA GeForce RTX 4060"
        );
    }

    #[test]
    fn a_device_s_readings_keep_to_it() {
        let snapshot = several();
        let (disks, gpus, nics) = (
            devices(&snapshot, "disk"),
            devices(&snapshot, "gpu"),
            devices(&snapshot, "network"),
        );
        let shown = gauges(
            &snapshot,
            &[
                of(Metric::DiskTemperature, &disks[1]),
                of(Metric::Disk, &disks[0]),
                of(Metric::Gpu, &gpus[1]),
                of(Metric::Network, &nics[1]),
                Reading::from(Metric::DiskActive),
                Reading::from(Metric::Gpu),
                Reading {
                    metric: Metric::DiskActive,
                    device: Some("disk:usb|Gone|1".into()),
                },
            ],
            false,
        );
        assert_eq!(
            texts(&shown[0]),
            [crate::regional::number!("{:.0} °C", 35.)]
        );
        assert_eq!(shown[0].short, "#1");
        // A device's rates: its own, under marks that name it.
        assert_eq!(shown[1].name, "C:");
        assert!(texts(&shown[1])[0].ends_with("MB/s"));
        assert_eq!(texts(&shown[1])[1], "0 KB/s");
        assert_eq!(shown[1].parts[0].mark, label("components.metric.read"));
        let cells = taskbar_groups(&shown, None);
        assert_eq!(
            cells[1].cells[0].label,
            format!("C:{}", label("components.metric.read"))
        );
        assert_eq!(cells[3].cells[1].label, "vEthe…↑");
        // Rates of every device keep their marks alone.
        assert_eq!(
            taskbar_groups(
                &gauges(&snapshot, &[Reading::from(Metric::Network)], false),
                None
            )[0]
            .cells[0]
                .label,
            "↓"
        );
        assert_eq!(
            (texts(&shown[2]), shown[2].short.as_str()),
            (vec!["7%".to_owned()], "GPU2")
        );
        assert_eq!(texts(&shown[3]), ["2 KB/s", "0 KB/s"]);
        // Without a device: the busiest disk, the main GPU.
        assert_eq!(texts(&shown[4]), ["60%"]);
        assert_eq!(texts(&shown[5]), ["40%"]);
        // A device away shows nothing known.
        assert_eq!(texts(&shown[6]), [label("format.missing")]);
        assert!(!shown[6].bar || shown[6].fraction.is_none());
    }

    #[test]
    fn readings_go_by_kind_then_device_then_metric() {
        let snapshot = several();
        let disks = devices(&snapshot, "disk");
        let gone = Reading {
            metric: Metric::Disk,
            device: Some("disk:usb|Gone|1".into()),
        };
        let arranged = arrange(
            &snapshot,
            vec![
                of(Metric::Disk, &disks[1]),
                Reading::from(Metric::Network),
                gone.clone(),
                of(Metric::DiskActive, &disks[0]),
                Reading::from(Metric::Cpu),
                Reading::from(Metric::Disk),
            ],
        );
        assert_eq!(
            arranged,
            [
                Reading::from(Metric::Cpu),
                Reading::from(Metric::Disk),
                of(Metric::DiskActive, &disks[0]),
                of(Metric::Disk, &disks[1]),
                gone,
                Reading::from(Metric::Network),
            ]
        );
    }

    #[test]
    fn a_curve_follows_the_timeline_and_scales_to_its_reading() {
        let mut timeline = super::model::Timeline::new();
        let at = |time: f64, cpu: f64, rx: f64| {
            json!({
                "timestamp": time,
                "system": {"cpu": cpu, "memUsed": 1, "memTotal": 4},
                "hardware": {"nics": [{"info": {"id": "1", "name": "WLAN"}, "rx": rx, "tx": 0.0}]}
            })
        };
        for (time, cpu, rx) in [
            (1000., 10., 0.),
            (2000., 50., 2097152.),
            (3000., 30., 4194304.),
        ] {
            timeline.push(&at(time, cpu, rx), 1000.);
        }
        let shown = gauges(
            &at(3000., 30., 4194304.),
            &plain(&[Metric::Cpu, Metric::Network]),
            false,
        );
        let groups = taskbar_groups(&shown, Some(&timeline));
        // Percent of 100; oldest first.
        assert_eq!(groups[0].history, [0.1, 0.5, 0.3]);
        // Rates against their own peak, here 4 MB/s.
        assert_eq!(groups[1].history, [0., 0.5, 1.]);
    }

    #[test]
    fn a_clock_s_curve_spans_its_own_range() {
        use super::{Combine, Scale, Trend, history};
        let mut timeline = super::model::Timeline::new();
        for (time, mhz) in [(1000., 3000.), (2000., 4000.), (3000., 3500.)] {
            timeline.push(
                &json!({"timestamp": time, "hardware": {"cpuMhz": mhz}}),
                1000.,
            );
        }
        let clock = Trend {
            keys: vec!["hardware.cpuMhz".into()],
            combine: Combine::Sum,
            scale: Scale::Span,
        };
        // A quarter of the span below and above: 2750 to 4250 MHz.
        let curve = history(&timeline, &clock);
        assert!((curve[0] - 1. / 6.).abs() < 1e-6 && (curve[1] - 5. / 6.).abs() < 1e-6);
        assert!((curve[2] - 0.5).abs() < 1e-6);
        // A clock that holds still lies in the middle.
        let mut steady = super::model::Timeline::new();
        for time in [1000., 2000.] {
            steady.push(
                &json!({"timestamp": time, "hardware": {"cpuMhz": 3000.}}),
                1000.,
            );
        }
        assert_eq!(history(&steady, &clock), [0.5, 0.5]);
    }
}
