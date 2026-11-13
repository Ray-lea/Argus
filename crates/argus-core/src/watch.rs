//! Explicit, bounded observation of an instance or program, independent of top lists.
//! Local versioned storage. A closed observation is never resumed implicitly.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{Snapshot, util::OwnedHandle};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Threading::WaitForSingleObject;
mod identity;
mod interval;
mod program;
mod store;
pub use identity::ProgramIdentity;
pub use interval::{ObservationInterval, ObservationRun, SavedMemory, observation_interval};
pub use store::{
    CAPACITY as PROGRAM_CAPACITY, MAX_BYTES as PROGRAM_MAX_BYTES,
    RETENTION_MS as PROGRAM_RETENTION_MS,
};

pub const SAMPLE_MS: i64 = 5_000;
pub const MAX_POINTS: usize = 720;
pub const MAX_BYTES: u64 = 1 << 20;
const FILE_NAME: &str = "process-watch.jsonl";

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum WatchAction {
    Start {
        pid: u32,
        #[serde(rename = "createTime")]
        create_time: i64,
        name: String,
    },
    StartProgram {
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchMode {
    #[default]
    Instance,
    Program,
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
    pub mode: WatchMode,
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

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Record {
    Header {
        version: u32,
        pid: u32,
        created: i64,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        program: Option<ProgramIdentity>,
    },
    Point(WatchPoint),
}

// Pure, bounded legacy reader shared by the recorder and interval investigation.
fn read_single(path: &std::path::Path) -> Result<Option<(WatchView, u64)>, WatchError> {
    let bytes = match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| WatchError::Storage)?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err(WatchError::Invalid);
            }
            bytes
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(WatchError::Storage),
    };
    let mut lines = bytes.split(|b| *b == b'\n');
    let header: Record = serde_json::from_slice(lines.next().ok_or(WatchError::Invalid)?)
        .map_err(|_| WatchError::Invalid)?;
    let Record::Header {
        version,
        pid,
        created,
        name,
        program,
    } = header
    else {
        return Err(WatchError::Invalid);
    };
    if pid == 0
        || created <= 0
        || name.is_empty()
        || name.len() > 1024
        || !matches!(version, 1 | 2)
        || program.as_ref().is_some_and(|program| !program.valid())
    {
        return Err(WatchError::Invalid);
    }
    let mut view = WatchView {
        version,
        pid,
        create_time: created,
        name,
        program,
        mode: WatchMode::Instance,
        runs: Vec::new(),
        total_runs: 0,
        total_points: 0,
        damaged: false,
        first_sequence: None,
        has_earlier: false,
        has_later: false,
        run_offset: 0,
        live_runs: 0,
        unconfirmed: 0,
        state: WatchState::Interrupted,
        points: Vec::new(),
    };
    let mut damaged = false;
    for line in lines.filter(|line| !line.is_empty()) {
        let Ok(Record::Point(point)) = serde_json::from_slice(line) else {
            damaged = true;
            break;
        };
        if view.points.len() > MAX_POINTS {
            return Err(WatchError::Invalid);
        }
        if point.ts <= 0
            || view.points.last().is_some_and(|last| {
                last.ts > point.ts || !matches!(last.state, WatchState::Recording | WatchState::Gap)
            })
            || (point.state != WatchState::Recording
                && (point.memory.is_some() || point.commit.is_some()))
        {
            damaged = true;
            break;
        }
        view.state = point.state;
        view.points.push(point);
    }
    view.damaged = damaged;
    Ok(Some((view, bytes.len() as u64)))
}

pub struct WatchRecorder {
    dir: PathBuf,
    program_watch: Option<program::ProgramWatch>,
    view: Option<WatchView>,
    handle: Option<OwnedHandle>,
    file: Option<File>,
    bytes: u64,
    started: Option<Instant>,
    last: Option<Instant>,
}

impl WatchRecorder {
    pub fn storage_bytes(dir: &std::path::Path) -> u64 {
        [FILE_NAME, store::FILE_NAME]
            .iter()
            .map(|name| fs::metadata(dir.join(name)).map_or(0, |meta| meta.len()))
            .sum()
    }
    pub fn delete_saved(dir: &std::path::Path) -> Result<(), WatchError> {
        for name in [FILE_NAME, store::FILE_NAME] {
            match fs::remove_file(dir.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(WatchError::Storage),
            }
        }
        Ok(())
    }

