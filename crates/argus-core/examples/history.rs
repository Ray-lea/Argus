//! Records history into a temporary folder for a while, then reads it back:
//! what recording costs per sample, what a frame costs on disk, how fast the
//! history page's queries are.
//! `cargo run -p argus-core --example history --release [-- seconds]`

use std::fs;
use std::time::{Duration, Instant};

use argus_core::history::{self, FRAME_MS, HistoryRecorder};
use argus_core::{Collector, MetaStore};

fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let dir = std::env::temp_dir().join(format!("argus-history-{}", std::process::id()));
    let mut collector = Collector::new(MetaStore::new());
    let mut recorder = HistoryRecorder::new(dir.clone());
    let mut times = Vec::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_secs(1));
        let snapshot = collector.sample();
        let t = Instant::now();
        recorder.record(&snapshot);
        times.push(t.elapsed().as_secs_f64() * 1e6);
    }
    drop(recorder);
    times.sort_by(f64::total_cmp);
    println!(
        "record: median {:.0} us, max {:.0} us over {} samples (the max includes a frame write)",
        times[times.len() / 2],
        times[times.len() - 1],
        times.len()
    );

    let bytes: u64 = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0);
    let Some((first, last)) = history::range(&dir) else {
        println!("no frames recorded (run for at least 10 seconds)");
        return;
    };
    let frames = (last - first) / FRAME_MS + 1;
    let per_frame = bytes as f64 / frames as f64;
    println!(
        "{frames} frames in {bytes} bytes: {per_frame:.0} bytes a frame, {:.1} MB a day of running",
        per_frame * (86_400_000 / FRAME_MS) as f64 / 1e6
    );

    let t = Instant::now();
    let points = history::series(&dir, first, last + FRAME_MS, 600);
    println!("series: {} points in {:?}", points.len(), t.elapsed());
    let t = Instant::now();
    if let Some(m) = history::moment(&dir, last) {
        println!(
            "moment: {} processes in {:?}",
            m.processes.len(),
            t.elapsed()
        );
        let mut procs = m.processes;
        procs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        for p in procs.iter().take(8) {
            println!(
                "  {:28} pid {:6}  cpu {:5.1}%  mem {:7.1} MB  io {:8} B/s  gpu {:4.1}%",
                p.name,
                p.pid,
                p.cpu,
                p.mem as f64 / 1048576.0,
                p.io,
                p.gpu
            );
        }
    }
    let _ = fs::remove_dir_all(&dir);
}
