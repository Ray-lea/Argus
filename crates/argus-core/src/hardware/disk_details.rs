//! On-demand, read-only disk information. Keep device queries off the sampler.

mod ata;
mod ata_health;
mod io;
mod legacy;
mod nvme;
mod pcie;
mod scsi;

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize, Serializer};

pub use ata::{AtaAttribute, AtaIdentity};
pub use ata_health::{AtaHealth, AtaMetric, AtaMetricSource};
pub use legacy::{SmartHealth, SmartSource};
pub use scsi::{ScsiCounter, ScsiIdentity};

pub use nvme::{NvmeHealth, NvmeIdentity, NvmeTemperature, NvmeThermalManagement, NvmeVersion};
pub use pcie::{PcieLink, PcieLinkRate};

/// NVMe counters are 128-bit. Decimal strings survive JSON and UI round trips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct DiskCounter(pub u128);

impl TryFrom<String> for DiskCounter {
    type Error = std::num::ParseIntError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse().map(Self)
    }
}

impl Serialize for DiskCounter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub ata_identity: Option<AtaIdentity>,
    pub scsi_identity: Option<ScsiIdentity>,
    pub smart_health: Option<SmartHealth>,
    /// Derived locally from the model and raw attributes, including older service replies.
    pub ata_health: Option<AtaHealth>,
    pub via_service: bool,
    pub queries: Vec<DiskQueryStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskIdentity {
    pub model: Option<String>,
    /// Returned only by this detail API; never added to periodic snapshots/logs.
    pub serial_number: Option<String>,
    pub firmware_revision: Option<String>,
    pub bus: String,
    pub bus_code: u32,
    pub removable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    FailurePrediction,
    SmartAccess,
    AtaIdentity,
    AtaAttributes,
    AtaThresholds,
    AtaStatus,
    ScsiIdentity,
    ScsiHealth,
    ScsiTemperature,
    ScsiReadErrors,
    ScsiWriteErrors,
    Service,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskQueryState {
    Ready,
    Unsupported,
    AccessDenied,
    Unavailable,
    InvalidData,
    ServiceOutdated,
    ServiceUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            ata_identity: None,
            scsi_identity: None,
            smart_health: None,
            ata_health: None,
            via_service: false,
            queries: Vec::new(),
        }
    }
}

/// Queries one PhysicalDrive by number with a zero-access device handle.
/// Missing devices and unsupported properties are represented in `queries`.
/// Each call reads fresh data exactly once and closes every handle on return.
/// Call on a worker. No cache, polling thread, timer, or sampler integration.
pub fn query(index: u32) -> DiskDetails {
    let mut details = io::query(index);
    details.refresh_ata_health();
    details
}

/// Fixed read-only ATA/SAT/SCSI queries, called by the existing service.
/// Opening the device with read/write access grants IOCTL access only: every
/// command below is an identity or health read. No settings or media are written.
pub fn query_privileged(index: u32) -> DiskDetails {
    let mut details = query(index);
    legacy::supplement(&mut details);
    details.refresh_ata_health();
    details
}

impl DiskDetails {
    /// Decode existing attributes without querying hardware or changing the service protocol.
    pub fn refresh_ata_health(&mut self) {
        self.ata_health = ata_health::decode(self);
    }

    pub fn needs_smart_supplement(&self) -> bool {
        self.nvme_identity.is_none()
            && self.nvme_health.is_none()
            && self.identity.as_ref().is_some_and(|i| {
                matches!(
                    i.bus.as_str(),
                    "ata" | "sata" | "usb" | "scsi" | "sas" | "raid"
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_remain_exact_in_json_values() {
        let value = DiskCounter(u128::MAX);
        assert_eq!(serde_json::to_value(value).unwrap(), u128::MAX.to_string());
        assert_eq!(serde_json::to_value(DiskCounter(0)).unwrap(), "0");
        assert_eq!(
            serde_json::from_value::<DiskCounter>(serde_json::to_value(value).unwrap())
                .unwrap()
                .0,
            u128::MAX
        );
    }
}
