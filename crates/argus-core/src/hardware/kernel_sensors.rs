//! The fixed, read-only sensor snapshot returned by the privileged broker.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SensorState {
    #[default]
    Pending,
    ServiceUnavailable,
    DriverUnavailable,
    ModuleRejected,
    Unsupported,
    NoData,
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreTemperature {
    pub group: u16,
    pub processor: u8,
    pub celsius: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CcdTemperature {
    pub index: u8,
    pub celsius: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CpuMetricKind {
    RingMhz,
    RingMinMhz,
    RingMaxMhz,
    RingMultiplier,
    PptPercent,
    TdcPercent,
    EdcPercent,
    CoreSmuVoltage,
    VddioWatts,
    Vdd18Watts,
    MemoryIoVolts,
    GraphicsMhz,
    VidLimitVolts,
    CoreVoltage,
    CoreCurrent,
    CoreRailWatts,
    CoreRailVid,
    CoreVrmCelsius,
    SecondCoreVoltage,
    SecondCoreCurrent,
    SecondCoreRailWatts,
    SecondCoreRailVid,
    SecondCoreVrmCelsius,
    SocVoltage,
    SocCurrent,
    SocRailWatts,
    SocVid,
    SocVrmCelsius,
    MiscVoltage,
    MiscCurrent,
    MiscRailWatts,
    MiscVid,
    MiscVrmCelsius,
    CoresWatts,
    SocWatts,
    MiscWatts,
    GraphicsWatts,
    MemoryWatts,
    PlatformWatts,
    PptWatts,
    PptLimitWatts,
    PptFastWatts,
    PptFastLimitWatts,
    PptSlowWatts,
    TdcAmps,
    TdcLimitAmps,
    EdcAmps,
    EdcLimitAmps,
    Pl1Watts,
    Pl2Watts,
    Pl1Seconds,
    Pl2Seconds,
    TdpWatts,
    ThermalLimitCelsius,
    AverageMhz,
    EffectiveMhz,
    BusMhz,
    FabricMhz,
    UncoreMhz,
    MemoryMhz,
    SocCelsius,
    IoDieCelsius,
    L3Ccd1Celsius,
    L3Ccd2Celsius,
    VddpVolts,
    VddgIodVolts,
    VddgCcdVolts,
    NorthbridgeVidVolts,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CpuSensorSource {
    Msr,
    Smu,
    Svi2,
    Svi3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuMetric {
    pub kind: CpuMetricKind,
    pub value: f32,
    pub source: CpuSensorSource,
}

/// One physical core; SMT siblings share energy and thermal sensors.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuCoreSensors {
    pub core: u32,
    pub group: u16,
    pub processor: u8,
    pub celsius: Option<f32>,
    pub distance_to_limit: Option<f32>,
    pub clock_mhz: Option<f32>,
    pub effective_mhz: Option<f32>,
    pub watts: Option<f32>,
    /// Requested voltage, never presented as a measured supply voltage.
    pub vid: Option<f32>,
    pub multiplier: Option<f32>,
    pub c0_percent: Option<f32>,
    pub thermal_throttling: Option<bool>,
    pub voltage: Option<f32>,
    pub smu_c0_percent: Option<f32>,
    pub c1_percent: Option<f32>,
    pub c6_percent: Option<f32>,
    pub os_c1_percent: Option<f32>,
    pub os_c2_percent: Option<f32>,
    pub os_c3_percent: Option<f32>,
    pub efficient: bool,
    pub limits: Vec<CpuLimitStatus>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CpuLimitReason {
    Thermal,
    Prochot,
    CriticalTemperature,
    Power,
    Current,
    CrossDomain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuLimitStatus {
    pub reason: CpuLimitReason,
    pub active: bool,
    /// Hardware latch since the last external reset; never cleared by Argus.
    pub occurred: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SmuState {
    Pending,
    Unavailable,
    UnsupportedTable,
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuFanReading {
    pub channel: u8,
    pub rpm: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CpuSensors {
    pub state: SensorState,
    pub package_celsius: Option<f32>,
    /// AMD control temperature; do not silently present Tctl as Tdie.
    pub control_celsius: Option<f32>,
    pub core_temperatures: Vec<CoreTemperature>,
    pub ccd_temperatures: Vec<CcdTemperature>,
    /// Measured SVI2/SVI3 core rail voltage, not the requested VID.
    pub core_voltage: Option<f32>,
    /// Requested voltage, where the processor documents a VID encoding.
    pub core_vid: Option<f32>,
    pub package_watts: Option<f32>,
    pub metrics: Vec<CpuMetric>,
    pub cores: Vec<CpuCoreSensors>,
    pub smu_state: Option<SmuState>,
    pub smu_table_version: Option<u32>,
    pub thermal_throttling: Option<bool>,
    pub board_sensor: Option<String>,
    pub board_fans: Vec<CpuFanReading>,
    pub limits: Vec<CpuLimitStatus>,
}

impl CpuSensors {
    pub fn unavailable(state: SensorState) -> Self {
        Self {
            state,
            ..Self::default()
        }
    }

    pub fn metric(&mut self, kind: CpuMetricKind, value: Option<f32>, source: CpuSensorSource) {
        if let Some(value) = value.filter(|v| v.is_finite()) {
            if let Some(existing) = self.metrics.iter_mut().find(|m| m.kind == kind) {
                *existing = CpuMetric {
                    kind,
                    value,
                    source,
                };
            } else {
                self.metrics.push(CpuMetric {
                    kind,
                    value,
                    source,
                });
            }
        }
    }

    pub fn value(&self, kind: CpuMetricKind) -> Option<f32> {
        self.metrics
            .iter()
            .find(|m| m.kind == kind)
            .map(|m| m.value)
    }
}
