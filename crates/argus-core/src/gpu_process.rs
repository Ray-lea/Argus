//! Per-process GPU usage from the `GPU Engine` and `GPU Process Memory`
//! performance counters. Windows publishes them for every process to standard
//! users (Task Manager's GPU columns read the same data), and one PDH
//! collection per sample covers all processes and adapters.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use windows::Win32::System::Performance::PDH_HCOUNTER;
use windows::core::w;

use crate::hardware::pretty_engine;
use crate::pdh::{self, Query};
use crate::util::AlignedBuf;

fn utilization(value: f64) -> Option<f32> {
    (value.is_finite() && value > 0.0).then_some(value.min(100.0) as f32)
}

/// How an adapter's memory falls into the budget segment groups that the
/// `Local Usage` and `Non Local Usage` counters report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuMemoryLayout {
    /// Video memory is local and system memory non-local.
    Discrete,
    /// Unified memory: Windows groups all of it, system memory included, as
    /// local, and the non-local group is empty.
    Unified,
}

/// The adapters an enumeration found, by LUID. The counters also cover
/// adapters the performance page leaves out; a LUID in neither set is one
/// the enumeration has not seen, such as an adapter restarted by a driver
/// update or newly attached.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuAdapters {
    /// The adapters the performance page lists, with how their memory is grouped.
    pub listed: HashMap<u64, GpuMemoryLayout>,
    /// Adapters it leaves out: software renderers such as the Microsoft Basic
    /// Render Driver that renders on the CPU, display-only and compute-only
    /// adapters, and LUIDs an enumeration did not return.
    pub ignored: HashSet<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessGpu {
    /// The process's share of its busiest engine, percent.
    pub util: f32,
    /// Adapter (LUID, as in `GpuInfo::id`) and name of that engine.
    pub engine: Option<(u64, Arc<str>)>,
    /// Resident GPU memory, bytes: in the adapters' own memory and in system
    /// memory. Absent when its source is unavailable.
    pub dedicated: Option<u64>,
    pub shared: Option<u64>,
    pub engines: Vec<ProcessGpuEngine>,
}

#[derive(Debug, Clone)]
pub struct ProcessGpuEngine {
    pub luid: u64,
    pub physical: u32,
    pub node: u32,
    pub name: Arc<str>,
    pub util: f32,
}

/// Sparse engine contributions. Identity includes the physical adapter and node;
/// a display name alone does not distinguish duplicated Copy/Compute engines.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GpuEngineUsage {
    pub id: Arc<str>,
    pub name: Arc<str>,
    pub util: f32,
}

/// Memory counter readings by process, adapter LUID and physical adapter.
type MemoryReadings = HashMap<(u32, u64, u32), Option<u64>>;

pub struct GpuProcessTracker {
    query: Query,
    /// Only the listed adapters count.
    adapters: GpuAdapters,
    /// LUIDs the last sample met that are neither listed nor ignored.
    unknown: HashSet<u64>,
    engines: Option<PDH_HCOUNTER>,
    /// `Local Usage`, `Non Local Usage` and, with a unified adapter, `Dedicated Usage`.
    memory_counters: [Option<PDH_HCOUNTER>; 3],
    buf: AlignedBuf,
    /// Pretty engine names by counter engine type (`VideoDecode` -> `Video Decode`).
    names: HashMap<String, Arc<str>>,
    memory_available: [bool; 2],
    memory: [MemoryReadings; 3],
}

// PDH handles are process-wide; the sampler uses them from one thread at a time.
unsafe impl Send for GpuProcessTracker {}

