//! Opt-in observation of future instances, with one validated handle per live instance.
use super::{
    SAMPLE_MS, WatchError, WatchMode, WatchPoint, WatchRun, WatchState, WatchView,
    identity::Instance,
    store::{self, Metadata, Store},
};
use crate::{Snapshot, util::OwnedHandle};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::WaitForSingleObject,
};

type Key = (u32, i64);
const MAX_ACTIVE: usize = 512;
const PREVIEW_POINTS: usize = 1000;
const PREVIEW_RUNS: usize = 256;

pub(super) struct ProgramWatch {
    store: Store,
    handles: HashMap<Key, OwnedHandle>,
    seen: HashSet<Key>,
    unconfirmed: HashMap<Key, Instant>,
    selected: Option<Key>,
    before: Option<u64>,
    run_offset: usize,
    active: bool,
    state: WatchState,
    last: Option<Instant>,
    last_ts: Option<i64>,
}

impl ProgramWatch {
    pub fn start(dir: &Path, pid: u32, created: i64, name: String) -> Result<Self, WatchError> {
        let instance = Instance::open(pid, created)?;
        let program = instance.program()?;
        std::fs::create_dir_all(dir).map_err(|_| WatchError::Storage)?;
        let store = Store::create(&dir.join(store::FILE_NAME), Metadata { program, name })?;
        Ok(Self {
            store,
            handles: HashMap::from([((pid, created), instance.into_handle())]),
            seen: HashSet::from([(pid, created)]),
            unconfirmed: HashMap::new(),
            selected: Some((pid, created)),
            before: None,
            run_offset: 0,
            active: true,
            state: WatchState::Recording,
            last: None,
            last_ts: None,
        })
    }

    pub fn open(dir: &Path) -> Result<Self, WatchError> {
        let store = Store::open(&dir.join(store::FILE_NAME), super::now_ms())?;
        // Restarting Argus never silently resumes an observation.
        let state = match store.entries.back().map(|e| e.point.state) {
            Some(WatchState::Stopped) => WatchState::Stopped,
            Some(WatchState::Limit) => WatchState::Limit,
            _ => WatchState::Interrupted,
        };
        Ok(Self {
            store,
            handles: HashMap::new(),
            seen: HashSet::new(),
            unconfirmed: HashMap::new(),
            selected: None,
            before: None,
            run_offset: 0,
            active: false,
            state,
            last: None,
            last_ts: None,
        })
    }

    pub fn select(&mut self, pid: u32, created: i64) -> Result<(), WatchError> {
        if !self.handles.contains_key(&(pid, created))
            && !self
                .store
                .entries
                .iter()
                .any(|e| (e.pid, e.created) == (pid, created))
        {
            return Err(WatchError::Missing);
        }
        self.selected = Some((pid, created));
        self.before = None;
        Ok(())
    }

    pub fn page(&mut self, before: Option<u64>) -> Result<(), WatchError> {
        if let Some(before) = before {
            let view = self.view();
            if !self
                .store
                .entries
                .iter()
                .any(|e| e.seq < before && (e.pid, e.created) == (view.pid, view.create_time))
            {
                return Err(WatchError::Missing);
            }
        }
        self.before = before;
        Ok(())
    }

    pub fn runs_page(&mut self, offset: usize) {
        self.run_offset = offset.min(store::CAPACITY - 1);
    }

    fn terminal(&mut self, key: Key, state: WatchState, ts: i64) -> Result<(), WatchError> {
        self.store.push(
            key.0,
            key.1,
            WatchPoint {
                ts,
                memory: None,
                commit: None,
                state,
            },
        )
    }

    pub fn stop(&mut self, state: WatchState, ts: i64) -> Result<(), WatchError> {
        self.active = false;
        self.state = state;
        let keys: Vec<_> = self.handles.keys().copied().collect();
        self.handles.clear();
        self.unconfirmed.clear();
        let ts = ts.max(self.last_ts.unwrap_or(ts));
        for key in keys {
            self.terminal(key, state, ts)?;
        }
        // Preserve a durable stop marker even while waiting between launches.
        if let Some(entry) = self.store.entries.back()
            && entry.point.state != state
        {
            self.terminal((entry.pid, entry.created), state, ts)?;
        }
        self.store.flush()
    }

