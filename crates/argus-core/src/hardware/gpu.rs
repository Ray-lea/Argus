//! GPUs via the D3DKMT kernel-mode thunks (the same source Task Manager uses):
//! per-engine utilization from node running time, memory from segment
//! residency, temperature/fan/power/memory clock from the WDDM 2.4+ adapter
//! perf data, and core clock/voltage from the 3D engine's node perf data.
//! NVIDIA cards leave power out of that perf data; their board power comes
//! from NVML (`nvml.rs`), matched to the adapter by PCI address.

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use windows::Wdk::Graphics::Direct3D::{
    D3DKMT_ADAPTER_PERFDATA, D3DKMT_ADAPTER_PERFDATACAPS, D3DKMT_ADAPTERADDRESS,
    D3DKMT_ADAPTERINFO, D3DKMT_ADAPTERREGISTRYINFO, D3DKMT_ADAPTERTYPE, D3DKMT_CLOSEADAPTER,
    D3DKMT_ENUMADAPTERS2, D3DKMT_NODE_PERFDATA, D3DKMT_NODEMETADATA, D3DKMT_QUERYADAPTERINFO,
    D3DKMT_QUERYSTATISTICS, D3DKMT_QUERYSTATISTICS_ADAPTER, D3DKMT_QUERYSTATISTICS_NODE,
    D3DKMT_QUERYSTATISTICS_SEGMENT, D3DKMT_SEGMENTSIZEINFO, D3DKMTCloseAdapter,
    D3DKMTEnumAdapters2, D3DKMTQueryAdapterInfo, D3DKMTQueryStatistics, DXGK_ENGINE_TYPE,
    DXGK_ENGINE_TYPE_3D, DXGK_ENGINE_TYPE_COPY, DXGK_ENGINE_TYPE_CRYPTO, DXGK_ENGINE_TYPE_OVERLAY,
    DXGK_ENGINE_TYPE_SCENE_ASSEMBLY, DXGK_ENGINE_TYPE_VIDEO_DECODE, DXGK_ENGINE_TYPE_VIDEO_ENCODE,
    DXGK_ENGINE_TYPE_VIDEO_PROCESSING, KMTQAITYPE_ADAPTERADDRESS, KMTQAITYPE_ADAPTERPERFDATA,
    KMTQAITYPE_ADAPTERPERFDATA_CAPS, KMTQAITYPE_ADAPTERREGISTRYINFO, KMTQAITYPE_ADAPTERTYPE,
    KMTQAITYPE_GETSEGMENTSIZE, KMTQAITYPE_NODEMETADATA, KMTQAITYPE_NODEPERFDATA,
    KMTQUERYADAPTERINFOTYPE,
};
use windows::Win32::Foundation::LUID;

use super::nvml::Nvml;
use super::vendor_gpu::{GpuPowerSource, Pci, VendorGpu};
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GpuUtilSource {
    Windows,
    Process,
    Nvml,
}

