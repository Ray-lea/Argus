//! NVMe Identify Controller and SMART/Health log 02h, decoded from bounded bytes.

use serde::{Deserialize, Serialize};

use super::DiskCounter;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NvmeIdentity {
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub firmware_revision: Option<String>,
    pub version: Option<NvmeVersion>,
    pub volatile_write_cache: bool,
    pub dataset_management: bool,
    pub warning_temperature_celsius: Option<f32>,
    pub critical_temperature_celsius: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NvmeVersion {
    pub major: u16,
    pub minor: u8,
    pub tertiary: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NvmeHealth {
    /// Log 02h describes the controller; namespaces can share these counters.
    pub scope: String,
    /// Preserve every warning bit, including bits introduced by newer revisions.
    pub critical_warning: u8,
    pub temperature: NvmeTemperature,
    pub available_spare_percent: Option<u8>,
    pub spare_threshold_percent: Option<u8>,
    /// Manufacturer endurance estimate, allowed to exceed 100 (up to 255).
    pub percentage_used: u8,
    /// 100 minus percentage_used, saturated at zero; separate from warning bits.
    pub remaining_life_percent: u8,
    /// One unit represents 1,000 x 512 bytes, rounded up by the controller.
    pub data_units_read: DiskCounter,
    pub data_units_written: DiskCounter,
    /// None only if multiplication would overflow u128; raw counters remain exact.
    pub host_read_bytes: Option<DiskCounter>,
    pub host_written_bytes: Option<DiskCounter>,
    pub host_read_commands: DiskCounter,
    pub host_write_commands: DiskCounter,
    pub controller_busy_minutes: DiskCounter,
    pub power_cycles: DiskCounter,
    pub power_on_hours: DiskCounter,
    pub unsafe_shutdowns: DiskCounter,
    pub media_errors: DiskCounter,
    pub error_log_entries: DiskCounter,
    pub warning_temperature_minutes: u32,
    pub critical_temperature_minutes: u32,
    /// All eight numbered slots are retained, including unavailable sensors.
    pub temperature_sensors: [NvmeTemperature; 8],
    /// NVMe 1.3+ thermal-management fields; older revisions leave them unknown.
    pub thermal_management: Option<NvmeThermalManagement>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NvmeTemperature {
    pub kelvin: u16,
    pub celsius: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NvmeThermalManagement {
    pub threshold1_transitions: u32,
    pub threshold2_transitions: u32,
    pub threshold1_seconds: u32,
    pub threshold2_seconds: u32,
}

pub(super) fn identify(data: &[u8]) -> Option<NvmeIdentity> {
    if data.len() < 4096 || data.iter().all(|b| *b == 0) || data.iter().all(|b| *b == 255) {
        return None;
    }
    let version = u32_at(data, 80);
    Some(NvmeIdentity {
        serial_number: text(&data[4..24]),
        model: text(&data[24..64]),
        firmware_revision: text(&data[64..72]),
        version: (version != 0 && version != u32::MAX).then_some(NvmeVersion {
            major: (version >> 16) as u16,
            minor: (version >> 8) as u8,
            tertiary: version as u8,
        }),
        volatile_write_cache: data[525] & 1 != 0,
        dataset_management: u16_at(data, 520) & (1 << 2) != 0,
        warning_temperature_celsius: temperature(u16_at(data, 266)).celsius,
        critical_temperature_celsius: temperature(u16_at(data, 268)).celsius,
    })
}

pub(super) fn health(data: &[u8], version: Option<NvmeVersion>) -> Option<NvmeHealth> {
    if data.len() < 512 || data.iter().all(|b| *b == 0) || data.iter().all(|b| *b == 255) {
        return None;
    }
    let counter = |offset| {
        DiskCounter(u128::from_le_bytes(
            data[offset..offset + 16]
                .try_into()
                .expect("bounded counter"),
        ))
    };
    let units_read = counter(32);
    let units_written = counter(48);
    Some(NvmeHealth {
        scope: "controller".into(),
        critical_warning: data[0],
        temperature: temperature(u16_at(data, 1)),
        available_spare_percent: (data[3] <= 100).then_some(data[3]),
        spare_threshold_percent: (data[4] <= 100).then_some(data[4]),
        percentage_used: data[5],
        remaining_life_percent: 100u8.saturating_sub(data[5]),
        data_units_read: units_read,
        data_units_written: units_written,
        host_read_bytes: units_read.0.checked_mul(512_000).map(DiskCounter),
        host_written_bytes: units_written.0.checked_mul(512_000).map(DiskCounter),
        host_read_commands: counter(64),
        host_write_commands: counter(80),
        controller_busy_minutes: counter(96),
        power_cycles: counter(112),
        power_on_hours: counter(128),
        unsafe_shutdowns: counter(144),
        media_errors: counter(160),
        error_log_entries: counter(176),
        warning_temperature_minutes: u32_at(data, 192),
        critical_temperature_minutes: u32_at(data, 196),
        temperature_sensors: std::array::from_fn(|i| temperature(u16_at(data, 200 + i * 2))),
        thermal_management: version
            .filter(|v| v.major > 1 || (v.major == 1 && v.minor >= 3))
            .map(|_| NvmeThermalManagement {
                threshold1_transitions: u32_at(data, 216),
                threshold2_transitions: u32_at(data, 220),
                threshold1_seconds: u32_at(data, 224),
                threshold2_seconds: u32_at(data, 228),
            }),
    })
}

fn temperature(kelvin: u16) -> NvmeTemperature {
    NvmeTemperature {
        kelvin,
        celsius: (kelvin != 0 && kelvin != u16::MAX).then_some(kelvin as f32 - 273.15),
    }
}

pub(super) fn text(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let value = String::from_utf8_lossy(&bytes[..end]);
    let value = value.trim();
    (!value.is_empty() && !value.chars().any(char::is_control)).then(|| value.to_owned())
}

fn u16_at(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().expect("bounded u16"))
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("bounded u32"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> [u8; 512] {
        let mut data = [0; 512];
        data[1..3].copy_from_slice(&323u16.to_le_bytes());
        data[3] = 100;
        data[4] = 1;
        data[5] = 2;
        for (offset, value) in [
            (32, 28_209_922u128),
            (48, 65_564_084),
            (112, 188),
            (128, 14_551),
            (144, 81),
        ] {
            data[offset..offset + 16].copy_from_slice(&value.to_le_bytes());
        }
        data[200..202].copy_from_slice(&323u16.to_le_bytes());
        data[202..204].copy_from_slice(&315u16.to_le_bytes());
        data[216..220].copy_from_slice(&2u32.to_le_bytes());
        data
    }

    #[test]
    fn reference_health_converts_units_and_keeps_sensor_slots() {
        let value = health(
            &sample(),
            Some(NvmeVersion {
                major: 1,
                minor: 4,
                tertiary: 0,
            }),
        )
        .unwrap();
        assert_eq!(value.remaining_life_percent, 98);
        assert!((value.temperature.celsius.unwrap() - 49.85).abs() < 0.001);
        assert_eq!(value.host_read_bytes.unwrap().0, 28_209_922 * 512_000);
        assert_eq!(value.host_written_bytes.unwrap().0, 65_564_084 * 512_000);
        assert_eq!(value.power_on_hours.0, 14_551);
        assert_eq!(value.power_cycles.0, 188);
        assert_eq!(value.unsafe_shutdowns.0, 81);
        assert!(value.temperature_sensors[2].celsius.is_none());
        assert_eq!(value.temperature_sensors[1].kelvin, 315);
        assert_eq!(value.thermal_management.unwrap().threshold1_transitions, 2);
    }

    #[test]
    fn exhaustion_warnings_and_large_counters_are_independent() {
        let mut data = sample();
        data[0] = 0x8c;
        data[5] = 255;
        data[32..48].copy_from_slice(&u128::MAX.to_le_bytes());
        let value = health(&data, None).unwrap();
        assert_eq!(value.remaining_life_percent, 0);
        assert_eq!(value.critical_warning, 0x8c);
        assert_eq!(value.data_units_read.0, u128::MAX);
        assert!(value.host_read_bytes.is_none());
        assert!(value.thermal_management.is_none());
        assert!(serde_json::to_value(value).is_ok());
    }

    #[test]
    fn malformed_or_absent_pages_do_not_become_healthy_disks() {
        for len in 0..512 {
            assert!(health(&sample()[..len], None).is_none());
        }
        assert!(health(&[0; 512], None).is_none());
        assert!(health(&[255; 512], None).is_none());
        assert!(identify(&[0; 4096]).is_none());
        assert!(identify(&[255; 4096]).is_none());
        let mut data = sample();
        data[3] = 255;
        data[4] = 255;
        data[1..3].copy_from_slice(&u16::MAX.to_le_bytes());
        let value = health(&data, None).unwrap();
        assert!(value.available_spare_percent.is_none());
        assert!(value.spare_threshold_percent.is_none());
        assert!(value.temperature.celsius.is_none());
    }

    #[test]
    fn identify_decodes_firmware_version_and_capability_bits() {
        let mut data = [0; 4096];
        data[24..32].copy_from_slice(b"Example ");
        data[64..72].copy_from_slice(b"1.02    ");
        data[80..84].copy_from_slice(&0x00010400u32.to_le_bytes());
        data[520] = 4;
        data[525] = 1;
        let value = identify(&data).unwrap();
        assert_eq!(value.model.as_deref(), Some("Example"));
        assert_eq!(value.firmware_revision.as_deref(), Some("1.02"));
        assert_eq!(value.version.unwrap().minor, 4);
        assert!(value.dataset_management && value.volatile_write_cache);
        assert!(value.serial_number.is_none());
    }
}
