//! Per-process GPU usage from the `GPU Engine` and `GPU Process Memory`
//! performance counters. Windows publishes them for every process to standard
//! users (Task Manager's GPU columns read the same data), and one PDH
//! collection per sample covers all processes and adapters.

use std::collections::HashMap;
use std::sync::Arc;

use windows::Win32::System::Performance::PDH_HCOUNTER;
use windows::core::w;

use crate::hardware::pretty_engine;
use crate::measurement::{Measurement, UNAVAILABLE};
use crate::pdh::{self, Query};
use crate::util::AlignedBuf;

fn utilization(value: f64) -> Option<f32> {
    (value.is_finite() && value > 0.0).then_some(value.min(100.0) as f32)
}

#[derive(Debug, Clone, Default)]
pub struct ProcessGpu {
    /// The process's share of its busiest engine, percent.
    pub util: f32,
    /// Adapter (LUID, as in `GpuInfo::id`) and name of that engine.
    pub engine: Option<(u64, Arc<str>)>,
    /// Resident/budgeted GPU memory, bytes; absent when its source is unavailable.
    pub dedicated: Option<u64>,
    pub shared: Option<u64>,
    pub engines: Vec<ProcessGpuEngine>,
    pub state: Measurement,
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

pub struct GpuProcessTracker {
    query: Query,
    engines: Option<PDH_HCOUNTER>,
    dedicated: Option<PDH_HCOUNTER>,
    shared: Option<PDH_HCOUNTER>,
    buf: AlignedBuf,
    /// Pretty engine names by counter engine type (`VideoDecode` -> `Video Decode`).
    names: HashMap<String, Arc<str>>,
    memory_available: [bool; 2],
    memory: HashMap<(u32, u64, u32), Option<u64>>,
    util_state: Measurement,
    engine_init: Measurement,
}

// PDH handles are process-wide; the sampler uses them from one thread at a time.
unsafe impl Send for GpuProcessTracker {}

impl GpuProcessTracker {
    /// None when the machine publishes no GPU counters (no WDDM 2.x adapter).
    pub fn new() -> Option<Self> {
        let query = Query::open()?;
        let engines = query.add(w!(r"\GPU Engine(*)\Utilization Percentage"));
        let engine_init = if engines.is_some() {
            Measurement::ready()
        } else {
            Measurement::failure(query.last_status())
        };
        // Local/Non Local Usage report resident budget usage. Dedicated/Shared Usage
        // are committed counters and can inflate on affected Windows/driver versions.
        let dedicated = query.add(w!(r"\GPU Process Memory(*)\Local Usage"));
        let shared = query.add(w!(r"\GPU Process Memory(*)\Non Local Usage"));
        if engines.is_none() && dedicated.is_none() && shared.is_none() {
            return None;
        }
        // Utilization is a rate: the first collection only sets the baseline.
        query.collect();
        Some(Self {
            query,
            engines,
            dedicated,
            shared,
            buf: AlignedBuf::with_capacity(256 * 1024),
            names: HashMap::new(),
            memory_available: [false; 2],
            memory: HashMap::new(),
            util_state: Measurement::default(),
            engine_init,
        })
    }

