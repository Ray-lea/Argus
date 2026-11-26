//! The original JSON structures that v3 frames are projected back into.
use super::*;

/// System totals for one frame (or one chart bucket).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    /// Unix ms: the frame's start, or a bucket's average.
    pub ts: i64,
    /// An actual frame, chosen as the first frame of a bucket. Unlike `ts`,
    /// this timestamp can always address its process table.
    pub frame_ts: i64,
    /// Actual frame containing the highest CPU sample; earliest frame on ties.
    pub cpu_peak_ts: i64,
    /// Average CPU, percent.
    pub cpu: f32,
    /// Highest CPU sample in the frame (or bucket).
    pub cpu_peak: f32,
    /// Busiest GPU engine, percent.
    pub gpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Bytes per second from available disk counters.
    pub disk: u64,
    /// Bytes per second, all adapters.
    pub net: u64,
    /// Battery charge, percent, averaged over the frame; None on machines
    /// without a battery (or in frames recorded while it was absent).
    pub battery: Option<f32>,
    /// Battery power flow, mW (positive while charging), averaged over the
    /// frame; None when no battery reports a rate.
    pub battery_rate: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryProcess {
    pub name: String,
    pub pid: u32,
    /// None in legacy history or when the instance could not be identified.
    pub create_time: Option<i64>,
    /// CPU, I/O, GPU and network are averages over the frame.
    pub cpu: f32,
    /// Maximum private working set in the frame, bytes.
    pub mem: u64,
    pub io: u64,
    pub gpu: f32,
    pub net: u64,
}

/// One frame: the system and the processes that stood out at that moment.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMoment {
    pub system: HistoryPoint,
    pub processes: Vec<HistoryProcess>,
}

/// System totals between `from` and `to` (Unix ms), averaged into at most
/// `buckets` points; empty buckets are left out, so gaps (Argus not running)
/// stay gaps. The CPU peak is the highest of the bucket.
pub fn legacy_series(dir: &Path, from: i64, to: i64, buckets: usize) -> Vec<HistoryPoint> {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    if to <= from || !(1..=MAX_POINTS).contains(&buckets) {
        return Vec::new();
    }
    #[derive(Default, Clone)]
    struct Acc {
        n: u64,
        ts: f64,
        frame_ts: i64,
        cpu_peak_ts: i64,
        cpu: f64,
        cpu_peak: f32,
        gpu: f64,
        mem_used: f64,
        mem_total: u64,
        disk: f64,
        net: f64,
        battery: f64,
        battery_n: u64,
        battery_rate: f64,
        battery_rate_n: u64,
    }
    let mut acc = vec![Acc::default(); buckets];
    let span = to.abs_diff(from) as f64;
    visit_days(dir, align(from, 0, MINUTE_MS), to, false, |d| {
        for i in d.range.clone() {
            let Some(row) = d.row(i) else { continue };
            let p = legacy_point(row.ts(), |k| finite(row.get(*d.idx.catalog.by_key.get(k)?)));
            if (p.ts >= from || (row.span() == MINUTE_MS && p.ts + MINUTE_MS > from)) && p.ts <= to
            {
                let i = ((p.ts.max(from).abs_diff(from) as f64 / span) * buckets as f64) as usize;
                let a = &mut acc[i.min(buckets - 1)];
                if a.n == 0 {
                    a.frame_ts = p.ts;
                }
                if a.n == 0
                    || p.cpu_peak > a.cpu_peak
                    || (p.cpu_peak == a.cpu_peak && p.ts < a.cpu_peak_ts)
                {
                    a.cpu_peak_ts = p.ts;
                }
                a.n += 1;
                a.ts += p.ts as f64;
                a.cpu += p.cpu as f64;
                a.cpu_peak = a.cpu_peak.max(p.cpu_peak);
                a.gpu += p.gpu as f64;
                a.mem_used += p.mem_used as f64;
                a.mem_total = p.mem_total;
                a.disk += p.disk as f64;
                a.net += p.net as f64;
                if let Some(v) = p.battery {
                    a.battery += v as f64;
                    a.battery_n += 1;
                }
                if let Some(v) = p.battery_rate {
                    a.battery_rate += v as f64;
                    a.battery_rate_n += 1;
                }
            }
        }
    });
    acc.into_iter()
        .filter(|a| a.n > 0)
        .map(|a| {
            let n = a.n as f64;
            HistoryPoint {
                ts: (a.ts / n) as i64,
                frame_ts: a.frame_ts,
                cpu_peak_ts: a.cpu_peak_ts,
                cpu: (a.cpu / n) as f32,
                cpu_peak: a.cpu_peak,
                gpu: (a.gpu / n) as f32,
                mem_used: (a.mem_used / n) as u64,
                mem_total: a.mem_total,
                disk: (a.disk / n) as u64,
                net: (a.net / n) as u64,
                battery: (a.battery_n > 0).then_some((a.battery / a.battery_n as f64) as f32),
                battery_rate: (a.battery_rate_n > 0)
                    .then_some((a.battery_rate / a.battery_rate_n as f64) as f32),
            }
        })
        .collect()
}