    pub fn record(&mut self, snapshot: &Snapshot) -> Result<(), WatchError> {
        if !self.active {
            return self.store.prune(snapshot.timestamp);
        }
        let result = self.sample(snapshot);
        if result.is_err() {
            self.handles.clear();
            self.unconfirmed.clear();
            self.active = false;
            self.state = WatchState::Interrupted;
        }
        result
    }

    fn sample(&mut self, snapshot: &Snapshot) -> Result<(), WatchError> {
        if self.last_ts.is_some_and(|ts| snapshot.timestamp < ts) {
            return self.stop(WatchState::Interrupted, self.last_ts.unwrap());
        }
        let present: HashSet<_> = snapshot
            .processes
            .iter()
            .map(|row| (row.pid, row.create_time))
            .collect();
        self.seen
            .retain(|key| present.contains(key) || self.handles.contains_key(key));
        self.unconfirmed.retain(|key, attempted| {
            if !present.contains(key) {
                return false;
            }
            if attempted.elapsed() >= Duration::from_secs(60) {
                self.seen.remove(key);
                return false;
            }
            true
        });
        let basename = self
            .store
            .meta
            .program
            .display_path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or("");
        for row in &snapshot.processes {
            let key = (row.pid, row.create_time);
            // The basename only reduces queries. Final identity always comes from the validated handle.
            if row.create_time <= 0
                || !row.name.eq_ignore_ascii_case(basename)
                || !self.seen.insert(key)
            {
                continue;
            }
            let Ok(instance) = Instance::open(row.pid, row.create_time) else {
                self.unconfirmed.insert(key, Instant::now());
                continue;
            };
            let Ok(identity) = instance.program() else {
                self.unconfirmed.insert(key, Instant::now());
                continue;
            };
            if !self.store.meta.program.same_program(&identity) {
                continue;
            }
            if self.handles.len() == MAX_ACTIVE {
                return self.stop(WatchState::Limit, snapshot.timestamp);
            }
            self.handles.insert(key, instance.into_handle());
        }
        let terminal: Vec<_> = self
            .handles
            .iter()
            .filter_map(
                |(key, handle)| match unsafe { WaitForSingleObject(handle.0, 0) } {
                    WAIT_TIMEOUT => None,
                    WAIT_OBJECT_0 => Some((*key, WatchState::Exited)),
                    _ => Some((*key, WatchState::Interrupted)),
                },
            )
            .collect();
        for (key, state) in terminal {
            self.terminal(key, state, snapshot.timestamp)?;
            self.handles.remove(&key);
        }
        if self
            .last
            .is_some_and(|last| last.elapsed() < Duration::from_millis(SAMPLE_MS as u64))
        {
            return Ok(());
        }
        if let Some(previous) = self.last_ts
            && snapshot.timestamp.saturating_sub(previous) > SAMPLE_MS * 2
        {
            let keys: Vec<_> = self.handles.keys().copied().collect();
            // Terminal records may already have this snapshot's timestamp; keep sequence time monotonic.
            let gap_ts = self.store.entries.back().map_or(previous + SAMPLE_MS, |e| {
                e.point.ts.max(previous + SAMPLE_MS)
            });
            for key in keys {
                self.terminal(key, WatchState::Gap, gap_ts)?;
            }
        }
        let rows: HashMap<_, _> = snapshot
            .processes
            .iter()
            .map(|p| ((p.pid, p.create_time), p))
            .collect();
        for key in self.handles.keys() {
            let row = rows
                .get(key)
                .filter(|p| !p.stale && !p.sample_reused && !p.counters_pending);
            self.store.push(
                key.0,
                key.1,
                WatchPoint {
                    ts: snapshot.timestamp,
                    memory: row.map(|p| p.private_ws),
                    commit: row.map(|p| p.commit),
                    state: if row.is_some() {
                        WatchState::Recording
                    } else {
                        WatchState::Gap
                    },
                },
            )?;
        }
        self.store.prune(snapshot.timestamp)?;
        self.store.flush()?;
        self.last = Some(Instant::now());
        self.last_ts = Some(snapshot.timestamp);
        Ok(())
    }

