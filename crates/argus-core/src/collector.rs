//! Combines system and process sampling into one snapshot.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::app_windows;
use crate::gpu_process::{GpuEngineUsage, GpuProcessTracker};
use crate::hardware::{Hardware, HardwareTracker};
use crate::meta::MetaStore;
use crate::net_process::NetworkTracker;
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
    /// Per-process network traffic is measured, locally or by the service.
    pub process_network: bool,
}

pub struct Collector {
    system: SystemTracker,
    processes: ProcessTracker,
    hardware: HardwareTracker,
    gpu_processes: Option<GpuProcessTracker>,
    network: Option<NetworkTracker>,
    /// `GPU 0 - 3D` labels by adapter LUID and engine, shared by every process.
    engine_labels: HashMap<(u64, Arc<str>), Arc<str>>,
    engine_ids: HashMap<(u64, u32, u32), Arc<str>>,
    seq: u64,
}

impl Collector {
    /// Creates a collector and takes a priming sample, so the first real
    /// snapshot already has meaningful rates.
    pub fn new(meta: Arc<MetaStore>) -> Self {
        let system = SystemTracker::new();
        let mut processes = ProcessTracker::new(meta);
        processes.sample(system.logical_cpus(), system.precise_idle_fraction());
        Self {
            system,
            processes,
            hardware: HardwareTracker::new(),
            gpu_processes: GpuProcessTracker::new(),
            network: None,
            engine_labels: HashMap::new(),
            engine_ids: HashMap::new(),
            seq: 0,
        }
    }

    /// Starts measuring per-process network traffic under the given ETW
    /// session name; false without the rights to start a trace session.
    pub fn enable_process_network(&mut self, session_name: &str) -> bool {
        if self.network.is_none() {
            self.network = NetworkTracker::start(session_name);
        }
        self.network.is_some()
    }

    pub fn sample(&mut self) -> Snapshot {
        let mut system = self.system.sample();
        let mut processes = self.processes.sample(
            self.system.logical_cpus(),
            self.system.precise_idle_fraction(),
        );
        let mut hardware = self.hardware.sample();
        self.attach_gpu_usage(&mut processes, &mut hardware);
        if self
            .network
            .as_ref()
            .is_some_and(|network| !network.is_active())
        {
            self.network = None;
        }
        if let Some(network) = &mut self.network {
            let rates = network.rates();
            for p in &mut processes {
                if let Some(&(sent, received)) = rates.get(&p.pid) {
                    p.net_sent = sent;
                    p.net_received = received;
                }
            }
        }

        let apps = app_windows::owners();
        for p in &mut processes {
            p.app_window = apps.contains(&p.pid);
        }

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
            process_network: self.network.is_some(),
        }
    }

    fn attach_gpu_usage(&mut self, processes: &mut [ProcessRow], hardware: &mut Hardware) {
        let Some(tracker) = &mut self.gpu_processes else {
            return;
        };
        let mut usage = tracker.sample();
        if usage.is_empty() {
            return;
        }
        // Adapters are numbered as on the performance page.
        let index: HashMap<u64, usize> = hardware
            .gpus
            .iter()
            .enumerate()
            .filter_map(|(i, g)| g.info.id.parse().ok().map(|id| (id, i)))
            .collect();
        // Aggregate the same query window by physical node. Taking the maximum
        // of already-reduced process percentages would lose secondary engines.
        let mut totals: HashMap<(u64, u32, u32), f32> = HashMap::new();
        for engine in usage.values().flat_map(|g| &g.engines) {
            let total = totals
                .entry((engine.luid, engine.physical, engine.node))
                .or_default();
            *total = (*total + engine.util).min(100.0);
        }
        for ((luid, physical, node), util) in totals {
            if let Some(&i) = index.get(&luid) {
                let engine = if physical == 0 {
                    self.hardware.gpu_node_engine(luid, node)
                } else {
                    None
                };
                hardware.gpus[i].include_process_engine(engine, util);
            }
        }
        for p in processes {
            let Some(g) = usage.remove(&p.pid) else {
                continue;
            };
            p.gpu = g.util;
            p.gpu_dedicated = g.dedicated;
            p.gpu_shared = g.shared;
            p.gpu_engine = g.engine.as_ref().map(|(luid, name)| {
                self.engine_labels
                    .entry((*luid, name.clone()))
                    .or_insert_with(|| match index.get(luid) {
                        Some(i) => format!("GPU {i} - {name}").into(),
                        None => name.clone(),
                    })
                    .clone()
            });
            for engine in g.engines {
                let adapter_index = index.get(&engine.luid).copied();
                let name = self
                    .engine_labels
                    .entry((engine.luid, engine.name.clone()))
                    .or_insert_with(|| match adapter_index {
                        Some(i) => format!("GPU {i} - {}", engine.name).into(),
                        None => engine.name.clone(),
                    })
                    .clone();
                let id = self
                    .engine_ids
                    .entry((engine.luid, engine.physical, engine.node))
                    .or_insert_with(|| {
                        format!("{}:{}:{}", engine.luid, engine.physical, engine.node).into()
                    })
                    .clone();
                p.gpu_engines.push(GpuEngineUsage {
                    id,
                    name,
                    util: engine.util,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "samples live GPU counters for three seconds"]
    fn live_gpu_snapshot_keeps_process_and_device_engine_totals_consistent() {
        let mut collector = Collector::new(MetaStore::new());
        for _ in 0..3 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let snapshot = collector.sample();
            let mut engines: HashMap<&str, f32> = HashMap::new();
            for process in &snapshot.processes {
                let peak = process
                    .gpu_engines
                    .iter()
                    .map(|e| e.util)
                    .fold(0.0, f32::max);
                assert!((peak - process.gpu).abs() < 0.01);
                for engine in &process.gpu_engines {
                    let value = engines.entry(&engine.id).or_default();
                    *value = (*value + engine.util).min(100.0);
                }
            }
            for gpu in &snapshot.hardware.gpus {
                let prefix = format!("{}:", gpu.info.id);
                let peak = engines
                    .iter()
                    .filter(|(id, _)| id.starts_with(&prefix))
                    .map(|(_, v)| *v)
                    .fold(0.0, f32::max);
                assert!(
                    gpu.util + 0.01 >= peak,
                    "device {}% below process engine {}%",
                    gpu.util,
                    peak
                );
                println!(
                    "{}: total={:.2}% process engines={:.2}% source={:?}",
                    gpu.info.name, gpu.util, peak, gpu.util_source
                );
            }
        }
    }
}
