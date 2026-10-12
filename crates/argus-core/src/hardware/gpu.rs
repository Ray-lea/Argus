//! GPUs via the D3DKMT kernel-mode thunks (the same source Task Manager uses):
//! per-engine utilization from node running time, memory from segment
//! residency, temperature/fan/power/memory clock from the WDDM 2.4+ adapter
//! perf data, and core clock/voltage from the 3D engine's node perf data.

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use windows::Wdk::Graphics::Direct3D::{
    D3DKMT_ADAPTER_PERFDATA, D3DKMT_ADAPTER_PERFDATACAPS, D3DKMT_ADAPTERINFO,
    D3DKMT_ADAPTERREGISTRYINFO, D3DKMT_ADAPTERTYPE, D3DKMT_CLOSEADAPTER, D3DKMT_ENUMADAPTERS2,
    D3DKMT_NODE_PERFDATA, D3DKMT_NODEMETADATA, D3DKMT_QUERYADAPTERINFO, D3DKMT_QUERYSTATISTICS,
    D3DKMT_QUERYSTATISTICS_ADAPTER, D3DKMT_QUERYSTATISTICS_NODE, D3DKMT_QUERYSTATISTICS_SEGMENT,
    D3DKMT_SEGMENTSIZEINFO, D3DKMTCloseAdapter, D3DKMTEnumAdapters2, D3DKMTQueryAdapterInfo,
    D3DKMTQueryStatistics, DXGK_ENGINE_TYPE, DXGK_ENGINE_TYPE_3D, DXGK_ENGINE_TYPE_COPY,
    DXGK_ENGINE_TYPE_CRYPTO, DXGK_ENGINE_TYPE_OVERLAY, DXGK_ENGINE_TYPE_SCENE_ASSEMBLY,
    DXGK_ENGINE_TYPE_VIDEO_DECODE, DXGK_ENGINE_TYPE_VIDEO_ENCODE,
    DXGK_ENGINE_TYPE_VIDEO_PROCESSING, KMTQAITYPE_ADAPTERPERFDATA, KMTQAITYPE_ADAPTERPERFDATA_CAPS,
    KMTQAITYPE_ADAPTERREGISTRYINFO, KMTQAITYPE_ADAPTERTYPE, KMTQAITYPE_GETSEGMENTSIZE,
    KMTQAITYPE_NODEMETADATA, KMTQAITYPE_NODEPERFDATA, KMTQUERYADAPTERINFOTYPE,
};
use windows::Win32::Foundation::LUID;

use crate::util::from_wide;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    /// Adapter LUID as a string.
    pub id: String,
    pub name: String,
    pub dedicated_total: u64,
    pub shared_total: u64,
    /// Distinct engine names, e.g. `3D`, `Copy`, `Video Decode`.
    pub engines: Vec<String>,
    pub temperature_max: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuStats {
    pub info: Arc<GpuInfo>,
    /// Busiest engine, 0–100 (Task Manager's headline number).
    pub util: f32,
    /// Per engine name, aligned with `info.engines`.
    pub engines: Vec<f32>,
    pub dedicated_used: u64,
    pub shared_used: u64,
    /// °C.
    pub temperature: Option<f32>,
    pub fan_rpm: Option<u32>,
    /// Percent of the board's power budget.
    pub power: Option<f32>,
    /// 3D engine clock and its maximum, MHz.
    pub clock_mhz: Option<u32>,
    pub clock_max_mhz: Option<u32>,
    /// 3D engine voltage, mV.
    pub voltage_mv: Option<u32>,
    pub memory_clock_mhz: Option<u32>,
}

struct Adapter {
    handle: u32,
    luid: LUID,
    info: Arc<GpuInfo>,
    /// Engine index (into `info.engines`) for every node.
    node_engine: Vec<usize>,
    prev_running: Vec<i64>,
    segments: u32,
    has_perf: bool,
    /// Node of the 3D engine, while its perf data keeps answering.
    node_3d: Option<u32>,
}

pub(crate) struct GpuTracker {
    adapters: Vec<Adapter>,
    last: Option<Instant>,
}

// Adapter handles are process-wide kernel handles.
unsafe impl Send for GpuTracker {}

impl GpuTracker {
    pub fn new() -> Self {
        Self {
            adapters: enumerate(),
            last: None,
        }
    }

