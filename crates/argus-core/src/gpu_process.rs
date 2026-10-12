//! Per-process GPU usage from the `GPU Engine` and `GPU Process Memory`
//! performance counters. Windows publishes them for every process to standard
//! users (Task Manager's GPU columns read the same data), and one PDH
//! collection per sample covers all processes and adapters.

use std::collections::HashMap;
use std::sync::Arc;

use windows::Win32::System::Performance::PDH_HCOUNTER;
use windows::core::w;

use crate::hardware::pretty_engine;
use crate::pdh::{self, Query};
use crate::util::AlignedBuf;

#[derive(Debug, Clone, Default)]
pub struct ProcessGpu {
    /// The process's share of its busiest engine, percent.
    pub util: f32,
    /// Adapter (LUID, as in `GpuInfo::id`) and name of that engine.
    pub engine: Option<(u64, Arc<str>)>,
    /// GPU memory in use, bytes: video memory and system memory.
    pub dedicated: u64,
    pub shared: u64,
}

pub struct GpuProcessTracker {
    query: Query,
    engines: Option<PDH_HCOUNTER>,
    dedicated: Option<PDH_HCOUNTER>,
    shared: Option<PDH_HCOUNTER>,
    buf: AlignedBuf,
    /// Pretty engine names by counter engine type (`VideoDecode` -> `Video Decode`).
    names: HashMap<String, Arc<str>>,
}

// PDH handles are process-wide; the sampler uses them from one thread at a time.
unsafe impl Send for GpuProcessTracker {}

impl GpuProcessTracker {
    /// None when the machine publishes no GPU counters (no WDDM 2.x adapter).
    pub fn new() -> Option<Self> {
        let query = Query::open()?;
        let engines = query.add(w!(r"\GPU Engine(*)\Utilization Percentage"));
        let dedicated = query.add(w!(r"\GPU Process Memory(*)\Dedicated Usage"));
        let shared = query.add(w!(r"\GPU Process Memory(*)\Shared Usage"));
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
        })
    }

    /// GPU usage of every process that has any, by PID.
    pub fn sample(&mut self) -> HashMap<u32, ProcessGpu> {
        let mut out: HashMap<u32, ProcessGpu> = HashMap::new();
        if !self.query.collect() {
            return out;
        }
        let (buf, names) = (&mut self.buf, &mut self.names);
        if let Some(c) = self.engines {
            // One instance per process and engine node; the busiest node wins,
            // as for the adapter-wide figures.
            pdh::each(c, buf, |instance, value| {
                if value <= 0.0 {
                    return;
                }
                let Some(i) = parse(instance) else { return };
                let entry = out.entry(i.pid).or_default();
                let util = value.min(100.0) as f32;
                if util > entry.util {
                    let name = match names.get(i.engine) {
                        Some(n) => n.clone(),
                        None => {
                            let n: Arc<str> = pretty_engine(i.engine).into();
                            names.insert(i.engine.to_owned(), n.clone());
                            n
                        }
                    };
                    entry.util = util;
                    entry.engine = Some((i.luid, name));
                }
            });
        }
        // One instance per process and adapter.
        for (counter, dedicated) in [(self.dedicated, true), (self.shared, false)] {
            let Some(c) = counter else { continue };
            pdh::each(c, buf, |instance, value| {
                if value < 1.0 {
                    return;
                }
                let Some(i) = parse(instance) else { return };
                let entry = out.entry(i.pid).or_default();
                if dedicated {
                    entry.dedicated += value as u64;
                } else {
                    entry.shared += value as u64;
                }
            });
        }
        out
    }
}

struct Instance<'a> {
    pid: u32,
    luid: u64,
    /// Empty for memory counters.
    engine: &'a str,
}

/// `pid_1234_luid_0x00000000_0x0000D1F3_phys_0_eng_0_engtype_VideoDecode`
/// (engine counters) or `pid_1234_luid_0x00000000_0x0000D1F3_phys_0` (memory).
fn parse(instance: &str) -> Option<Instance<'_>> {
    let rest = instance.strip_prefix("pid_")?;
    let (pid, rest) = rest.split_once('_')?;
    let rest = rest.strip_prefix("luid_0x")?;
    let (high, rest) = rest.split_once("_0x")?;
    let (low, rest) = rest.split_once('_').unwrap_or((rest, ""));
    let luid = ((u32::from_str_radix(high, 16).ok()? as u64) << 32)
        | u32::from_str_radix(low, 16).ok()? as u64;
    let engine = rest.split_once("engtype_").map_or("", |(_, e)| e);
    Some(Instance {
        pid: pid.parse().ok()?,
        luid,
        engine,
    })
}

#[cfg(test)]
mod tests {
    use super::parse;

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
        let m = parse("pid_42_luid_0x00000000_0x00010A2B_phys_0").unwrap();
        assert_eq!((m.pid, m.luid, m.engine), (42, 0x10A2B, ""));
        assert!(parse("_Total").is_none());
    }
}