    pub fn view(&self) -> WatchView {
        let mut runs: BTreeMap<Key, WatchRun> = BTreeMap::new();
        for entry in &self.store.entries {
            let run = runs.entry((entry.pid, entry.created)).or_insert(WatchRun {
                pid: entry.pid,
                create_time: entry.created,
                first_ts: entry.point.ts,
                last_ts: entry.point.ts,
                points: 0,
                state: WatchState::Interrupted,
            });
            run.last_ts = entry.point.ts;
            run.points += 1;
            run.state = if !self.active
                && matches!(entry.point.state, WatchState::Recording | WatchState::Gap)
            {
                WatchState::Interrupted
            } else {
                entry.point.state
            };
        }
        for key in self.handles.keys() {
            runs.entry(*key).or_insert(WatchRun {
                pid: key.0,
                create_time: key.1,
                first_ts: 0,
                last_ts: 0,
                points: 0,
                state: WatchState::Recording,
            });
        }
        let key = self
            .selected
            .filter(|key| runs.contains_key(key) || self.handles.contains_key(key))
            .or_else(|| {
                self.store
                    .entries
                    .back()
                    .map(|entry| (entry.pid, entry.created))
            })
            .or(self.selected)
            .unwrap_or((0, 0));
        let mut entries: Vec<_> = self
            .store
            .entries
            .iter()
            .rev()
            .filter(|entry| {
                (entry.pid, entry.created) == key
                    && self.before.is_none_or(|before| entry.seq < before)
            })
            .take(PREVIEW_POINTS)
            .collect();
        entries.reverse();
        let first_sequence = entries.first().map(|entry| entry.seq);
        let has_earlier = first_sequence.is_some_and(|seq| {
            self.store
                .entries
                .iter()
                .any(|e| e.seq < seq && (e.pid, e.created) == key)
        });
        let mut points: Vec<WatchPoint> = Vec::new();
        for entry in entries {
            if let Some(previous) = points.last()
                && entry.point.ts.saturating_sub(previous.ts) > SAMPLE_MS * 2
                && previous.memory.is_some()
            {
                points.push(WatchPoint {
                    ts: previous.ts + SAMPLE_MS,
                    memory: None,
                    commit: None,
                    state: WatchState::Gap,
                });
            }
            points.push(entry.point.clone());
        }
        let mut runs: Vec<_> = runs.into_values().collect();
        runs.sort_unstable_by_key(|run| (run.create_time, run.pid));
        let total_runs = runs.len();
        let selected_run = runs
            .iter()
            .find(|run| (run.pid, run.create_time) == key)
            .cloned();
        let run_offset = self.run_offset.min(total_runs.saturating_sub(1));
        let end = total_runs - run_offset;
        let start = end.saturating_sub(PREVIEW_RUNS);
        let runs = runs[start..end].to_vec();
        WatchView {
            version: 2,
            pid: key.0,
            create_time: key.1,
            name: self.store.meta.name.clone(),
            program: Some(self.store.meta.program.clone()),
            mode: WatchMode::Program,
            state: self.state,
            points,
            runs,
            total_runs,
            total_points: selected_run.map_or(0, |run| run.points),
            damaged: self.store.damaged,
            first_sequence,
            has_earlier,
            has_later: self.before.is_some(),
            run_offset,
            live_runs: self.handles.len(),
            unconfirmed: self.unconfirmed.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Collector,
        meta::MetaStore,
        util::{filetime_ticks, ticks_to_unix_ms},
    };
    use std::{
        os::windows::{io::AsRawHandle, process::CommandExt},
        process::{Child, Command, Stdio},
    };
    use windows::Win32::{Foundation::HANDLE, System::Threading::GetProcessTimes};

    #[test]
    fn pages_keep_older_points_and_launches_accessible_and_clock_reversal_interrupts() {
        let dir = std::env::temp_dir().join(format!("argus-watch-pages-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let mut store = Store::create(
            &dir.join(store::FILE_NAME),
            Metadata {
                name: "same.exe".into(),
                program: super::super::ProgramIdentity {
                    native_path: "\\Device\\disk\\same.exe".into(),
                    display_path: "C:\\same.exe".into(),
                },
            },
        )
        .unwrap();
        let start = super::super::now_ms() - 10_000_000;
        for i in 0..2001 {
            store
                .push(
                    17,
                    100,
                    WatchPoint {
                        ts: start + i * SAMPLE_MS,
                        memory: Some(i as u64),
                        commit: Some(0),
                        state: WatchState::Recording,
                    },
                )
                .unwrap();
        }
        for pid in 1000..1300 {
            store
                .push(
                    pid,
                    i64::from(pid),
                    WatchPoint {
                        ts: start + 2000 * SAMPLE_MS,
                        memory: Some(0),
                        commit: Some(0),
                        state: WatchState::Recording,
                    },
                )
                .unwrap();
        }
        store.flush().unwrap();
        drop(store);
        let mut watch = ProgramWatch::open(&dir).unwrap();
        watch.select(17, 100).unwrap();
        let view = watch.view();
        assert_eq!(view.total_runs, 301);
        assert_eq!(view.runs.len(), 256);
        assert_eq!(view.total_points, 2001);
        assert_eq!(view.points.first().unwrap().memory, Some(1001));
        assert!(view.has_earlier);
        watch.page(view.first_sequence).unwrap();
        let view = watch.view();
        assert_eq!(view.points.first().unwrap().memory, Some(1));
        assert!(view.has_earlier && view.has_later);
        watch.page(view.first_sequence).unwrap();
        assert_eq!(watch.view().points.first().unwrap().memory, Some(0));
        assert!(!watch.view().has_earlier);
        watch.page(None).unwrap();
        assert!(!watch.view().has_later);
        watch.runs_page(256);
        assert_eq!(watch.view().runs.len(), 45);
        assert_eq!(watch.view().runs[0].pid, 17);
        watch.runs_page(usize::MAX);
        assert_eq!(watch.view().runs.len(), 1);
        watch.active = true;
        watch.state = WatchState::Recording;
        let unconfirmed = Snapshot {
            seq: 1,
            timestamp: start + 2000 * SAMPLE_MS,
            system: Default::default(),
            hardware: Default::default(),
            sessions: Vec::new(),
            process_network: false,
            processes: vec![crate::ProcessRow {
                pid: std::process::id(),
                create_time: 1,
                name: "same.exe".into(),
                ..Default::default()
            }],
        };
        watch.record(&unconfirmed).unwrap();
        assert_eq!(watch.view().unconfirmed, 1);
        assert!(watch.handles.is_empty());
        assert!(watch.view().points.iter().all(|p| p.memory.is_some()));
        watch.last_ts = Some(start + 2000 * SAMPLE_MS);
        let snapshot = Snapshot {
            seq: 1,
            timestamp: start,
            system: Default::default(),
            hardware: Default::default(),
            sessions: Vec::new(),
            process_network: false,
            processes: Vec::new(),
        };
        watch.record(&snapshot).unwrap();
        assert!(!watch.active);
        assert_eq!(watch.state, WatchState::Interrupted);
        drop(watch);
        std::fs::remove_file(dir.join(store::FILE_NAME)).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    struct Helper(Child);
    impl Helper {
        fn spawn(path: &Path) -> Self {
            Self(
                Command::new(path)
                    .args(["--exact", "process::tests::liveness_child", "--ignored"])
                    .creation_flags(0x08000000)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        }
        fn key(&self) -> Key {
            let (mut created, mut exited, mut kernel, mut user) = Default::default();
            unsafe {
                GetProcessTimes(
                    HANDLE(self.0.as_raw_handle()),
                    &mut created,
                    &mut exited,
                    &mut kernel,
                    &mut user,
                )
            }
            .unwrap();
            (self.0.id(), ticks_to_unix_ms(filetime_ticks(created)))
        }
    }
    impl Drop for Helper {
        fn drop(&mut self) {
            drop(self.0.stdin.take());
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if self.0.try_wait().unwrap().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    #[ignore = "100 owned hidden idle helpers, real collector and two image folders; run explicitly"]
    fn real_idle_instances_restart_and_same_name_other_folder_remain_separate() {
        let dir =
            std::env::temp_dir().join(format!("argus-program-watch-real-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        for folder in ["one", "two"] {
            std::fs::create_dir(dir.join(folder)).unwrap();
            std::fs::copy(
                std::env::current_exe().unwrap(),
                dir.join(folder).join("same.exe"),
            )
            .unwrap();
        }
        let one = dir.join("one/same.exe");
        let two = dir.join("two/same.exe");
        let mut helpers: Vec<_> = (0..100).map(|_| Helper::spawn(&one)).collect();
        let other = Helper::spawn(&two);
        let seed = helpers[0].key();
        let mut watch =
            ProgramWatch::start(&dir, seed.0, seed.1, "UI label is not an identity".into())
                .unwrap();
        let mut collector = Collector::new(MetaStore::new());
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut max_record_us = 0;
        let mut max_view_us = 0;
        loop {
            let snapshot = collector.sample();
            watch.last = None;
            let started = Instant::now();
            watch.record(&snapshot).unwrap();
            max_record_us = max_record_us.max(started.elapsed().as_micros());
            let started = Instant::now();
            let view = watch.view();
            max_view_us = max_view_us.max(started.elapsed().as_micros());
            let measured = view
                .runs
                .iter()
                .filter(|run| {
                    watch.store.entries.iter().any(|e| {
                        (e.pid, e.created) == (run.pid, run.create_time) && e.point.memory.is_some()
                    })
                })
                .count();
            if measured == 100 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "not all 100 idle helpers received real measurements"
            );
            std::thread::sleep(Duration::from_millis(150));
        }
        assert_eq!(watch.handles.len(), 100);
        assert!(!watch.handles.contains_key(&other.key()));
        assert_eq!(watch.view().total_runs, 100);
        // Removing one instance never reuses its curve for the next launch.
        drop(helpers.remove(0));
        watch.record(&collector.sample()).unwrap();
        assert!(
            watch
                .store
                .entries
                .iter()
                .any(|e| (e.pid, e.created) == seed && e.point.state == WatchState::Exited)
        );
        helpers.push(Helper::spawn(&one));
        let restarted = helpers.last().unwrap().key();
        assert_ne!(seed, restarted);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !watch
            .store
            .entries
            .iter()
            .any(|e| (e.pid, e.created) == restarted && e.point.memory.is_some())
        {
            watch.last = None;
            watch.record(&collector.sample()).unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(150));
        }
        assert_eq!(watch.view().total_runs, 101);
        if let Some(path) = std::env::var_os("ARGUS_TEST_PROGRAM_WATCH_OUTPUT") {
            std::fs::write(path, serde_json::to_vec_pretty(&watch.view()).unwrap()).unwrap();
        }
        watch.select(seed.0, seed.1).unwrap();
        assert_eq!(
            watch.view().points.last().unwrap().state,
            WatchState::Exited
        );
        watch.select(restarted.0, restarted.1).unwrap();
        assert!(
            watch
                .view()
                .points
                .iter()
                .all(|p| p.state != WatchState::Exited)
        );
        assert!(matches!(
            watch.select(seed.0, seed.1 + 1),
            Err(WatchError::Missing)
        ));
        // A delayed observation is explicitly broken; old readings are never filled forward.
        let mut snapshot = collector.sample();
        snapshot.timestamp = watch.last_ts.unwrap() + 3 * SAMPLE_MS;
        watch.last = None;
        watch.record(&snapshot).unwrap();
        assert!(
            watch
                .view()
                .points
                .iter()
                .any(|p| p.state == WatchState::Gap && p.memory.is_none())
        );
        watch.stop(WatchState::Stopped, snapshot.timestamp).unwrap();
        let saved = watch.view().points.len();
        watch.record(&snapshot).unwrap();
        assert_eq!(watch.view().points.len(), saved);
        assert!(watch.handles.is_empty());
        println!(
            "owned_idle=100 runs=101 other_folder_attached=0 max_record_us={max_record_us} max_view_us={max_view_us} bytes={}",
            std::fs::metadata(dir.join(store::FILE_NAME)).unwrap().len()
        );
        drop(watch);
        let mut reopened = ProgramWatch::open(&dir).unwrap();
        assert!(!reopened.active);
        assert_eq!(reopened.state, WatchState::Stopped);
        reopened.select(seed.0, seed.1).unwrap();
        assert_eq!(
            reopened.view().points.last().unwrap().state,
            WatchState::Exited
        );
        drop(reopened);
        drop((helpers, other, collector));
        std::fs::remove_file(dir.join(store::FILE_NAME)).unwrap();
        for folder in ["one", "two"] {
            std::fs::remove_file(dir.join(folder).join("same.exe")).unwrap();
            std::fs::remove_dir(dir.join(folder)).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
