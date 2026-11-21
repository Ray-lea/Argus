//! GPUs via the D3DKMT kernel-mode thunks (the same source Task Manager uses):
//! per-engine utilization from node running time, memory from segment
//! residency, temperature/fan/power/memory clock from the WDDM 2.4+ adapter
//! perf data, and core clock/voltage from the 3D engine's node perf data.
//! NVIDIA cards leave power out of that perf data; their board power comes
//! from NVML (`nvml.rs`), matched to the adapter by PCI address. The perf data
//! and the vendor libraries are answered by the adapter's driver, which wakes
//! an adapter that has powered down: `power.rs` decides when to ask.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use windows::Wdk::Graphics::Direct3D::{
    D3DKMT_ADAPTER_PERFDATA, D3DKMT_ADAPTER_PERFDATACAPS, D3DKMT_ADAPTERADDRESS,
    D3DKMT_ADAPTERINFO, D3DKMT_ADAPTERREGISTRYINFO, D3DKMT_ADAPTERTYPE, D3DKMT_CLOSEADAPTER,
    D3DKMT_ENUMADAPTERS2, D3DKMT_NODE_PERFDATA, D3DKMT_NODEMETADATA, D3DKMT_QUERY_DEVICE_IDS,
    D3DKMT_QUERYADAPTERINFO, D3DKMT_QUERYSTATISTICS, D3DKMT_QUERYSTATISTICS_ADAPTER,
    D3DKMT_QUERYSTATISTICS_NODE, D3DKMT_QUERYSTATISTICS_SEGMENT, D3DKMT_SEGMENTGROUPSIZEINFO,
    D3DKMT_SEGMENTSIZEINFO, D3DKMTCloseAdapter, D3DKMTEnumAdapters2, D3DKMTQueryAdapterInfo,
    D3DKMTQueryStatistics, DXGK_ENGINE_TYPE, DXGK_ENGINE_TYPE_3D, DXGK_ENGINE_TYPE_COPY,
    DXGK_ENGINE_TYPE_CRYPTO, DXGK_ENGINE_TYPE_OVERLAY, DXGK_ENGINE_TYPE_SCENE_ASSEMBLY,
    DXGK_ENGINE_TYPE_VIDEO_DECODE, DXGK_ENGINE_TYPE_VIDEO_ENCODE,
    DXGK_ENGINE_TYPE_VIDEO_PROCESSING, KMTQAITYPE_ADAPTERADDRESS, KMTQAITYPE_ADAPTERPERFDATA,
    KMTQAITYPE_ADAPTERPERFDATA_CAPS, KMTQAITYPE_ADAPTERREGISTRYINFO, KMTQAITYPE_ADAPTERTYPE,
    KMTQAITYPE_GETSEGMENTGROUPSIZE, KMTQAITYPE_GETSEGMENTSIZE, KMTQAITYPE_NODEMETADATA,
    KMTQAITYPE_NODEPERFDATA, KMTQAITYPE_PHYSICALADAPTERDEVICEIDS, KMTQUERYADAPTERINFOTYPE,
};
use windows::Win32::Foundation::LUID;