impl GpuProcessTracker {
    /// Measures the processes' use of the listed adapters. None without one
    /// (a machine without a GPU has only the software renderer) or when the
    /// machine publishes no GPU counters (no WDDM 2.x adapter).
    pub fn new(adapters: GpuAdapters) -> Option<Self> {
        if adapters.listed.is_empty() {
            return None;
        }
        let query = Query::open()?;
        let engines = query.add(w!(r"\GPU Engine(*)\Utilization Percentage"));
        // Local/Non Local Usage report resident budget usage. Dedicated/Shared Usage
        // are committed counters and can inflate on affected Windows/driver versions,
        // so Dedicated Usage only bounds the carve-out of unified adapters.
        let memory_counters = [
            query.add(w!(r"\GPU Process Memory(*)\Local Usage")),
            query.add(w!(r"\GPU Process Memory(*)\Non Local Usage")),
            committed_dedicated(&query, &adapters),
        ];
        if engines.is_none() && memory_counters[..2].iter().all(Option::is_none) {
            return None;
        }
        // Utilization is a rate: the first collection only sets the baseline.
        query.collect();
        Some(Self {
            query,
            adapters,
            unknown: HashSet::new(),
            engines,
            memory_counters,
            buf: AlignedBuf::with_capacity(256 * 1024),
            names: HashMap::new(),
            memory_available: [false; 2],
            memory: Default::default(),
        })
    }

    /// GPU usage of every process that has any, by PID.
    pub fn sample(&mut self) -> HashMap<u32, ProcessGpu> {
        let mut out: HashMap<u32, ProcessGpu> = HashMap::new();
        self.memory_available = [false; 2];
        self.unknown.clear();
        if !self.query.collect() {
            return out;
        }
        let (buf, names, adapters, unknown) = (
            &mut self.buf,
            &mut self.names,
            &self.adapters,
            &mut self.unknown,
        );
        if let Some(c) = self.engines {
            // One instance per process and engine node; the busiest node wins,
            // as for the adapter-wide figures.
            pdh::each(c, buf, |instance, value| {
                let Some(util) = utilization(value) else {
                    return;
                };
                let Some(i) = listed(instance, adapters, unknown) else {
                    return;
                };
                let entry = out.entry(i.pid).or_default();
                let Some(node) = i.node else { return };
                let name = match names.get(i.engine) {
                    Some(n) => n.clone(),
                    None => {
                        let n: Arc<str> = pretty_engine(i.engine).into();
                        names.insert(i.engine.to_owned(), n.clone());
                        n
                    }
                };
                if let Some(existing) = entry
                    .engines
                    .iter_mut()
                    .find(|e| (e.luid, e.physical, e.node) == (i.luid, i.physical, node))
                {
                    existing.util = existing.util.max(util);
                } else {
                    entry.engines.push(ProcessGpuEngine {
                        luid: i.luid,
                        physical: i.physical,
                        node,
                        name: name.clone(),
                        util,
                    });
                }
                if util > entry.util {
                    entry.util = util;
                    entry.engine = Some((i.luid, name));
                }
            });
        }
        // Retain distinct physical adapters, but count duplicate instances once.
        let mut read = [false; 3];
        for ((counter, values), read) in self
            .memory_counters
            .into_iter()
            .zip(&mut self.memory)
            .zip(&mut read)
        {
            values.clear();
            if let Some(c) = counter {
                *read = pdh::each_value(c, buf, |instance, value| {
                    record_memory(values, adapters, unknown, instance, value);
                });
            }
        }
        self.memory_available = [read[0], read[1]];
        let available = self.memory_available;
        for entry in out.values_mut() {
            entry.dedicated = available[0].then_some(0);
            entry.shared = available[1].then_some(0);
        }
        let [local, non_local, committed] = &self.memory;
        let keys = local
            .keys()
            .chain(non_local.keys().filter(|key| !local.contains_key(*key)));
        for &key in keys {
            let (pid, luid, _) = key;
            let reading = |values: &MemoryReadings, read: bool| {
                if read {
                    values.get(&key).copied().unwrap_or(Some(0))
                } else {
                    None
                }
            };
            let (dedicated, shared) = split_memory(
                adapters.listed[&luid],
                reading(local, read[0]),
                reading(non_local, read[1]),
                committed.get(&key).copied().flatten(),
            );
            let entry = out.entry(pid).or_insert_with(|| ProcessGpu {
                dedicated: available[0].then_some(0),
                shared: available[1].then_some(0),
                ..Default::default()
            });
            entry.dedicated = entry
                .dedicated
                .zip(dedicated)
                .map(|(a, b)| a.saturating_add(b));
            entry.shared = entry.shared.zip(shared).map(|(a, b)| a.saturating_add(b));
        }
        out
    }

