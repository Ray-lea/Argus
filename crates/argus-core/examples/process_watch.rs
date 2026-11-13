//! Controlled real-memory observation. Only owns helper processes and a temporary folder.
//! cargo run -p argus-core --example process_watch --release
use argus_core::{
    Collector, MetaStore,
    history::{self, HistoryRecorder},
    watch::{WatchAction, WatchRecorder, WatchState},
};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::{io::AsRawHandle, process::CommandExt};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::HANDLE,
    System::{
        Memory::{MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree},
        Threading::GetProcessTimes,
    },
};

fn helper(competitor: bool) {
    let running = Arc::new(AtomicBool::new(true));
    let worker = competitor.then(|| {
        let running = running.clone();
        std::thread::spawn(move || {
            let mut value = 1u64;
            while running.load(Ordering::Relaxed) {
                for _ in 0..10_000 {
                    value =
                        std::hint::black_box(value.wrapping_mul(1664525).wrapping_add(1013904223));
                }
            }
            value
        })
    });
    let mut allocation: *mut std::ffi::c_void = std::ptr::null_mut();
    println!("ready");
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        if !allocation.is_null() {
            unsafe { VirtualFree(allocation, 0, MEM_RELEASE) }.unwrap();
            allocation = std::ptr::null_mut();
        }
        if line == "exit" {
            break;
        }
        let mib: usize = line.parse().unwrap();
        if mib > 0 {
            let size = mib * 1024 * 1024;
            allocation =
                unsafe { VirtualAlloc(None, size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
            assert!(!allocation.is_null());
            for offset in (0..size).step_by(4096) {
                unsafe { std::ptr::write_volatile(allocation.cast::<u8>().add(offset), 1) };
            }
        }
        println!("allocated {mib}");
    }
    if !allocation.is_null() {
        unsafe { VirtualFree(allocation, 0, MEM_RELEASE) }.unwrap();
    }
    running.store(false, Ordering::Relaxed);
    if let Some(worker) = worker {
        worker.join().unwrap();
    }
}

struct Helper {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}
impl Helper {
    fn spawn(mode: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg(mode)
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut helper = Self {
            child,
            input,
            output,
        };
        let mut ready = String::new();
        helper.output.read_line(&mut ready).unwrap();
        assert_eq!(ready.trim(), "ready");
        helper
    }
    fn memory(&mut self, mib: usize) {
        writeln!(self.input.as_mut().unwrap(), "{mib}").unwrap();
        let mut reply = String::new();
        self.output.read_line(&mut reply).unwrap();
        assert_eq!(reply.trim(), format!("allocated {mib}"));
    }
    fn creation(&self) -> i64 {
        let (mut c, mut e, mut k, mut u) = Default::default();
        unsafe {
            GetProcessTimes(
                HANDLE(self.child.as_raw_handle()),
                &mut c,
                &mut e,
                &mut k,
                &mut u,
            )
        }
        .unwrap();
        let ticks = ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64;
        (ticks / 10000) as i64 - 11_644_473_600_000
    }
    fn exit(&mut self) {
        if let Some(mut input) = self.input.take() {
            writeln!(input, "exit").unwrap();
            drop(input);
        }
        assert!(self.child.wait().unwrap().success());
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        self.input = None;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("memory-helper") => return helper(false),
        Some("competitor") => return helper(true),
        _ => {}
    }
    let dir = std::env::temp_dir().join(format!("argus-watch-real-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let mut target = Helper::spawn("memory-helper");
    let mut competitor = Helper::spawn("competitor");
    let pid = target.child.id();
    let competitor_pid = competitor.child.id();
    let created = target.creation();
    let mut collector = Collector::new(MetaStore::new());
    let mut history = HistoryRecorder::new(dir.clone());
    let mut recorder = WatchRecorder::open(dir.clone()).unwrap();
    recorder
        .action(WatchAction::Start {
            pid,
            create_time: created,
            name: "controlled-memory-helper".into(),
        })
        .unwrap();
    let mut times = Vec::new();
    let mut ranges = Vec::new();
    for mib in [4, 12, 2] {
        target.memory(mib);
        println!("phase {mib} MiB: sampling real process for 7 seconds");
        let mut last = None;
        for _ in 0..7 {
            std::thread::sleep(Duration::from_secs(1));
            let snapshot = collector.sample();
            last = Some(snapshot.timestamp);
            history.record(&snapshot);
            let t = Instant::now();
            recorder.record(&snapshot).unwrap();
            times.push(t.elapsed().as_micros());
        }
        let view = recorder.view().unwrap();
        let point = view
            .points
            .iter()
            .rev()
            .find(|p| p.memory.is_some())
            .unwrap();
        ranges.push((mib, point.memory.unwrap(), point.commit.unwrap()));
        if mib == 12 {
            println!("controlled sampling pause: 11 seconds (creates an explicit gap)");
            std::thread::sleep(Duration::from_secs(11));
        }
        if mib == 2 {
            let snapshot = collector.sample();
            history.record(&snapshot);
            let moment = history::moment(&dir, last.unwrap() - 5000).unwrap();
            assert!(
                !moment.processes.iter().any(|p| p.pid == pid),
                "watched helper unexpectedly entered the overall top"
            );
            assert!(
                moment.processes.iter().any(|p| p.pid == competitor_pid),
                "controlled CPU competitor missing from top"
            );
        }
    }
    assert!(ranges[1].1 > ranges[0].1 + 4 * 1024 * 1024);
    assert!(ranges[1].2 > ranges[0].2 + 4 * 1024 * 1024);
    assert!(ranges[2].1 + 4 * 1024 * 1024 < ranges[1].1);
    assert!(ranges[2].2 + 4 * 1024 * 1024 < ranges[1].2);
    target.exit();
    std::thread::sleep(Duration::from_millis(100));
    recorder.record(&collector.sample()).unwrap();
    let view = recorder.view().unwrap();
    assert_eq!(view.state, WatchState::Exited);
    if let Some(path) = std::env::args().nth(1) {
        fs::write(path, serde_json::to_vec_pretty(&view).unwrap()).unwrap();
    }
    assert!(view.points.iter().any(|p| p.state == WatchState::Gap));
    assert!(
        recorder
            .action(WatchAction::Start {
                pid: competitor_pid,
                create_time: competitor.creation(),
                name: "other-run".into()
            })
            .is_err()
    );
    recorder.action(WatchAction::Stop).unwrap();
    recorder.action(WatchAction::Delete).unwrap();
    assert!(recorder.view().is_none());
    assert!(!dir.join("process-watch.jsonl").exists());
    competitor.exit();
    drop(history);
    drop(recorder);
    fs::remove_dir_all(&dir).unwrap();
    println!(
        "PASS: private WS and commit grew/released; target below top; competitor in top; explicit gap; exact-instance exit; stop/delete; no replacement attached."
    );
    println!("memory phases (MiB requested, private WS bytes, private commit bytes): {ranges:?}");
    times.sort();
    println!("watch record time us (dev/release per invocation): {times:?}");
}