    pub fn open(dir: PathBuf) -> Result<Self, WatchError> {
        let mut recorder = Self {
            dir,
            program_watch: None,
            view: None,
            handle: None,
            file: None,
            bytes: 0,
            started: None,
            last: None,
        };
        let path = recorder.dir.join(FILE_NAME);
        if recorder
            .dir
            .join(store::FILE_NAME)
            .try_exists()
            .map_err(|_| WatchError::Storage)?
        {
            if path.try_exists().map_err(|_| WatchError::Storage)? {
                return Err(WatchError::Existing);
            }
            recorder.program_watch = Some(program::ProgramWatch::open(&recorder.dir)?);
            return Ok(recorder);
        }
        let Some((mut view, bytes)) = read_single(&path)? else {
            return Ok(recorder);
        };
        if view.damaged || matches!(view.state, WatchState::Recording | WatchState::Gap) {
            view.state = WatchState::Interrupted;
        }
        recorder.bytes = bytes;
        recorder.view = Some(view);
        Ok(recorder)
    }

    pub fn view(&self) -> Option<WatchView> {
        self.program_watch
            .as_ref()
            .map(program::ProgramWatch::view)
            .or_else(|| {
                self.view.clone().map(|mut view| {
                    view.live_runs = usize::from(self.handle.is_some());
                    view.total_runs = 1;
                    view.total_points = view.points.len();
                    view
                })
            })
    }

    pub fn action(&mut self, action: WatchAction) -> Result<Option<WatchView>, WatchError> {
        match action {
            WatchAction::Status => {}
            WatchAction::Start {
                pid,
                create_time,
                name,
            } => {
                if self.view.is_some() || self.program_watch.is_some() {
                    return Err(WatchError::Existing);
                }
                if pid == 0 || create_time <= 0 || name.is_empty() || name.len() > 1024 {
                    return Err(WatchError::Identity);
                }
                let instance = identity::Instance::open(pid, create_time)?;
                // Image identity is optional for a single-run observation.
                let program = instance.program().ok();
                let handle = instance.into_handle();
                fs::create_dir_all(&self.dir).map_err(|_| WatchError::Storage)?;
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(self.dir.join(FILE_NAME))
                    .map_err(|_| WatchError::Storage)?;
                let mut header = serde_json::to_vec(&Record::Header {
                    version: 2,
                    pid,
                    created: create_time,
                    name: name.clone(),
                    program: program.clone(),
                })
                .map_err(|_| WatchError::Storage)?;
                header.push(b'\n');
                if file.write_all(&header).and_then(|()| file.flush()).is_err() {
                    drop(file);
                    let _ = fs::remove_file(self.dir.join(FILE_NAME));
                    return Err(WatchError::Storage);
                }
                self.bytes = header.len() as u64;
                self.file = Some(file);
                self.handle = Some(handle);
                self.started = Some(Instant::now());
                self.last = None;
                self.view = Some(WatchView {
                    version: 2,
                    pid,
                    create_time,
                    name,
                    program,
                    mode: WatchMode::Instance,
                    runs: Vec::new(),
                    total_runs: 0,
                    total_points: 0,
                    damaged: false,
                    first_sequence: None,
                    has_earlier: false,
                    has_later: false,
                    run_offset: 0,
                    live_runs: 1,
                    unconfirmed: 0,
                    state: WatchState::Recording,
                    points: Vec::new(),
                });
            }
            WatchAction::StartProgram {
                pid,
                create_time,
                name,
            } => {
                if self.view.is_some() || self.program_watch.is_some() {
                    return Err(WatchError::Existing);
                }
                if name.is_empty() || name.len() > 1024 {
                    return Err(WatchError::Identity);
                }
                self.program_watch = Some(program::ProgramWatch::start(
                    &self.dir,
                    pid,
                    create_time,
                    name,
                )?);
            }
            WatchAction::SelectRun { pid, create_time } => {
                self.program_watch
                    .as_mut()
                    .ok_or(WatchError::Missing)?
                    .select(pid, create_time)?;
            }
            WatchAction::Page { before } => {
                self.program_watch
                    .as_mut()
                    .ok_or(WatchError::Missing)?
                    .page(before)?;
            }
            WatchAction::Runs { offset } => {
                self.program_watch
                    .as_mut()
                    .ok_or(WatchError::Missing)?
                    .runs_page(offset);
            }
            WatchAction::Stop => {
                if let Some(program) = &mut self.program_watch {
                    program.stop(WatchState::Stopped, now_ms())?;
                    return Ok(self.view());
                }
                if self.view.is_none() {
                    return Err(WatchError::Missing);
                }
                if self.file.is_some() {
                    self.finish(WatchState::Stopped, now_ms())?;
                }
            }
            WatchAction::Delete => {
                self.program_watch = None;
                self.file = None;
                self.handle = None;
                if Self::delete_saved(&self.dir).is_err() {
                    if let Some(view) = &mut self.view {
                        view.state = WatchState::Interrupted;
                    }
                    return Err(WatchError::Storage);
                }
                self.view = None;
                self.started = None;
                self.last = None;
                self.bytes = 0;
            }
        }
        Ok(self.view())
    }