use super::nvml::Nvml;
use super::query::Retry;
use super::vendor_gpu::{GpuPowerSource, Pci, VendorGpu};
use crate::diagnostics::{startup_native, startup_phase};
use crate::gpu_process::{GpuAdapters, GpuMemoryLayout};
use crate::util::from_wide;
mod metadata;
mod power;
pub use metadata::DirectXInfo;
use power::Access;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    /// Adapter LUID as a string.
    pub id: String,
    #[serde(skip)]
    pub(crate) history_id: Option<String>,
    pub name: String,
    pub dedicated_total: u64,
    pub shared_total: u64,
    /// Distinct engine names, e.g. `3D`, `Copy`, `Video Decode`.
    pub engines: Vec<String>,
    pub temperature_max: Option<f32>,
    /// Intel's processor graphics at PCI 0:2:0, verified by vendor ID.
    pub intel_integrated: bool,
    pub driver_version: Option<String>,
    pub driver_date: Option<String>,
    pub direct_x: Option<DirectXInfo>,
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
pub(super) struct NodeCounter(Option<(i64, Instant)>);
impl NodeCounter {
    pub(super) fn sample(&mut self, running: Option<i64>, now: Instant) -> Option<f32> {
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
    pub clock_mhz: Option<f32>,
    pub clock_max_mhz: Option<f32>,
    /// 3D engine voltage, mV.
    pub voltage_mv: Option<u32>,
    pub memory_clock_mhz: Option<f32>,
    pub core_vid_mv: Option<u32>,
    pub kernel_sensor_state: Option<super::SensorState>,
    pub kernel_sensor_supported: bool,
    pub integrated_sensors: Option<super::IntegratedGpuSensors>,
    /// The adapter has powered down (the idle discrete GPU of a hybrid
    /// laptop). Its driver is not asked meanwhile, so the readings that come
    /// from the driver are absent.
    pub low_power: bool,
}

/// What only the adapter's driver or its vendor library reports.
#[derive(Clone, Default)]
struct DriverReadings {
    temperature: Option<f32>,
    fan_rpm: Option<u32>,
    power: Option<f32>,
    board_power: Option<f32>,
    board_power_limit: Option<f32>,
    power_source: Option<GpuPowerSource>,
    clock_mhz: Option<f32>,
    clock_max_mhz: Option<f32>,
    voltage_mv: Option<u32>,
    memory_clock_mhz: Option<f32>,
}

impl DriverReadings {
    fn of(stats: &GpuStats) -> Self {
        Self {
            temperature: stats.temperature,
            fan_rpm: stats.fan_rpm,
            power: stats.power,
            board_power: stats.board_power,
            board_power_limit: stats.board_power_limit,
            power_source: stats.power_source,
            clock_mhz: stats.clock_mhz,
            clock_max_mhz: stats.clock_max_mhz,
            voltage_mv: stats.voltage_mv,
            memory_clock_mhz: stats.memory_clock_mhz,
        }
    }

    fn apply(&self, stats: &mut GpuStats) {
        stats.temperature = self.temperature;
        stats.fan_rpm = self.fan_rpm;
        stats.power = self.power;
        stats.board_power = self.board_power;
        stats.board_power_limit = self.board_power_limit;
        stats.power_source = self.power_source;
        stats.clock_mhz = self.clock_mhz;
        stats.clock_max_mhz = self.clock_max_mhz;
        stats.voltage_mv = self.voltage_mv;
        stats.memory_clock_mhz = self.memory_clock_mhz;
    }
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
    memory: GpuMemoryLayout,
    /// Engine index (into `info.engines`) for every node.
    node_engine: Vec<usize>,
    prev_running: Vec<NodeCounter>,
    segments: u32,
    perf_query: Retry,
    node_perf_query: Retry,
    /// Node identity survives transient failures of its optional perf query.
    node_3d: Option<u32>,
    /// PCI bus and device number.
    pci: Option<Pci>,
    nvidia: bool,
    /// The adapter's device, for its power state; None when PnP names none.
    device: Option<power::Device>,
    gate: power::Gate,
    /// The driver readings of the last sample, shown while the driver is left
    /// alone.
    kept: DriverReadings,
    /// The driver reported work that the engine counters do not show.
    driver_busy: bool,
}

pub(crate) struct GpuTracker {
    adapters: Vec<Adapter>,
    /// LUIDs of the adapters left out: software, display-only and compute-only
    /// adapters, and LUIDs another source named that no enumeration returned.
    ignored: HashSet<u64>,
    last: Option<Instant>,
    nvml: Option<Nvml>,
    /// NVML is loaded with the first NVIDIA adapter that may be asked:
    /// initializing it wakes every NVIDIA adapter.
    nvml_due: bool,
    vendor: VendorGpu,
    vendor_telemetry: bool,
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
    /// The listed adapters, with how their memory is grouped, and the ignored ones.
    pub(crate) fn adapters(&self) -> GpuAdapters {
        GpuAdapters {
            listed: self
                .adapters
                .iter()
                .map(|a| (luid_key(a.luid), a.memory))
                .collect(),
            ignored: self.ignored.clone(),
        }
    }
    pub fn new(excluded: &HashSet<u64>) -> Self {
        Self::new_with_vendor(true, excluded)
    }

    pub(super) fn new_with_vendor(vendor_telemetry: bool, excluded: &HashSet<u64>) -> Self {
        let (adapters, ignored) = startup_phase("gpu.enumerate", || enumerate(excluded))
            .unwrap_or_else(|| (Vec::new(), excluded.clone()));
        Self {
            adapters,
            ignored,
            last: None,
            nvml: None,
            nvml_due: vendor_telemetry,
            vendor: if vendor_telemetry {
                VendorGpu::spawn()
            } else {
                VendorGpu::inactive()
            },
            vendor_telemetry,
        }
    }

