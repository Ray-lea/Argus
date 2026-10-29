//! Device-level telemetry for the performance view: CPU identity, clocks,
//! temperature and power, memory modules, physical disks, network adapters,
//! GPUs and batteries.

mod battery;
mod cpu;
mod disk;
mod gpu;
mod kernel_sensors;
mod memory;
mod net;
mod nvml;
mod query;
mod registry;
mod sensors;
mod vendor_gpu;

use std::sync::Arc;

use serde::Serialize;

pub use battery::{BatteryInfo, BatteryStats};
pub use cpu::cpu_info;
pub use cpu::{CpuInfo, CpuThread};
pub use disk::{DiskInfo, DiskStats};
pub(crate) use gpu::pretty_engine;
pub use gpu::{GpuInfo, GpuStats, GpuUtilSource};
pub use kernel_sensors::{
    BoardMetric, BoardMetricKind, CcdTemperature, CoreTemperature, CpuCoreSensors, CpuFanReading,
    CpuLimitReason, CpuLimitStatus, CpuMetric, CpuMetricKind, CpuSensorSource, CpuSensors,
    IntegratedGpuLimit, IntegratedGpuLimitReason, IntegratedGpuSensors, SensorState, SmuState,
};
pub use memory::MemoryInfo;
pub use net::{NicInfo, NicStats};
pub use sensors::{CpuIdle, CpuPower, ThermalZone};
pub use vendor_gpu::GpuPowerSource;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub cpu: Arc<CpuInfo>,
    /// Effective clock in MHz (base clock scaled by current performance).
    pub cpu_mhz: Option<u32>,
    /// Per logical processor, MHz; empty when Windows does not publish it.
    pub core_mhz: Vec<u32>,
    /// Logical-thread idle-state residency reported by Windows, not hardware C6/C10.
    pub cpu_idle: Vec<CpuIdle>,
    /// ACPI thermal zones; on most laptops one of them tracks the CPU package.
    pub thermal: Vec<ThermalZone>,
    pub power: CpuPower,
    pub cpu_sensors: Option<CpuSensors>,
    pub memory: Arc<MemoryInfo>,
    pub disks: Vec<DiskStats>,
    pub nics: Vec<NicStats>,
    pub gpus: Vec<GpuStats>,
    pub batteries: Vec<BatteryStats>,
}

/// Comet Lake S/H: the Gen9 GT register layout, not the newer Xe layout.
pub fn intel_mchbar_supported(cpu: &CpuInfo) -> bool {
    cpu.vendor == "GenuineIntel" && cpu.family == 6 && cpu.model == 0xa5 && cpu.sockets == 1
}

