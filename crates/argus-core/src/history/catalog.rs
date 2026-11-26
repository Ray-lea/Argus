//! Channels: the readings a snapshot records (`readings`), the catalog that
//! numbers them in a day file or the live buffer, and the channels recorded
//! over a range (`channels`).
use super::*;

/// Index in a file or live buffer is local to its catalog; the string key is permanent.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryChannel {
    pub key: String,
    pub unit: String,
    pub group: String,
    pub label: String,
    #[serde(rename = "deviceId")]
    pub device: Option<String>,
    pub service: bool,
    pub first: i64,
    pub last: i64,
}

#[derive(Default, Clone)]
pub(super) struct Catalog {
    pub(super) rows: Vec<HistoryChannel>,
    pub(super) by_key: HashMap<String, usize>,
    pub(super) offsets: Vec<usize>,
    pub(super) bytes: usize,
}

impl Catalog {
    pub(super) fn register(
        &mut self,
        key: &str,
        unit: &str,
        group: &str,
        label: &str,
        device: Option<&str>,
        ts: i64,
    ) -> Option<usize> {
        if let Some(&i) = self.by_key.get(key) {
            if self.rows[i].label != label {
                self.rows[i].label = label.into();
            }
            return Some(i);
        }
        let i = self.rows.len();
        if i >= MAX_CHANNELS {
            return None;
        }
        self.offsets.push(self.bytes);
        self.bytes += if unit == "bytes" { 8 } else { 4 };
        self.by_key.insert(key.into(), i);
        self.rows.push(HistoryChannel {
            key: key.into(),
            unit: unit.into(),
            group: group.into(),
            label: label.into(),
            service: key == "cpuVoltage" || key.starts_with("fan."),
            device: device.map(str::to_owned),
            first: ts,
            last: ts,
        });
        Some(i)
    }
}

// FNV-1a is specified here (unlike DefaultHasher) so the same hardware identity
// produces the same compact key across builds. Never persist raw serials or PnP paths.
fn hardware_id(identity: &str) -> String {
    let hash = identity.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b.to_ascii_lowercase())).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

