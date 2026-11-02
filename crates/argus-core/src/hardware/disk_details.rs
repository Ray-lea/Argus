//! On-demand, read-only disk information. Keep device queries off the sampler.

mod io;
mod nvme;
mod pcie;

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Serialize, Serializer};

pub use nvme::{NvmeHealth, NvmeIdentity, NvmeTemperature, NvmeThermalManagement, NvmeVersion};
pub use pcie::{PcieLink, PcieLinkRate};

/// NVMe counters are 128-bit. Decimal strings survive JSON and UI round trips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskCounter(pub u128);

impl Serialize for DiskCounter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskDetails {
    pub index: u32,
    /// Unix milliseconds when this explicit query started.
    pub queried_at: u64,
    pub identity: Option<DiskIdentity>,
    pub capacity_bytes: Option<u64>,
    pub volumes: Vec<String>,
    pub solid_state: Option<bool>,
    pub logical_sector_bytes: Option<u32>,
    pub physical_sector_bytes: Option<u32>,
    /// Windows storage-stack TRIM support, independent of the NVMe capability bit.
    pub trim_enabled: Option<bool>,
    pub pcie_link: Option<PcieLink>,
    pub nvme_identity: Option<NvmeIdentity>,
    pub nvme_health: Option<NvmeHealth>,
    pub queries: Vec<DiskQueryStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskIdentity {
    pub model: Option<String>,
    /// Returned only by this detail API; never added to periodic snapshots/logs.
    pub serial_number: Option<String>,
    pub firmware_revision: Option<String>,
    pub bus: &'static str,
    pub bus_code: u32,
    pub removable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskQuery {
    Open,
    Identity,
    Capacity,
    SeekPenalty,
    Alignment,
    Trim,
    PcieLink,
    NvmeIdentity,
    NvmeHealth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskQueryState {
    Ready,
    Unsupported,
    AccessDenied,
    Unavailable,
    InvalidData,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskQueryStatus {
    pub query: DiskQuery,
    pub state: DiskQueryState,
    pub win32_error: Option<u32>,
}

impl DiskDetails {
    fn empty(index: u32) -> Self {
        Self {
            index,
            queried_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            identity: None,
            capacity_bytes: None,
            volumes: Vec::new(),
            solid_state: None,
            logical_sector_bytes: None,
            physical_sector_bytes: None,
            trim_enabled: None,
            pcie_link: None,
            nvme_identity: None,
            nvme_health: None,
            queries: Vec::new(),
        }
    }
}

/// Queries one PhysicalDrive by number with a zero-access device handle.
/// Missing devices and unsupported properties are represented in `queries`.
/// Each call reads fresh data exactly once and closes every handle on return.
/// Call on a worker. No cache, polling thread, timer, or sampler integration.
pub fn query(index: u32) -> DiskDetails {
    io::query(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_remain_exact_in_json_values() {
        let value = DiskCounter(u128::MAX);
        assert_eq!(serde_json::to_value(value).unwrap(), u128::MAX.to_string());
        assert_eq!(serde_json::to_value(DiskCounter(0)).unwrap(), "0");
    }
}