impl Hardware {
    /// PP1 belongs to the processor graphics, never to a discrete Intel card.
    /// Require a single socket and an unambiguous hardware identity.
    pub fn supplement_integrated_gpu(&mut self) {
        if self.cpu.vendor != "GenuineIntel" || self.cpu.sockets != 1 {
            return;
        }
        let mut matches = self.gpus.iter_mut().filter(|g| g.info.intel_integrated);
        let Some(gpu) = matches.next() else { return };
        if matches.next().is_some() {
            return;
        }
        if gpu.board_power.is_none()
            || matches!(
                gpu.power_source,
                Some(GpuPowerSource::Rapl | GpuPowerSource::RaplMsr)
            )
        {
            let valid = |v: &f32| v.is_finite() && (0.0..=1000.0).contains(v);
            let direct = self
                .cpu_sensors
                .as_ref()
                .filter(|s| s.state == SensorState::Ready)
                .and_then(|s| {
                    s.metrics.iter().find(|m| {
                        m.kind == CpuMetricKind::GraphicsWatts
                            && matches!(m.source, CpuSensorSource::Msr)
                    })
                })
                .map(|m| m.value)
                .filter(valid);
            gpu.board_power = direct.or(self.power.graphics.filter(valid));
            gpu.power_source = gpu.board_power.map(|_| {
                if direct.is_some() {
                    GpuPowerSource::RaplMsr
                } else {
                    GpuPowerSource::Rapl
                }
            });
        }
        if !intel_mchbar_supported(&self.cpu) {
            return;
        }
        gpu.kernel_sensor_supported = true;
        let sensors = self.cpu_sensors.as_ref();
        let data = sensors.and_then(|s| s.integrated_gpu.as_ref());
        gpu.kernel_sensor_state = data.map(|d| d.state).or_else(|| {
            sensors.map(|s| {
                if s.state == SensorState::Ready {
                    SensorState::NoData
                } else {
                    s.state
                }
            })
        });
        if let Some(data) = data.filter(|d| d.state == SensorState::Ready) {
            gpu.integrated_sensors = Some(data.clone());
            gpu.temperature = gpu.temperature.or(data
                .temperature_celsius
                .filter(|v| v.is_finite() && (1.0..=150.0).contains(v)));
            gpu.clock_mhz = gpu.clock_mhz.or(data
                .clock_mhz
                .filter(|v| v.is_finite() && (0.0..=3000.0).contains(v)));
            gpu.memory_clock_mhz = gpu.memory_clock_mhz.or(data
                .memory_clock_mhz
                .filter(|v| v.is_finite() && (1.0..=10000.0).contains(v)));
            gpu.clock_max_mhz = gpu.clock_max_mhz.or(data
                .maximum_clock_mhz
                .filter(|v| v.is_finite() && (1.0..=3000.0).contains(v)));
            gpu.core_vid_mv = data
                .vid_volts
                .filter(|v| v.is_finite() && (0.0..=1.52).contains(v))
                .map(|v| (v * 1000.0).round() as u32);
        }
    }
}

pub(crate) struct HardwareTracker {
    cpu: Arc<CpuInfo>,
    sensors: Option<sensors::Sensors>,
    memory: Arc<MemoryInfo>,
    disks: disk::DiskTracker,
    nics: net::NicTracker,
    gpus: gpu::GpuTracker,
    batteries: battery::BatteryTracker,
}

impl HardwareTracker {
    pub(crate) fn gpu_node_engine(&self, luid: u64, node: u32) -> Option<usize> {
        self.gpus.node_engine(luid, node)
    }
    pub fn new() -> Self {
        let cpu = cpu::cpu_info();
        let mut t = Self {
            sensors: sensors::Sensors::new(cpu.reference_mhz, &cpu.threads),
            cpu: Arc::new(cpu),
            memory: Arc::new(memory::memory_info()),
            disks: disk::DiskTracker::new(),
            nics: net::NicTracker::new(),
            gpus: gpu::GpuTracker::new(),
            batteries: battery::BatteryTracker::new(),
        };
        t.sample(); // establish baselines for rates
        t
    }

    pub fn sample(&mut self) -> Hardware {
        use windows::Win32::System::Threading::{ALL_PROCESSOR_GROUPS, GetActiveProcessorCount};
        if unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) } != self.cpu.logical {
            let cpu = cpu::cpu_info();
            self.sensors = sensors::Sensors::new(cpu.reference_mhz, &cpu.threads);
            self.cpu = Arc::new(cpu);
        }
        let sensed = self
            .sensors
            .as_mut()
            .map(|s| s.sample())
            .unwrap_or_default();
        let mut hardware = Hardware {
            cpu: self.cpu.clone(),
            cpu_mhz: sensed.cpu_mhz,
            core_mhz: sensed.core_mhz,
            cpu_idle: sensed.idle,
            thermal: sensed.thermal,
            power: sensed.power,
            cpu_sensors: None,
            memory: self.memory.clone(),
            disks: self.disks.sample(),
            nics: self.nics.sample(),
            gpus: self.gpus.sample(),
            batteries: self.batteries.sample(),
        };
        hardware.supplement_integrated_gpu();
        hardware
    }
}
