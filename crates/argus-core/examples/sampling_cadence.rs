//! Measure snapshot cadence without printing process or device identities.
//! `cargo run -p argus-core --example sampling_cadence -- 60 --trace`

use std::time::{Duration, Instant};

use argus_core::{Collector, MetaStore};

struct Timings;
impl log::Log for Timings {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "argus_sampling"
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            println!(
                "{}",
                serde_json::json!({"trace": record.args().to_string()})
            );
        }
    }

    fn flush(&self) {}
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let count = args
        .iter()
        .find_map(|arg| arg.parse::<usize>().ok())
        .unwrap_or(60)
        .clamp(1, 300);
    if args.iter().any(|arg| arg == "--trace") {
        log::set_logger(&Timings).expect("timing logger");
        log::set_max_level(log::LevelFilter::Trace);
    }
    let initialized = Instant::now();
    let mut collector = Collector::new(MetaStore::new());
    println!(
        "{}",
        serde_json::json!({"initialization_ms": initialized.elapsed().as_secs_f64() * 1000.})
    );
    let mut previous = Instant::now();
    for _ in 0..count {
        std::thread::sleep(Duration::from_secs(1).saturating_sub(previous.elapsed()));
        let started = Instant::now();
        let snapshot = collector.sample();
        println!(
            "{}",
            serde_json::json!({
                "seq": snapshot.seq,
                "elapsed_ms": started.elapsed().as_secs_f64() * 1000.,
                "interval_ms": started.duration_since(previous).as_secs_f64() * 1000.,
                "processes": snapshot.processes.len(),
                "stale_processes": snapshot.processes.iter().filter(|process| process.stale).count(),
                "timestamp": snapshot.timestamp,
            })
        );
        previous = started;
    }
}