    pub fn sample(&mut self) -> Vec<GpuStats> {
        let now = Instant::now();
        let elapsed_ticks = self.last.map(|t| now.duration_since(t).as_secs_f64() * 1e7);
        self.last = Some(now);

        self.adapters
            .iter_mut()
            .map(|a| {
                let mut stats = GpuStats {
                    info: a.info.clone(),
                    util: 0.0,
                    engines: vec![0.0; a.info.engines.len()],
                    dedicated_used: 0,
                    shared_used: 0,
                    temperature: None,
                    fan_rpm: None,
                    power: None,
                    clock_mhz: None,
                    clock_max_mhz: None,
                    voltage_mv: None,
                    memory_clock_mhz: None,
                };
                for (node, prev) in a.prev_running.iter_mut().enumerate() {
                    let mut q = statistics_query(D3DKMT_QUERYSTATISTICS_NODE, a.luid);
                    q.Anonymous.QueryNode.NodeId = node as u32;
                    if unsafe { D3DKMTQueryStatistics(&q) }.0 < 0 {
                        continue;
                    }
                    let running =
                        unsafe { q.QueryResult.NodeInformation.GlobalInformation.RunningTime };
                    if let Some(ticks) = elapsed_ticks.filter(|&t| t > 0.0)
                        && *prev > 0
                    {
                        let pct =
                            ((running - *prev).max(0) as f64 / ticks * 100.0).min(100.0) as f32;
                        let e = a.node_engine[node];
                        stats.engines[e] = stats.engines[e].max(pct);
                    }
                    *prev = running;
                }
                stats.util = stats.engines.iter().copied().fold(0.0, f32::max);

                for seg in 0..a.segments {
                    let mut q = statistics_query(D3DKMT_QUERYSTATISTICS_SEGMENT, a.luid);
                    q.Anonymous.QuerySegment.SegmentId = seg;
                    if unsafe { D3DKMTQueryStatistics(&q) }.0 < 0 {
                        continue;
                    }
                    let info = unsafe { &q.QueryResult.SegmentInformation };
                    if info.Aperture != 0 {
                        stats.shared_used += info.BytesResident;
                    } else {
                        stats.dedicated_used += info.BytesResident;
                    }
                }

                if a.has_perf {
                    let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
                    if query_info(a.handle, KMTQAITYPE_ADAPTERPERFDATA, &mut perf) {
                        stats.temperature =
                            (perf.Temperature > 0).then(|| perf.Temperature as f32 / 10.0);
                        stats.fan_rpm = (perf.FanRPM > 0).then_some(perf.FanRPM);
                        stats.power = (perf.Power > 0).then(|| perf.Power as f32 / 10.0);
                        stats.memory_clock_mhz = mhz(perf.MemoryFrequency);
                    } else {
                        a.has_perf = false;
                    }
                }
                if let Some(node) = a.node_3d {
                    let mut perf = D3DKMT_NODE_PERFDATA {
                        NodeOrdinal: node,
                        ..Default::default()
                    };
                    if query_info(a.handle, KMTQAITYPE_NODEPERFDATA, &mut perf) {
                        stats.clock_mhz = mhz(perf.Frequency);
                        stats.clock_max_mhz = mhz(perf.MaxFrequency);
                        stats.voltage_mv = (perf.Voltage > 0).then_some(perf.Voltage);
                    } else {
                        a.node_3d = None;
                    }
                }
                stats
            })
            .collect()
    }
}

impl Drop for GpuTracker {
    fn drop(&mut self) {
        for a in &self.adapters {
            let close = D3DKMT_CLOSEADAPTER { hAdapter: a.handle };
            unsafe {
                let _ = D3DKMTCloseAdapter(&close);
            }
        }
    }
}

fn statistics_query(
    kind: windows::Wdk::Graphics::Direct3D::D3DKMT_QUERYSTATISTICS_TYPE,
    luid: LUID,
) -> D3DKMT_QUERYSTATISTICS {
    // SAFETY: a plain-old-data struct for which all-zero is a valid value.
    let mut q: D3DKMT_QUERYSTATISTICS = unsafe { std::mem::zeroed() };
    q.Type = kind;
    q.AdapterLuid = luid;
    q
}

fn query_info<T>(handle: u32, kind: KMTQUERYADAPTERINFOTYPE, data: &mut T) -> bool {
    let mut q = D3DKMT_QUERYADAPTERINFO {
        hAdapter: handle,
        Type: kind,
        pPrivateDriverData: (data as *mut T).cast(),
        PrivateDriverDataSize: std::mem::size_of::<T>() as u32,
    };
    unsafe { D3DKMTQueryAdapterInfo(&mut q) }.0 >= 0
}

fn enumerate() -> Vec<Adapter> {
    let mut e = D3DKMT_ENUMADAPTERS2 {
        NumAdapters: 0,
        pAdapters: std::ptr::null_mut(),
    };
    if unsafe { D3DKMTEnumAdapters2(&mut e) }.0 < 0 || e.NumAdapters == 0 {
        return Vec::new();
    }
    let mut list = vec![D3DKMT_ADAPTERINFO::default(); e.NumAdapters as usize];
    e.pAdapters = list.as_mut_ptr();
    if unsafe { D3DKMTEnumAdapters2(&mut e) }.0 < 0 {
        return Vec::new();
    }
    list.truncate(e.NumAdapters as usize);

    let mut out = Vec::new();
    for a in list {
        match describe(&a) {
            Some(adapter) => out.push(adapter),
            None => {
                let close = D3DKMT_CLOSEADAPTER {
                    hAdapter: a.hAdapter,
                };
                unsafe {
                    let _ = D3DKMTCloseAdapter(&close);
                }
            }
        }
    }
    out
}