    /// GPU usage of every process that has any, by PID.
    pub fn sample(&mut self) -> HashMap<u32, ProcessGpu> {
        let mut out: HashMap<u32, ProcessGpu> = HashMap::new();
        self.memory_available = [false; 2];
        self.util_state = self.engine_init;
        if !self.query.collect() {
            self.util_state = Measurement::failure(self.query.last_status());
            return out;
        }
        let (buf, names) = (&mut self.buf, &mut self.names);
        if let Some(c) = self.engines {
            // One instance per process and engine node; the busiest node wins,
            // as for the adapter-wide figures.
            let mut valid = false;
            let mut invalid = false;
            let collected = pdh::each_value(c, buf, |instance, value| {
                let Some(i) = parse(instance) else { return };
                let Some(node) = i.node else { return };
                let entry = out.entry(i.pid).or_insert_with(|| ProcessGpu {
                    state: Measurement {
                        valid: false,
                        partial: false,
                        missing: 0,
                    },
                    ..Default::default()
                });
                let value = value.filter(|v| v.is_finite() && *v >= 0.0);
                let Some(value) = value else {
                    invalid = true;
                    entry.state.missing |= UNAVAILABLE;
                    return;
                };
                valid = true;
                entry.state.valid = true;
                let Some(util) = utilization(value) else {
                    return;
                };
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
            self.util_state = if collected && valid {
                Measurement {
                    valid: true,
                    partial: invalid,
                    missing: if invalid { UNAVAILABLE } else { 0 },
                }
            } else {
                Measurement::missing(UNAVAILABLE)
            };
            for entry in out.values_mut() {
                entry.state.partial = entry.state.valid && entry.state.missing != 0;
            }
        }
        // Retain distinct physical adapters, but count duplicate instances once.
        for (index, counter) in [self.dedicated, self.shared].into_iter().enumerate() {
            let Some(c) = counter else { continue };
            self.memory.clear();
            self.memory_available[index] = pdh::each_value(c, buf, |instance, value| {
                record_memory(&mut self.memory, instance, value);
            });
            let mut totals: HashMap<u32, Option<u64>> = HashMap::new();
            for ((pid, _, _), value) in &self.memory {
                let total = totals.entry(*pid).or_insert(Some(0));
                *total = total.zip(*value).map(|(a, b)| a.saturating_add(b));
            }
            if self.memory_available[index] {
                for entry in out.values_mut() {
                    if index == 0 {
                        entry.dedicated = Some(0);
                    } else {
                        entry.shared = Some(0);
                    }
                }
            }
            for (pid, value) in totals {
                let entry = out.entry(pid).or_insert_with(|| ProcessGpu {
                    state: self.util_state,
                    dedicated: self.memory_available[0].then_some(0),
                    shared: self.memory_available[1].then_some(0),
                    ..Default::default()
                });
                if index == 0 {
                    entry.dedicated = value;
                } else {
                    entry.shared = value;
                }
            }
        }
        out
    }

    pub fn memory_available(&self) -> [bool; 2] {
        self.memory_available
    }

    pub fn utilization_state(&self) -> Measurement {
        self.util_state
    }
}

fn record_memory(
    values: &mut HashMap<(u32, u64, u32), Option<u64>>,
    instance: &str,
    value: Option<f64>,
) {
    let Some(instance) = parse(instance).filter(|i| i.node.is_none()) else {
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
    use super::{parse, record_memory, utilization};
    use std::collections::HashMap;

    #[test]
    fn memory_deduplicates_physical_instances_and_keeps_zero_and_missing_distinct() {
        let mut memory = HashMap::new();
        let instance = "pid_42_luid_0x00000000_0x00010A2B_phys_0";
        record_memory(&mut memory, instance, Some(128.));
        record_memory(&mut memory, &format!("{instance}#1"), Some(128.));
        assert_eq!(memory.len(), 1);
        assert_eq!(memory[&(42, 0x10A2B, 0)], Some(128));
        record_memory(
            &mut memory,
            "pid_42_luid_0x00000000_0x00010A2B_phys_1",
            Some(0.),
        );
        record_memory(&mut memory, "pid_7_luid_0x00000000_0x00010A2B_phys_0", None);
        record_memory(
            &mut memory,
            "pid_8_luid_0x00000000_0x00010A2B_phys_0",
            Some(f64::INFINITY),
        );
        assert_eq!(memory[&(42, 0x10A2B, 1)], Some(0));
        assert_eq!(memory[&(7, 0x10A2B, 0)], None);
        assert_eq!(memory[&(8, 0x10A2B, 0)], None);
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
