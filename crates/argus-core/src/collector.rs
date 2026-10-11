//! Combines system and process sampling into one snapshot.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::hardware::{Hardware, HardwareTracker};
use crate::meta::MetaStore;
use crate::process::{ProcessRow, ProcessTracker};
use crate::system::{SystemStats, SystemTracker};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub seq: u64,
    /// Unix ms when the sample was taken.
    pub timestamp: i64,
    pub system: SystemStats,
    pub hardware: Hardware,
    pub processes: Vec<ProcessRow>,
}

pub struct Collector {
    system: SystemTracker,
    processes: ProcessTracker,
    hardware: HardwareTracker,
    seq: u64,
}

impl Collector {
    /// Creates a collector and takes a priming sample, so the first real
    /// snapshot already has meaningful rates.
    pub fn new(meta: Arc<MetaStore>) -> Self {
        let system = SystemTracker::new();
        let mut processes = ProcessTracker::new(meta);
        processes.sample(system.logical_cpus());
        Self {
            system,
            processes,
            hardware: HardwareTracker::new(),
            seq: 0,
        }
    }

    pub fn sample(&mut self) -> Snapshot {
        let mut system = self.system.sample();
        let processes = self.processes.sample(self.system.logical_cpus());
        let hardware = self.hardware.sample();

        system.process_count = processes.len() as u32;
        for p in &processes {
            system.thread_count += p.threads;
            system.handle_count += p.handles;
            system.io_read += p.io_read;
            system.io_write += p.io_write;
            system.io_other += p.io_other;
        }

        self.seq += 1;
        Snapshot {
            seq: self.seq,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or_default(),
            system,
            hardware,
            processes,
        }
    }
}
