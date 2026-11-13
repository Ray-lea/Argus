//! Recorded interval analysis; source coverage and process identities remain separate.
mod resources;
use super::{DAY_MS, FRAME_MS, HistoryPoint, KEEP_DAYS, days, entries, parse, supported};
use resources::ResourceAcc;
pub use resources::ResourceSummary;
use serde::Deserialize;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
};

const FILE_BYTES: u64 = 256 << 20;
const MAX_IDENTITIES: usize = 4096;
const MAX_LEADERS: usize = 100;
const MAX_GAPS: usize = 128;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuSummary {
    /// Mean of available frame averages, never a mean with missing frames set to zero.
    pub mean: Option<f64>,
    pub peak: Option<f32>,
    /// Start of the frame containing the recorded sample peak.
    pub peak_ts: Option<i64>,
    pub valid_frames: usize,
    pub invalid_frames: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuLeader {
    pub name: String,
    pub pid: u32,
    pub create_time: Option<i64>,
    /// Unknown legacy instances are separate for every frame.
    pub first_ts: i64,
    pub last_ts: i64,
    pub observed_frames: usize,
    /// Process peaks are peaks of frame averages, not instantaneous CPU samples.
    pub cpu: CpuSummary,
    /// Sum of recorded frame means, used only to rank this interval's known entries.
    /// Sampling durations and missing top-list entries do not establish actual CPU time.
    pub recorded_cpu_score: f64,
    /// History stores per-frame private working set maxima, so only a peak is reported.
    pub private_ws_peak: u64,
    pub io: ResourceSummary,
    pub gpu: ResourceSummary,
    pub network: ResourceSummary,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryGap {
    pub from: i64,
    pub to: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntervalReport {
    pub metric: IntervalMetric,
    pub requested_from: i64,
    pub requested_to: i64,
    /// Frame-aligned half-open interval [from, to).
    pub from: i64,
    pub to: i64,
    pub frame_ms: i64,
    pub expected_frames: usize,
    pub recorded_frames: usize,
    pub missing_frames: usize,
    pub gaps: Vec<HistoryGap>,
    pub gap_count: usize,
    pub cpu: CpuSummary,
    pub memory: ResourceSummary,
    pub disk: ResourceSummary,
    pub gpu: ResourceSummary,
    pub network: ResourceSummary,
    pub leaders: Vec<CpuLeader>,
    /// Targeted saved memory observations, independent of presence in ranked history.
    pub observations: crate::watch::ObservationInterval,
    pub identities_seen: usize,
    pub omitted_entries: usize,
    pub incomplete_files: usize,
    pub unreadable_files: usize,
    pub invalid_frames: usize,
}

pub type CpuInterval = IntervalReport;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntervalMetric {
    Cpu,
    Memory,
    Io,
    Gpu,
    Network,
}

#[derive(Default)]
struct CpuAcc {
    sum: f64,
    n: usize,
    invalid: usize,
    peak: Option<(f32, i64)>,
}
impl CpuAcc {
    fn add(&mut self, mean: f32, peak: f32, ts: i64) {
        if !(mean.is_finite()
            && peak.is_finite()
            && (0. ..=100.).contains(&mean)
            && (mean..=100.).contains(&peak))
        {
            self.invalid += 1;
            return;
        }
        self.sum += f64::from(mean);
        self.n += 1;
        if self
            .peak
            .is_none_or(|(value, at)| peak > value || (peak == value && ts < at))
        {
            self.peak = Some((peak, ts));
        }
    }
    fn finish(&self) -> CpuSummary {
        CpuSummary {
            mean: (self.n > 0).then(|| self.sum / self.n as f64),
            peak: self.peak.map(|(value, _)| value),
            peak_ts: self.peak.map(|(_, ts)| ts),
            valid_frames: self.n,
            invalid_frames: self.invalid,
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Identity {
    Known(u32, i64),
    Legacy(u32, String, i64),
}
struct Leader {
    name: String,
    pid: u32,
    created: Option<i64>,
    first: i64,
    last: i64,
    observed: usize,
    cpu: CpuAcc,
    memory_peak: u64,
    io: ResourceAcc,
    gpu: ResourceAcc,
    network: ResourceAcc,
}

/// Read-only CPU-first interval investigation. Other resources require their own coverage.
/// Bounds are expanded to complete five-second frames and are returned explicitly.
pub fn cpu_interval(dir: &Path, from: i64, to: i64) -> Option<CpuInterval> {
    interval(dir, from, to, IntervalMetric::Cpu)
}

pub fn interval(dir: &Path, from: i64, to: i64, metric: IntervalMetric) -> Option<IntervalReport> {
    if from < 0 || to <= from || to.checked_sub(from)? > KEEP_DAYS * DAY_MS {
        return None;
    }
    let requested_from = from;
    let requested_to = to;
    let from = from - from.rem_euclid(FRAME_MS);
    let to = to.checked_add(FRAME_MS - 1)? / FRAME_MS * FRAME_MS;
    let mut seen = BTreeSet::new();
    let mut leaders: BTreeMap<Identity, Leader> = BTreeMap::new();
    let mut cpu = CpuAcc::default();
    let (mut memory, mut disk, mut gpu, mut network) = (
        ResourceAcc::default(),
        ResourceAcc::default(),
        ResourceAcc::default(),
        ResourceAcc::default(),
    );
    let mut omitted_entries = 0;
    let mut incomplete_files = 0;
    let mut unreadable_files = 0;
    let mut invalid_frames = 0;
    for day in days(dir) {
        if day < from.div_euclid(DAY_MS) || day > (to - 1).div_euclid(DAY_MS) {
            continue;
        }
        // Newer formats win for overlapping frame timestamps, as in series().
        for path in [
            super::day_path(dir, day),
            dir.join(format!("{}.v4.hist", super::day_name(day))),
            dir.join(format!("{}.hist", super::day_name(day))),
        ] {
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    unreadable_files += 1;
                    continue;
                }
            };
            let mut bytes = Vec::new();
            if !file.metadata().is_ok_and(|meta| meta.len() <= FILE_BYTES)
                || file.take(FILE_BYTES + 1).read_to_end(&mut bytes).is_err()
                || bytes.len() as u64 > FILE_BYTES
                || !supported(&bytes)
            {
                unreadable_files += 1;
                continue;
            }
            let version = bytes[4];
            let valid_prefix = parse(
                &bytes,
                &mut Vec::new(),
                |point: &HistoryPoint, raw, names| {
                    if point.ts < from || point.ts >= to {
                        return true;
                    }
                    if point.ts.rem_euclid(FRAME_MS) != 0 {
                        invalid_frames += 1;
                        return true;
                    }
                    if !seen.insert(point.ts) {
                        return true;
                    }
                    cpu.add(point.cpu, point.cpu_peak, point.ts);
                    memory.level(
                        point.mem_used,
                        point.mem_total > 0 && point.mem_used <= point.mem_total,
                        point.ts,
                    );
                    disk.measured(
                        point.disk as f64,
                        point.disk_coverage.as_ref(),
                        point.ts,
                        None,
                    );
                    gpu.measured(
                        f64::from(point.gpu),
                        point.gpu_coverage.as_ref(),
                        point.ts,
                        Some(100.),
                    );
                    network.measured(
                        point.net as f64,
                        point.net_coverage.as_ref(),
                        point.ts,
                        None,
                    );
                    let mut frame_seen = BTreeSet::new();
                    for process in entries(raw, names, version) {
                        let key = match process.create_time.filter(|created| *created > 0) {
                            Some(created) => Identity::Known(process.pid, created),
                            None => Identity::Legacy(process.pid, process.name.clone(), point.ts),
                        };
                        if !frame_seen.insert(match &key {
                            Identity::Known(pid, created) => (*pid, *created),
                            Identity::Legacy(pid, _, _) => (*pid, 0),
                        }) {
                            omitted_entries += 1;
                            continue;
                        }
                        if !leaders.contains_key(&key) && leaders.len() >= MAX_IDENTITIES {
                            omitted_entries += 1;
                            continue;
                        }
                        let leader = leaders.entry(key).or_insert_with(|| Leader {
                            name: process.name.clone(),
                            pid: process.pid,
                            created: process.create_time.filter(|created| *created > 0),
                            first: point.ts,
                            last: point.ts,
                            observed: 0,
                            cpu: CpuAcc::default(),
                            memory_peak: 0,
                            io: ResourceAcc::default(),
                            gpu: ResourceAcc::default(),
                            network: ResourceAcc::default(),
                        });
                        leader.first = leader.first.min(point.ts);
                        leader.last = leader.last.max(point.ts);
                        leader.observed += 1;
                        leader.cpu.add(process.cpu, process.cpu, point.ts);
                        leader.memory_peak = leader.memory_peak.max(process.mem);
                        leader.io.measured(
                            process.io as f64,
                            process.io_coverage.as_ref(),
                            point.ts,
                            None,
                        );
                        leader.gpu.measured(
                            f64::from(process.gpu),
                            process.gpu_coverage.as_ref(),
                            point.ts,
                            Some(100.),
                        );
                        leader.network.measured(
                            process.net as f64,
                            process.net_coverage.as_ref(),
                            point.ts,
                            None,
                        );
                    }
                    true
                },
            );
            if valid_prefix < bytes.len() {
                incomplete_files += 1;
            }
        }
    }
    let expected_frames = ((to - from) / FRAME_MS) as usize;
    let recorded_frames = seen.len();
    let mut gaps = Vec::new();
    let mut gap_count = 0;
    let mut next = from;
    for ts in seen.iter().copied().chain(std::iter::once(to)) {
        if ts > next {
            gap_count += 1;
            if gaps.len() < MAX_GAPS {
                gaps.push(HistoryGap { from: next, to: ts });
            }
        }
        next = ts.saturating_add(FRAME_MS);
    }
    let identities_seen = leaders.len();
    let mut leaders: Vec<_> = leaders
        .into_values()
        .map(|leader| CpuLeader {
            name: leader.name,
            pid: leader.pid,
            create_time: leader.created,
            first_ts: leader.first,
            last_ts: leader.last,
            observed_frames: leader.observed,
            cpu: leader.cpu.finish(),
            recorded_cpu_score: leader.cpu.sum,
            private_ws_peak: leader.memory_peak,
            io: leader.io.finish(),
            gpu: leader.gpu.finish(),
            network: leader.network.finish(),
        })
        .collect();
    let score = |leader: &CpuLeader| match metric {
        IntervalMetric::Cpu => (leader.cpu.valid_frames > 0).then_some(leader.recorded_cpu_score),
        IntervalMetric::Memory => Some(leader.private_ws_peak as f64),
        IntervalMetric::Io => leader.io.recorded_score,
        IntervalMetric::Gpu => leader.gpu.recorded_score,
        IntervalMetric::Network => leader.network.recorded_score,
    };
    leaders.sort_by(|a, b| {
        match (score(a), score(b)) {
            (Some(a), Some(b)) => b.total_cmp(&a),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| a.first_ts.cmp(&b.first_ts))
        .then_with(|| a.pid.cmp(&b.pid))
        .then_with(|| a.create_time.cmp(&b.create_time))
    });
    leaders.truncate(MAX_LEADERS);
    Some(IntervalReport {
        metric,
        requested_from,
        requested_to,
        from,
        to,
        frame_ms: FRAME_MS,
        expected_frames,
        recorded_frames,
        missing_frames: expected_frames.saturating_sub(recorded_frames),
        gaps,
        gap_count,
        cpu: cpu.finish(),
        memory: memory.finish(),
        disk: disk.finish(),
        gpu: gpu.finish(),
        network: network.finish(),
        leaders,
        observations: crate::watch::observation_interval(dir, from, to)?,
        identities_seen,
        omitted_entries,
        incomplete_files,
        unreadable_files,
        invalid_frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProcessRow, Snapshot, history::HistoryRecorder};

    fn snapshot(ts: i64, cpu: f32, pid: u32, created: i64, name: &str) -> Snapshot {
        Snapshot {
            seq: 1,
            timestamp: ts,
            system: crate::system::SystemStats {
                cpu,
                ..Default::default()
            },
            hardware: Default::default(),
            // This controlled history fixture has no logged-on users.
            sessions: Vec::new(),
            process_network: false,
            processes: vec![ProcessRow {
                pid,
                create_time: created,
                name: name.into(),
                cpu,
                private_ws: 1024,
                ..Default::default()
            }],
        }
    }
    #[test]
    fn adjacent_compile_and_copy_phases_do_not_mix_and_absence_is_not_zero() {
        use crate::hardware::{DiskInfo, DiskStats};
        use crate::measurement::Measurement;
        use std::sync::Arc;
        let phase = |ts, cpu, pid, created, name, io, disk| {
            let mut sample = snapshot(ts, cpu, pid, created, name);
            sample.system.mem_used = 1 << 30;
            sample.system.mem_total = 2 << 30;
            sample.processes[0].io_valid = true;
            sample.processes[0].io_read = io;
            sample.hardware.disks.push(DiskStats {
                info: Arc::new(DiskInfo {
                    index: 0,
                    model: "controlled disk".into(),
                    size: 1 << 30,
                    bus: "nvme",
                    ssd: Some(true),
                    removable: false,
                    volumes: vec![],
                }),
                active: Some(0.),
                read: Some(disk),
                write: Some(0),
                counter_state: Measurement::ready(),
                response_ms: None,
                temperature: None,
                temperature_warning: None,
                performance_volumes: None,
            });
            sample
        };
        let dir =
            std::env::temp_dir().join(format!("argus-interval-phases-{}", std::process::id()));
        let start = 20_000 * DAY_MS;
        let mut recorder = HistoryRecorder::new(dir.clone());
        recorder.record(&phase(start, 98., 17, 100, "compile.exe", 0, 0));
        recorder.record(&phase(start + FRAME_MS, 2., 17, 100, "compile.exe", 0, 0));
        recorder.record(&phase(
            start + 2 * FRAME_MS,
            4.,
            18,
            200,
            "copy.exe",
            1000,
            500,
        ));
        recorder.record(&phase(
            start + 3 * FRAME_MS,
            6.,
            18,
            200,
            "copy.exe",
            3000,
            900,
        ));
        recorder.record(&snapshot(start + 4 * FRAME_MS, 0., 19, 300, "next.exe"));
        drop(recorder);
        let first = cpu_interval(&dir, start, start + 2 * FRAME_MS).unwrap();
        assert_eq!(first.recorded_frames, 2);
        assert_eq!(first.cpu.mean, Some(50.));
        assert_eq!(first.cpu.peak, Some(98.));
        assert_eq!(first.cpu.peak_ts, Some(start));
        assert_eq!(first.leaders.len(), 1);
        assert_eq!(first.leaders[0].name, "compile.exe");
        assert_eq!(first.disk.mean, Some(0.));
        assert_eq!(first.leaders[0].io.mean, Some(0.));
        let second = cpu_interval(&dir, start + 2 * FRAME_MS, start + 4 * FRAME_MS).unwrap();
        assert_eq!(second.cpu.mean, Some(5.));
        assert_eq!(second.leaders.len(), 1);
        assert_eq!(second.leaders[0].name, "copy.exe");
        assert_eq!(second.disk.mean, Some(700.));
        assert_eq!(second.leaders[0].io.mean, Some(2000.));
        assert_eq!(second.disk.peak_ts, Some(start + 3 * FRAME_MS));
        let both = cpu_interval(&dir, start, start + 4 * FRAME_MS).unwrap();
        assert_eq!(both.leaders[0].observed_frames, 2);
        assert_eq!(both.leaders[0].cpu.mean, Some(50.));
        assert_eq!(both.leaders[0].recorded_cpu_score, 100.);
        if let Some(path) = std::env::var_os("ARGUS_TEST_INTERVAL_OUTPUT") {
            std::fs::write(
                path,
                serde_json::to_vec_pretty(
                    &serde_json::json!({"first":first,"second":second,"both":both,
                        "ioFirst":interval(&dir,start,start+2*FRAME_MS,IntervalMetric::Io),
                        "ioSecond":interval(&dir,start+2*FRAME_MS,start+4*FRAME_MS,IntervalMetric::Io),
                        "_series":super::super::series(&dir,start,start+4*FRAME_MS-1,4),
                        "_coarseSeries":super::super::series(&dir,start,start+4*FRAME_MS-1,1),
                        "_moments":(0..4).map(|i|super::super::moment(&dir,start+i*FRAME_MS).unwrap()).collect::<Vec<_>>()
                    }),
                )
                .unwrap(),
            )
            .unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_frames_and_reused_pid_keep_separate_instances() {
        let dir = std::env::temp_dir().join(format!("argus-interval-gap-{}", std::process::id()));
        let start = 20_001 * DAY_MS - FRAME_MS;
        let mut recorder = HistoryRecorder::new(dir.clone());
        recorder.record(&snapshot(start, 20., 17, 100, "same.exe"));
        recorder.record(&snapshot(start + 3 * FRAME_MS, 80., 17, 200, "same.exe"));
        recorder.record(&snapshot(start + 4 * FRAME_MS, 0., 19, 300, "next.exe"));
        drop(recorder);
        let report = cpu_interval(&dir, start, start + 4 * FRAME_MS).unwrap();
        assert_eq!(
            (
                report.expected_frames,
                report.recorded_frames,
                report.missing_frames
            ),
            (4, 2, 2)
        );
        assert_eq!(report.gap_count, 1);
        assert_eq!(
            (report.gaps[0].from, report.gaps[0].to),
            (start + FRAME_MS, start + 3 * FRAME_MS)
        );
        assert_eq!(report.leaders.len(), 2);
        assert_ne!(report.leaders[0].create_time, report.leaders[1].create_time);
        assert_eq!(report.cpu.mean, Some(50.));
        let empty = cpu_interval(&dir, start + FRAME_MS, start + 3 * FRAME_MS).unwrap();
        assert_eq!(empty.cpu.mean, None);
        assert_eq!(empty.recorded_frames, 0);
        assert!(empty.leaders.is_empty());
        assert!(cpu_interval(&dir, start, start).is_none());
        assert!(cpu_interval(&dir, 0, 8 * DAY_MS).is_none());
        assert!(cpu_interval(&dir, i64::MAX - 1, i64::MAX).is_none());
        let empty_far = cpu_interval(&dir, i64::MAX - 20_000, i64::MAX - 10_000).unwrap();
        assert_eq!(empty_far.recorded_frames, 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn resource_leaders_use_their_own_valid_readings_and_io_is_not_physical_disk_traffic() {
        let dir = std::env::temp_dir().join(format!(
            "argus-interval-resource-leaders-{}",
            std::process::id()
        ));
        let start = 20_003 * DAY_MS;
        let mut recorder = HistoryRecorder::new(dir.clone());
        for i in 0..=2 {
            let mut sample = snapshot(start + i * FRAME_MS, 20., 17, 100, "cpu.exe");
            sample.process_network = true;
            sample.system.mem_used = 1 << 30;
            sample.system.mem_total = 2 << 30;
            sample.processes[0].gpu = 99.;
            sample.processes[0].gpu_state = crate::measurement::Measurement::failure(5);
            sample.processes[0].io_read = 999;
            sample.processes[0].io_valid = false;
            sample.processes[0].net_received = 999;
            sample.processes[0].net_state =
                crate::measurement::Measurement::missing(crate::measurement::STALE);
            sample.processes.push(ProcessRow {
                pid: 18,
                create_time: 200,
                name: "gpu-io.exe".into(),
                private_ws: 10 << 20,
                gpu: if i == 0 { 0. } else { 12. },
                gpu_state: crate::measurement::Measurement::ready(),
                io_read: if i == 0 { 0 } else { 100 },
                io_valid: true,
                net_received: if i == 0 { 0 } else { 200 },
                net_state: crate::measurement::Measurement::ready(),
                ..Default::default()
            });
            recorder.record(&sample);
        }
        drop(recorder);
        let cpu = interval(&dir, start, start + 2 * FRAME_MS, IntervalMetric::Cpu).unwrap();
        assert_eq!(cpu.leaders[0].name, "cpu.exe");
        assert_eq!(cpu.memory.mean, Some((1 << 30) as f64));
        assert!(cpu.disk.mean.is_none());
        assert!(cpu.gpu.mean.is_none());
        let mut exported = serde_json::Map::new();
        exported.insert("cpu".into(), serde_json::to_value(&cpu).unwrap());
        for metric in [
            IntervalMetric::Memory,
            IntervalMetric::Io,
            IntervalMetric::Gpu,
            IntervalMetric::Network,
        ] {
            let report = interval(&dir, start, start + 2 * FRAME_MS, metric).unwrap();
            assert_eq!(report.leaders[0].name, "gpu-io.exe");
            assert_eq!(report.leaders[0].gpu.mean, Some(6.));
            assert_eq!(report.leaders[0].io.mean, Some(50.));
            assert_eq!(report.leaders[0].network.mean, Some(100.));
            assert_eq!(report.leaders[0].gpu.valid_frames, 2);
            let missing = report.leaders.iter().find(|p| p.pid == 17).unwrap();
            assert!(
                missing.gpu.mean.is_none()
                    && missing.io.mean.is_none()
                    && missing.network.mean.is_none()
            );
            exported.insert(
                serde_json::to_value(metric)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .into(),
                serde_json::to_value(&report).unwrap(),
            );
        }
        if let Some(path) = std::env::var_os("ARGUS_TEST_INTERVAL_RESOURCES_OUTPUT") {
            exported.insert(
                "_series".into(),
                serde_json::to_value(super::super::series(
                    &dir,
                    start,
                    start + 2 * FRAME_MS - 1,
                    2,
                ))
                .unwrap(),
            );
            exported.insert(
                "_moments".into(),
                serde_json::to_value(
                    (0..2)
                        .map(|i| super::super::moment(&dir, start + i * FRAME_MS).unwrap())
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
            );
            std::fs::write(path, serde_json::to_vec_pretty(&exported).unwrap()).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gpu_frames_preserve_real_zero_unavailable_partial_and_legacy_through_disk() {
        use crate::hardware::{GpuInfo, GpuStats, GpuUtilSource};
        use crate::measurement::{self, Measurement};
        use std::sync::Arc;
        let dir =
            std::env::temp_dir().join(format!("argus-interval-gpu-states-{}", std::process::id()));
        let start = 20_004 * DAY_MS;
        let mut recorder = HistoryRecorder::new(dir.clone());
        for (i, value, state) in [
            (0, 0., Measurement::ready()),
            (1, 99., Measurement::failure(5)),
            (
                2,
                12.,
                Measurement {
                    valid: true,
                    partial: true,
                    missing: measurement::UNAVAILABLE,
                },
            ),
        ] {
            let mut sample = snapshot(start + i * FRAME_MS, 2., 17, 100, "gpu.exe");
            sample.system.mem_used = 1 << 30;
            sample.system.mem_total = 2 << 30;
            sample.processes[0].gpu = value;
            sample.processes[0].gpu_state = state;
            sample.hardware.gpus.push(GpuStats {
                info: Arc::new(GpuInfo {
                    id: "synthetic".into(),
                    name: "test".into(),
                    dedicated_total: 0,
                    shared_total: 0,
                    engines: vec!["3D".into()],
                    temperature_max: None,
                    intel_integrated: false,
                    driver_version: None,
                    driver_date: None,
                    direct_x: None,
                }),
                util: value,
                util_source: GpuUtilSource::Windows,
                util_state: state,
                engines: vec![value],
                dedicated_used: 0,
                shared_used: 0,
                temperature: None,
                fan_rpm: None,
                power: None,
                board_power: None,
                board_power_limit: None,
                power_source: None,
                clock_mhz: None,
                clock_max_mhz: None,
                voltage_mv: None,
                memory_clock_mhz: None,
                core_vid_mv: None,
                kernel_sensor_state: None,
                kernel_sensor_supported: false,
                integrated_sensors: None,
            });
            recorder.record(&sample);
        }
        recorder.record(&snapshot(start + 3 * FRAME_MS, 0., 19, 200, "next.exe"));
        drop(recorder);
        let zero = interval(&dir, start, start + FRAME_MS, IntervalMetric::Gpu).unwrap();
        let unavailable = interval(
            &dir,
            start + FRAME_MS,
            start + 2 * FRAME_MS,
            IntervalMetric::Gpu,
        )
        .unwrap();
        let partial = interval(
            &dir,
            start + 2 * FRAME_MS,
            start + 3 * FRAME_MS,
            IntervalMetric::Gpu,
        )
        .unwrap();
        assert_eq!(zero.gpu.mean, Some(0.));
        assert_eq!(zero.leaders[0].gpu.mean, Some(0.));
        assert!(unavailable.gpu.mean.is_none() && unavailable.leaders[0].gpu.mean.is_none());
        assert_ne!(
            unavailable.gpu.sample_coverage.as_ref().unwrap().missing & measurement::ACCESS_DENIED,
            0
        );
        assert_eq!(partial.gpu.mean, Some(12.));
        assert_eq!(partial.gpu.partial_frames, 1);
        let mut legacy = b"ARGH\x02".to_vec();
        legacy.extend([1, 0, 0, 7]);
        legacy.extend(b"old.exe");
        legacy.push(2);
        let mut head = vec![0; super::super::FRAME_HEAD];
        head[..8].copy_from_slice(&(start + 3 * FRAME_MS).to_le_bytes());
        head[8..12].copy_from_slice(&2f32.to_le_bytes());
        head[12..16].copy_from_slice(&2f32.to_le_bytes());
        head[20..28].copy_from_slice(&(1u64 << 30).to_le_bytes());
        head[28..36].copy_from_slice(&(2u64 << 30).to_le_bytes());
        head[16..20].copy_from_slice(&99f32.to_le_bytes());
        *head.last_mut().unwrap() = 1;
        legacy.extend(head);
        let mut row = vec![0; super::super::ENTRY];
        row[2..6].copy_from_slice(&17u32.to_le_bytes());
        row[6..10].copy_from_slice(&2f32.to_le_bytes());
        row[10..14].copy_from_slice(&1u32.to_le_bytes());
        row[18..22].copy_from_slice(&99f32.to_le_bytes());
        legacy.extend(row);
        std::fs::write(
            dir.join(format!("{}.hist", super::super::day_name(20_004))),
            legacy,
        )
        .unwrap();
        let legacy = interval(
            &dir,
            start + 3 * FRAME_MS,
            start + 4 * FRAME_MS,
            IntervalMetric::Gpu,
        )
        .unwrap();
        assert!(legacy.gpu.mean.is_none());
        assert_eq!(legacy.gpu.legacy_frames, 1);
        if let Some(path) = std::env::var_os("ARGUS_TEST_INTERVAL_GPU_OUTPUT") {
            std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"zero":zero,"unavailable":unavailable,"partial":partial,"legacy":legacy,
                "_series":super::super::series(&dir,start,start+4*FRAME_MS-1,4),
                "_coarseSeries":super::super::series(&dir,start,start+4*FRAME_MS-1,1),
                "_moments":(0..4).map(|i|super::super::moment(&dir,start+i*FRAME_MS).unwrap()).collect::<Vec<_>>()
            })).unwrap()).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_instances_are_not_merged_new_format_wins_and_partial_files_are_reported() {
        use std::io::Write;
        let dir =
            std::env::temp_dir().join(format!("argus-interval-legacy-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let start = 20_002 * DAY_MS;
        let mut legacy = b"ARGH\x02".to_vec();
        legacy.extend([1, 0, 0, 8]);
        legacy.extend(b"same.exe");
        for ts in [start, start + FRAME_MS] {
            legacy.push(2);
            let mut header = vec![0; super::super::FRAME_HEAD];
            header[..8].copy_from_slice(&ts.to_le_bytes());
            header[8..12].copy_from_slice(&99f32.to_le_bytes());
            header[12..16].copy_from_slice(&99f32.to_le_bytes());
            *header.last_mut().unwrap() = 1;
            legacy.extend(header);
            let mut row = vec![0; super::super::ENTRY];
            row[2..6].copy_from_slice(&17u32.to_le_bytes());
            row[6..10].copy_from_slice(&99f32.to_le_bytes());
            legacy.extend(row);
        }
        let legacy_path = dir.join(format!("{}.hist", super::super::day_name(20_002)));
        std::fs::write(&legacy_path, &legacy).unwrap();
        let report = cpu_interval(&dir, start, start + 2 * FRAME_MS).unwrap();
        assert_eq!(report.leaders.len(), 2);
        assert!(
            report
                .leaders
                .iter()
                .all(|p| p.create_time.is_none() && p.observed_frames == 1)
        );
        let mut recorder = HistoryRecorder::new(dir.clone());
        recorder.record(&snapshot(start, 20., 17, 100, "same.exe"));
        recorder.record(&snapshot(start + FRAME_MS, 40., 17, 100, "same.exe"));
        recorder.record(&snapshot(start + 2 * FRAME_MS, 0., 19, 200, "other.exe"));
        drop(recorder);
        let new_path = super::super::day_path(&dir, 20_002);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&new_path)
            .unwrap()
            .write_all(&[2, 0])
            .unwrap();
        let unsupported = dir.join(format!("{}.v4.hist", super::super::day_name(20_002)));
        std::fs::write(&unsupported, b"ARGH\x7f").unwrap();
        let report = cpu_interval(&dir, start, start + 2 * FRAME_MS).unwrap();
        assert_eq!(report.recorded_frames, 2);
        assert_eq!(report.cpu.mean, Some(30.));
        assert_eq!(report.leaders.len(), 1);
        assert_eq!(report.leaders[0].create_time, Some(100));
        assert_eq!(report.incomplete_files, 1);
        assert_eq!(report.unreadable_files, 1);
        assert_eq!(std::fs::read(&legacy_path).unwrap(), legacy);
        let oversized = std::fs::OpenOptions::new()
            .write(true)
            .open(&unsupported)
            .unwrap();
        oversized.set_len(FILE_BYTES + 1).unwrap();
        drop(oversized);
        assert_eq!(
            cpu_interval(&dir, start, start + 2 * FRAME_MS)
                .unwrap()
                .unreadable_files,
            1
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
