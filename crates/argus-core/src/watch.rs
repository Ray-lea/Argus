//! Explicit, bounded observation of a process and its future launches, independent of top lists.
//! Local versioned storage. A closed observation is never resumed implicitly.
use std::fs;
use std::path::PathBuf;

use crate::Snapshot;
use serde::{Deserialize, Serialize};
mod identity;
mod program;
mod store;
pub use identity::ProgramIdentity;
pub use store::{
    CAPACITY as PROGRAM_CAPACITY, FILE_NAME, MAX_BYTES as PROGRAM_MAX_BYTES,
    RETENTION_MS as PROGRAM_RETENTION_MS,
};

pub const SAMPLE_MS: i64 = 5_000;
/// The layout of the views returned to the interface.
const VERSION: u32 = 1;

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
    pub memory: Option<u64>,
    pub commit: Option<u64>,
    pub state: WatchState,
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
    dir: PathBuf,
    watch: Option<program::ProgramWatch>,
}

impl WatchRecorder {
    pub fn suspend(mut self) -> impl FnOnce(PathBuf) -> Result<Self, WatchError> {
        let watch = self.watch.take().map(program::ProgramWatch::suspend);
        move |dir| {
            if let Some(resume) = watch {
                self.watch = Some(resume(&dir)?);
            }
            self.dir = dir;
            Ok(self)
        }
    }

    pub fn storage_bytes(dir: &std::path::Path) -> u64 {
        fs::metadata(dir.join(FILE_NAME)).map_or(0, |meta| meta.len())
    }

    pub fn delete_saved(dir: &std::path::Path) -> Result<(), WatchError> {
        match fs::remove_file(dir.join(FILE_NAME)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(WatchError::Storage),
        }
    }

    pub fn open(dir: PathBuf) -> Result<Self, WatchError> {
        let watch = if dir
            .join(FILE_NAME)
            .try_exists()
            .map_err(|_| WatchError::Storage)?
        {
            Some(program::ProgramWatch::open(&dir)?)
        } else {
            None
        };
        Ok(Self { dir, watch })
    }

    pub fn view(&self) -> Option<WatchView> {
        self.watch.as_ref().map(program::ProgramWatch::view)
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
        if let Some(watch) = &mut self.watch {
            return watch.record(snapshot);
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
        let mut reopened = WatchRecorder::open(dir.clone()).unwrap();
        assert_eq!(reopened.view().unwrap().state, WatchState::Stopped);
        assert!(matches!(
            reopened.action(own_start()),
            Err(WatchError::Existing)
        ));
        reopened.action(WatchAction::Delete).unwrap();
        assert!(!dir.join(FILE_NAME).exists());
        assert!(reopened.view().is_none());
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn restart_is_interrupted_and_invalid_store_can_be_deleted() {
        let dir = folder("restart");
        let _ = fs::remove_dir_all(&dir);
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
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn observation_without_program_identity_stays_valid_and_keeps_exited_state() {
        let dir = folder("no-program");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
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
                    ts,
                    memory: Some(4096),
                    commit: Some(8192),
                    state: WatchState::Recording,
                },
            )
            .unwrap();
        store
            .push(
                42,
                1,
                WatchPoint {
                    ts: ts + SAMPLE_MS,
                    memory: None,
                    commit: None,
                    state: WatchState::Exited,
                },
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
        WatchRecorder::delete_saved(&dir).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