    pub fn memory_available(&self) -> [bool; 2] {
        self.memory_available
    }

    /// LUIDs the last sample met that the enumeration did not return: the
    /// adapters are worth enumerating again.
    pub fn unknown_adapters(&self) -> &HashSet<u64> {
        &self.unknown
    }

    /// Follows a new enumeration from the next sample on, keeping the query
    /// and its utilization baseline.
    pub fn set_adapters(&mut self, adapters: GpuAdapters) {
        if self.memory_counters[2].is_none() {
            self.memory_counters[2] = committed_dedicated(&self.query, &adapters);
        }
        self.adapters = adapters;
    }
}

/// `Dedicated Usage`, read only to bound the carve-out of unified adapters.
fn committed_dedicated(query: &Query, adapters: &GpuAdapters) -> Option<PDH_HCOUNTER> {
    adapters
        .listed
        .values()
        .any(|a| *a == GpuMemoryLayout::Unified)
        .then(|| query.add(w!(r"\GPU Process Memory(*)\Dedicated Usage")))
        .flatten()
}

/// One process's resident memory on one adapter as (dedicated, shared), from
/// its `Local Usage`, `Non Local Usage` and committed `Dedicated Usage`.
/// A unified adapter reports system memory as local: only what fits in its
/// own memory segments (a firmware carve-out, if any) is dedicated.
fn split_memory(
    layout: GpuMemoryLayout,
    local: Option<u64>,
    non_local: Option<u64>,
    committed_dedicated: Option<u64>,
) -> (Option<u64>, Option<u64>) {
    match layout {
        GpuMemoryLayout::Discrete => (local, non_local),
        GpuMemoryLayout::Unified => {
            let dedicated = local.map(|local| committed_dedicated.unwrap_or(0).min(local));
            let shared =
                local
                    .zip(dedicated)
                    .zip(non_local)
                    .map(|((local, dedicated), non_local)| {
                        (local - dedicated).saturating_add(non_local)
                    });
            (dedicated, shared)
        }
    }
}

fn record_memory(
    values: &mut MemoryReadings,
    adapters: &GpuAdapters,
    unknown: &mut HashSet<u64>,
    instance: &str,
    value: Option<f64>,
) {
    let Some(instance) = listed(instance, adapters, unknown).filter(|i| i.node.is_none()) else {
        return;
    };
    let value = value
        .filter(|value| value.is_finite() && *value >= 0. && *value < u64::MAX as f64)
        .map(|value| value as u64);
    values
        .entry((instance.pid, instance.luid, instance.physical))
        .and_modify(|known| {
            *known = match (*known, value) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (Some(a), None) => Some(a),
                (None, value) => value,
            }
        })
        .or_insert(value);
}

/// A counter instance on a listed adapter. An instance on an adapter that is
/// neither listed nor ignored adds its LUID to `unknown`.
fn listed<'a>(
    instance: &'a str,
    adapters: &GpuAdapters,
    unknown: &mut HashSet<u64>,
) -> Option<Instance<'a>> {
    let instance = parse(instance)?;
    if adapters.listed.contains_key(&instance.luid) {
        return Some(instance);
    }
    if !adapters.ignored.contains(&instance.luid) {
        unknown.insert(instance.luid);
    }
    None
}

struct Instance<'a> {
    pid: u32,
    luid: u64,
    physical: u32,
    node: Option<u32>,
    /// Empty for memory counters.
    engine: &'a str,
}

