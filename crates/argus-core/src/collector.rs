//! Combines system and process sampling into one snapshot.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::app_windows;
use crate::diagnostics::{sample_phase, startup_phase};
use crate::gpu_process::{GpuEngineUsage, GpuProcessTracker};
use crate::hardware::{Hardware, HardwareTracker};
use crate::leaks::{Leak, LeakTracker};
use crate::meta::MetaStore;
use crate::net_process::NetworkTracker;
use crate::process::{ProcessRow, ProcessTracker};
use crate::sessions::{LogonSession, SessionTracker};
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
    /// Processes that appear to leak handles or processes (`leaks.rs`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub leaks: Vec<Leak>,
    /// The sessions a user is signed in to: a process belongs to the user of
    /// its session, and to the system where its session is not listed.
    pub sessions: Vec<LogonSession>,
    /// Per-process network traffic is measured, locally or by the service.
    pub process_network: bool,
}

pub struct Collector {
    system: SystemTracker,
    processes: ProcessTracker,
    leaks: LeakTracker,
    sessions: SessionTracker,
    hardware: HardwareTracker,
    /// Per-process GPU usage, absent without an adapter the performance page lists.
    gpu_processes: Option<GpuProcessTracker>,
    /// When the GPUs were last enumerated again.
    gpus_enumerated: Option<Instant>,
    network: Option<NetworkTracker>,
    /// `GPU 0 - 3D` labels by adapter LUID and engine, shared by every process.
    engine_labels: HashMap<(u64, Arc<str>), Arc<str>>,
    engine_ids: HashMap<(u64, u32, u32), Arc<str>>,
    seq: u64,
}

impl Collector {
    /// Start of the latest accepted process query, retained while a query is
    /// pending or fails. Roster-only updates do not advance this observation.
    pub fn process_sample_started(&self) -> Option<Instant> {
        self.processes.sample_started()
    }

    /// Creates a collector and takes a priming sample, so the first real
    /// snapshot already has meaningful rates.
    pub fn new(meta: Arc<MetaStore>) -> Self {
        let system = startup_phase("collector.system", SystemTracker::new);
        let mut processes = startup_phase("collector.processes", || ProcessTracker::new(meta));
        startup_phase("collector.prime-processes", || {
            processes.sample(system.logical_cpus(), system.precise_idle_fraction())
        });
        let sessions = startup_phase("collector.sessions", SessionTracker::new);
        let hardware = startup_phase("collector.hardware", HardwareTracker::new);
        // Processes are measured on the adapters the performance page lists.
        let gpu_processes = startup_phase("collector.gpu-pdh", || {
            GpuProcessTracker::new(hardware.gpu_adapters())
        });
        Self {
            system,
            processes,
            leaks: LeakTracker::new(),
            sessions,
            hardware,
            gpu_processes,
            gpus_enumerated: None,
            network: None,
            engine_labels: HashMap::new(),
            engine_ids: HashMap::new(),
            seq: 0,
        }
    }

    /// Whether process rows carry the counters only optional columns show
    /// (`ProcessRow::extra`), from the next sample on.
    pub fn set_extra_counters(&mut self, enabled: bool) {
        self.processes.set_extra_counters(enabled);
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
        let mut system = sample_phase("system", || self.system.sample());
        let mut processes = sample_phase("processes", || {
            self.processes.sample(
                self.system.logical_cpus(),
                self.system.precise_idle_fraction(),
            )
        });
        // Where the processor clock checks out, total and per-processor usage
        // come from idle cycles over the processes' own interval: the time
        // counters' figures stay only where it does not.
        if let Some(usage) = self.processes.cycle_usage() {
            system.apply_cycle_usage(usage);
        }
        let mut hardware = self.hardware.sample();
        sample_phase("gpu-processes", || {
            self.attach_gpu_usage(&mut processes, &mut hardware)
        });
        self.follow_gpu_changes();
        if self
            .network
            .as_ref()
            .is_some_and(|network| !network.is_active())
        {
            self.network = None;
        }
        if let Some(network) = &mut self.network {
            network.apply_rates(&mut processes);
        }

        let sessions = sample_phase("sessions", || self.sessions.sample());

        let apps = sample_phase("app-windows", app_windows::owners);
        for p in &mut processes {
            p.app_window = apps.contains(&p.pid);
        }

        system.memory_compression = crate::system::compression_store().map(|store| {
            let process = (store.pid != 0)
                .then(|| processes.iter().find(|p| p.pid == store.pid))
                .flatten();
            store.memory(process.map(|p| p.working_set))
        });
        system.process_count = processes.len() as u32;
        if system.thread_count == 0 {
            system.thread_count = processes.iter().map(|p| p.threads).sum();
        }
        if system.handle_count == 0 {
            system.handle_count = processes.iter().map(|p| p.handles).sum();
        }
        for p in &processes {
            system.io_read += p.io_read;
            system.io_write += p.io_write;
            system.io_other += p.io_other;
        }

        self.seq += 1;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        self.leaks.observe(timestamp, &processes);
        Snapshot {
            seq: self.seq,
            timestamp,
            system,
            hardware,
            processes,
            leaks: self.leaks.findings().to_vec(),
            sessions,
            process_network: self.network.is_some(),
        }
    }

