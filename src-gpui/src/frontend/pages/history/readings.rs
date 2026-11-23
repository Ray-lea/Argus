//! The readings the page can draw: what each is called, its colour, unit and
//! axis, and which recorded channels it is drawn from. The base readings are
//! always there; the rest come from what was recorded (`history_channels`).
use super::series::{Part, Points, nice_top};
use crate::frontend::charts::{self, GIB, KIB, MIB};
use crate::frontend::theme::{self, Palette, label};
use argus_core::tr;
use gpui_kit::gpui::Hsla;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unit {
    Percent,
    Bytes,
    /// Bytes per second; a network rate follows the chosen unit (bytes or bits).
    Rate,
    NetworkRate,
    Celsius,
    Megahertz,
    Watts,
    /// Signed milliwatts (a battery charging or discharging), shown in watts.
    Milliwatts,
    Volts,
    Rpm,
    Count,
    Milliseconds,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Axis {
    /// 0 to a round percentage, 10 … 100.
    Percent,
    /// 0 to the highest value of the reading's `total` channel (memory to its size).
    Total,
    /// 0 to a round value above the highest.
    Auto,
    /// 30 °C to a round ten above the highest (60 … 110).
    Temperature,
    /// Around zero, as far either way as the furthest value.
    Signed,
    /// From a round value below the lowest count.
    Count,
    Fixed(f64, f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Group {
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
    Battery,
    System,
}

impl Group {
    pub(super) const ALL: [Group; 7] = [
        Group::Cpu,
        Group::Memory,
        Group::Disk,
        Group::Network,
        Group::Gpu,
        Group::Battery,
        Group::System,
    ];
    pub(super) fn label(self) -> String {
        label(match self {
            Group::Cpu => "history.group.cpu",
            Group::Memory => "history.group.memory",
            Group::Disk => "history.group.disk",
            Group::Network => "history.group.network",
            Group::Gpu => "history.group.gpu",
            Group::Battery => "history.group.battery",
            Group::System => "history.group.system",
        })
    }
}

/// What a lane or a tab draws.
#[derive(Clone, Debug)]
pub(super) struct Reading {
    /// Saved in the lanes' preference: a channel key, or `disk`, `network`,
    /// `disk.<id>`, `nic.<id>` for a pair drawn together.
    pub(super) id: String,
    /// Short, for the tabs: "CPU", "GPU 1".
    pub(super) name: String,
    /// For the lane's title and menus: "CPU 使用率".
    pub(super) long: String,
    pub(super) group: Group,
    pub(super) color: Hsla,
    pub(super) unit: Unit,
    pub(super) axis: Axis,
    /// The channels it draws: one, or a pair (read and write) whose sum is its value.
    pub(super) keys: Vec<String>,
    /// The names of a pair's two parts.
    pub(super) parts: Option<(String, String)>,
    /// One channel standing in for a pair in records from before the split.
    pub(super) fallback: Option<String>,
    /// The channel of the highest sample in each frame (CPU), the band's top.
    pub(super) peak: Option<String>,
    /// The channel the axis tops at.
    pub(super) total: Option<String>,
    /// Only the privileged service reads it.
    pub(super) service: bool,
    /// The per-process value the table's trend column follows for it.
    pub(super) process: Option<&'static str>,
    /// Recorded in the history at hand, or one of the base readings.
    pub(super) recorded: bool,
    /// A device's reading: the device's name, and the reading's own short
    /// name ("Active time"), which the chooser lists under the device.
    pub(super) device: Option<String>,
    pub(super) part: Option<String>,
}

/// The channels one query needs to draw `reading`.
pub(super) fn query_keys(reading: &Reading) -> Vec<String> {
    let mut keys = reading.keys.clone();
    keys.extend(reading.fallback.clone());
    keys.extend(reading.peak.clone());
    keys.extend(reading.total.clone());
    keys
}

impl Reading {
    fn new(
        id: &str,
        name: String,
        long: String,
        group: Group,
        color: Hsla,
        unit: Unit,
        axis: Axis,
    ) -> Self {
        Self {
            id: id.into(),
            name,
            long,
            group,
            color,
            unit,
            axis,
            keys: vec![id.into()],
            parts: None,
            fallback: None,
            peak: None,
            total: None,
            service: false,
            process: None,
            recorded: false,
            device: None,
            part: None,
        }
    }
    /// A reading of device `name`, `part` its locale key under `history.devicePart`.
    fn of_device(mut self, name: &str, part: &str) -> Self {
        self.device = Some(name.to_owned());
        self.part = Some(label(&format!("history.devicePart.{part}")));
        self
    }
    fn pair(mut self, a: &str, b: &str, parts: (&str, &str)) -> Self {
        self.keys = vec![a.into(), b.into()];
        self.parts = Some((label(parts.0), label(parts.1)));
        self
    }
    fn process(mut self, key: &'static str) -> Self {
        self.process = Some(key);
        self
    }
    fn total(mut self, key: &str) -> Self {
        self.total = Some(key.into());
        self
    }

    pub(super) fn is_pair(&self) -> bool {
        self.keys.len() == 2
    }

    /// The channel drawn as the main line: a pair's first part, or the
    /// combined channel of older records.
    pub(super) fn main_key<'a>(&'a self, points: &Points) -> &'a str {
        match &self.fallback {
            Some(fallback) if !points.has(&self.keys[0]) && points.has(fallback) => fallback,
            _ => &self.keys[0],
        }
    }

    /// The value at point `i`: a pair's sum. With `parts`, also each part.
    pub(super) fn value(&self, points: &Points, i: usize) -> Option<f64> {
        if self.is_pair() {
            if let (Some(a), Some(b)) = (
                points.value(&self.keys[0], Part::Mean, i),
                points.value(&self.keys[1], Part::Mean, i),
            ) {
                return Some(a + b);
            }
            return self
                .fallback
                .as_deref()
                .and_then(|key| points.value(key, Part::Mean, i));
        }
        points.value(&self.keys[0], Part::Mean, i)
    }

    pub(super) fn part_values(&self, points: &Points, i: usize) -> Option<(f64, f64)> {
        if !self.is_pair() {
            return None;
        }
        Some((
            points.value(&self.keys[0], Part::Mean, i)?,
            points.value(&self.keys[1], Part::Mean, i)?,
        ))
    }

    /// The highest reading inside point `i` when it covers several: CPU's
    /// highest sample, or a percentage's highest record.
    pub(super) fn peak_value(&self, points: &Points, i: usize) -> Option<f64> {
        if let Some(key) = self.peak.as_deref().filter(|key| points.has(key)) {
            return points
                .value(key, Part::High, i)
                .or_else(|| points.value(key, Part::Mean, i));
        }
        (self.unit == Unit::Percent && !self.is_pair())
            .then(|| points.value(&self.keys[0], Part::High, i))
            .flatten()
    }

    /// Whether the title shows a highest value beside the reading's own.
    pub(super) fn shows_peak(&self) -> bool {
        self.peak.is_some() || (self.unit == Unit::Percent && !self.is_pair())
    }

    /// What the axis spans for the points drawn in `from..=to`.
    pub(super) fn axis_range(
        &self,
        points: &Points,
        from: i64,
        to: i64,
        end: i64,
        banded: bool,
    ) -> (f64, f64) {
        let main = self.main_key(points);
        let mut highest = points.highest(main, Part::Mean, from, to, end);
        if self.is_pair() {
            highest = larger(
                highest,
                points.highest(&self.keys[1], Part::Mean, from, to, end),
            );
        }
        if banded {
            highest = larger(highest, points.highest(main, Part::High, from, to, end));
            if let Some(peak) = &self.peak {
                highest = larger(highest, points.highest(peak, Part::High, from, to, end));
            }
        }
        let lowest = points.lowest(
            main,
            if banded { Part::Low } else { Part::Mean },
            from,
            to,
            end,
        );
        let high = highest.unwrap_or(0.);
        match self.axis {
            Axis::Percent => (0., nice_top(high, true).max(10.)),
            Axis::Total => {
                let total = self
                    .total
                    .as_deref()
                    .and_then(|key| points.highest(key, Part::Mean, from, to, end))
                    .filter(|t| *t > 0.);
                (0., total.unwrap_or_else(|| self.round_top(high)))
            }
            Axis::Auto => (0., self.round_top(high)),
            Axis::Temperature => {
                let top = (((high.max(50.) + 4.) / 10.).ceil() * 10.).clamp(60., 110.);
                (30_f64.min(lowest.unwrap_or(30.)).max(0.), top)
            }
            Axis::Signed => {
                let far = high.abs().max(lowest.unwrap_or(0.).abs()).max(1.);
                let top = self.round_top(far);
                (-top, top)
            }
            Axis::Count => {
                let low = (lowest.unwrap_or(0.) / 50.).floor() * 50.;
                (low, low + nice_top((high - low).max(50.), false))
            }
            Axis::Fixed(low, high) => (low, high),
        }
    }

    /// A round top in the unit the labels use, so its rows fall on round values.
    fn round_top(&self, value: f64) -> f64 {
        let unit = self.label_unit(value);
        nice_top(value / unit, false) * unit
    }

    /// The size of the unit an axis value is labelled in.
    pub(super) fn label_unit(&self, value: f64) -> f64 {
        match self.unit {
            Unit::Rate | Unit::NetworkRate if value >= GIB => GIB,
            Unit::Rate | Unit::NetworkRate if value >= MIB => MIB,
            Unit::Rate | Unit::NetworkRate => KIB,
            Unit::Bytes if value >= GIB => GIB,
            Unit::Bytes => MIB,
            Unit::Megahertz => 1000.,
            Unit::Milliwatts => 1000.,
            _ => 1.,
        }
    }

    /// The value as a tab, lane title or table shows it.
    pub(super) fn format(&self, value: f64, bits: bool) -> String {
        format_unit(self.unit, value, bits)
    }

    /// An axis label: as short as the axis allows (no decimals where the rows
    /// are whole).
    pub(super) fn axis_label(&self, value: f64, top: f64, bits: bool) -> String {
        let unit = self.label_unit(top.abs().max(value.abs()));
        let scaled = value / unit;
        let number = if (scaled - scaled.round()).abs() < 0.05 {
            format!("{}", scaled.round())
        } else {
            crate::regional::number!("{scaled:.1}")
        };
        match self.unit {
            Unit::Percent => format!("{number}%"),
            Unit::Rate | Unit::NetworkRate if bits && self.unit == Unit::NetworkRate => {
                charts::bit_rate(value)
            }
            Unit::Rate | Unit::NetworkRate => {
                let name = if unit >= GIB {
                    "GB/s"
                } else if unit >= MIB {
                    "MB/s"
                } else {
                    "KB/s"
                };
                if value == 0. {
                    "0".into()
                } else {
                    format!("{number} {name}")
                }
            }
            Unit::Bytes => {
                let name = if unit >= GIB { "GB" } else { "MB" };
                if value == 0. {
                    "0".into()
                } else {
                    format!("{number} {name}")
                }
            }
            Unit::Celsius => format!("{number}°C"),
            Unit::Megahertz => format!("{number} GHz"),
            Unit::Watts | Unit::Milliwatts => format!("{number} W"),
            Unit::Volts => crate::regional::number!("{value:.2} V"),
            Unit::Rpm => number.to_string(),
            Unit::Count => crate::regional::count(value),
            Unit::Milliseconds => format!("{number} ms"),
        }
    }

    /// Values as wide as this reading's widest, for a slot that keeps its width
    /// while the value changes.
    pub(super) fn widest_samples(&self, bits: bool) -> Vec<String> {
        let values: &[f64] = match self.unit {
            Unit::Percent => &[100., 88.8],
            Unit::Bytes => &[88.8 * GIB, 8.88 * GIB, 888.8 * MIB],
            Unit::Rate | Unit::NetworkRate => &[888. * KIB, 88.8 * MIB, 888.8 * MIB, 8.88 * GIB],
            Unit::Celsius => &[100.],
            Unit::Megahertz => &[8888.],
            Unit::Watts => &[888.8, 88.8],
            Unit::Milliwatts => &[-188_000., 88_800.],
            Unit::Volts => &[8.888],
            Unit::Rpm => &[8888.],
            Unit::Count => &[888_888.],
            Unit::Milliseconds => &[888.8],
        };
        values.iter().map(|v| self.format(*v, bits)).collect()
    }
}

/// The larger of two values either of which may be unknown.
fn larger(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

pub(super) fn format_unit(unit: Unit, value: f64, bits: bool) -> String {
    match unit {
        Unit::Percent => format!("{}%", theme::cpu(value)),
        Unit::Bytes => theme::memory(value),
        Unit::Rate => charts::byte_rate(value),
        Unit::NetworkRate => charts::net_rate(value, bits),
        Unit::Celsius => format!("{}°C", value.round()),
        Unit::Megahertz => crate::regional::number!("{:.2} GHz", value / 1000.),
        Unit::Watts => format!("{} W", charts::watts_tenths(value.max(0.))),
        Unit::Milliwatts => {
            let watts = value / 1000.;
            let text = charts::watts_tenths(watts.abs());
            if watts > 0.05 {
                format!("+{text} W")
            } else if watts < -0.05 {
                format!("-{text} W")
            } else {
                format!("{text} W")
            }
        }
        Unit::Volts => crate::regional::number!("{value:.3} V"),
        Unit::Rpm => format!("{} RPM", value.round()),
        Unit::Count => crate::regional::count(value),
        Unit::Milliseconds => crate::regional::number!("{value:.1} ms"),
    }
}

/// A recorded channel, as `history_channels` lists it.
#[derive(Clone, Debug, Default)]
pub(super) struct Channel {
    pub(super) key: String,
    pub(super) label: Option<String>,
    pub(super) service: bool,
}

pub(super) fn parse_channels(answer: &Value) -> Vec<Channel> {
    answer
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|channel| {
                    Some(Channel {
                        key: channel["key"].as_str()?.to_owned(),
                        label: channel["label"]
                            .as_str()
                            .filter(|l| !l.is_empty())
                            .map(str::to_owned),
                        service: channel["service"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The device part of a per-device key (`gpu.<id>.util` → `<id>`), whose ids
/// may contain dots: everything between the prefix and the last part.
fn device_of<'a>(key: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let rest = key.strip_prefix(prefix)?.strip_prefix('.')?;
    let (id, part) = rest.rsplit_once('.')?;
    (!id.is_empty()).then_some((id, part))
}

/// A device's reading named by `key` (`history.device.diskActive`: "{name} 活动时间").
fn named(key: &str, name: &str) -> String {
    argus_core::i18n::format(key, &[("name", &name as &dyn std::fmt::Display)])
}

/// Every reading there is to draw: the base readings, then what was recorded.
pub(super) fn catalogue(channels: &[Channel], p: &Palette) -> Vec<Reading> {
    let has = |key: &str| channels.iter().any(|c| c.key == key);
    let service = |key: &str| channels.iter().any(|c| c.key == key && c.service);
    let resource = |name: &str| label(&format!("common.resource.{name}"));
    let long = |id: &str| label(&format!("history.reading.long.{id}"));
    let short = |id: &str| label(&format!("history.reading.short.{id}"));
    let mut out = Vec::new();

    // Processor.
    let mut cpu = Reading::new(
        "cpu",
        resource("cpu"),
        long("cpu"),
        Group::Cpu,
        p.cpu,
        Unit::Percent,
        Axis::Percent,
    )
    .process("cpu");
    cpu.peak = Some("cpuPeak".into());
    cpu.recorded = true;
    out.push(cpu);
    for (key, color, unit, axis) in [
        ("cpuKernel", p.cpu_e, Unit::Percent, Axis::Percent),
        ("cpuClock", p.cpu_e, Unit::Megahertz, Axis::Auto),
        ("cpuPower", p.group_users[0], Unit::Watts, Axis::Auto),
        ("cpuTemp", p.temperature, Unit::Celsius, Axis::Temperature),
        ("cpuVoltage", p.cpu_e, Unit::Volts, Axis::Fixed(0.6, 1.4)),
    ] {
        let mut reading =
            Reading::new(key, short(key), long(key), Group::Cpu, color, unit, axis).process("cpu");
        reading.service = matches!(key, "cpuTemp" | "cpuVoltage") || service(key);
        reading.recorded = has(key);
        out.push(reading);
    }
    for channel in channels.iter().filter(|c| c.key.starts_with("fan.")) {
        let name = channel
            .label
            .clone()
            .unwrap_or_else(|| channel.key[4..].to_owned());
        let mut reading = Reading::new(
            &channel.key,
            tr!("history.device.fan", name = name.clone()),
            tr!("history.device.fan", name = name),
            Group::Cpu,
            p.group_users[0],
            Unit::Rpm,
            Axis::Auto,
        );
        reading.service = true;
        reading.recorded = true;
        out.push(reading);
    }

    // Memory.
    let mut memory = Reading::new(
        "memUsed",
        resource("memory"),
        long("memUsed"),
        Group::Memory,
        p.memory,
        Unit::Bytes,
        Axis::Total,
    )
    .total("memTotal")
    .process("mem");
    memory.recorded = true;
    out.push(memory);
    for (key, total) in [
        ("commitUsed", Some("commitLimit")),
        ("memCached", Some("memTotal")),
        ("memCompressed", Some("memTotal")),
        ("pagedPool", None),
        ("nonpagedPool", None),
    ] {
        let mut reading = Reading::new(
            key,
            short(key),
            long(key),
            Group::Memory,
            p.group_users[2],
            Unit::Bytes,
            if total.is_some() {
                Axis::Total
            } else {
                Axis::Auto
            },
        )
        .process("mem");
        reading.total = total.map(str::to_owned);
        reading.recorded = has(key);
        out.push(reading);
    }

    // Storage: all disks together, then each.
    let mut disk = Reading::new(
        "disk",
        resource("disk"),
        long("disk"),
        Group::Disk,
        p.io,
        Unit::Rate,
        Axis::Auto,
    )
    .pair(
        "diskRead",
        "diskWrite",
        ("history.part.read", "history.part.write"),
    )
    .process("disk");
    disk.fallback = Some("disk".into());
    disk.recorded = true;
    out.push(disk);
    let mut disks: Vec<(&str, Option<&str>)> = Vec::new();
    for channel in channels {
        if let Some((id, _)) = device_of(&channel.key, "disk")
            && !disks.iter().any(|(known, _)| *known == id)
        {
            disks.push((id, channel.label.as_deref()));
        }
    }
    for (id, name) in &disks {
        let name = name.map(str::to_owned).unwrap_or_else(|| (*id).to_owned());
        let key = |part: &str| format!("disk.{id}.{part}");
        if has(&key("read")) {
            let mut pair = Reading::new(
                &format!("disk.{id}"),
                name.clone(),
                tr!("history.device.diskOf", name = name.clone()),
                Group::Disk,
                p.io,
                Unit::Rate,
                Axis::Auto,
            )
            .pair(
                &key("read"),
                &key("write"),
                ("history.part.read", "history.part.write"),
            )
            .process("disk")
            .of_device(&name, "diskOf");
            pair.recorded = true;
            out.push(pair);
        }
        for (part, text, unit, axis, color) in [
            (
                "active",
                "history.device.diskActive",
                Unit::Percent,
                Axis::Fixed(0., 100.),
                p.io,
            ),
            (
                "response",
                "history.device.diskResponse",
                Unit::Milliseconds,
                Axis::Auto,
                p.io,
            ),
            (
                "temp",
                "history.device.deviceTemp",
                Unit::Celsius,
                Axis::Temperature,
                p.temperature,
            ),
        ] {
            if has(&key(part)) {
                let long = named(text, &name);
                let mut reading = Reading::new(
                    &key(part),
                    long.clone(),
                    long,
                    Group::Disk,
                    color,
                    unit,
                    axis,
                )
                .of_device(&name, text.trim_start_matches("history.device."));
                reading.recorded = true;
                out.push(reading);
            }
        }
    }

    // Network.
    let mut network = Reading::new(
        "network",
        resource("network"),
        long("network"),
        Group::Network,
        p.network,
        Unit::NetworkRate,
        Axis::Auto,
    )
    .pair(
        "netReceive",
        "netSend",
        ("history.part.receive", "history.part.send"),
    )
    .process("net");
    network.fallback = Some("network".into());
    network.recorded = true;
    out.push(network);
    let mut nics: Vec<(&str, Option<&str>)> = Vec::new();
    for channel in channels {
        if let Some((id, _)) = device_of(&channel.key, "nic")
            && !nics.iter().any(|(known, _)| *known == id)
        {
            nics.push((id, channel.label.as_deref()));
        }
    }
    for (id, name) in &nics {
        let name = name.map(str::to_owned).unwrap_or_else(|| (*id).to_owned());
        let mut pair = Reading::new(
            &format!("nic.{id}"),
            name.clone(),
            tr!("history.device.nicOf", name = name.clone()),
            Group::Network,
            p.network,
            Unit::NetworkRate,
            Axis::Auto,
        )
        .pair(
            &format!("nic.{id}.receive"),
            &format!("nic.{id}.send"),
            ("history.part.receive", "history.part.send"),
        )
        .process("net")
        .of_device(&name, "nicOf");
        pair.recorded = true;
        out.push(pair);
    }

    // Graphics: the busiest engine of any adapter, then each adapter.
    let mut gpu = Reading::new(
        "gpu",
        resource("gpu"),
        long("gpu"),
        Group::Gpu,
        p.gpu,
        Unit::Percent,
        Axis::Percent,
    )
    .process("gpu");
    gpu.recorded = true;
    out.push(gpu);
    let mut gpus: Vec<(&str, Option<&str>)> = Vec::new();
    for channel in channels {
        if let Some((id, _)) = device_of(&channel.key, "gpu")
            && !gpus.iter().any(|(known, _)| *known == id)
        {
            gpus.push((id, channel.label.as_deref()));
        }
    }
    let colors = [p.group_background, p.gpu];
    for (index, (id, adapter)) in gpus.iter().enumerate() {
        let number = format!("{} {index}", resource("gpu"));
        let key = |part: &str| format!("gpu.{id}.{part}");
        let color = if gpus.len() > 1 {
            colors[index % 2]
        } else {
            p.gpu
        };
        for (part, text, unit, axis, tone, total) in [
            (
                "util",
                "history.device.gpuUtil",
                Unit::Percent,
                Axis::Percent,
                color,
                None,
            ),
            (
                "dedicated",
                "history.device.gpuDedicated",
                Unit::Bytes,
                Axis::Total,
                p.group_users[2],
                Some("dedicatedTotal"),
            ),
            (
                "shared",
                "history.device.gpuShared",
                Unit::Bytes,
                Axis::Total,
                p.group_users[2],
                Some("sharedTotal"),
            ),
            (
                "temp",
                "history.device.deviceTemp",
                Unit::Celsius,
                Axis::Temperature,
                p.temperature,
                None,
            ),
            (
                "power",
                "history.device.gpuPower",
                Unit::Watts,
                Axis::Auto,
                p.group_users[0],
                None,
            ),
            (
                "clock",
                "history.device.gpuClock",
                Unit::Megahertz,
                Axis::Auto,
                p.cpu_e,
                None,
            ),
            (
                "fan",
                "history.device.gpuFan",
                Unit::Rpm,
                Axis::Auto,
                p.group_users[0],
                None,
            ),
        ] {
            if !has(&key(part)) {
                continue;
            }
            let long = named(text, &number);
            let mut reading = Reading::new(
                &key(part),
                if part == "util" {
                    number.clone()
                } else {
                    long.clone()
                },
                match adapter {
                    Some(adapter) if part == "util" => format!("{long} · {adapter}"),
                    _ => long,
                },
                Group::Gpu,
                tone,
                unit,
                axis,
            )
            .process("gpu")
            .of_device(
                &adapter.map_or_else(|| number.clone(), |adapter| format!("{number} · {adapter}")),
                text.trim_start_matches("history.device."),
            );
            reading.total = total.map(&key);
            reading.recorded = true;
            out.push(reading);
        }
    }

    // Battery.
    let battery_recorded = has("battery");
    let mut battery = Reading::new(
        "battery",
        resource("battery"),
        long("battery"),
        Group::Battery,
        p.battery,
        Unit::Percent,
        Axis::Fixed(0., 100.),
    );
    battery.recorded = battery_recorded;
    out.push(battery);
    for (key, unit, axis, color) in [
        (
            "batteryRate",
            Unit::Milliwatts,
            Axis::Signed,
            p.group_users[0],
        ),
        ("batteryVoltage", Unit::Volts, Axis::Auto, p.cpu_e),
        (
            "batteryTemp",
            Unit::Celsius,
            Axis::Temperature,
            p.temperature,
        ),
    ] {
        let mut reading = Reading::new(
            key,
            short(key),
            long(key),
            Group::Battery,
            color,
            unit,
            axis,
        );
        reading.recorded = has(key);
        out.push(reading);
    }

    // System counts.
    for key in ["processes", "threads", "handles"] {
        let mut reading = Reading::new(
            key,
            long(key),
            long(key),
            Group::System,
            p.muted,
            Unit::Count,
            Axis::Count,
        );
        reading.recorded = has(key);
        out.push(reading);
    }
    out
}

/// The tabs: CPU, memory, disk, network, GPU (each adapter when there are
/// several), battery when recorded.
/// What an observed process's run records (`argus_core::watch::WatchPoint`),
/// as readings: memory, processor, graphics, I/O and counts, in that order.
pub(super) fn watch_catalogue(p: &Palette) -> Vec<Reading> {
    let column = |id: &str| label(&format!("history.column.moment.{id}"));
    let long = |id: &str| label(&format!("history.reading.long.{id}"));
    let reading = |id: &str, name: String, long: String, group, color, unit, axis| {
        let mut reading = Reading::new(id, name, long, group, color, unit, axis);
        reading.recorded = true;
        reading
    };
    let memory = label("history.watch.metric.memory");
    let commit = label("history.watch.metric.commit");
    let working = label("details.workingSet");
    vec![
        reading(
            "memory",
            memory.clone(),
            memory,
            Group::Memory,
            p.memory,
            Unit::Bytes,
            Axis::Auto,
        ),
        reading(
            "commit",
            commit.clone(),
            commit,
            Group::Memory,
            p.cpu,
            Unit::Bytes,
            Axis::Auto,
        ),
        reading(
            "workingSet",
            working.clone(),
            working,
            Group::Memory,
            p.cpu_e,
            Unit::Bytes,
            Axis::Auto,
        ),
        reading(
            "cpu",
            column("cpu"),
            long("cpu"),
            Group::Cpu,
            p.cpu,
            Unit::Percent,
            Axis::Percent,
        ),
        reading(
            "gpu",
            column("gpu"),
            long("gpu"),
            Group::Gpu,
            p.gpu,
            Unit::Percent,
            Axis::Percent,
        ),
        reading(
            "gpuMemory",
            column("gpuMem"),
            column("gpuMem"),
            Group::Gpu,
            p.npu,
            Unit::Bytes,
            Axis::Auto,
        ),
        reading(
            "disk",
            column("disk"),
            long("disk"),
            Group::Disk,
            p.io,
            Unit::Rate,
            Axis::Auto,
        )
        .pair(
            "diskRead",
            "diskWrite",
            ("history.part.read", "history.part.write"),
        ),
        reading(
            "io",
            column("io"),
            column("io"),
            Group::Disk,
            p.battery,
            Unit::Rate,
            Axis::Auto,
        )
        .pair(
            "ioRead",
            "ioWrite",
            ("history.part.read", "history.part.write"),
        ),
        reading(
            "net",
            column("net"),
            long("network"),
            Group::Network,
            p.network,
            Unit::NetworkRate,
            Axis::Auto,
        )
        .pair(
            "netReceived",
            "netSent",
            ("history.part.receive", "history.part.send"),
        ),
        reading(
            "threads",
            column("threads"),
            long("threads"),
            Group::System,
            p.accent,
            Unit::Count,
            Axis::Count,
        ),
        reading(
            "handles",
            column("handles"),
            long("handles"),
            Group::System,
            p.temperature,
            Unit::Count,
            Axis::Count,
        ),
    ]
}

/// The observation's tabs; its other readings are in the chooser.
pub(super) const WATCH_TABS: [&str; 6] = ["memory", "commit", "cpu", "gpu", "disk", "net"];

pub(super) fn tab_ids(readings: &[Reading]) -> Vec<String> {
    let mut ids = vec![
        "cpu".to_owned(),
        "memUsed".into(),
        "disk".into(),
        "network".into(),
    ];
    let adapters = readings
        .iter()
        .filter(|r| r.id.starts_with("gpu.") && r.id.ends_with(".util"))
        .map(|r| r.id.clone())
        .collect::<Vec<_>>();
    if adapters.len() > 1 {
        ids.extend(adapters);
    } else {
        ids.push("gpu".into());
    }
    if readings.iter().any(|r| r.id == "battery" && r.recorded) {
        ids.push("battery".into());
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::device_of;

    #[test]
    fn device_ids_may_contain_dots() {
        assert_eq!(
            device_of("gpu.10de.2860.util", "gpu"),
            Some(("10de.2860", "util"))
        );
        assert_eq!(device_of("disk.0.read", "disk"), Some(("0", "read")));
        assert_eq!(device_of("diskRead", "disk"), None);
        assert_eq!(device_of("nic.{a-b}.send", "nic"), Some(("{a-b}", "send")));
    }
}