    /// Enumerates the adapters again after another source named `unknown`
    /// LUIDs: a driver update restarts an adapter under a new LUID, and an
    /// adapter can be attached later. Adapters still present keep their
    /// handles and counter baselines; LUIDs of `unknown` that this enumeration
    /// does not return either are ignored from now on, so each causes one
    /// successful enumeration at most. A failed enumeration changes nothing.
    pub(crate) fn refresh(&mut self, unknown: &HashSet<u64>, excluded: &HashSet<u64>) {
        let Some((found, mut ignored)) = enumerate(excluded) else {
            crate::diagnostics::warn("gpu.enumerate", "adapters could not be enumerated again");
            return;
        };
        let previous = std::mem::take(&mut self.adapters);
        let (adapters, closed) = reconcile(previous, found, |a| luid_key(a.luid));
        for adapter in closed {
            close(adapter.handle, adapter.luid);
        }
        self.adapters = adapters;
        let listed: HashSet<u64> = self.adapters.iter().map(|a| luid_key(a.luid)).collect();
        ignored.extend(unknown.iter().filter(|luid| !listed.contains(luid)));
        self.ignored = ignored;
        // A driver installed since can bring NVML with it.
        self.nvml_due = self.vendor_telemetry && self.nvml.is_none();
        log::info!(target: "argus_diagnostics", "gpu adapters enumerated again; listed={}; ignored={}", self.adapters.len(), self.ignored.len());
    }

    #[cfg(test)]
    pub(crate) fn forget(&mut self) {
        for adapter in std::mem::take(&mut self.adapters) {
            close(adapter.handle, adapter.luid);
        }
    }

    pub fn sample(&mut self) -> Vec<GpuStats> {
        let now = Instant::now();
        let interval = self
            .last
            .map(|at| now.duration_since(at))
            .unwrap_or_default();
        self.last = Some(now);

        // The graphics kernel's own statistics first: they show whether an
        // adapter is in use, which decides whether its driver is asked.
        let mut samples: Vec<(GpuStats, Access)> = self
            .adapters
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
                    core_vid_mv: None,
                    kernel_sensor_state: None,
                    kernel_sensor_supported: false,
                    integrated_sensors: None,
                    low_power: false,
                };
                for (node, prev) in a.prev_running.iter_mut().enumerate() {
                    let running = node_running_time("gpu.node-statistics", a.luid, node as u32);
                    if let Some(pct) = prev.sample(running, Instant::now()) {
                        let e = a.node_engine[node];
                        stats.engines[e] = stats.engines[e].max(pct);
                    }
                }
                stats.util = stats.engines.iter().copied().fold(0.0, f32::max);

                for seg in 0..a.segments {
                    let mut q = statistics_query(D3DKMT_QUERYSTATISTICS_SEGMENT, a.luid);
                    q.Anonymous.QuerySegment.SegmentId = seg;
                    if startup_native(
                        "gpu.segment-statistics",
                        luid_key(a.luid),
                        seg,
                        || unsafe { D3DKMTQueryStatistics(&raw mut q) }.0,
                    ) < 0
                    {
                        continue;
                    }
                    let info = unsafe { &q.QueryResult.SegmentInformation };
                    if info.Aperture != 0 {
                        stats.shared_used += info.BytesResident;
                    } else {
                        stats.dedicated_used += info.BytesResident;
                    }
                }

