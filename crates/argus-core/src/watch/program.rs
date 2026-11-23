//! Opt-in process observation, with one validated handle per live instance.
use super::{
    SAMPLE_MS, WatchError, WatchPoint, WatchRun, WatchState, WatchView,
    identity::Instance,
    series::RecentRun,
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
    recent: HashMap<Key, RecentRun>,
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
        // Image identity is optional: without it, only this live instance is observed.
        let program = instance.program().ok();
        std::fs::create_dir_all(dir).map_err(|_| WatchError::Storage)?;
        let store = Store::create(&dir.join(store::FILE_NAME), Metadata { program, name })?;
        Ok(Self {
            store,
            handles: HashMap::from([((pid, created), instance.into_handle())]),
            recent: HashMap::new(),
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

    /// Keep process handles and selection across a history-folder transaction.
    /// Only the storage handle has to close for a cross-volume transfer.
    pub(super) fn suspend(self) -> impl FnOnce(&Path) -> Result<Self, WatchError> {
        let Self {
            store,
            handles,
            recent,
            seen,
            unconfirmed,
            selected,
            before,
            run_offset,
            active,
            state,
            last,
            last_ts,
        } = self;
        drop(store);
        move |dir| {
            Ok(Self {
                store: Store::open(&dir.join(store::FILE_NAME), super::now_ms())?,
                handles,
                recent,
                seen,
                unconfirmed,
                selected,
                before,
                run_offset,
                active,
                state,
                last,
                last_ts,
            })
        }
    }
    pub fn open(dir: &Path) -> Result<Self, WatchError> {
        let store = Store::open(&dir.join(store::FILE_NAME), super::now_ms())?;
        // Restarting Argus never silently resumes an observation.
        let state = match store.entries.back().map(|e| e.point.state) {
            Some(WatchState::Stopped) => WatchState::Stopped,
            Some(WatchState::Limit) => WatchState::Limit,
            Some(WatchState::Exited) if store.meta.program.is_none() => WatchState::Exited,
            _ => WatchState::Interrupted,
        };
        Ok(Self {
            store,
            handles: HashMap::new(),
            recent: HashMap::new(),
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

    #[cfg(test)]
    pub(super) fn clear_sample_timer_for_test(&mut self) {
        self.last = None;
    }

    fn terminal(&mut self, key: Key, state: WatchState, ts: i64) -> Result<(), WatchError> {
        let point = WatchPoint::marker(ts, state);
        self.store.push(key.0, key.1, point.clone())?;
        if let Some(recent) = self.recent.get_mut(&key) {
            recent.push(point, 0);
        }
        Ok(())
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

    #[cfg(test)]
    fn record(&mut self, snapshot: &Snapshot) -> Result<(), WatchError> {
        self.record_at_interval(snapshot, Duration::from_secs(1))
    }

    pub fn record_at_interval(
        &mut self,
        snapshot: &Snapshot,
        cadence: Duration,
    ) -> Result<(), WatchError> {
        self.recent.retain(|_, run| run.prune(snapshot.timestamp));
        if !self.active {
            return self.store.prune(snapshot.timestamp);
        }
        let result = self.sample(snapshot, cadence);
        if result.is_err() {
            self.handles.clear();
            self.unconfirmed.clear();
            self.active = false;
            self.state = WatchState::Interrupted;
        }
        result
    }

    fn sample(&mut self, snapshot: &Snapshot, cadence: Duration) -> Result<(), WatchError> {
        if self.last_ts.is_some_and(|ts| snapshot.timestamp < ts) {
            return self.stop(WatchState::Interrupted, self.last_ts.unwrap());
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
        let mut ended_state = None;
        for (key, state) in terminal {
            self.terminal(key, state, snapshot.timestamp)?;
            self.handles.remove(&key);
            ended_state = Some(state);
        }
        if let Some(target_program) = self.store.meta.program.clone() {
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
            let basename = target_program
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
                if !target_program.same_program(&identity) {
                    continue;
                }
                if self.handles.len() == MAX_ACTIVE {
                    return self.stop(WatchState::Limit, snapshot.timestamp);
                }
                self.handles.insert(key, instance.into_handle());
            }
        }
        if self.store.meta.program.is_none() && self.handles.is_empty() {
            self.active = false;
            self.state = ended_state.unwrap_or(WatchState::Exited);
            return self.store.flush();
        }
        let save = self
            .last
            .is_none_or(|last| last.elapsed() >= Duration::from_millis(SAMPLE_MS as u64));
        let expected = cadence.as_millis().clamp(1, 60_000) as i64;
        let elapsed = self.last_ts.map(|ts| snapshot.timestamp.saturating_sub(ts));
        let interrupted = elapsed.is_some_and(|dt| dt > (expected + SAMPLE_MS).max(2 * SAMPLE_MS));
        if let Some(previous) = self.last_ts
            && interrupted
        {
            // New launches start at their first sample; an earlier gap belongs to older runs.
            let keys: Vec<_> = self
                .handles
                .keys()
                .filter(|key| key.1 <= previous)
                .copied()
                .collect();
            // Terminal records may already have this snapshot's timestamp; keep sequence time monotonic.
            let gap_start = previous.saturating_add(expected);
            let gap_ts = self
                .store
                .entries
                .back()
                .map_or(gap_start, |e| e.point.ts.max(gap_start));
            for key in keys {
                self.terminal(key, WatchState::Gap, gap_ts)?;
            }
        }
        let span = elapsed
            .filter(|dt| *dt > 0 && !interrupted)
            .unwrap_or(expected);
        let rows: HashMap<_, _> = snapshot
            .processes
            .iter()
            .filter(|p| self.handles.contains_key(&(p.pid, p.create_time)))
            .map(|p| ((p.pid, p.create_time), p))
            .collect();
        let net_enabled = snapshot.process_network;
        for key in self.handles.keys() {
            let row = rows
                .get(key)
                .filter(|p| !p.stale && !p.sample_reused && !p.counters_pending);
            let point = match row {
                Some(p) => WatchPoint {
                    ts: snapshot.timestamp,
                    memory: Some(p.private_ws),
                    commit: Some(p.commit),
                    working_set: Some(p.working_set),
                    cpu: p.cpu.is_finite().then_some(p.cpu.max(0.)),
                    gpu: p.gpu.is_finite().then_some(p.gpu.max(0.)),
                    gpu_memory: p.gpu_dedicated,
                    disk_read: p.disk_read,
                    disk_write: p.disk_write,
                    io_read: Some(p.io_read),
                    io_write: Some(p.io_write),
                    net_received: net_enabled.then_some(p.net_received),
                    net_sent: net_enabled.then_some(p.net_sent),
                    threads: Some(p.threads),
                    handles: Some(p.handles),
                    state: WatchState::Recording,
                },
                None => WatchPoint::marker(snapshot.timestamp, WatchState::Gap),
            };
            // The first sample of a short-lived instance also survives a restart.
            if save || !self.recent.contains_key(key) {
                self.store.push(key.0, key.1, point.clone())?;
            }
            self.recent.entry(*key).or_default().push(point, span);
        }
        if save {
            self.store.prune(snapshot.timestamp)?;
            self.store.flush()?;
            self.last = Some(Instant::now());
        }
        self.last_ts = Some(snapshot.timestamp);
        Ok(())
    }

    /// One run's readings over a window (`SeriesQuery`), from its records.
    pub fn series(&self, query: &super::SeriesQuery) -> crate::history::HistorySeries {
        let key = (query.pid, query.create_time);
        super::series::series(
            |from| {
                let entries = &self.store.entries;
                let at = entries.partition_point(|e| e.point.ts < from);
                // Two earlier readings give both the cadence at the left edge
                // and the start of a gap crossing it. Markers between them stay.
                let start = entries
                    .range(..at)
                    .enumerate()
                    .rev()
                    .filter(|(_, e)| (e.pid, e.created) == key && e.point.has_any_metric())
                    .nth(1)
                    .map_or(0, |(i, _)| i);
                entries
                    .range(start..)
                    .filter(move |e| (e.pid, e.created) == key)
                    .map(|e| &e.point)
            },
            self.recent.get(&key),
            query,
        )
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
            let run = runs.entry(*key).or_insert(WatchRun {
                pid: key.0,
                create_time: key.1,
                first_ts: 0,
                last_ts: 0,
                points: 0,
                state: WatchState::Recording,
            });
            if let Some(latest) = self.recent.get(key).and_then(RecentRun::latest) {
                run.last_ts = latest.ts;
                run.state = latest.state;
            }
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
                && entry.point.ts.saturating_sub(previous.ts) > super::series::MAX_SPACING_MS
                && previous.memory.is_some()
            {
                points.push(WatchPoint::marker(previous.ts + SAMPLE_MS, WatchState::Gap));
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
            version: super::VERSION,
            pid: key.0,
            create_time: key.1,
            name: self.store.meta.name.clone(),
            program: self.store.meta.program.clone(),
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
                program: Some(super::super::ProgramIdentity {
                    native_path: "\\Device\\disk\\same.exe".into(),
                    display_path: "C:\\same.exe".into(),
                }),
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
                        memory: Some(i as u64),
                        commit: Some(0),
                        ..WatchPoint::marker(start + i * SAMPLE_MS, WatchState::Recording)
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
                        memory: Some(0),
                        commit: Some(0),
                        ..WatchPoint::marker(start + 2000 * SAMPLE_MS, WatchState::Recording)
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
            leaks: Vec::new(),
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
            leaks: Vec::new(),
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

    #[test]
    fn full_watch_reaps_exits_before_admitting_a_confirmed_launch() {
        use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcess};
        let pid = std::process::id();
        let (mut created, mut exit, mut kernel, mut user) = Default::default();
        unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        }
        .unwrap();
        let birth = ticks_to_unix_ms(filetime_ticks(created));
        let name = std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for exited in [true, false] {
            let dir = std::env::temp_dir().join(format!("argus-watch-cap-{pid}-{exited}"));
            let mut w = ProgramWatch::start(&dir, pid, birth, name.clone()).unwrap();
            w.handles.clear();
            w.seen.clear();
            // Waitable events supply 512 independent liveness states. The new
            // instance still takes the production PID, birth and image checks.
            for i in 1..=MAX_ACTIVE as u32 {
                let h = unsafe { CreateEventW(None, true, exited && i == 1, None) }.unwrap();
                w.handles.insert((i, 1), OwnedHandle(h));
            }
            let ts = super::super::now_ms();
            w.record(&Snapshot {
                seq: 1,
                timestamp: ts,
                system: Default::default(),
                hardware: Default::default(),
                sessions: vec![],
                leaks: vec![],
                process_network: false,
                processes: vec![crate::ProcessRow {
                    pid,
                    create_time: birth,
                    name: name.clone().into(),
                    ..Default::default()
                }],
            })
            .unwrap();
            if exited {
                assert!(w.active && w.handles.contains_key(&(pid, birth)));
                assert_eq!(w.handles.len(), MAX_ACTIVE);
                let first = w.store.entries.front().unwrap();
                assert_eq!(
                    (first.pid, first.created, first.point.ts, first.point.state),
                    (1, 1, ts, WatchState::Exited)
                );
            } else {
                assert!(!w.active);
                assert_eq!(w.state, WatchState::Limit);
                assert_eq!(w.store.entries.len(), MAX_ACTIVE);
            }
            drop(w);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    struct Helper(Child);

    #[test]
    fn a_new_launch_does_not_inherit_a_gap_before_its_creation() {
        let dir = std::env::temp_dir().join(format!(
            "argus-program-watch-birth-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_str().unwrap().to_owned();
        let first = Helper::spawn(&exe);
        let first_key = first.key();
        let mut watch = ProgramWatch::start(&dir, first_key.0, first_key.1, name.clone()).unwrap();
        let snapshot = |keys: &[Key]| Snapshot {
            seq: 1,
            timestamp: super::super::now_ms(),
            system: Default::default(),
            hardware: Default::default(),
            sessions: Vec::new(),
            leaks: Vec::new(),
            process_network: false,
            processes: keys
                .iter()
                .map(|&(pid, created)| crate::ProcessRow {
                    pid,
                    create_time: created,
                    name: name.clone().into(),
                    ..Default::default()
                })
                .collect(),
        };
        watch.record(&snapshot(&[first_key])).unwrap();
        std::thread::sleep(Duration::from_millis((SAMPLE_MS * 2 + 1000) as u64));
        let later = Helper::spawn(&exe);
        let later_key = later.key();
        watch.record(&snapshot(&[first_key, later_key])).unwrap();
        let before_birth = watch
            .store
            .entries
            .iter()
            .filter(|entry| (entry.pid, entry.created) == later_key && entry.point.ts < later_key.1)
            .map(|entry| (entry.point.ts, entry.point.state))
            .collect::<Vec<_>>();
        let existing_gap = watch.store.entries.iter().any(|entry| {
            (entry.pid, entry.created) == first_key && entry.point.state == WatchState::Gap
        });
        drop(watch);
        drop(later);
        drop(first);
        std::fs::remove_file(dir.join(store::FILE_NAME)).unwrap();
        std::fs::remove_dir(dir).unwrap();
        assert!(
            before_birth.is_empty(),
            "records before creation: {before_birth:?}"
        );
        assert!(
            existing_gap,
            "the existing run must retain the sampling gap"
        );
    }

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
