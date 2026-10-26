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
    SensorState, SmuState,
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
        Hardware {
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
        }
    }
}
