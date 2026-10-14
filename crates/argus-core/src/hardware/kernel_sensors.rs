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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuSensors {
    pub state: SensorState,
    pub package_celsius: Option<f32>,
    /// AMD control temperature; do not silently present Tctl as Tdie.
    pub control_celsius: Option<f32>,
    pub core_temperatures: Vec<CoreTemperature>,
    pub ccd_temperatures: Vec<CcdTemperature>,
    /// AMD SVI2 telemetry, not the requested VID.
    pub core_voltage: Option<f32>,
    /// Requested voltage, where the processor documents a VID encoding.
    pub core_vid: Option<f32>,
    pub package_watts: Option<f32>,
}

impl CpuSensors {
    pub fn unavailable(state: SensorState) -> Self {
        Self {
            state,
            ..Self::default()
        }
    }
}