pub(super) fn readings(catalog: &mut Catalog, s: &Snapshot, groups: RecordGroups) -> Vec<f64> {
    let sys = &s.system;
    let hw = &s.hardware;
    let sensors = hw.cpu_sensors.as_ref();
    let mut values = vec![f64::NEG_INFINITY; catalog.rows.len()];
    let mut set = |key: &str,
                   unit: &str,
                   group: &str,
                   label: &str,
                   device: Option<&str>,
                   value: Option<f64>| {
        if !groups.includes(key) {
            return;
        }
        // Leave room for the system totals registered after disk, NIC and GPU readings.
        if device.is_some()
            && catalog.rows.len() >= MAX_CHANNELS - 32
            && !catalog.by_key.contains_key(key)
        {
            return;
        }
        let Some(i) = catalog.register(key, unit, group, label, device, s.timestamp) else {
            return;
        };
        if i >= values.len() {
            values.resize(i + 1, f64::NEG_INFINITY);
        }
        values[i] = value.filter(|v| v.is_finite()).unwrap_or(f64::NAN);
    };
    for (key, unit, value) in [
        ("cpu", "percent", Some(sys.cpu as f64)),
        ("cpuPeak", "percent", Some(sys.cpu as f64)),
        ("cpuKernel", "percent", Some(sys.cpu_kernel as f64)),
        ("cpuClock", "megahertz", hw.cpu_mhz.map(f64::from)),
        (
            "cpuPower",
            "watts",
            hw.power.package.or(hw.power.cores).map(f64::from),
        ),
        (
            "cpuTemp",
            "celsius",
            sensors
                .and_then(|s| s.package_celsius)
                .or_else(|| {
                    sensors.and_then(|s| {
                        s.core_temperatures
                            .iter()
                            .map(|c| c.celsius)
                            .reduce(f32::max)
                    })
                })
                .or_else(|| hw.thermal.iter().map(|z| z.celsius).reduce(f32::max))
                .map(f64::from),
        ),
        (
            "cpuVoltage",
            "volts",
            sensors.and_then(|s| s.core_voltage).map(f64::from),
        ),
    ] {
        set(key, unit, "cpu", "", None, value);
    }
    if let Some(sensor) = sensors {
        for f in &sensor.board_fans {
            let id = f.channel.to_string();
            let label = match &sensor.board_sensor {
                Some(chip) => format!("{chip} #{}", u16::from(f.channel) + 1),
                None => format!("#{}", u16::from(f.channel) + 1),
            };
            set(
                &format!("fan.{id}"),
                "rpm",
                "cpu",
                &label,
                Some(&id),
                Some(f.rpm.into()),
            );
        }
    }
    for (key, value) in [
        ("memUsed", Some(sys.mem_used)),
        ("memTotal", Some(sys.mem_total)),
        ("commitUsed", Some(sys.commit_used)),
        ("commitLimit", Some(sys.commit_limit)),
        ("memCached", Some(sys.cached)),
        ("memCompressed", sys.memory_compression.map(|c| c.resident)),
        ("pagedPool", Some(sys.kernel_paged)),
        ("nonpagedPool", Some(sys.kernel_nonpaged)),
    ] {
        set(key, "bytes", "memory", "", None, value.map(|v| v as f64));
    }
    for (key, v) in [
        ("processes", sys.process_count),
        ("threads", sys.thread_count),
        ("handles", sys.handle_count),
    ] {
        set(key, "count", "system", "", None, Some(v.into()));
    }
    let mut dr = (!hw.disks.is_empty()).then_some(0.);
    let mut dw = dr;
    for d in &hw.disks {
        dr = dr.zip(d.read).map(|(a, b)| a + b as f64);
        dw = dw.zip(d.write).map(|(a, b)| a + b as f64);
        let id = if !d.info.history_id.is_empty() {
            hardware_id(&d.info.history_id)
        } else if !d.info.model.is_empty()
            && hw
                .disks
                .iter()
                .filter(|other| other.info.model == d.info.model && other.info.size == d.info.size)
                .count()
                == 1
        {
            hardware_id(&format!("{}-{}", d.info.model, d.info.size))
        } else {
            continue;
        };
        for (metric, unit, v) in [
            ("read", "bytesPerSecond", d.read.map(|v| v as f64)),
            ("write", "bytesPerSecond", d.write.map(|v| v as f64)),
            ("active", "percent", d.active.map(f64::from)),
            ("response", "milliseconds", d.response_ms.map(f64::from)),
            ("temp", "celsius", d.temperature.map(f64::from)),
        ] {
            set(
                &format!("disk.{id}.{metric}"),
                unit,
                "disk",
                &d.info.model,
                Some(&id),
                v,
            );
        }
    }
    set("diskRead", "bytesPerSecond", "disk", "", None, dr);
    set("diskWrite", "bytesPerSecond", "disk", "", None, dw);
    let (mut rx, mut tx) = (0., 0.);
    for n in &hw.nics {
        rx += n.rx as f64;
        tx += n.tx as f64;
        let id = &n.info.history_guid;
        for (metric, v) in [("receive", n.rx), ("send", n.tx)] {
            set(
                &format!("nic.{id}.{metric}"),
                "bytesPerSecond",
                "network",
                &n.info.name,
                Some(id),
                Some(v as f64),
            );
        }
    }
    set(
        "netReceive",
        "bytesPerSecond",
        "network",
        "",
        None,
        Some(rx),
    );
    set("netSend", "bytesPerSecond", "network", "", None, Some(tx));
    set(
        "gpu",
        "percent",
        "gpu",
        "",
        None,
        Some(hw.gpus.iter().map(|g| g.util).fold(0., f32::max) as f64),
    );
    for g in &hw.gpus {
        let id = if let Some(identity) = &g.info.history_id {
            hardware_id(identity)
        } else if !g.info.name.is_empty()
            && hw
                .gpus
                .iter()
                .filter(|other| other.info.name == g.info.name)
                .count()
                == 1
        {
            hardware_id(&g.info.name)
        } else {
            continue;
        };
        for (metric, unit, v) in [
            ("util", "percent", Some(g.util as f64)),
            ("dedicated", "bytes", Some(g.dedicated_used as f64)),
            ("shared", "bytes", Some(g.shared_used as f64)),
            (
                "dedicatedTotal",
                "bytes",
                Some(g.info.dedicated_total as f64),
            ),
            ("sharedTotal", "bytes", Some(g.info.shared_total as f64)),
            ("temp", "celsius", g.temperature.map(f64::from)),
            ("power", "watts", g.board_power.map(f64::from)),
            ("clock", "megahertz", g.clock_mhz.map(f64::from)),
            ("fan", "rpm", g.fan_rpm.map(f64::from)),
        ] {
            set(
                &format!("gpu.{id}.{metric}"),
                unit,
                "gpu",
                &g.info.name,
                Some(&id),
                v,
            );
        }
    }
    let mean = |values: &[f64]| {
        (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
    };
    let bs = &hw.batteries;
    for (key, unit, v) in [
        (
            "battery",
            "percent",
            mean(&bs.iter().map(|b| b.percent as f64).collect::<Vec<_>>()),
        ),
        (
            "batteryRate",
            "milliwatts",
            bs.iter()
                .filter_map(|b| b.rate.map(f64::from))
                .reduce(|a, b| a + b),
        ),
        (
            "batteryVoltage",
            "volts",
            mean(
                &bs.iter()
                    .filter_map(|b| b.voltage.map(|mv| f64::from(mv) / 1000.))
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            "batteryTemp",
            "celsius",
            mean(
                &bs.iter()
                    .filter_map(|b| b.temperature.map(f64::from))
                    .collect::<Vec<_>>(),
            ),
        ),
    ] {
        set(key, unit, "battery", "", None, v);
    }
    values
}

pub fn channels(dir: &Path, from: i64, to: i64) -> Vec<HistoryChannel> {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    if from > to {
        return Vec::new();
    }
    let mut result = BTreeMap::<String, HistoryChannel>::new();
    let mut merge = |row: &HistoryChannel, first: i64, last: i64, label: &str| {
        if result.len() >= MAX_POINTS && !result.contains_key(&row.key) {
            return false;
        }
        let c = result
            .entry(row.key.clone())
            .or_insert_with(|| HistoryChannel {
                first,
                last,
                ..row.clone()
            });
        c.first = c.first.min(first);
        if last >= c.last {
            c.last = last;
            if c.label != label {
                c.label = label.into();
            }
        }
        true
    };
    for day in days(dir)
        .into_iter()
        .filter(|&d| d >= from.div_euclid(DAY_MS) && d <= to.div_euclid(DAY_MS))
    {
        let Some(idx) = index(&day_path(dir, day)) else {
            continue;
        };
        let start = day * DAY_MS;
        let from = if idx.version == MINUTE_VERSION {
            align(from, 0, MINUTE_MS)
        } else {
            from
        };
        let ts = |i: usize| start + i64::from(idx.frames[i].0);
        let first = idx
            .frames
            .partition_point(|&(t, _)| start + i64::from(t) < from);
        let end = idx
            .frames
            .partition_point(|&(t, _)| start + i64::from(t) <= to);
        if end <= first {
            continue;
        }
        let mut bounds = vec![None; idx.catalog.rows.len()];
        let mut observe = |j: usize, a: usize, b: usize| {
            bounds[j].get_or_insert((ts(a), ts(a))).1 = ts(b);
        };
        for (m, (begin, mask)) in idx.masks.iter().enumerate() {
            let a = first.max(*begin);
            let b = end.min(idx.masks.get(m + 1).map_or(end, |e| e.0));
            if a >= b {
                continue;
            }
            for j in 0..idx.catalog.rows.len() {
                if mask
                    .get(j / 8)
                    .is_some_and(|byte| byte & (1 << (j % 8)) != 0)
                {
                    observe(j, a, b - 1);
                }
            }
        }
        for (j, bounds) in bounds.into_iter().enumerate() {
            if let Some((first, last)) = bounds {
                let c = &idx.catalog.rows[j];
                let label = idx
                    .labels
                    .iter()
                    .rev()
                    .find(|(t, i, _)| *i == j && *t <= last)
                    .map_or(c.label.as_str(), |e| e.2.as_str());
                if !merge(c, first, last, label) {
                    return Vec::new();
                }
            }
        }
    }
    let live = live(dir);
    let (front, back) = live.samples.as_slices();
    for samples in [front, back]
        .into_iter()
        .flat_map(|part| part.chunk_by(|a, b| Arc::ptr_eq(&a.catalog, &b.catalog)))
    {
        let Some(sample) = samples.first() else {
            continue;
        };
        for (j, c) in sample.catalog.iter().enumerate() {
            let mut observed = samples.iter().filter(|s| {
                (from..=to).contains(&s.ts)
                    && s.values.get(j).is_some_and(|&v| v != f64::NEG_INFINITY)
            });
            if let Some(first) = observed.next() {
                let last = observed.next_back().unwrap_or(first);
                if !merge(c, first.ts, last.ts, &c.label) {
                    return Vec::new();
                }
            }
        }
    }
    result.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{battery, snapshot, temp_dir};

    #[test]
    fn records_battery_level_and_rate() {
        let dir = temp_dir("battery");
        let base = 20_737 * DAY_MS + 3_600_000;
        let mut rec = HistoryRecorder::new(dir.clone());
        // Frame A: 40% drawing 10 W; frame B: 50% charging 5 W.
        for i in 0..10 {
            let mut s = snapshot(base + i * 1000, 5.0, Vec::new());
            let (pct, rate) = if i < 5 {
                (40.0, Some(-10_000))
            } else {
                (50.0, Some(5_000))
            };
            s.hardware.batteries.push(battery(pct, rate));
            rec.record(&s);
        }
        // A machine without a battery writes frames without battery data.
        // (The sample at +15 s only closes frame C; frames need a successor.)
        for i in 10..16 {
            rec.record(&snapshot(base + i * 1000, 5.0, Vec::new()));
        }
        drop(rec);
        let m = moment(&dir, base + 1_000).unwrap();
        assert_eq!(m.system.battery, Some(40.0));
        assert_eq!(m.system.battery_rate, Some(-10_000.0));
        let m = moment(&dir, base + 6_000).unwrap();
        assert_eq!(m.system.battery, Some(50.0));
        assert_eq!(m.system.battery_rate, Some(5_000.0));
        // The battery-less frames keep both absent.
        let m = moment(&dir, base + 12_000).unwrap();
        assert_eq!(m.system.battery, None);
        assert_eq!(m.system.battery_rate, None);
        // Buckets carry the average of the frames that had a battery.
        let points = legacy_series(&dir, base, base + 15_000, 3);
        assert_eq!(points[0].battery, Some(40.0));
        assert_eq!(points[1].battery, Some(50.0));
        assert_eq!(points[2].battery, None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn averages_multiple_batteries() {
        let dir = temp_dir("batteries");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        // Six one-second samples: the first frame is complete and written.
        for i in 0..6 {
            let mut s = snapshot(base + i * 1000, 5.0, Vec::new());
            // Two batteries: 20% drawing 8 W, 80% with an unknown rate.
            s.hardware.batteries.push(battery(20.0, Some(-8_000)));
            s.hardware.batteries.push(battery(80.0, None));
            rec.record(&s);
        }
        drop(rec);
        let m = moment(&dir, base).unwrap();
        assert_eq!(m.system.battery, Some(50.0));
        assert_eq!(m.system.battery_rate, Some(-8_000.0));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn adds_channels_and_preserves_unknowns_across_midnight() {
        let dir = temp_dir("channels");
        let midnight = 20_738 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for n in 0..20 {
            let mut s = snapshot(midnight - 10_000 + n * 1000, n as f32, vec![]);
            s.system.mem_used = (n as u64 + 1) * 1024;
            if n >= 5 {
                s.hardware.cpu_sensors = Some(crate::hardware::CpuSensors {
                    board_sensor: Some(if n < 10 { "chip-a" } else { "chip-b" }.into()),
                    board_fans: vec![crate::hardware::CpuFanReading {
                        channel: 2,
                        rpm: 900.,
                    }],
                    core_voltage: (n % 2 == 1).then_some(1.25),
                    ..Default::default()
                });
            }
            rec.record(&s);
        }
        drop(rec);
        let list = channels(&dir, midnight - 10_000, midnight + 8_000);
        let f = list.iter().find(|c| c.key == "fan.2").unwrap();
        assert_eq!((f.first, f.last), (midnight - 5000, midnight + 5000));
        assert!(f.service);
        assert_eq!(f.device.as_deref(), Some("2"));
        assert!(f.label.starts_with("chip-b"));
        assert!(channels(&dir, midnight - 9000, midnight - 1)[0].last < midnight);
        assert!(
            channels(&dir, midnight - 10_000, midnight - 6000)
                .iter()
                .all(|c| c.key != "fan.2")
        );
        assert_eq!(frame(&dir, midnight - 1000).unwrap().ts, midnight);
        assert_eq!(
            frame(&dir, midnight - 5000).unwrap().system["cpuVoltage"],
            Some(1.25)
        );
        assert_eq!(
            frame(&dir, midnight - 10_000).unwrap().system["memUsed"],
            Some(3072.)
        );
        let keys = ["fan.2".into(), "missing".into(), "cpuPeak".into()];
        let points = series(
            &dir,
            midnight - 10_000,
            midnight + 9999,
            10_000,
            midnight,
            &keys,
        );
        assert_eq!(points.series["fan.2"].mean, vec![Some(900.); 2]);
        assert_eq!(points.series["missing"].mean, vec![None; 2]);
        assert_eq!(points.series["cpuPeak"].low, vec![Some(4.), Some(14.)]);
        assert_eq!(points.series["cpuPeak"].high, vec![Some(9.), Some(19.)]);
        assert_eq!(points.peak, vec![midnight - 5000, midnight + 5000]);
        fs::remove_dir_all(dir).unwrap();
    }
}
