//! Opt-in frame timing, buffered outside the render path. No window contents.
use std::sync::{Mutex, OnceLock, mpsc};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

struct Trace {
    frequency: u64,
    records: Mutex<Vec<[u64; 3]>>,
    sender: mpsc::SyncSender<Vec<[u64; 3]>>,
}

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("ARGUS_GPUI_FRAME_TRACE").is_some())
}

pub(crate) fn record(stage: u64, value: u64) {
    if !enabled() {
        return;
    }
    static TRACE: OnceLock<Trace> = OnceLock::new();
    let trace = TRACE.get_or_init(|| {
        let mut frequency = 0;
        unsafe {
            QueryPerformanceFrequency(&mut frequency).expect("QPC frequency");
        }
        let (sender, receiver) = mpsc::sync_channel::<Vec<[u64; 3]>>(8);
        std::thread::spawn(move || {
            while let Ok(records) = receiver.recv() {
                println!("{{\"event\":\"gpu-frame-trace\",\"records\":{records:?}}}");
            }
        });
        Trace {
            frequency: frequency as u64,
            records: Mutex::new(Vec::with_capacity(512)),
            sender,
        }
    });
    let mut counter = 0;
    unsafe {
        QueryPerformanceCounter(&mut counter).expect("QPC counter");
    }
    let at = (counter as u128 * 10_000_000 / trace.frequency as u128) as u64;
    let mut records = trace.records.lock().unwrap();
    records.push([stage, at, value]);
    if records.len() == 512 {
        let batch = std::mem::replace(&mut *records, Vec::with_capacity(512));
        drop(records);
        let _ = trace.sender.try_send(batch);
    }
}