/// `pid_1234_luid_0x00000000_0x0000D1F3_phys_0_eng_0_engtype_VideoDecode`
/// (engine counters) or `pid_1234_luid_0x00000000_0x0000D1F3_phys_0` (memory).
fn parse(instance: &str) -> Option<Instance<'_>> {
    let instance = instance
        .rsplit_once('#')
        .filter(|(_, suffix)| !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit()))
        .map_or(instance, |(name, _)| name);
    let rest = instance.strip_prefix("pid_")?;
    let (pid, rest) = rest.split_once('_')?;
    let rest = rest.strip_prefix("luid_0x")?;
    let (high, rest) = rest.split_once("_0x")?;
    let (low, rest) = rest.split_once('_').unwrap_or((rest, ""));
    let luid = ((u32::from_str_radix(high, 16).ok()? as u64) << 32)
        | u32::from_str_radix(low, 16).ok()? as u64;
    let rest = rest.strip_prefix("phys_")?;
    let (physical, rest) = rest.split_once('_').unwrap_or((rest, ""));
    let (node, engine) = if rest.is_empty() {
        (None, "")
    } else {
        let (node, engine) = rest.strip_prefix("eng_")?.split_once("_engtype_")?;
        (Some(node.parse().ok()?), engine)
    };
    Some(Instance {
        pid: pid.parse().ok()?,
        luid,
        physical: physical.parse().ok()?,
        node,
        engine,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        GpuAdapters, GpuMemoryLayout, listed, parse, record_memory, split_memory, utilization,
    };
    use std::collections::{HashMap, HashSet};

    /// An RTX 4060 Laptop GPU and the Microsoft Basic Render Driver, as measured.
    fn adapters() -> GpuAdapters {
        GpuAdapters {
            listed: HashMap::from([(0x10A2B, GpuMemoryLayout::Discrete)]),
            ignored: HashSet::from([0x196FB]),
        }
    }

    #[test]
    fn memory_deduplicates_physical_instances_and_keeps_zero_and_missing_distinct() {
        let mut memory = HashMap::new();
        let (adapters, mut unknown) = (adapters(), HashSet::new());
        let mut record = |instance: &str, value| {
            record_memory(&mut memory, &adapters, &mut unknown, instance, value)
        };
        let instance = "pid_42_luid_0x00000000_0x00010A2B_phys_0";
        record(instance, Some(128.));
        record(&format!("{instance}#1"), Some(128.));
        record("pid_42_luid_0x00000000_0x00010A2B_phys_1", Some(0.));
        record("pid_7_luid_0x00000000_0x00010A2B_phys_0", None);
        record(
            "pid_8_luid_0x00000000_0x00010A2B_phys_0",
            Some(f64::INFINITY),
        );
        assert_eq!(memory.len(), 4);
        assert_eq!(memory[&(42, 0x10A2B, 0)], Some(128));
        assert_eq!(memory[&(42, 0x10A2B, 1)], Some(0));
        assert_eq!(memory[&(7, 0x10A2B, 0)], None);
        assert_eq!(memory[&(8, 0x10A2B, 0)], None);
        assert!(unknown.is_empty());
    }

    #[test]
    fn adapters_the_performance_page_leaves_out_contribute_no_usage() {
        // The Microsoft Basic Render Driver (WARP) is all a machine without a
        // GPU has, and its CPU rendering is published as 3D engine time.
        let (adapters, mut unknown) = (adapters(), HashSet::new());
        let warp_engine = "pid_42_luid_0x00000000_0x000196FB_phys_0_eng_3_engtype_3D";
        assert!(parse(warp_engine).is_some());
        assert!(listed(warp_engine, &adapters, &mut unknown).is_none());
        assert!(
            listed(
                "pid_42_luid_0x00000000_0x00010A2B_phys_0_eng_0_engtype_3D",
                &adapters,
                &mut unknown
            )
            .is_some()
        );
        let mut memory = HashMap::new();
        record_memory(
            &mut memory,
            &adapters,
            &mut unknown,
            "pid_42_luid_0x00000000_0x000196FB_phys_0",
            Some(384. * 1048576.),
        );
        assert!(memory.is_empty());
        assert!(unknown.is_empty(), "an ignored adapter is not news");
    }

    #[test]
    fn an_adapter_no_enumeration_returned_is_reported_and_not_counted() {
        // A driver update restarts the adapter under a new LUID.
        let (adapters, mut unknown) = (adapters(), HashSet::new());
        let restarted = "pid_42_luid_0x00000000_0x00020C11_phys_0_eng_0_engtype_3D";
        assert!(listed(restarted, &adapters, &mut unknown).is_none());
        let mut memory = HashMap::new();
        record_memory(
            &mut memory,
            &adapters,
            &mut unknown,
            "pid_42_luid_0x00000000_0x00020C11_phys_0",
            Some(1048576.),
        );
        assert!(memory.is_empty());
        assert_eq!(unknown, HashSet::from([0x20C11]));
        // Without any listed adapter, every non-ignored LUID is unknown.
        let none = GpuAdapters::default();
        let mut unknown = HashSet::new();
        assert!(listed(restarted, &none, &mut unknown).is_none());
        assert_eq!(unknown, HashSet::from([0x20C11]));
    }

    #[test]
    fn discrete_adapters_report_video_memory_as_local() {
        assert_eq!(
            split_memory(GpuMemoryLayout::Discrete, Some(266), Some(136), Some(300)),
            (Some(266), Some(136))
        );
        assert_eq!(
            split_memory(GpuMemoryLayout::Discrete, None, Some(136), None),
            (None, Some(136))
        );
    }

    #[test]
    fn unified_adapters_count_local_system_memory_as_shared() {
        // No carve-out: Dedicated Usage is zero or unavailable.
        assert_eq!(
            split_memory(GpuMemoryLayout::Unified, Some(400), Some(0), Some(0)),
            (Some(0), Some(400))
        );
        assert_eq!(
            split_memory(GpuMemoryLayout::Unified, Some(400), Some(0), None),
            (Some(0), Some(400))
        );
        // A carve-out holds what is committed to it, up to what is resident.
        assert_eq!(
            split_memory(GpuMemoryLayout::Unified, Some(400), Some(0), Some(150)),
            (Some(150), Some(250))
        );
        assert_eq!(
            split_memory(GpuMemoryLayout::Unified, Some(100), Some(20), Some(900)),
            (Some(100), Some(20))
        );
        // An unknown resident reading stays unknown.
        assert_eq!(
            split_memory(GpuMemoryLayout::Unified, None, Some(0), Some(150)),
            (None, None)
        );
        assert_eq!(
            split_memory(GpuMemoryLayout::Unified, Some(400), None, Some(150)),
            (Some(150), None)
        );
    }

    #[test]
    fn invalid_provider_values_do_not_turn_into_100_percent() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0] {
            assert_eq!(utilization(value), None);
        }
        assert_eq!(utilization(4.4), Some(4.4));
        assert_eq!(utilization(150.0), Some(100.0));
    }

    #[test]
    fn parses_engine_and_memory_instances() {
        let e =
            parse("pid_1234_luid_0x00000001_0x0000D1F3_phys_0_eng_3_engtype_VideoDecode").unwrap();
        assert_eq!(
            (e.pid, e.luid, e.engine),
            (1234, (1 << 32) | 0xD1F3, "VideoDecode")
        );
        let e = parse("pid_8_luid_0x00000000_0x0000D1F3_phys_0_eng_0_engtype_Compute_1").unwrap();
        assert_eq!(e.engine, "Compute_1");
        assert_eq!((e.physical, e.node), (0, Some(0)));
        let other = parse("pid_8_luid_0x00000000_0x0000D1F3_phys_1_eng_2_engtype_Copy").unwrap();
        assert_eq!((other.physical, other.node), (1, Some(2)));
        let m = parse("pid_42_luid_0x00000000_0x00010A2B_phys_0").unwrap();
        assert_eq!((m.pid, m.luid, m.engine), (42, 0x10A2B, ""));
        assert!(parse("_Total").is_none());
        assert!(parse("pid_8_luid_0x00000000_0x0000D1F3_phys_0_eng_bad_engtype_Copy").is_none());
    }
}