                let access = a
                    .gate
                    .sample(a.device.as_ref(), stats.util > 0.0, a.driver_busy, now);
                (stats, access)
            })
            .collect();

        let asked = self
            .adapters
            .iter()
            .zip(&samples)
            .filter(|(_, (_, access))| *access == Access::Read)
            .filter_map(|(a, _)| a.pci)
            .collect();
        let vendor = self.vendor.sample(interval, asked);

        for (a, (stats, access)) in self.adapters.iter_mut().zip(&mut samples) {
            match access {
                Access::Asleep => {
                    a.kept = DriverReadings::default();
                    a.driver_busy = false;
                    stats.low_power = true;
                    continue;
                }
                Access::Keep => a.kept.apply(stats),
                Access::Read => {
                    if let Some(perf) = a.perf_query.query(now, || {
                        let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
                        query_info(a.handle, KMTQAITYPE_ADAPTERPERFDATA, &mut perf).then_some(perf)
                    }) {
                        stats.temperature =
                            (perf.Temperature > 0).then(|| perf.Temperature as f32 / 10.0);
                        stats.fan_rpm = (perf.FanRPM > 0).then_some(perf.FanRPM);
                        stats.power = (perf.Power > 0).then(|| perf.Power as f32 / 10.0);
                        stats.memory_clock_mhz = mhz(perf.MemoryFrequency);
                    }
                    a.driver_busy = false;
                    if a.nvidia && std::mem::take(&mut self.nvml_due) {
                        self.nvml = Nvml::load();
                    }
                    if let (Some(nvml), Some((bus, device, function))) = (&self.nvml, a.pci) {
                        (stats.board_power, stats.board_power_limit) =
                            nvml.power(bus, device, function);
                        stats.power_source = stats.board_power.map(|_| GpuPowerSource::Nvml);
                        if let Some(util) = nvml.utilization(bus, device, function) {
                            stats.include_usage(util, GpuUtilSource::Nvml);
                            // Compute work can be missing from the engine
                            // counters; it is no reason to stop asking.
                            a.driver_busy = util > 0.0;
                        }
                    }
                    if let Some(node) = a.node_3d
                        && let Some(perf) = a.node_perf_query.query(now, || {
                            let mut perf = D3DKMT_NODE_PERFDATA {
                                NodeOrdinal: node,
                                ..Default::default()
                            };
                            query_info(a.handle, KMTQAITYPE_NODEPERFDATA, &mut perf).then_some(perf)
                        })
                    {
                        stats.clock_mhz = mhz(perf.Frequency);
                        stats.clock_max_mhz = mhz(perf.MaxFrequency);
                        stats.voltage_mv = (perf.Voltage > 0).then_some(perf.Voltage);
                    }
                }
            }
            // The vendor worker answers a round late: its readings also reach
            // a sample that asks for none itself.
            if let Some((_, reading)) = vendor.iter().find(|(pci, _)| Some(*pci) == a.pci) {
                if reading.watts.is_some() {
                    stats.board_power = reading.watts;
                    stats.board_power_limit = reading.limit;
                    stats.power_source = reading.source;
                }
                stats.temperature = stats.temperature.or(reading.temperature);
                stats.clock_mhz = stats.clock_mhz.or(reading.clock.map(|v| v as f32));
                stats.memory_clock_mhz = stats
                    .memory_clock_mhz
                    .or(reading.memory_clock.map(|v| v as f32));
                stats.voltage_mv = stats.voltage_mv.or(reading.voltage);
            }
            a.kept = DriverReadings::of(stats);
        }
        samples.into_iter().map(|(stats, _)| stats).collect()
    }
}

impl Drop for GpuTracker {
    fn drop(&mut self) {
        for a in &self.adapters {
            close(a.handle, a.luid);
        }
    }
}

fn close(handle: u32, luid: LUID) {
    let close = D3DKMT_CLOSEADAPTER { hAdapter: handle };
    startup_native(
        "gpu.close-adapter",
        luid_key(luid),
        0,
        || unsafe { D3DKMTCloseAdapter(&close) }.0,
    );
}

/// The adapters of a new enumeration in its order, each one taken from
/// `previous` when it is there (its handle and counter baselines stay), and
/// the adapters to close: the new duplicates and those that are gone.
pub(super) fn reconcile<T>(
    mut previous: Vec<T>,
    found: Vec<T>,
    key: impl Fn(&T) -> u64,
) -> (Vec<T>, Vec<T>) {
    let mut adapters = Vec::with_capacity(found.len());
    let mut closed = Vec::new();
    for adapter in found {
        match previous.iter().position(|kept| key(kept) == key(&adapter)) {
            Some(index) => {
                adapters.push(previous.swap_remove(index));
                closed.push(adapter);
            }
            None => adapters.push(adapter),
        }
    }
    closed.extend(previous);
    (adapters, closed)
}

