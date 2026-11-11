//! One explicitly selected startup path, with durable first-call records.
//! No UI, privileged service, ETW network session or device configuration.
//! The host package supplies the same static VC runtime as the portable app.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use argus_core::diagnostics::startup_phase;
use argus_core::{Collector, MetaStore};

struct Journal {
    file: Mutex<File>,
    began: Instant,
}

impl log::Log for Journal {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("argus") && metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let row = serde_json::json!({
            "elapsed_us": self.began.elapsed().as_micros(),
            "thread": format!("{:?}", std::thread::current().id()),
            "message": record.args().to_string(),
        });
        let mut file = self.file.lock().unwrap();
        if writeln!(file, "{row}")
            .and_then(|_| file.sync_data())
            .is_err()
        {
            drop(file);
            eprintln!("Startup diagnostic record could not be persisted");
            std::process::exit(74);
        }
    }

    fn flush(&self) {
        let _ = self.file.lock().unwrap().sync_data();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut stage = None;
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--stage" => stage = args.next(),
            "--output" => output = args.next().map(PathBuf::from),
            _ => {
                return Err(
                    "Use --stage collector|gpu-windows|nvml --output <new log file>".into(),
                );
            }
        }
    }
    let stage = stage.ok_or("An explicit stage is required")?;
    if !matches!(
        stage.as_str(),
        "collector" | "gpu-windows" | "nvml" | "verify-journal"
    ) {
        return Err("Unknown startup stage".into());
    }
    let output = output.ok_or("An explicit output file is required")?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    log::set_logger(Box::leak(Box::new(Journal {
        file: Mutex::new(file),
        began: Instant::now(),
    })))
    .map_err(|_| "Startup diagnostic logger is already installed")?;
    log::set_max_level(log::LevelFilter::Info);
    std::panic::set_hook(Box::new(|panic| {
        if let Some(at) = panic.location() {
            log::error!(target: "argus_startup", "phase=probe.panic; source={}:{}", argus_core::diagnostics::source_file(at.file()), at.line());
        }
    }));
    log::info!(target: "argus_startup", "phase=probe.start; stage={stage}; version={}", env!("CARGO_PKG_VERSION"));
    let result = match stage.as_str() {
        "collector" => {
            let mut collector = startup_phase("collector.construct", || {
                Collector::new(startup_phase("collector.metadata", MetaStore::new))
            });
            let snapshot = startup_phase("collector.sample", || collector.sample());
            let result = serde_json::json!({
                "processes": snapshot.processes.len(),
                "gpus": snapshot.hardware.gpus.len(),
                "disks": snapshot.hardware.disks.len(),
            });
            startup_phase("collector.close", || drop(collector));
            result
        }
        "gpu-windows" => serde_json::json!({
            "adapters": argus_core::hardware::startup_probe::windows_gpu(),
        }),
        "nvml" => serde_json::json!({
            "devices": argus_core::hardware::startup_probe::nvidia(),
        }),
        "verify-journal" => {
            // Exercise deduplication without querying any hardware.
            for _ in 0..2 {
                startup_phase("probe.journal-check", || ());
            }
            serde_json::json!({"journal": true})
        }
        _ => unreachable!(),
    };
    log::info!(target: "argus_startup", "phase=probe.complete; stage={stage}; counts={result}");
    println!("{result}");
    Ok(())
}
