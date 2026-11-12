//! Read-only saved observation statistics, independent of the general history top list.
use super::{SAMPLE_MS, WatchMode, WatchPoint, WatchState, read_single, store};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

const MAX_RUNS: usize = 4096;
const DISPLAY_RUNS: usize = 100;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedMemory {
    /// Arithmetic mean of saved points, not an elapsed-time average.
    pub mean: Option<f64>,
    pub peak: Option<u64>,
    pub peak_ts: Option<i64>,
    pub samples: usize,
    pub missing: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationRun {
    pub mode: WatchMode,
    pub name: String,
    pub pid: u32,
    pub create_time: i64,
    pub first_ts: i64,
    pub last_ts: i64,
    pub points: usize,
    pub gap_events: usize,
    /// Unrecorded nominal slots between saved points. Never establishes why sampling stopped.
    pub missing_sample_slots: usize,
    pub terminal_events: usize,
    /// A recorded marker, never a claim that this process or recorder is currently alive.
    pub last_recorded_state: WatchState,
    pub memory: SavedMemory,
    pub commit: SavedMemory,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationInterval {
    pub sample_ms: i64,
    pub source_files: usize,
    pub unreadable_files: usize,
    pub damaged_files: usize,
    pub expired_points: usize,
    pub saved_points: usize,
    pub first_saved_ts: Option<i64>,
    pub last_saved_ts: Option<i64>,
    pub selected_points: usize,
    pub runs_seen: usize,
    pub omitted_points: usize,
    pub omitted_runs: usize,
    pub runs: Vec<ObservationRun>,
}

#[derive(Default)]
struct MemoryAcc {
    summary: SavedMemory,
    sum: u128,
}
impl MemoryAcc {
    fn add(&mut self, value: Option<u64>, ts: i64) {
        let Some(value) = value else {
            self.summary.missing += 1;
            return;
        };
        self.sum += u128::from(value);
        self.summary.samples += 1;
        if self.summary.peak.is_none_or(|old| value > old) {
            self.summary.peak = Some(value);
            self.summary.peak_ts = Some(ts);
        }
    }
    fn finish(mut self) -> SavedMemory {
        self.summary.mean =
            (self.summary.samples > 0).then(|| self.sum as f64 / self.summary.samples as f64);
        self.summary
    }
}

struct RunAcc {
    run: ObservationRun,
    memory: MemoryAcc,
    commit: MemoryAcc,
}

/// Reads only the two known bounded watch files; no process handles, repair or recording actions.
/// Returns the selected half-open interval without aligning individual observation timestamps.
pub fn observation_interval(dir: &Path, from: i64, to: i64) -> Option<ObservationInterval> {
    read_at(dir, from, to, super::now_ms())
}

fn read_at(dir: &Path, from: i64, to: i64, now: i64) -> Option<ObservationInterval> {
    // The general history reader can expand a seven-day request to complete boundary frames.
    if from < 0 || to <= from || to.checked_sub(from)? > store::RETENTION_MS + SAMPLE_MS {
        return None;
    }
    let mut report = ObservationInterval {
        sample_ms: SAMPLE_MS,
        ..Default::default()
    };
    let mut runs = BTreeMap::<(bool, u32, i64), RunAcc>::new();
    let mut add = |program: bool, name: &str, pid, created, point: &WatchPoint| {
        report.saved_points += 1;
        report.first_saved_ts = Some(
            report
                .first_saved_ts
                .map_or(point.ts, |ts| ts.min(point.ts)),
        );
        report.last_saved_ts = Some(report.last_saved_ts.map_or(point.ts, |ts| ts.max(point.ts)));
        if !(from..to).contains(&point.ts) {
            return;
        }
        report.selected_points += 1;
        let key = (program, pid, created);
        if !runs.contains_key(&key) && runs.len() >= MAX_RUNS {
            report.omitted_points += 1;
            return;
        }
        let run = runs.entry(key).or_insert_with(|| RunAcc {
            run: ObservationRun {
                mode: if program {
                    WatchMode::Program
                } else {
                    WatchMode::Instance
                },
                name: name.into(),
                pid,
                create_time: created,
                first_ts: point.ts,
                last_ts: point.ts,
                points: 0,
                gap_events: 0,
                missing_sample_slots: 0,
                terminal_events: 0,
                last_recorded_state: point.state,
                memory: SavedMemory::default(),
                commit: SavedMemory::default(),
            },
            memory: MemoryAcc::default(),
            commit: MemoryAcc::default(),
        });
        if run.run.points > 0 {
            run.run.missing_sample_slots +=
                ((point.ts - run.run.last_ts).saturating_sub(1) / SAMPLE_MS).max(0) as usize;
        }
        run.run.last_ts = point.ts;
        run.run.points += 1;
        run.run.last_recorded_state = point.state;
        match point.state {
            WatchState::Recording => {
                run.memory.add(point.memory, point.ts);
                run.commit.add(point.commit, point.ts);
            }
            WatchState::Gap => {
                run.run.gap_events += 1;
            }
            _ => {
                run.run.terminal_events += 1;
            }
        }
    };
    match read_single(&dir.join(super::FILE_NAME)) {
        Ok(Some((view, _))) => {
            report.source_files += 1;
            report.damaged_files += usize::from(view.damaged);
            for point in &view.points {
                add(false, &view.name, view.pid, view.create_time, point);
            }
        }
        Ok(None) => {}
        Err(_) => report.unreadable_files += 1,
    }
    match store::Saved::open(&dir.join(store::FILE_NAME)) {
        Ok(saved) => {
            report.source_files += 1;
            report.damaged_files += usize::from(saved.damaged);
            let cutoff = now.saturating_sub(store::RETENTION_MS);
            for entry in saved.entries {
                if entry.point.ts < cutoff {
                    report.expired_points += 1;
                } else {
                    add(
                        true,
                        &saved.meta.name,
                        entry.pid,
                        entry.created,
                        &entry.point,
                    );
                }
            }
        }
        Err(super::WatchError::Missing) => {}
        Err(_) => report.unreadable_files += 1,
    }
    report.runs_seen = runs.len();
    report.omitted_runs = runs.len().saturating_sub(DISPLAY_RUNS);
    report.runs = runs
        .into_values()
        .map(|mut acc| {
            acc.run.memory = acc.memory.finish();
            acc.run.commit = acc.commit.finish();
            acc.run
        })
        .collect();
    report
        .runs
        .sort_by_key(|run| (run.first_ts, run.pid, run.create_time));
    report.runs.truncate(DISPLAY_RUNS);
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::{ProgramIdentity, Record, WatchError};
    use std::io::Write;

    fn folder(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "argus-watch-interval-{}-{tag}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }
    fn point(ts: i64, memory: Option<u64>, state: WatchState) -> WatchPoint {
        WatchPoint {
            ts,
            memory,
            commit: memory.map(|v| v + 10),
            state,
        }
    }
    fn meta() -> store::Metadata {
        store::Metadata {
            name: "watched.exe".into(),
            program: ProgramIdentity {
                native_path: "\\Device\\disk\\private-user\\watched.exe".into(),
                display_path: "C:\\private-user\\watched.exe".into(),
            },
        }
    }
    fn single(path: &Path, version: u32, points: &[WatchPoint]) {
        let header = Record::Header {
            version,
            pid: 17,
            created: 100,
            name: "low-load.exe".into(),
            program: None,
        };
        let mut file = std::fs::File::create(path.join(super::super::FILE_NAME)).unwrap();
        serde_json::to_writer(&mut file, &header).unwrap();
        file.write_all(b"\n").unwrap();
        for point in points {
            serde_json::to_writer(&mut file, &Record::Point(point.clone())).unwrap();
            file.write_all(b"\n").unwrap();
        }
    }

    #[test]
    fn saved_points_keep_zero_gap_lifecycle_and_reused_pid_separate_while_writer_is_open() {
        let dir = folder("active");
        let start = super::super::now_ms() / SAMPLE_MS * SAMPLE_MS;
        let path = dir.join(store::FILE_NAME);
        let mut writer = store::Store::create(&path, meta()).unwrap();
        for (pid, created, p) in [
            (17, 100, point(start, Some(0), WatchState::Recording)),
            (17, 100, point(start + SAMPLE_MS, None, WatchState::Gap)),
            (
                17,
                100,
                point(start + 3 * SAMPLE_MS, Some(20), WatchState::Recording),
            ),
            (
                17,
                100,
                point(start + 4 * SAMPLE_MS, None, WatchState::Exited),
            ),
            (
                17,
                200,
                point(start + 4 * SAMPLE_MS, Some(100), WatchState::Recording),
            ),
            (
                17,
                200,
                point(start + 5 * SAMPLE_MS, None, WatchState::Stopped),
            ),
        ] {
            writer.push(pid, created, p).unwrap();
        }
        writer.flush().unwrap();
        let before = std::fs::read(&path).unwrap();
        let report = read_at(&dir, start, start + 5 * SAMPLE_MS, start).unwrap();
        assert_eq!(
            (
                report.source_files,
                report.unreadable_files,
                report.damaged_files
            ),
            (1, 0, 0)
        );
        assert_eq!(report.runs.len(), 2);
        assert_eq!(report.runs[0].memory.mean, Some(10.));
        assert_eq!(report.runs[0].commit.mean, Some(20.));
        assert_eq!(report.runs[0].memory.samples, 2);
        assert_eq!(report.runs[0].gap_events, 1);
        assert_eq!(report.runs[0].missing_sample_slots, 1);
        assert_eq!(report.runs[0].terminal_events, 1);
        assert_eq!(report.runs[1].memory.mean, Some(100.));
        assert_eq!(report.runs[1].terminal_events, 0);
        assert_eq!(report.runs[1].last_recorded_state, WatchState::Recording);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("private-user")
        );
        let mut history = crate::history::HistoryRecorder::new(dir.clone());
        for i in 0..=5 {
            let snapshot = crate::Snapshot {
                timestamp: start + i * SAMPLE_MS,
                system: crate::system::SystemStats {
                    cpu: 3.,
                    mem_used: 1 << 30,
                    mem_total: 2 << 30,
                    ..Default::default()
                },
                processes: vec![crate::ProcessRow {
                    pid: 88,
                    create_time: 500,
                    name: "top-only.exe".into(),
                    cpu: 3.,
                    ..Default::default()
                }],
                hardware: Default::default(),
                sessions: Vec::new(),
                process_network: false,
                seq: i as u64 + 1,
            };
            history.record(&snapshot);
        }
        drop(history);
        let combined = crate::history::interval(
            &dir,
            start,
            start + 5 * SAMPLE_MS,
            crate::history::IntervalMetric::Memory,
        )
        .unwrap();
        assert_eq!(combined.leaders.len(), 1);
        assert_eq!(combined.leaders[0].pid, 88);
        assert_eq!(combined.observations.runs.len(), 2);
        assert!(combined.observations.runs.iter().all(|run| run.pid == 17));
        let expanded = crate::history::interval(
            &dir,
            start + 1,
            start + store::RETENTION_MS + 1,
            crate::history::IntervalMetric::Memory,
        )
        .unwrap();
        assert_eq!(expanded.to - expanded.from, store::RETENTION_MS + SAMPLE_MS);
        assert_eq!(expanded.observations.runs.len(), 2);
        // A read leaves the original writer usable, without attaching or resuming a process.
        writer
            .push(
                18,
                300,
                point(start + 6 * SAMPLE_MS, Some(7), WatchState::Recording),
            )
            .unwrap();
        writer.flush().unwrap();
        assert_eq!(
            read_at(&dir, start, start + 7 * SAMPLE_MS, start)
                .unwrap()
                .runs
                .len(),
            3
        );
        drop(writer);
        super::super::WatchRecorder::delete_saved(&dir).unwrap();
        let cleared = crate::history::interval(
            &dir,
            start,
            start + 5 * SAMPLE_MS,
            crate::history::IntervalMetric::Memory,
        )
        .unwrap();
        assert_eq!(cleared.leaders.len(), 1);
        assert_eq!(cleared.observations.source_files, 0);
        assert!(cleared.observations.runs.is_empty());
        if let Some(output) = std::env::var_os("ARGUS_TEST_OBSERVATION_INTERVAL_OUTPUT") {
            std::fs::write(output, serde_json::to_vec_pretty(&serde_json::json!({
                "report":combined,"cleared":cleared,"_series":crate::history::series(&dir,start,start+5*SAMPLE_MS-1,5),
                "_moments":(0..5).map(|i|crate::history::moment(&dir,start+i*SAMPLE_MS).unwrap()).collect::<Vec<_>>()
            })).unwrap()).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn expired_and_torn_program_storage_is_never_repaired_by_an_interval_read() {
        let dir = folder("readonly");
        let start = super::super::now_ms();
        let path = dir.join(store::FILE_NAME);
        let mut writer = store::Store::create(&path, meta()).unwrap();
        writer
            .push(17, 100, point(start, Some(12345), WatchState::Recording))
            .unwrap();
        writer.flush().unwrap();
        drop(writer);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[1, 2, 3])
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let report = read_at(
            &dir,
            start,
            start + SAMPLE_MS,
            start + store::RETENTION_MS + 1,
        )
        .unwrap();
        assert_eq!(
            (
                report.expired_points,
                report.damaged_files,
                report.saved_points
            ),
            (1, 1, 0)
        );
        assert!(report.runs.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // The normal recorder still owns repair and physical expiry.
        let writer = store::Store::open(&path, start + store::RETENTION_MS + 1).unwrap();
        assert!(writer.entries.is_empty());
        drop(writer);
        assert_ne!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_jsonl_and_limits_keep_unknowns_without_changing_recorded_state_or_bytes() {
        let dir = folder("legacy");
        let start = super::super::now_ms();
        for version in [1, 2] {
            single(
                &dir,
                version,
                &[
                    point(start, Some(0), WatchState::Recording),
                    point(start + SAMPLE_MS, None, WatchState::Recording),
                ],
            );
            let path = dir.join(super::super::FILE_NAME);
            let before = std::fs::read(&path).unwrap();
            let report = read_at(&dir, start, start + 2 * SAMPLE_MS, start).unwrap();
            assert_eq!(report.runs[0].memory.mean, Some(0.));
            assert_eq!(
                (report.runs[0].memory.samples, report.runs[0].memory.missing),
                (1, 1)
            );
            assert_eq!(report.runs[0].last_recorded_state, WatchState::Recording);
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        single(&dir, 1, &[point(start, Some(99), WatchState::Gap)]);
        let damaged = read_at(&dir, start, start + SAMPLE_MS, start).unwrap();
        assert_eq!(damaged.damaged_files, 1);
        assert!(damaged.runs.is_empty());
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(dir.join(super::super::FILE_NAME))
            .unwrap();
        file.set_len(super::super::MAX_BYTES + 1).unwrap();
        drop(file);
        assert_eq!(
            read_at(&dir, start, start + SAMPLE_MS, start)
                .unwrap()
                .unreadable_files,
            1
        );
        assert!(matches!(
            read_single(&dir.join(super::super::FILE_NAME)),
            Err(WatchError::Invalid)
        ));
        assert!(read_at(&dir, 0, i64::MAX, start).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn display_and_tracking_limits_are_explicit_without_merging_runs() {
        let dir = folder("runs");
        let start = super::super::now_ms();
        let path = dir.join(store::FILE_NAME);
        let mut writer = store::Store::create(&path, meta()).unwrap();
        for i in 0..MAX_RUNS + 2 {
            writer
                .push(
                    17,
                    100 + i as i64,
                    point(start, Some(i as u64), WatchState::Recording),
                )
                .unwrap();
        }
        writer.flush().unwrap();
        let report = read_at(&dir, start, start + SAMPLE_MS, start).unwrap();
        assert_eq!(
            (
                report.runs.len(),
                report.runs_seen,
                report.omitted_runs,
                report.omitted_points
            ),
            (DISPLAY_RUNS, MAX_RUNS, MAX_RUNS - DISPLAY_RUNS, 2)
        );
        drop(writer);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "full bounded observation ring cost probe; run explicitly in release"]
    fn full_ring_read_cost_is_bounded_and_does_not_mutate_storage() {
        let dir = folder("full-ring");
        let start = super::super::now_ms() - store::CAPACITY as i64;
        let path = dir.join(store::FILE_NAME);
        let mut writer = store::Store::create(&path, meta()).unwrap();
        for i in 0..store::CAPACITY {
            let run = i % 100;
            writer
                .push(
                    17 + run as u32,
                    100 + run as i64,
                    point(start + i as i64, Some(i as u64), WatchState::Recording),
                )
                .unwrap();
        }
        writer.flush().unwrap();
        let before = std::fs::read(&path).unwrap();
        let end = start + store::CAPACITY as i64;
        let mut costs = Vec::new();
        for _ in 0..4 {
            let began = std::time::Instant::now();
            let report = read_at(&dir, start, end, end).unwrap();
            costs.push(began.elapsed().as_micros());
            assert_eq!(
                (
                    report.saved_points,
                    report.selected_points,
                    report.runs.len()
                ),
                (store::CAPACITY, store::CAPACITY, 100)
            );
            assert_eq!(
                report
                    .runs
                    .iter()
                    .map(|run| run.memory.samples)
                    .sum::<usize>(),
                store::CAPACITY
            );
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
        println!(
            "observation ring bytes={} records={} instances=100 warmup_us={} warm_reads_us={:?}",
            before.len(),
            store::CAPACITY,
            costs[0],
            &costs[1..]
        );
        drop(writer);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