impl From<FrameProcess> for HistoryProcess {
    fn from(p: FrameProcess) -> Self {
        Self {
            name: p.name,
            pid: p.pid,
            create_time: p.create_time,
            cpu: p.cpu,
            gpu: p.gpu,
            mem: p.mem,
            io: p.io as u64,
            net: p.net as u64,
        }
    }
}

fn legacy_point(ts: i64, get: impl Fn(&str) -> Option<f64>) -> HistoryPoint {
    HistoryPoint {
        ts,
        frame_ts: ts,
        cpu_peak_ts: ts,
        cpu: get("cpu").unwrap_or_default() as f32,
        cpu_peak: get("cpuPeak").unwrap_or_default() as f32,
        gpu: get("gpu").unwrap_or_default() as f32,
        mem_used: get("memUsed").unwrap_or_default() as u64,
        mem_total: get("memTotal").unwrap_or_default() as u64,
        disk: get("disk")
            .or_else(|| Some(get("diskRead")? + get("diskWrite")?))
            .unwrap_or_default() as u64,
        net: get("network")
            .or_else(|| Some(get("netReceive")? + get("netSend")?))
            .unwrap_or_default() as u64,
        battery: get("battery").map(|v| v as f32),
        battery_rate: get("batteryRate").map(|v| v as f32),
    }
}

pub fn moment(dir: &Path, ts: i64) -> Option<HistoryMoment> {
    let f = frame(dir, ts)?;
    Some(HistoryMoment {
        system: legacy_point(f.ts, |k| f.system.get(k).copied().flatten()),
        processes: f.processes.into_iter().map(HistoryProcess::from).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{proc, snapshot, temp_dir};

    #[test]
    fn bucket_addresses_a_real_frame_and_the_earliest_cpu_peak() {
        let dir = temp_dir("peak-frame");
        let base = 20_738 * DAY_MS - FRAME_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for (offset, cpu, name) in [
            (0, 98., "burst.exe"),
            (5000, 2., "quiet.exe"),
            (10000, 2., "quiet.exe"),
            (15000, 0., "end.exe"),
        ] {
            rec.record(&snapshot(
                base + offset,
                cpu,
                vec![proc(42, name, cpu, 1024)],
            ));
        }
        let bucket = legacy_series(&dir, base, base + 10000, 1).remove(0);
        assert_eq!((bucket.cpu, bucket.cpu_peak), (34., 98.));
        assert_eq!(bucket.ts, base + 5000);
        assert_eq!(bucket.cpu_peak_ts, base);
        assert_eq!(bucket.frame_ts, base);
        assert_eq!(
            moment(&dir, bucket.cpu_peak_ts).unwrap().processes[0].name,
            "burst.exe"
        );
        // Equal peaks keep the earliest actual frame, including across UTC midnight.
        rec.clear().unwrap();
        for offset in [0, 5000, 10000] {
            rec.record(&snapshot(base + offset, 98., Vec::new()));
        }
        assert_eq!(
            legacy_series(&dir, base, base + 10000, 1)[0].cpu_peak_ts,
            base
        );
        // An average timestamp can lie in a long gap. Its representative cannot.
        rec.clear().unwrap();
        for offset in [0, 600000, 605000] {
            rec.record(&snapshot(
                base + offset,
                1.,
                vec![proc(42, "sample.exe", 1., 1024)],
            ));
        }
        let bucket = legacy_series(&dir, base, base + 7 * DAY_MS, 500).remove(0);
        assert!(moment(&dir, bucket.ts).is_none());
        assert!(moment(&dir, bucket.frame_ts).is_some());
        assert!(moment(&dir, bucket.cpu_peak_ts).is_some());
        drop(rec);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn records_and_reads_back() {
        let dir = temp_dir("rw");
        let base = 20_737 * DAY_MS + 3_600_000; // 2026-10-11 01:00 UTC
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..12 {
            let ts = base + i * 1000;
            rec.record(&snapshot(
                ts,
                10.0 + i as f32,
                vec![
                    proc(42, "busy.exe", 30.0, 1 << 30),
                    proc(7, "idle.exe", 0.0, 10 << 20),
                ],
            ));
        }
        drop(rec);
        // Shutdown also persists the two-sample final frame.
        let (first, last) = range(&dir).unwrap();
        assert_eq!((first, last), (base, base + 10_000));
        let m = moment(&dir, base + 1_000).unwrap();
        assert_eq!(m.system.ts, base);
        assert!((m.system.cpu - 12.0).abs() < 0.01); // average of 10..=14
        assert_eq!(m.system.cpu_peak, 14.0);
        let busy = m.processes.iter().find(|p| p.name == "busy.exe").unwrap();
        assert_eq!((busy.pid, busy.cpu, busy.mem), (42, 30.0, 1 << 30));
        let points = legacy_series(&dir, base, base + 10_000, 100);
        assert_eq!(points.len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }
}