pub(super) fn luid_key(luid: LUID) -> u64 {
    (u64::from(luid.HighPart as u32) << 32) | u64::from(luid.LowPart)
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

pub(super) fn adapter_statistics(phase: &'static str, luid: LUID) -> Option<(u32, u32)> {
    let mut stats = statistics_query(D3DKMT_QUERYSTATISTICS_ADAPTER, luid);
    if startup_native(phase, luid_key(luid), 0, || {
        unsafe { D3DKMTQueryStatistics(&raw mut stats) }.0
    }) >= 0
    {
        let i = unsafe { &stats.QueryResult.AdapterInformation };
        Some((i.NodeCount, i.NbSegments))
    } else {
        None
    }
}

pub(super) fn node_running_time(phase: &'static str, luid: LUID, node: u32) -> Option<i64> {
    let mut q = statistics_query(D3DKMT_QUERYSTATISTICS_NODE, luid);
    q.Anonymous.QueryNode.NodeId = node;
    if startup_native(phase, luid_key(luid), node, || {
        unsafe { D3DKMTQueryStatistics(&raw mut q) }.0
    }) >= 0
    {
        Some(unsafe { q.QueryResult.NodeInformation.GlobalInformation.RunningTime })
    } else {
        None
    }
}

fn query_info<T>(handle: u32, kind: KMTQUERYADAPTERINFOTYPE, data: &mut T) -> bool {
    let mut q = D3DKMT_QUERYADAPTERINFO {
        hAdapter: handle,
        Type: kind,
        pPrivateDriverData: (data as *mut T).cast(),
        PrivateDriverDataSize: std::mem::size_of::<T>() as u32,
    };
    startup_native(
        "gpu.adapter-info",
        u64::from(handle),
        kind.0 as u32,
        || unsafe { D3DKMTQueryAdapterInfo(&mut q) }.0,
    ) >= 0
}

/// Open handles to every adapter; None when they could not be listed.
fn adapter_list() -> Option<Vec<D3DKMT_ADAPTERINFO>> {
    // The count can change between the two calls while an adapter starts or
    // stops, as during a driver update; the second call then fails.
    for _ in 0..3 {
        let mut e = D3DKMT_ENUMADAPTERS2 {
            NumAdapters: 0,
            pAdapters: std::ptr::null_mut(),
        };
        if startup_native(
            "gpu.enumerate-count",
            0,
            0,
            || unsafe { D3DKMTEnumAdapters2(&mut e) }.0,
        ) < 0
        {
            return None;
        }
        if e.NumAdapters == 0 {
            return Some(Vec::new());
        }
        let mut list = vec![D3DKMT_ADAPTERINFO::default(); e.NumAdapters as usize];
        e.pAdapters = list.as_mut_ptr();
        if startup_native(
            "gpu.enumerate-adapters",
            0,
            0,
            || unsafe { D3DKMTEnumAdapters2(&mut e) }.0,
        ) >= 0
        {
            list.truncate(e.NumAdapters as usize);
            return Some(list);
        }
    }
    None
}

/// The adapters to list and the LUIDs of the others; None when the adapters
/// could not be enumerated. `excluded` carries adapters owned by another class
/// (such as NPUs) so neither hybrid WDDM drivers nor PDH engine counters fold
/// them into the GPU list.
fn enumerate(excluded: &HashSet<u64>) -> Option<(Vec<Adapter>, HashSet<u64>)> {
    let list = adapter_list()?;
    let mut out = Vec::new();
    let mut ignored = excluded.clone();
    if list.is_empty() {
        return Some((out, ignored));
    }
    let mut metadata = startup_phase("gpu.metadata", metadata::Metadata::new);
    for a in list {
        let key = luid_key(a.AdapterLuid);
        let adapter = (!excluded.contains(&key))
            .then(|| describe(&a, &mut metadata))
            .flatten();
        match adapter {
            Some(adapter) => out.push(adapter),
            None => {
                ignored.insert(key);
                close(a.hAdapter, a.AdapterLuid);
            }
        }
    }
    Some((out, ignored))
}

fn describe(a: &D3DKMT_ADAPTERINFO, metadata: &mut metadata::Metadata) -> Option<Adapter> {
    // Skip software renderers and display-only adapters (no engines to watch).
    let mut kind = D3DKMT_ADAPTERTYPE::default();
    let mut adapter_type = 0;
    if query_info(a.hAdapter, KMTQAITYPE_ADAPTERTYPE, &mut kind) {
        let bits = unsafe { kind.Anonymous.Value };
        log::info!(target: "argus_startup", "phase=gpu.adapter-type; instance={}; handle={}; bits=0x{bits:08X}", luid_key(a.AdapterLuid), a.hAdapter);
        let render = bits & 0x1 != 0;
        let software = bits & 0x4 != 0;
        if !render || software {
            return None;
        }
        adapter_type = bits;
    }

    let (nodes, segments) = adapter_statistics("gpu.adapter-statistics", a.AdapterLuid)?;

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
    let mut groups = D3DKMT_SEGMENTGROUPSIZEINFO::default();
    let memory = memory_layout(
        query_info(a.hAdapter, KMTQAITYPE_GETSEGMENTGROUPSIZE, &mut groups).then_some(&groups),
    );
    log::info!(target: "argus_startup", "phase=gpu.memory-groups; instance={}; local_mb={}; non_local_mb={}; layout={memory:?}", luid_key(a.AdapterLuid), groups.LocalMemory >> 20, groups.NonLocalMemory >> 20);

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
    let mut ids = D3DKMT_QUERY_DEVICE_IDS::default();
    let vendor = query_info(a.hAdapter, KMTQAITYPE_PHYSICALADAPTERDEVICEIDS, &mut ids)
        .then_some(ids.DeviceIds.VendorID);
    let driver = metadata.driver(luid);
    let powers_down = power::powers_down(adapter_type);
    // Answers "why are there no sensor readings" for a GPU that was asleep.
    log::info!(target: "argus::gpu", "gpu adapter: vendor=0x{:04X}; type=0x{adapter_type:08X}; powers down when idle: {}; power state: {}", vendor.unwrap_or(0), if powers_down { "yes" } else { "not reported" }, match driver.device.as_ref().map(power::Device::working) {
        Some(Some(true)) => "D0",
        Some(Some(false)) => "low power",
        Some(None) => "unknown",
        None => "no device",
    });
    Some(Adapter {
        handle: a.hAdapter,
        luid,
        info: Arc::new(GpuInfo {
            history_id: driver
                .device
                .as_ref()
                .map(|device| device.instance_id())
                .or_else(|| pci.map(|(bus, dev, func)| format!("pci-{bus}-{dev}-{func}"))),
            id: (((luid.HighPart as u32 as u64) << 32) | luid.LowPart as u64).to_string(),
            name: if name.is_empty() { "GPU".into() } else { name },
            dedicated_total: sizes.DedicatedVideoMemorySize,
            shared_total: sizes.SharedSystemMemorySize,
            engines,
            temperature_max: (has_caps && caps.TemperatureMax > 0)
                .then(|| caps.TemperatureMax as f32 / 10.0),
            intel_integrated: processor_graphics(vendor, pci),
            driver_version: driver.version,
            driver_date: driver.date,
            direct_x: metadata.direct_x(luid),
        }),
        memory,
        node_engine,
        node_3d,
        prev_running: (0..nodes).map(|_| NodeCounter::default()).collect(),
        segments,
        perf_query: Retry::default(),
        node_perf_query: Retry::default(),
        pci,
        // Without an ID, as before: any adapter with a PCI address may be one.
        nvidia: pci.is_some() && vendor.is_none_or(|id| id == 0x10de),
        device: driver.device,
        gate: power::Gate::new(powers_down),
        kept: DriverReadings::default(),
        driver_busy: false,
    })
}

fn processor_graphics(vendor: Option<u32>, pci: Option<Pci>) -> bool {
    vendor == Some(0x8086) && pci == Some((0, 2, 0))
}

/// A unified-memory (integrated) adapter puts all of its memory, the system
/// memory it shares included, in the local budget group and has no non-local
/// memory. Without the WDDM 2.0 query, the counters keep their discrete meaning.
fn memory_layout(groups: Option<&D3DKMT_SEGMENTGROUPSIZEINFO>) -> GpuMemoryLayout {
    match groups {
        Some(groups) if groups.LocalMemory > 0 && groups.NonLocalMemory == 0 => {
            GpuMemoryLayout::Unified
        }
        _ => GpuMemoryLayout::Discrete,
    }
}

/// Hz to MHz; drivers report 0 for "not supported".
fn mhz(hz: u64) -> Option<f32> {
    (1_000_000..=100_000_000_000)
        .contains(&hz)
        .then_some(hz as f32 / 1_000_000.0)
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
    fn gpu_frequency_preserves_fractional_mhz_and_rejects_unavailable_values() {
        assert_eq!(mhz(1_194_400_000), Some(1194.4));
        assert_eq!(mhz(0), None);
        assert_eq!(mhz(u64::MAX), None);
    }

    #[test]
    fn enumerating_again_keeps_adapters_still_present_in_the_new_order() {
        // (LUID, enumeration): kept adapters retain their handles and baselines.
        let previous = vec![(1, "old"), (2, "old"), (3, "old")];
        let found = vec![(3, "new"), (4, "new"), (1, "new")];
        let (adapters, closed) = reconcile(previous, found, |a| a.0);
        assert_eq!(adapters, [(3, "old"), (4, "new"), (1, "old")]);
        assert_eq!(closed, [(3, "new"), (1, "new"), (2, "old")]);
        let (adapters, closed) = reconcile(vec![(1, "old")], Vec::new(), |a| a.0);
        assert!(adapters.is_empty());
        assert_eq!(closed, [(1, "old")]);
    }

    #[test]
    fn adapters_without_non_local_memory_have_unified_memory() {
        let groups = |local: u64, non_local: u64| D3DKMT_SEGMENTGROUPSIZEINFO {
            LocalMemory: local << 20,
            NonLocalMemory: non_local << 20,
            ..Default::default()
        };
        // Measured: an RTX 4060 Laptop GPU and the Microsoft Basic Render Driver.
        assert_eq!(
            memory_layout(Some(&groups(7956, 78419))),
            GpuMemoryLayout::Discrete
        );
        assert_eq!(
            memory_layout(Some(&groups(78419, 0))),
            GpuMemoryLayout::Unified
        );
        assert_eq!(
            memory_layout(Some(&groups(0, 0))),
            GpuMemoryLayout::Discrete
        );
        assert_eq!(memory_layout(None), GpuMemoryLayout::Discrete);
    }

    #[test]
    fn processor_graphics_requires_the_intel_vendor_and_processor_pci_function() {
        assert!(processor_graphics(Some(0x8086), Some((0, 2, 0))));
        for (vendor, pci) in [
            (None, Some((0, 2, 0))),
            (Some(0x10de), Some((0, 2, 0))),
            (Some(0x8086), Some((1, 0, 0))),
            (Some(0x8086), None),
        ] {
            assert!(!processor_graphics(vendor, pci));
        }
    }

    fn integrated_fixture() -> super::super::Hardware {
        use super::super::{
            CpuInfo, CpuPower, CpuSensors, Hardware, IntegratedGpuSensors, SensorState,
        };
        let mut gpu = stats();
        Arc::get_mut(&mut gpu.info).unwrap().intel_integrated = true;
        Hardware {
            cpu: Arc::new(CpuInfo {
                vendor: "GenuineIntel".into(),
                family: 6,
                model: 0xa5,
                sockets: 1,
                ..Default::default()
            }),
            power: CpuPower {
                package: Some(60.0),
                graphics: Some(0.014),
                ..Default::default()
            },
            cpu_sensors: Some(CpuSensors {
                state: SensorState::Ready,
                package_celsius: Some(75.0),
                integrated_gpu: Some(IntegratedGpuSensors {
                    state: SensorState::Ready,
                    temperature_celsius: Some(35.0),
                    clock_mhz: Some(350.0),
                    vid_volts: Some(0.8),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            gpus: vec![stats(), gpu, stats()],
            ..Default::default()
        }
    }

    #[test]
    fn kernel_readings_and_pp1_only_reach_the_identified_integrated_gpu() {
        let mut hw = integrated_fixture();
        hw.supplement_integrated_gpu();
        let gpu = &hw.gpus[1];
        assert_eq!(
            (gpu.temperature, gpu.clock_mhz, gpu.core_vid_mv),
            (Some(35.0), Some(350.0), Some(800))
        );
        assert_eq!(gpu.voltage_mv, None);
        assert_eq!(gpu.board_power, Some(0.014));
        assert!(matches!(gpu.power_source, Some(GpuPowerSource::Rapl)));
        assert!(hw.gpus[0].temperature.is_none() && hw.gpus[2].board_power.is_none());
        let mut hw = integrated_fixture();
        hw.power.graphics = Some(0.0);
        hw.cpu_sensors
            .as_mut()
            .unwrap()
            .integrated_gpu
            .as_mut()
            .unwrap()
            .vid_volts = Some(0.0);
        hw.supplement_integrated_gpu();
        assert_eq!(
            (hw.gpus[1].board_power, hw.gpus[1].core_vid_mv),
            (Some(0.0), Some(0))
        );
    }

    #[test]
    fn native_readings_win_and_missing_gpu_data_never_uses_cpu_temperature_or_power() {
        let mut hw = integrated_fixture();
        hw.gpus[1].temperature = Some(36.0);
        hw.gpus[1].clock_mhz = Some(400.0);
        hw.gpus[1].board_power = Some(1.0);
        hw.gpus[1].power_source = Some(GpuPowerSource::Igcl);
        hw.supplement_integrated_gpu();
        assert_eq!(
            (
                hw.gpus[1].temperature,
                hw.gpus[1].clock_mhz,
                hw.gpus[1].board_power
            ),
            (Some(36.0), Some(400.0), Some(1.0))
        );
        assert!(matches!(
            hw.gpus[1].power_source,
            Some(GpuPowerSource::Igcl)
        ));
        let mut hw = integrated_fixture();
        hw.power.graphics = None;
        hw.cpu_sensors.as_mut().unwrap().integrated_gpu = None;
        hw.supplement_integrated_gpu();
        assert_eq!(
            (hw.gpus[1].temperature, hw.gpus[1].board_power),
            (None, None)
        );
        assert_eq!(
            hw.gpus[1].kernel_sensor_state,
            Some(super::super::SensorState::NoData)
        );
    }

    #[test]
    fn direct_pp1_energy_is_not_masked_by_a_zero_windows_power_meter() {
        use super::super::{CpuMetricKind, CpuSensorSource};
        let mut hw = integrated_fixture();
        hw.power.graphics = Some(0.0);
        hw.cpu_sensors = None;
        hw.supplement_integrated_gpu();
        assert_eq!(hw.gpus[1].board_power, Some(0.0));
        let mut frame = integrated_fixture().cpu_sensors.unwrap();
        frame.metric(
            CpuMetricKind::GraphicsWatts,
            Some(0.060),
            CpuSensorSource::Msr,
        );
        hw.cpu_sensors = Some(frame);
        hw.supplement_integrated_gpu();
        assert_eq!(hw.gpus[1].board_power, Some(0.060));
        assert!(matches!(
            hw.gpus[1].power_source,
            Some(GpuPowerSource::RaplMsr)
        ));
        hw.cpu_sensors.as_mut().unwrap().metric(
            CpuMetricKind::GraphicsWatts,
            Some(0.0),
            CpuSensorSource::Msr,
        );
        hw.supplement_integrated_gpu();
        assert_eq!(hw.gpus[1].board_power, Some(0.0));
        assert!(matches!(
            hw.gpus[1].power_source,
            Some(GpuPowerSource::RaplMsr)
        ));
        hw.cpu_sensors = None;
        hw.supplement_integrated_gpu();
        assert!(matches!(
            hw.gpus[1].power_source,
            Some(GpuPowerSource::Rapl)
        ));
    }

    #[test]
    fn ambiguous_or_multi_socket_graphics_do_not_receive_package_sensor_values() {
        let mut hw = integrated_fixture();
        hw.gpus.push(hw.gpus[1].clone());
        hw.supplement_integrated_gpu();
        assert!(
            hw.gpus
                .iter()
                .all(|g| g.board_power.is_none() && g.core_vid_mv.is_none())
        );
        let mut hw = integrated_fixture();
        Arc::get_mut(&mut hw.cpu).unwrap().sockets = 2;
        hw.supplement_integrated_gpu();
        assert!(hw.gpus[1].temperature.is_none());
    }
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
    #[test]
    #[ignore = "Requires live Windows graphics adapters"]
    fn live_driver_access() {
        let npus = crate::hardware::npu::NpuTracker::new();
        let mut tracker = GpuTracker::new(&npus.luids());
        for round in 0..30 {
            if round > 0 {
                std::thread::sleep(Duration::from_secs(1));
            }
            let started = Instant::now();
            let stats = tracker.sample();
            let micros = started.elapsed().as_micros();
            for (adapter, stats) in tracker.adapters.iter().zip(&stats) {
                println!(
                    "{}",
                    serde_json::json!({"round": round, "sampleMicros": micros,
                        "name": stats.info.name, "powersDown": adapter.gate.powers_down(),
                        "access": format!("{:?}", adapter.gate.last), "lowPower": stats.low_power,
                        "util": stats.util, "temperature": stats.temperature,
                        "boardPower": stats.board_power, "clockMhz": stats.clock_mhz})
                );
            }
        }
    }
    fn stats() -> GpuStats {
        GpuStats {
            info: Arc::new(GpuInfo {
                id: "1".into(),
                history_id: None,
                name: "test".into(),
                dedicated_total: 0,
                shared_total: 0,
                engines: vec!["3D".into()],
                temperature_max: None,
                intel_integrated: false,
                driver_version: None,
                driver_date: None,
                direct_x: None,
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
            core_vid_mv: None,
            kernel_sensor_state: None,
            kernel_sensor_supported: false,
            integrated_sensors: None,
            low_power: false,
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
