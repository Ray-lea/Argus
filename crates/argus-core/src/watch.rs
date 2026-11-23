//! Explicit, bounded observation of a process and its future launches, independent of top lists.
//! Local versioned storage. A closed observation is never resumed implicitly.
use std::fs;
use std::path::PathBuf;

use crate::Snapshot;
use serde::{Deserialize, Serialize};
mod identity;
mod program;
mod series;
mod store;
pub use identity::ProgramIdentity;
pub use store::{
    CAPACITY as PROGRAM_CAPACITY, FILE_NAME, MAX_BYTES as PROGRAM_MAX_BYTES,
    RETENTION_MS as PROGRAM_RETENTION_MS,
};

pub const SAMPLE_MS: i64 = 5_000;
/// The layout of the views returned to the interface.
const VERSION: u32 = 1;

/// One run's readings over a window, asked for as the history page asks for
/// the system's (`history::series`): buckets of `bucket` ms aligned to
/// `origin`, for the readings named in `keys` (a `WatchPoint`'s fields).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesQuery {
    pub pid: u32,
    pub create_time: i64,
    pub from: i64,
    pub to: i64,
    pub bucket: i64,
    pub origin: i64,
    pub keys: Vec<String>,
}

impl SeriesQuery {
    /// Query a run for which only the saved observation records are available.
    pub fn recorded(&self, points: &[WatchPoint]) -> crate::history::HistorySeries {
        series::series(|_| points.iter(), None, self)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum WatchAction {
    Start {
        pid: u32,
        #[serde(rename = "createTime")]
        create_time: i64,
        name: String,
    },
    SelectRun {
        pid: u32,
        #[serde(rename = "createTime")]
        create_time: i64,
    },
    Page {
        before: Option<u64>,
    },
    Runs {
        offset: usize,
    },
    Status,
    Stop,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchState {
    Recording,
    Gap,
    Exited,
    Stopped,
    Interrupted,
    Limit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchPoint {
    pub ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_set: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_memory: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_read: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_write: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io_read: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io_write: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_received: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_sent: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threads: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handles: Option<u32>,
    pub state: WatchState,
}

impl WatchPoint {
    pub fn marker(ts: i64, state: WatchState) -> Self {
        Self {
            ts,
            memory: None,
            commit: None,
            working_set: None,
            cpu: None,
            gpu: None,
            gpu_memory: None,
            disk_read: None,
            disk_write: None,
            io_read: None,
            io_write: None,
            net_received: None,
            net_sent: None,
            threads: None,
            handles: None,
            state,
        }
    }

    /// The reading named `key` (the field's name as serialized), if recorded.
    pub fn metric(&self, key: &str) -> Option<f64> {
        let bytes = |v: Option<u64>| v.map(|v| v as f64);
        match key {
            "memory" => bytes(self.memory),
            "commit" => bytes(self.commit),
            "workingSet" => bytes(self.working_set),
            "cpu" => self.cpu.map(f64::from),
            "gpu" => self.gpu.map(f64::from),
            "gpuMemory" => bytes(self.gpu_memory),
            "diskRead" => bytes(self.disk_read),
            "diskWrite" => bytes(self.disk_write),
            "ioRead" => bytes(self.io_read),
            "ioWrite" => bytes(self.io_write),
            "netReceived" => bytes(self.net_received),
            "netSent" => bytes(self.net_sent),
            "threads" => self.threads.map(f64::from),
            "handles" => self.handles.map(f64::from),
            _ => None,
        }
        .filter(|v| v.is_finite())
    }

    pub fn has_any_metric(&self) -> bool {
        self.memory.is_some()
            || self.commit.is_some()
            || self.working_set.is_some()
            || self.cpu.is_some()
            || self.gpu.is_some()
            || self.gpu_memory.is_some()
            || self.disk_read.is_some()
            || self.disk_write.is_some()
            || self.io_read.is_some()
            || self.io_write.is_some()
            || self.net_received.is_some()
            || self.net_sent.is_some()
            || self.threads.is_some()
            || self.handles.is_some()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchRun {
    pub pid: u32,
    pub create_time: i64,
    pub first_ts: i64,
    pub last_ts: i64,
    pub points: usize,
    pub state: WatchState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchView {
    pub version: u32,
    pub pid: u32,
    pub create_time: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<ProgramIdentity>,
    #[serde(default)]
    pub runs: Vec<WatchRun>,
    #[serde(default)]
    pub total_runs: usize,
    #[serde(default)]
    pub total_points: usize,
    #[serde(default)]
    pub damaged: bool,
    #[serde(default)]
    pub first_sequence: Option<u64>,
    #[serde(default)]
    pub has_earlier: bool,
    #[serde(default)]
    pub has_later: bool,
    #[serde(default)]
    pub run_offset: usize,
    #[serde(default)]
    pub live_runs: usize,
    #[serde(default)]
    pub unconfirmed: usize,
    pub state: WatchState,
    pub points: Vec<WatchPoint>,
}

#[derive(Debug)]
pub enum WatchError {
    Identity,
    Access,
    Existing,
    Missing,
    Storage,
    Invalid,
}

pub struct WatchRecorder {
    _owner: fs::File,
    dir: PathBuf,
    watch: Option<program::ProgramWatch>,
}

impl WatchRecorder {
    pub fn suspend(mut self) -> impl FnOnce(PathBuf) -> Result<Self, WatchError> {
        let watch = self.watch.take().map(program::ProgramWatch::suspend);
        drop(self);
        move |dir| {
            let owner = crate::history::owned_directory(&dir).map_err(|_| WatchError::Storage)?;
            Ok(Self {
                watch: watch.map(|resume| resume(&dir)).transpose()?,
                dir,
                _owner: owner,
            })
        }
    }

    pub(crate) fn prune_saved(dir: &std::path::Path) -> std::io::Result<()> {
        store::Store::prune_saved(&dir.join(FILE_NAME), now_ms())
    }

    pub fn storage_bytes(dir: &std::path::Path) -> u64 {
        fs::metadata(dir.join(FILE_NAME)).map_or(0, |meta| meta.len())
    }

    pub fn delete_saved(dir: &std::path::Path) -> Result<(), WatchError> {
        let _owner = crate::history::owned_directory(dir).map_err(|_| WatchError::Storage)?;
        store::Store::delete_saved(&dir.join(FILE_NAME))
    }

    pub fn open(dir: PathBuf) -> Result<Self, WatchError> {
        let _owner = crate::history::owned_directory(&dir).map_err(|_| WatchError::Storage)?;
        let watch = if dir
            .join(FILE_NAME)
            .try_exists()
            .map_err(|_| WatchError::Storage)?
        {
            Some(program::ProgramWatch::open(&dir)?)
        } else {
            None
        };
        Ok(Self { dir, watch, _owner })
    }

    pub fn view(&self) -> Option<WatchView> {
        self.watch.as_ref().map(program::ProgramWatch::view)
    }

    /// One run's readings over a window (`SeriesQuery`).
    pub fn series(&self, query: &SeriesQuery) -> Result<crate::history::HistorySeries, WatchError> {
        self.watch
            .as_ref()
            .map(|watch| watch.series(query))
            .ok_or(WatchError::Missing)
    }

    pub fn action(&mut self, action: WatchAction) -> Result<Option<WatchView>, WatchError> {
        match action {
            WatchAction::Status => {}
            WatchAction::Start {
                pid,
                create_time,
                name,
            } => {
                if self.watch.is_some() {
                    return Err(WatchError::Existing);
                }
                if pid == 0 || create_time <= 0 || name.is_empty() || name.len() > 1024 {
                    return Err(WatchError::Identity);
                }
                self.watch = Some(program::ProgramWatch::start(
                    &self.dir,
                    pid,
                    create_time,
                    name,
                )?);
            }
            WatchAction::SelectRun { pid, create_time } => {
                self.watch
                    .as_mut()
                    .ok_or(WatchError::Missing)?
                    .select(pid, create_time)?;
            }
            WatchAction::Page { before } => {
                self.watch
                    .as_mut()
                    .ok_or(WatchError::Missing)?
                    .page(before)?;
            }
            WatchAction::Runs { offset } => {
                self.watch
                    .as_mut()
                    .ok_or(WatchError::Missing)?
                    .runs_page(offset);
            }
            WatchAction::Stop => {
                self.watch
                    .as_mut()
                    .ok_or(WatchError::Missing)?
                    .stop(WatchState::Stopped, now_ms())?;
            }
            WatchAction::Delete => {
                self.watch = None;
                Self::delete_saved(&self.dir)?;
            }
        }
        Ok(self.view())
    }

    pub fn record(&mut self, snapshot: &Snapshot) -> Result<(), WatchError> {
        self.record_at_interval(snapshot, std::time::Duration::from_secs(1))
    }

    pub fn record_at_interval(
        &mut self,
        snapshot: &Snapshot,
        cadence: std::time::Duration,
    ) -> Result<(), WatchError> {
        if let Some(watch) = &mut self.watch {
            return watch.record_at_interval(snapshot, cadence);
        }
        Ok(())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProcessRow;
    use crate::util::{OwnedHandle, filetime_ticks, ticks_to_unix_ms};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    fn own_start() -> WatchAction {
        let handle = OwnedHandle(
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, std::process::id()) }
                .unwrap(),
        );
        let (mut created, mut exited, mut kernel, mut user) = Default::default();
        unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }
            .unwrap();
        WatchAction::Start {
            pid: std::process::id(),
            create_time: ticks_to_unix_ms(filetime_ticks(created)),
            name: "owned-helper.exe".into(),
        }
    }

    fn folder(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("argus-watch-{tag}-{}", std::process::id()))
    }

    fn sample(view: &WatchView) -> Snapshot {
        Snapshot {
            seq: 1,
            timestamp: now_ms(),
            system: Default::default(),
            hardware: Default::default(),
            sessions: Vec::new(),
            leaks: Vec::new(),
            process_network: false,
            processes: vec![ProcessRow {
                pid: view.pid,
                create_time: view.create_time,
                private_ws: 1024,
                commit: 8192,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn observes_below_top_and_never_attaches_to_a_replacement() {
        let dir = folder("identity");
        let _ = fs::remove_dir_all(&dir);
        crate::history::prepare_directory(&dir, None).unwrap();
        let mut rec = WatchRecorder::open(dir.clone()).unwrap();
        rec.action(own_start()).unwrap();
        assert!(
            rec.view()
                .unwrap()
                .program
                .as_ref()
                .is_some_and(ProgramIdentity::valid)
        );
        let mut snapshot = sample(&rec.view().unwrap());
        for pid in 100000..100100 {
            snapshot.processes.push(ProcessRow {
                pid,
                private_ws: 1 << 30,
                ..Default::default()
            });
        }
        rec.record(&snapshot).unwrap();
        assert_eq!(rec.view().unwrap().points[0].memory, Some(1024));
        rec.watch.as_mut().unwrap().clear_sample_timer_for_test();
        snapshot.processes[0].create_time += 1;
        snapshot.timestamp += SAMPLE_MS;
        rec.record(&snapshot).unwrap();
        assert_eq!(rec.view().unwrap().points[1].state, WatchState::Gap);
        assert_eq!(rec.view().unwrap().points[1].memory, None);
        rec.action(WatchAction::Stop).unwrap();
        drop(rec);
        let mut reopened = WatchRecorder::open(dir.clone()).unwrap();
        assert_eq!(reopened.view().unwrap().state, WatchState::Stopped);
        assert!(matches!(
            reopened.action(own_start()),
            Err(WatchError::Existing)
        ));
        reopened.action(WatchAction::Delete).unwrap();
        assert!(!dir.join(FILE_NAME).exists());
        assert!(reopened.view().is_none());
        drop(reopened);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn samples_between_disk_writes_survive_a_folder_move_and_expire_after_stop() {
        let dir = folder("recent-cadence");
        let _ = fs::remove_dir_all(&dir);
        crate::history::prepare_directory(&dir, None).unwrap();
        let mut rec = WatchRecorder::open(dir.clone()).unwrap();
        rec.action(own_start()).unwrap();
        let view = rec.view().unwrap();
        let mut snapshot = sample(&view);
        let start = snapshot.timestamp;
        let q = SeriesQuery {
            pid: view.pid,
            create_time: view.create_time,
            from: start,
            to: start + 60_000,
            bucket: 1_000,
            origin: 0,
            keys: vec!["memory".into()],
        };
        for (dt, expected) in [(0, 500), (492, 500), (1_518, 1_000), (31_530, 30_000)] {
            snapshot.timestamp = start + dt;
            snapshot.processes[0].private_ws = dt as u64;
            rec.record_at_interval(&snapshot, std::time::Duration::from_millis(expected))
                .unwrap();
        }
        let query = rec.series(&q).unwrap();
        assert_eq!(query.span, [500, 492, 1_026, 30_012]);
        assert!(query.gaps.is_empty());
        assert_eq!(rec.view().unwrap().total_points, 1);
        assert_eq!(rec.view().unwrap().runs[0].last_ts, snapshot.timestamp);
        let other_run = SeriesQuery {
            create_time: q.create_time + 1,
            ..q.clone()
        };
        assert!(rec.series(&other_run).unwrap().ts.is_empty());
        let resume = rec.suspend();
        let moved = folder("recent-cadence-moved");
        fs::rename(&dir, &moved).unwrap();
        let dir = moved;
        rec = resume(dir.clone()).unwrap();
        assert_eq!(rec.series(&q).unwrap().ts, query.ts);
        rec.action(WatchAction::Stop).unwrap();
        assert_eq!(rec.series(&q).unwrap().ts, query.ts);
        snapshot.timestamp += crate::history::RECENT_MS + 1;
        rec.record(&snapshot).unwrap();
        assert_eq!(rec.series(&q).unwrap().ts, [start]);
        drop(rec);
        assert_eq!(
            WatchRecorder::open(dir.clone())
                .unwrap()
                .series(&q)
                .unwrap()
                .ts,
            [start]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn restart_is_interrupted_and_invalid_store_can_be_deleted() {
        let dir = folder("restart");
        let _ = fs::remove_dir_all(&dir);
        crate::history::prepare_directory(&dir, None).unwrap();
        let mut rec = WatchRecorder::open(dir.clone()).unwrap();
        rec.action(own_start()).unwrap();
        let snapshot = sample(&rec.view().unwrap());
        rec.record(&snapshot).unwrap();
        drop(rec);
        assert_eq!(
            WatchRecorder::open(dir.clone())
                .unwrap()
                .view()
                .unwrap()
                .state,
            WatchState::Interrupted
        );
        WatchRecorder::delete_saved(&dir).unwrap();
        fs::write(dir.join(FILE_NAME), b"corrupt").unwrap();
        assert!(matches!(
            WatchRecorder::open(dir.clone()),
            Err(WatchError::Invalid)
        ));
        WatchRecorder::delete_saved(&dir).unwrap();
        assert!(!dir.join(FILE_NAME).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn observation_without_program_identity_stays_valid_and_keeps_exited_state() {
        let dir = folder("no-program");
        let _ = fs::remove_dir_all(&dir);
        crate::history::prepare_directory(&dir, None).unwrap();
        let mut store = store::Store::create(
            &dir.join(FILE_NAME),
            store::Metadata {
                name: "no-path.exe".into(),
                program: None,
            },
        )
        .unwrap();
        let ts = now_ms();
        store
            .push(
                42,
                1,
                WatchPoint {
                    memory: Some(4096),
                    commit: Some(8192),
                    ..WatchPoint::marker(ts, WatchState::Recording)
                },
            )
            .unwrap();
        store
            .push(
                42,
                1,
                WatchPoint::marker(ts + SAMPLE_MS, WatchState::Exited),
            )
            .unwrap();
        store.flush().unwrap();
        drop(store);
        let recorder = WatchRecorder::open(dir.clone()).unwrap();
        let view = recorder.view().unwrap();
        assert_eq!(view.version, VERSION);
        assert!(view.program.is_none());
        assert_eq!(view.state, WatchState::Exited);
        assert_eq!(view.points.len(), 2);
        drop(recorder);
        WatchRecorder::delete_saved(&dir).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