    /// The counters named an adapter that no enumeration returned: a driver
    /// update restarts an adapter under a new LUID, and an adapter can be
    /// attached later. Enumerating again lets the performance page and the
    /// processes follow from the next sample on; at most every 10 seconds.
    fn follow_gpu_changes(&mut self) {
        const INTERVAL: Duration = Duration::from_secs(10);
        let Some(unknown) = self
            .gpu_processes
            .as_ref()
            .map(GpuProcessTracker::unknown_adapters)
            .filter(|unknown| !unknown.is_empty())
            .cloned()
        else {
            return;
        };
        if self
            .gpus_enumerated
            .is_some_and(|at| at.elapsed() < INTERVAL)
        {
            return;
        }
        self.gpus_enumerated = Some(Instant::now());
        sample_phase("gpu-enumerate", || self.hardware.refresh_gpus(&unknown));
        let adapters = self.hardware.gpu_adapters();
        // Labels carry adapter numbers, which follow the new order.
        self.engine_labels.clear();
        self.engine_ids.clear();
        if adapters.listed.is_empty() {
            self.gpu_processes = None;
        } else if let Some(tracker) = &mut self.gpu_processes {
            tracker.set_adapters(adapters);
        }
    }

    fn attach_gpu_usage(&mut self, processes: &mut [ProcessRow], hardware: &mut Hardware) {
        let Some(tracker) = &mut self.gpu_processes else {
            return;
        };
        let mut usage = tracker.sample();
        let available = tracker.memory_available();
        if usage.is_empty() && !available.iter().any(|available| *available) {
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
            p.gpu_dedicated = available[0].then_some(0);
            p.gpu_shared = available[1].then_some(0);
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

    #[test]
    #[ignore = "enumerates live GPUs and samples their counters"]
    fn a_gpu_missing_from_a_stale_list_returns_after_one_sample() {
        use crate::gpu_process::{GpuAdapters, GpuMemoryLayout};
        let mut collector = Collector::new(MetaStore::new());
        let real = collector.hardware.gpu_adapters();
        if real.listed.is_empty() {
            return;
        }
        // As after a driver update: the list holds a LUID that is gone and
        // lacks the adapter's new one.
        collector.hardware.forget_gpus();
        collector
            .gpu_processes
            .as_mut()
            .unwrap()
            .set_adapters(GpuAdapters {
                listed: HashMap::from([(0xDEAD_BEEF, GpuMemoryLayout::Discrete)]),
                ignored: real.ignored.clone(),
            });
        std::thread::sleep(std::time::Duration::from_secs(1));
        let stale = collector.sample();
        assert!(stale.hardware.gpus.is_empty());
        assert!(stale.processes.iter().all(|p| p.gpu == 0.0));
        assert!(collector.gpus_enumerated.is_some());
        let found = collector.hardware.gpu_adapters();
        assert_eq!(found, real);
        std::thread::sleep(std::time::Duration::from_secs(1));
        let after = collector.sample();
        let ids: std::collections::HashSet<u64> = after
            .hardware
            .gpus
            .iter()
            .map(|g| g.info.id.parse().unwrap())
            .collect();
        assert_eq!(ids, real.listed.keys().copied().collect());
        assert!(
            after
                .processes
                .iter()
                .any(|p| p.gpu_dedicated.is_some_and(|bytes| bytes > 0))
        );
        let labelled = after
            .processes
            .iter()
            .filter_map(|p| p.gpu_engine.as_deref())
            .collect::<Vec<_>>();
        assert!(labelled.iter().all(|label| label.starts_with("GPU ")));
        assert!(
            collector
                .gpu_processes
                .as_ref()
                .unwrap()
                .unknown_adapters()
                .is_empty()
        );
        println!("re-found {ids:x?}; engines {labelled:?}");
    }
}
