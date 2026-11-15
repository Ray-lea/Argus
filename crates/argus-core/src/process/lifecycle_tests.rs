use super::*;
use std::os::windows::{io::AsRawHandle, process::CommandExt};
use std::process::{Child, Command, Stdio};

struct ChildProcess(Child);

impl ChildProcess {
    fn start(suspended: bool) -> Self {
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let image = std::path::Path::new(&system)
            .join("System32")
            .join("PING.EXE");
        Self(
            Command::new(image)
                .args(["-n", "30", "127.0.0.1"])
                .creation_flags(0x0800_0000 | if suspended { 4 } else { 0 })
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    fn key(&self) -> (u32, i64) {
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
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
        (self.0.id(), filetime_ticks(created))
    }

    fn stop(&mut self) -> Instant {
        self.0.kill().unwrap();
        self.0.wait().unwrap();
        // Keep the external process handle open so Windows may still include
        // the exited object in its snapshot. Argus must release its own handle.
        Instant::now()
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn fresh_counters(tracker: &mut ProcessTracker, after: Instant) -> Vec<ProcessRow> {
    let cpus = std::thread::available_parallelism().unwrap().get() as u32;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let rows = tracker.sample_counters(cpus, None);
        if tracker.counted.is_some_and(|(started, _)| started >= after) {
            return rows;
        }
        assert!(
            Instant::now() < deadline,
            "fresh process snapshot timed out"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn zero_thread_snapshot(tracker: &mut ProcessTracker, key: (u32, i64), cycles: u64) {
    // Whether an exited object remains in native enumeration varies. Reproduce
    // the reported zero-thread record while retaining a real process identity
    // and real handles, so this regression is independent of the Windows build.
    tracker.query = Some(
        ProcessQuery::spawn(move |buffer| {
            let mut record: crate::nt::SystemProcessInformation = unsafe { std::mem::zeroed() };
            record.unique_process_id = HANDLE(key.0 as usize as *mut _);
            record.create_time = key.1;
            record.cycle_time = cycles;
            unsafe {
                buffer
                    .as_mut_ptr()
                    .cast::<crate::nt::SystemProcessInformation>()
                    .write(record)
            };
            Ok(size_of::<crate::nt::SystemProcessInformation>())
        })
        .unwrap(),
    );
}

#[test]
fn exited_process_releases_its_cached_handle_and_keeps_final_cycles() {
    let mut child = ChildProcess::start(false);
    let key = child.key();
    let mut tracker = ProcessTracker::new(MetaStore::new());
    fresh_counters(&mut tracker, Instant::now());
    let entry = tracker.tracked.get(&key).expect("running child is tracked");
    assert!(entry.handle.is_some());
    let before = entry.cpu_cycles;
    let stopped = child.stop();
    let final_cycles = process_cycles(HANDLE(child.0.as_raw_handle())).unwrap();
    zero_thread_snapshot(&mut tracker, key, final_cycles);
    fresh_counters(&mut tracker, stopped);
    let entry = tracker
        .tracked
        .get(&key)
        .expect("zero-thread record was sampled");
    assert!(
        entry.handle.is_none(),
        "exited process handle is still cached"
    );
    assert_eq!(entry.cpu_cycles, final_cycles);
    assert!(entry.cpu_cycles >= before);
    // A later snapshot must not recreate the handle while an external owner
    // still keeps the exited process object in the native enumeration.
    fresh_counters(&mut tracker, Instant::now());
    assert!(
        tracker
            .tracked
            .get(&key)
            .is_none_or(|entry| entry.handle.is_none())
    );
}

#[test]
fn zero_thread_snapshot_never_gets_a_cached_handle() {
    let child = ChildProcess::start(false);
    let key = child.key();
    // Keep this process queryable to prove the snapshot prevents opening a
    // handle, rather than accidentally passing because OpenProcess failed.
    assert!(query_handle(key.0, key.1).is_some());
    let mut tracker = ProcessTracker::new(MetaStore::new());
    zero_thread_snapshot(&mut tracker, key, 0);
    fresh_counters(&mut tracker, Instant::now());
    assert!(
        tracker.tracked.get(&key).unwrap().handle.is_none(),
        "a process first observed after exit acquired a cached handle"
    );
}

#[test]
fn a_suspended_process_keeps_its_query_handle() {
    let child = ChildProcess::start(true);
    let key = child.key();
    let mut tracker = ProcessTracker::new(MetaStore::new());
    let rows = fresh_counters(&mut tracker, Instant::now());
    let row = rows.iter().find(|row| row.pid == key.0).unwrap();
    assert!(row.suspended && row.threads > 0);
    assert!(tracker.tracked.get(&key).unwrap().handle.is_some());
}

#[test]
fn native_observation_time_survives_cached_rows_and_late_replies() {
    let mut tracker = ProcessTracker::new(MetaStore::new());
    fresh_counters(&mut tracker, Instant::now());
    let original = tracker.sample_started().unwrap();
    let (ready, captured) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let mut calls = 0;
    tracker.query = Some(
        ProcessQuery::spawn(move |buffer| {
            let result = query_system_information(SPI_CLASS, buffer);
            if calls < 2 {
                ready.send(()).unwrap();
                // A failed assertion drops the sender and releases the worker.
                let _ = released.recv_timeout(Duration::from_secs(5));
            }
            calls += 1;
            result
        })
        .unwrap(),
    );
    let cpus = std::thread::available_parallelism().unwrap().get() as u32;
    let rows = tracker.sample_counters(cpus, None);
    captured.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!rows.is_empty() && rows.iter().all(|row| row.sample_reused));
    assert_eq!(tracker.sample_started(), Some(original));
    let requested = Instant::now();
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while tracker.sample_started() == Some(original) {
        tracker.sample_counters(cpus, None);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    // This newly accepted reply was captured before the action's refresh.
    assert!(tracker.sample_started().unwrap() < requested);
    captured.recv_timeout(Duration::from_secs(5)).unwrap();
    release.send(()).unwrap();
    fresh_counters(&mut tracker, requested);
    assert!(tracker.sample_started().unwrap() >= requested);
}