    pub fn record(&mut self, snapshot: &Snapshot) -> Result<(), WatchError> {
        if let Some(program) = &mut self.program_watch {
            return program.record(snapshot);
        }
        let result = self.record_sample(snapshot);
        if result.is_err() {
            self.file = None;
            self.handle = None;
            if let Some(view) = &mut self.view {
                view.state = WatchState::Interrupted;
            }
        }
        result
    }

    fn record_sample(&mut self, snapshot: &Snapshot) -> Result<(), WatchError> {
        let Some(handle) = &self.handle else {
            return Ok(());
        };
        match unsafe { WaitForSingleObject(handle.0, 0) } {
            WAIT_OBJECT_0 => return self.finish(WatchState::Exited, snapshot.timestamp),
            WAIT_TIMEOUT => {}
            _ => return self.finish(WatchState::Interrupted, snapshot.timestamp),
        }
        if self
            .started
            .is_some_and(|start| start.elapsed() >= Duration::from_secs(3600))
            || self
                .view
                .as_ref()
                .is_some_and(|view| view.points.len() >= MAX_POINTS)
        {
            return self.finish(WatchState::Limit, snapshot.timestamp);
        }
        if self
            .last
            .is_some_and(|last| last.elapsed() < Duration::from_millis(SAMPLE_MS as u64))
        {
            return Ok(());
        }
        let view = self.view.as_ref().ok_or(WatchError::Missing)?;
        if let Some(previous) = view.points.last() {
            if snapshot.timestamp < previous.ts {
                return self.finish(WatchState::Interrupted, previous.ts);
            }
            if snapshot.timestamp - previous.ts > SAMPLE_MS * 2 {
                let missing = previous.ts + SAMPLE_MS;
                self.push(WatchPoint {
                    ts: missing,
                    memory: None,
                    commit: None,
                    state: WatchState::Gap,
                })?;
                if self.handle.is_none() {
                    return Ok(());
                }
            }
        }
        let view = self.view.as_ref().ok_or(WatchError::Missing)?;
        let row = snapshot.processes.iter().find(|p| {
            p.pid == view.pid
                && p.create_time == view.create_time
                && !p.stale
                && !p.sample_reused
                && !p.counters_pending
        });
        let point = WatchPoint {
            ts: snapshot.timestamp,
            memory: row.map(|p| p.private_ws),
            commit: row.map(|p| p.commit),
            state: if row.is_some() {
                WatchState::Recording
            } else {
                WatchState::Gap
            },
        };
        self.push(point)?;
        self.last = Some(Instant::now());
        Ok(())
    }

    fn push(&mut self, point: WatchPoint) -> Result<(), WatchError> {
        if matches!(point.state, WatchState::Recording | WatchState::Gap)
            && self
                .view
                .as_ref()
                .is_some_and(|view| view.points.len() >= MAX_POINTS)
        {
            return self.finish(WatchState::Limit, point.ts);
        }
        let mut bytes =
            serde_json::to_vec(&Record::Point(point.clone())).map_err(|_| WatchError::Storage)?;
        bytes.push(b'\n');
        if self.bytes + bytes.len() as u64 > MAX_BYTES - 512
            && matches!(point.state, WatchState::Recording | WatchState::Gap)
        {
            return self.finish(WatchState::Limit, point.ts);
        }
        if self.bytes + bytes.len() as u64 > MAX_BYTES {
            return Err(WatchError::Storage);
        }
        self.file
            .as_mut()
            .ok_or(WatchError::Storage)?
            .write_all(&bytes)
            .map_err(|_| WatchError::Storage)?;
        self.bytes += bytes.len() as u64;
        let view = self.view.as_mut().ok_or(WatchError::Missing)?;
        view.state = point.state;
        view.points.push(point);
        Ok(())
    }