/// Each node owns its baseline: zero is valid and a failed query breaks the pair.
#[derive(Default)]
struct NodeCounter(Option<(i64, Instant)>);
impl NodeCounter {
    fn sample(&mut self, running: Option<i64>, now: Instant) -> Option<f32> {
        let Some(running) = running.filter(|v| *v >= 0) else {
            self.0 = None;
            return None;
        };
        let previous = self.0.replace((running, now));
        let (before, at) = previous?;
        let ticks = now.checked_duration_since(at)?.as_secs_f64() * 1e7;
        let delta = running.checked_sub(before).filter(|v| *v >= 0)?;
        (ticks > 0.0).then(|| (delta as f64 / ticks * 100.0).min(100.0) as f32)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuStats {
    pub info: Arc<GpuInfo>,
    /// Busiest engine, supplemented by process and GPU-core telemetry, 0–100.
    pub util: f32,
    pub util_source: GpuUtilSource,
    /// Per engine name, aligned with `info.engines`.
    pub engines: Vec<f32>,
    pub dedicated_used: u64,
    pub shared_used: u64,
    /// °C.
    pub temperature: Option<f32>,
    pub fan_rpm: Option<u32>,
    /// Percent of the board's power budget.
    pub power: Option<f32>,
    /// Measured power and enforced limit, W; source specifies the scope.
    pub board_power: Option<f32>,
    pub board_power_limit: Option<f32>,
    pub power_source: Option<GpuPowerSource>,
    /// 3D engine clock and its maximum, MHz.
    pub clock_mhz: Option<u32>,
    pub clock_max_mhz: Option<u32>,
    /// 3D engine voltage, mV.
    pub voltage_mv: Option<u32>,
    pub memory_clock_mhz: Option<u32>,
}

impl GpuStats {
    pub(crate) fn include_usage(&mut self, usage: f32, source: GpuUtilSource) {
        if usage.is_finite() && (0.0..=100.0).contains(&usage) && usage > self.util {
            self.util = usage;
            self.util_source = source;
        }
    }
    pub(crate) fn include_process_engine(&mut self, engine: Option<usize>, usage: f32) {
        if !usage.is_finite() || !(0.0..=100.0).contains(&usage) {
            return;
        }
        self.include_usage(usage, GpuUtilSource::Process);
        if let Some(value) = engine.and_then(|i| self.engines.get_mut(i)) {
            *value = value.max(usage);
        }
    }
}

struct Adapter {
    handle: u32,
    luid: LUID,
    info: Arc<GpuInfo>,
    /// Engine index (into `info.engines`) for every node.
    node_engine: Vec<usize>,
    prev_running: Vec<NodeCounter>,
    segments: u32,
    has_perf: bool,
    /// Node of the 3D engine, while its perf data keeps answering.
    node_3d: Option<u32>,
    /// PCI bus and device number.
    pci: Option<Pci>,
}

pub(crate) struct GpuTracker {
    adapters: Vec<Adapter>,
    last: Option<Instant>,
    nvml: Option<Nvml>,
    vendor: VendorGpu,
}

// Adapter handles are process-wide kernel handles.
unsafe impl Send for GpuTracker {}

impl GpuTracker {
    pub(crate) fn node_engine(&self, luid: u64, node: u32) -> Option<usize> {
        self.adapters
            .iter()
            .find(|a| (((a.luid.HighPart as u32 as u64) << 32) | a.luid.LowPart as u64) == luid)?
            .node_engine
            .get(node as usize)
            .copied()
    }
    pub fn new() -> Self {
        let adapters = enumerate();
        // Only hardware adapters have a PCI address for NVML to match.
        let nvml = adapters
            .iter()
            .any(|a| a.pci.is_some())
            .then(Nvml::load)
            .flatten();
        Self {
            adapters,
            last: None,
            nvml,
            vendor: VendorGpu::spawn(),
        }
    }

    pub fn sample(&mut self) -> Vec<GpuStats> {
        let now = Instant::now();
        let vendor = self.vendor.sample(
            self.last
                .map(|at| now.duration_since(at))
                .unwrap_or_default(),
        );
        self.last = Some(now);

        self.adapters
            .iter_mut()
            .map(|a| {
                let mut stats = GpuStats {
                    info: a.info.clone(),
                    util: 0.0,
                    util_source: GpuUtilSource::Windows,
                    engines: vec![0.0; a.info.engines.len()],
                    dedicated_used: 0,
                    shared_used: 0,
                    temperature: None,
                    fan_rpm: None,
                    power: None,
                    board_power: None,
                    board_power_limit: None,
                    power_source: None,
                    clock_mhz: None,
                    clock_max_mhz: None,
                    voltage_mv: None,
                    memory_clock_mhz: None,
                };
                for (node, prev) in a.prev_running.iter_mut().enumerate() {
                    let mut q = statistics_query(D3DKMT_QUERYSTATISTICS_NODE, a.luid);
                    q.Anonymous.QueryNode.NodeId = node as u32;
                    let running = if unsafe { D3DKMTQueryStatistics(&raw mut q) }.0 >= 0 {
                        Some(unsafe { q.QueryResult.NodeInformation.GlobalInformation.RunningTime })
                    } else {
                        None
                    };
                    if let Some(pct) = prev.sample(running, Instant::now()) {
                        let e = a.node_engine[node];
                        stats.engines[e] = stats.engines[e].max(pct);
                    }
                }
                stats.util = stats.engines.iter().copied().fold(0.0, f32::max);

                for seg in 0..a.segments {
                    let mut q = statistics_query(D3DKMT_QUERYSTATISTICS_SEGMENT, a.luid);
                    q.Anonymous.QuerySegment.SegmentId = seg;
                    if unsafe { D3DKMTQueryStatistics(&raw mut q) }.0 < 0 {
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
                if let (Some(nvml), Some((bus, device, function))) = (&self.nvml, a.pci) {
                    (stats.board_power, stats.board_power_limit) =
                        nvml.power(bus, device, function);
                    stats.power_source = stats.board_power.map(|_| GpuPowerSource::Nvml);
                    if let Some(util) = nvml.utilization(bus, device, function) {
                        stats.include_usage(util, GpuUtilSource::Nvml);
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
                if let Some((_, reading)) = vendor.iter().find(|(pci, _)| Some(*pci) == a.pci) {
                    if reading.watts.is_some() {
                        stats.board_power = reading.watts;
                        stats.board_power_limit = reading.limit;
                        stats.power_source = reading.source;
                    }
                    stats.temperature = stats.temperature.or(reading.temperature);
                    stats.clock_mhz = stats.clock_mhz.or(reading.clock);
                    stats.memory_clock_mhz = stats.memory_clock_mhz.or(reading.memory_clock);
                    stats.voltage_mv = stats.voltage_mv.or(reading.voltage);
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

    let mut stats = statistics_query(D3DKMT_QUERYSTATISTICS_ADAPTER, a.AdapterLuid);
    if unsafe { D3DKMTQueryStatistics(&raw mut stats) }.0 < 0 {
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

    let mut address = D3DKMT_ADAPTERADDRESS::default();
    let pci = query_info(a.hAdapter, KMTQAITYPE_ADAPTERADDRESS, &mut address).then_some((
        address.BusNumber,
        address.DeviceNumber,
        address.FunctionNumber,
    ));

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
        prev_running: (0..nodes).map(|_| NodeCounter::default()).collect(),
        segments,
        has_perf,
        pci,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn an_idle_zero_counter_includes_the_first_busy_interval() {
        let now = Instant::now();
        let mut counter = NodeCounter::default();
        assert_eq!(counter.sample(Some(0), now), None);
        assert_eq!(
            counter.sample(Some(9_800_000), now + Duration::from_secs(1)),
            Some(98.0)
        );
    }
    #[test]
    fn failed_queries_do_not_turn_three_intervals_of_work_into_a_75_percent_spike() {
        let now = Instant::now();
        let mut counter = NodeCounter::default();
        counter.sample(Some(0), now);
        assert_eq!(
            counter.sample(Some(2_500_000), now + Duration::from_secs(1)),
            Some(25.0)
        );
        assert_eq!(counter.sample(None, now + Duration::from_secs(2)), None);
        assert_eq!(counter.sample(None, now + Duration::from_secs(3)), None);
        assert_eq!(
            counter.sample(Some(10_000_000), now + Duration::from_secs(4)),
            None
        );
        assert_eq!(
            counter.sample(Some(12_500_000), now + Duration::from_secs(5)),
            Some(25.0)
        );
    }
    #[test]
    fn reset_and_invalid_counters_reprime_without_underflow() {
        let now = Instant::now();
        let mut counter = NodeCounter::default();
        counter.sample(Some(i64::MAX), now);
        assert_eq!(counter.sample(Some(0), now + Duration::from_secs(1)), None);
        assert_eq!(
            counter.sample(Some(5_000_000), now + Duration::from_secs(2)),
            Some(50.0)
        );
        assert_eq!(counter.sample(Some(-1), now + Duration::from_secs(3)), None);
    }
    fn stats() -> GpuStats {
        GpuStats {
            info: Arc::new(GpuInfo {
                id: "1".into(),
                name: "test".into(),
                dedicated_total: 0,
                shared_total: 0,
                engines: vec!["3D".into()],
                temperature_max: None,
            }),
            util: 1.0,
            util_source: GpuUtilSource::Windows,
            engines: vec![1.0],
            dedicated_used: 0,
            shared_used: 0,
            temperature: None,
            fan_rpm: None,
            power: None,
            board_power: None,
            board_power_limit: None,
            power_source: None,
            clock_mhz: None,
            clock_max_mhz: None,
            voltage_mv: None,
            memory_clock_mhz: None,
        }
    }
    #[test]
    fn process_and_driver_activity_cover_missing_compute_load_without_faking_engine_data() {
        let mut g = stats();
        g.include_process_engine(Some(0), 98.0);
        assert_eq!(
            (g.util, g.engines[0], g.util_source),
            (98.0, 98.0, GpuUtilSource::Process)
        );
        g.include_usage(100.0, GpuUtilSource::Nvml);
        assert_eq!(
            (g.util, g.engines[0], g.util_source),
            (100.0, 98.0, GpuUtilSource::Nvml)
        );
        g.include_usage(f32::NAN, GpuUtilSource::Nvml);
        g.include_usage(101.0, GpuUtilSource::Nvml);
        g.include_process_engine(Some(0), f32::INFINITY);
        assert_eq!((g.util, g.engines[0]), (100.0, 98.0));
    }
}