fn describe(a: &D3DKMT_ADAPTERINFO) -> Option<Adapter> {
    // Skip software renderers and display-only adapters (no engines to watch).
    let mut kind = D3DKMT_ADAPTERTYPE::default();
    if query_info(a.hAdapter, KMTQAITYPE_ADAPTERTYPE, &mut kind) {
        let bits = unsafe { kind.Anonymous.Value };
        let render = bits & 0x1 != 0;
        let software = bits & 0x4 != 0;
        if !render || software {
            return None;
        }
    }

    let stats = statistics_query(D3DKMT_QUERYSTATISTICS_ADAPTER, a.AdapterLuid);
    if unsafe { D3DKMTQueryStatistics(&stats) }.0 < 0 {
        return None;
    }
    let (nodes, segments) = unsafe {
        let i = &stats.QueryResult.AdapterInformation;
        (i.NodeCount, i.NbSegments)
    };

    let mut registry: Box<D3DKMT_ADAPTERREGISTRYINFO> = Box::new(unsafe { std::mem::zeroed() });
    let name = if query_info(
        a.hAdapter,
        KMTQAITYPE_ADAPTERREGISTRYINFO,
        registry.as_mut(),
    ) {
        from_wide(&registry.AdapterString)
    } else {
        String::new()
    };

    let mut sizes = D3DKMT_SEGMENTSIZEINFO::default();
    query_info(a.hAdapter, KMTQAITYPE_GETSEGMENTSIZE, &mut sizes);

    let mut engines: Vec<String> = Vec::new();
    let mut node_engine = Vec::with_capacity(nodes as usize);
    for node in 0..nodes {
        let mut meta: D3DKMT_NODEMETADATA = unsafe { std::mem::zeroed() };
        meta.NodeOrdinalAndAdapterIndex = node; // physical adapter 0
        let label = if query_info(a.hAdapter, KMTQAITYPE_NODEMETADATA, &mut meta) {
            // Copy out of the packed struct before borrowing.
            let (name, kind) = (meta.NodeData.FriendlyName, meta.NodeData.EngineType);
            let name = from_wide(&name);
            if name.is_empty() {
                engine_type_name(kind).to_owned()
            } else {
                pretty_engine(&name)
            }
        } else {
            format!("Engine {node}")
        };
        let idx = engines.iter().position(|e| *e == label).unwrap_or_else(|| {
            engines.push(label);
            engines.len() - 1
        });
        node_engine.push(idx);
    }

    let mut caps = D3DKMT_ADAPTER_PERFDATACAPS::default();
    let has_caps = query_info(a.hAdapter, KMTQAITYPE_ADAPTERPERFDATA_CAPS, &mut caps);
    let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
    let has_perf = query_info(a.hAdapter, KMTQAITYPE_ADAPTERPERFDATA, &mut perf);

    let node_3d = node_engine
        .iter()
        .position(|&e| engines[e] == "3D")
        .map(|n| n as u32);
    let luid = a.AdapterLuid;
    Some(Adapter {
        handle: a.hAdapter,
        luid,
        info: Arc::new(GpuInfo {
            id: (((luid.HighPart as u32 as u64) << 32) | luid.LowPart as u64).to_string(),
            name: if name.is_empty() { "GPU".into() } else { name },
            dedicated_total: sizes.DedicatedVideoMemorySize,
            shared_total: sizes.SharedSystemMemorySize,
            engines,
            temperature_max: (has_caps && caps.TemperatureMax > 0)
                .then(|| caps.TemperatureMax as f32 / 10.0),
        }),
        node_engine,
        node_3d,
        prev_running: vec![0; nodes as usize],
        segments,
        has_perf,
    })
}

/// Hz to MHz; drivers report 0 for "not supported".
fn mhz(hz: u64) -> Option<u32> {
    (hz >= 1_000_000).then_some((hz / 1_000_000) as u32)
}

/// Drivers only fill FriendlyName for non-standard engines.
fn engine_type_name(kind: DXGK_ENGINE_TYPE) -> &'static str {
    match kind {
        DXGK_ENGINE_TYPE_3D => "3D",
        DXGK_ENGINE_TYPE_VIDEO_DECODE => "Video Decode",
        DXGK_ENGINE_TYPE_VIDEO_ENCODE => "Video Encode",
        DXGK_ENGINE_TYPE_VIDEO_PROCESSING => "Video Processing",
        DXGK_ENGINE_TYPE_SCENE_ASSEMBLY => "Scene Assembly",
        DXGK_ENGINE_TYPE_COPY => "Copy",
        DXGK_ENGINE_TYPE_OVERLAY => "Overlay",
        DXGK_ENGINE_TYPE_CRYPTO => "Crypto",
        _ => "Other",
    }
}

/// `VideoDecode` -> `Video Decode`; numbered duplicates like `Copy_1` stay distinct.
pub(crate) fn pretty_engine(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 4);
    let mut prev_lower = false;
    for c in raw.chars() {
        if c.is_ascii_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_ascii_lowercase();
        out.push(c);
    }
    if out.is_empty() { "Engine".into() } else { out }
}
