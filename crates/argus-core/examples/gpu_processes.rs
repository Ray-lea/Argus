//! Prints per-process GPU usage and how long collecting it takes.
//! `cargo run -p argus-core --example gpu_processes --release`

use std::time::{Duration, Instant};

use argus_core::gpu_process::GpuProcessTracker;

fn main() {
    let t0 = Instant::now();
    let Some(mut tracker) = GpuProcessTracker::new() else {
        println!("no GPU counters on this machine");
        return;
    };
    println!("setup {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
    let mut times = Vec::new();
    for round in 0..10 {
        std::thread::sleep(Duration::from_secs(1));
        let t = Instant::now();
        let usage = tracker.sample();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        times.push(ms);
        if round == 9 {
            let mut top: Vec<_> = usage.iter().collect();
            top.sort_by(|a, b| b.1.util.total_cmp(&a.1.util));
            println!("{} processes with GPU use", usage.len());
            for (pid, g) in top.iter().take(8) {
                let engine = g
                    .engine
                    .as_ref()
                    .map(|(l, n)| format!("{l:x}/{n}"))
                    .unwrap_or_default();
                println!(
                    "  pid {pid:6}  {:5.1}%  {engine:24}  dedicated {:6.1} MB  shared {:6.1} MB",
                    g.util,
                    g.dedicated as f64 / 1048576.0,
                    g.shared as f64 / 1048576.0
                );
            }
        }
    }
    times.sort_by(f64::total_cmp);
    println!(
        "sample: median {:.2} ms, max {:.2} ms",
        times[times.len() / 2],
        times[times.len() - 1]
    );
}