    fn finish(&mut self, state: WatchState, ts: i64) -> Result<(), WatchError> {
        let result = self.push(WatchPoint {
            ts,
            memory: None,
            commit: None,
            state,
        });
        self.file = None;
        self.handle = None;
        self.started = None;
        if let Some(view) = &mut self.view {
            view.state = if result.is_ok() {
                state
            } else {
                WatchState::Interrupted
            };
        }
        result
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
    use crate::util::{filetime_ticks, ticks_to_unix_ms};
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
    fn legacy_observation_has_no_invented_program_identity_and_is_not_rewritten() {
        let dir = folder("legacy-program");
        fs::create_dir(&dir).unwrap();
        let header =
            b"{\"kind\":\"header\",\"version\":1,\"pid\":42,\"created\":1,\"name\":\"same.exe\"}\n";
        fs::write(dir.join(FILE_NAME), header).unwrap();
        let recorder = WatchRecorder::open(dir.clone()).unwrap();
        let view = recorder.view().unwrap();
        assert_eq!(view.version, 1);
        assert!(view.program.is_none());
        assert_eq!(fs::read(dir.join(FILE_NAME)).unwrap(), header);
        WatchRecorder::delete_saved(&dir).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn observes_below_top_and_never_attaches_to_a_replacement() {
        let dir = folder("identity");
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
        rec.last = None;
        snapshot.processes[0].create_time += 1;
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
    fn restart_is_interrupted_and_storage_is_bounded() {
        let dir = folder("bounded");
        let mut rec = WatchRecorder::open(dir.clone()).unwrap();
        rec.action(own_start()).unwrap();
        let snapshot = sample(&rec.view().unwrap());
        rec.record(&snapshot).unwrap();
        assert_eq!(
            WatchRecorder::open(dir.clone())
                .unwrap()
                .view()
                .unwrap()
                .state,
            WatchState::Interrupted
        );
        for _ in 1..=MAX_POINTS {
            rec.last = None;
            rec.record(&snapshot).unwrap();
        }
        assert_eq!(rec.view().unwrap().state, WatchState::Limit);
        assert!(fs::metadata(dir.join(FILE_NAME)).unwrap().len() <= MAX_BYTES);
        assert!(rec.handle.is_none());
        rec.action(WatchAction::Delete).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn torn_or_oversized_records_are_bounded_and_can_be_deleted() {
        let dir = folder("corrupt");
        let mut rec = WatchRecorder::open(dir.clone()).unwrap();
        rec.action(own_start()).unwrap();
        rec.record(&sample(&rec.view().unwrap())).unwrap();
        rec.action(WatchAction::Stop).unwrap();
        OpenOptions::new()
            .append(true)
            .open(dir.join(FILE_NAME))
            .unwrap()
            .write_all(b"{\"kind\":")
            .unwrap();
        assert_eq!(
            WatchRecorder::open(dir.clone())
                .unwrap()
                .view()
                .unwrap()
                .state,
            WatchState::Interrupted
        );
        WatchRecorder::delete_saved(&dir).unwrap();
        let file = File::create(dir.join(FILE_NAME)).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        drop(file);
        assert!(matches!(
            WatchRecorder::open(dir.clone()),
            Err(WatchError::Invalid)
        ));
        WatchRecorder::delete_saved(&dir).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn gaps_at_the_quota_boundary_remain_readable() {
        let dir = folder("gap-quota");
        let mut rec = WatchRecorder::open(dir.clone()).unwrap();
        rec.action(own_start()).unwrap();
        let mut snapshot = sample(&rec.view().unwrap());
        for _ in 0..MAX_POINTS - 1 {
            rec.last = None;
            rec.record(&snapshot).unwrap();
        }
        snapshot.timestamp += 60_000;
        rec.last = None;
        rec.record(&snapshot).unwrap();
        assert_eq!(rec.view().unwrap().state, WatchState::Limit);
        assert!(rec.view().unwrap().points.len() <= MAX_POINTS + 1);
        assert_eq!(
            WatchRecorder::open(dir.clone())
                .unwrap()
                .view()
                .unwrap()
                .state,
            WatchState::Limit
        );
        rec.action(WatchAction::Delete).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
