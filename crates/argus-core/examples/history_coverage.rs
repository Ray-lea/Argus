//! Bounded synthetic cost measurement: seven days, twelve selected instances per frame.
//! cargo run -p argus-core --example history_coverage --release
use argus_core::{
    ProcessRow, Snapshot,
    history::{self, FRAME_MS, HistoryRecorder},
};
use std::{fs, time::Instant};

fn main() {
    let dir = std::env::temp_dir().join(format!("argus-history-coverage-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let base = 1_699_920_000_000i64;
    let mut snapshot = Snapshot {
        seq: 0,
        timestamp: base,
        system: Default::default(),
        hardware: Default::default(),
        sessions: Vec::new(),
        process_network: true,
        processes: (0..12)
            .map(|i| ProcessRow {
                pid: 100 + i,
                create_time: base - 1000,
                name: format!("sample{i}.exe").into(),
                cpu: if i < 6 { 2. } else { 0. },
                private_ws: if i >= 6 { (i as u64 + 1) << 20 } else { 1024 },
                net_received: 10,
                net_state: argus_core::measurement::Measurement::ready(),
                gpu_state: argus_core::measurement::Measurement::ready(),
                io_valid: true,
                ..Default::default()
            })
            .collect(),
    };
    let frames = 7 * 86400000 / FRAME_MS;
    let mut recorder = HistoryRecorder::new(dir.clone());
    let start = Instant::now();
    for index in 0..=frames {
        snapshot.timestamp = base + index * FRAME_MS;
        recorder.record(&snapshot);
    }
    drop(recorder);
    let write_elapsed = start.elapsed();
    let bytes = history::usage(&dir).bytes;
    let mut times = Vec::new();
    for _ in 0..11 {
        let start = Instant::now();
        let series = history::series(&dir, base, base + frames * FRAME_MS - 1, 500);
        assert_eq!(series.len(), 500);
        times.push(start.elapsed().as_micros());
    }
    times.remove(0);
    times.sort();
    let moment = history::moment(&dir, base + (frames - 1) * FRAME_MS).unwrap();
    assert_eq!(moment.processes.len(), 12);
    assert!(
        moment
            .processes
            .iter()
            .all(|p| p.net_coverage.as_ref().is_some_and(|c| c.valid == 1))
    );
    println!(
        "7 days, {frames} frames, 12 entries/frame, v5 bytes={bytes}; write total={write_elapsed:?}; bytes/frame={:.1}",
        bytes as f64 / frames as f64
    );
    println!("series 7 days / 500 points, warm cache, 1 warmup + 10 reads us: {times:?}");
    let mut interval_times = Vec::new();
    for _ in 0..4 {
        let started = Instant::now();
        let interval = history::cpu_interval(&dir, base, base + frames * FRAME_MS).unwrap();
        assert_eq!(interval.recorded_frames, frames as usize);
        assert_eq!(interval.missing_frames, 0);
        assert_eq!(interval.leaders.len(), 12);
        interval_times.push(started.elapsed().as_micros());
    }
    interval_times.remove(0);
    interval_times.sort();
    println!(
        "CPU interval 7 days / 12 instances, warm cache, 1 warmup + 3 reads us: {interval_times:?}"
    );
    history::clear(&dir).unwrap();
    fs::remove_dir(dir).unwrap();
}
