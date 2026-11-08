//! Model-specific interpretations of existing ATA SMART attributes.
//! Units and endurance fields are deliberately scoped to documented families.
//! References and supported families: docs/architecture/core.md.
use serde::{Deserialize, Serialize};

use super::{AtaAttribute, DiskCounter, DiskDetails};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtaHealth {
    pub remaining_life_percent: Option<u8>,
    pub available_spare_percent: Option<u8>,
    pub spare_threshold_percent: Option<u8>,
    pub temperature_celsius: Option<f32>,
    pub host_read_bytes: Option<DiskCounter>,
    pub host_written_bytes: Option<DiskCounter>,
    pub power_on_hours: Option<DiskCounter>,
    pub power_cycles: Option<DiskCounter>,
    pub unsafe_shutdowns: Option<DiskCounter>,
    pub uncorrectable_errors: Option<DiskCounter>,
    /// Attribute roles let the UI name the same source it used for a reading.
    pub attributes: Vec<AtaMetricSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtaMetricSource {
    pub id: u8,
    pub metric: AtaMetric,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AtaMetric {
    RemainingLife,
    AvailableSpare,
    Temperature,
    HostRead,
    HostWritten,
    PowerOnHours,
    PowerCycles,
    UnsafeShutdowns,
    UncorrectableErrors,
}

#[derive(Clone, Copy)]
enum Profile {
    Samsung,
    Micron,
    Intel,
    KingstonA400 { normalized: bool },
    KingstonUv,
    KingstonKc600,
    WdSandisk,
}

fn profile(model: &str, firmware: &str) -> Option<Profile> {
    let model = model
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    let samsung = model.strip_prefix("SAMSUNG ").unwrap_or(&model);
    if [
        "SSD 750 ", "SSD 830 ", "SSD 840 ", "SSD 850 ", "SSD 860 ", "SSD 870 ",
    ]
    .iter()
    .any(|prefix| samsung.starts_with(prefix))
        && model.starts_with("SAMSUNG ")
        || [
            "MZ7PC", "MZMPC", "MZ7TD", "MZMTD", "MZ7PD", "MZ7TE", "MZNTE", "MZ7LN", "MZNLN",
            "MZYLN",
        ]
        .iter()
        .any(|prefix| samsung.starts_with(prefix))
    {
        return Some(Profile::Samsung);
    }
    let crucial = model.strip_prefix("CRUCIAL_").unwrap_or(&model);
    if crucial.strip_prefix("CT").is_some_and(|part| {
        let series = part.trim_start_matches(|c: char| c.is_ascii_digit());
        [
            "MX100SSD", "MX200SSD", "MX300SSD", "MX500SSD", "BX300SSD", "BX500SSD", "M500SSD",
            "M550SSD",
        ]
        .iter()
        .any(|prefix| series.starts_with(prefix))
    }) || [
        "MICRON_1100_",
        "MICRON 1100 SATA ",
        "MICRON_1300_",
        "MICRON_M600_",
    ]
    .iter()
    .any(|prefix| model.starts_with(prefix))
    {
        return Some(Profile::Micron);
    }
    if model.strip_prefix("INTEL ").is_some_and(|part| {
        [
            "SSDSC2BB", "SSDSC2BP", "SSDSC2BX", "SSDSC2BA", "SSDSC2BW", "SSDSC2CW", "SSDSC2CT",
        ]
        .iter()
        .any(|prefix| part.starts_with(prefix))
    }) {
        return Some(Profile::Intel);
    }
    if let Some(part) = model.strip_prefix("KINGSTON ") {
        if part.starts_with("SA400S37") || part.starts_with("SA400M8") {
            return Some(Profile::KingstonA400 {
                normalized: firmware.starts_with("03070009"),
            });
        }
        if part.starts_with("SKC600") {
            return Some(Profile::KingstonKc600);
        }
        if part.starts_with("SUV400") || part.starts_with("SUV500") {
            return Some(Profile::KingstonUv);
        }
    }
    let wd_code = model.strip_prefix("WDC WDS").is_some_and(|part| {
        ["1B0", "2B0", "1G0", "2G0", "3G0", "5G0", "1R0", "2R0"]
            .iter()
            .any(|series| part.contains(series))
    });
    if wd_code
        || [
            "WDC WDBNCE",
            "WD BLUE SA510 ",
            "WD RED SA500 ",
            "WD GREEN 2.5 ",
            "WD GREEN M.2 ",
            "SANDISK SDSSDH3",
        ]
        .iter()
        .any(|prefix| model.starts_with(prefix))
    {
        return Some(Profile::WdSandisk);
    }
    None
}

fn raw(attribute: &AtaAttribute, bytes: usize) -> Option<u128> {
    if attribute.raw == [255; 6] {
        return None;
    }
    Some(
        attribute.raw[..bytes]
            .iter()
            .enumerate()
            .map(|(index, byte)| u128::from(*byte) << (index * 8))
            .sum(),
    )
}

fn normalized(attribute: &AtaAttribute) -> Option<u8> {
    (attribute.current <= 100).then_some(attribute.current)
}

pub(super) fn decode(details: &DiskDetails) -> Option<AtaHealth> {
    let health = details.smart_health.as_ref()?;
    if details.nvme_health.is_some()
        || health.attributes.is_empty()
        || details
            .ata_identity
            .as_ref()
            .and_then(|i| i.solid_state)
            .or(details.solid_state)
            == Some(false)
    {
        return None;
    }
    // An ATA identity belongs to the device behind a bridge; its model wins.
    let model = details
        .ata_identity
        .as_ref()
        .and_then(|i| i.model.as_deref())
        .or_else(|| details.identity.as_ref().and_then(|i| i.model.as_deref()))?;
    let firmware = details
        .ata_identity
        .as_ref()
        .and_then(|i| i.firmware_revision.as_deref())
        .or_else(|| {
            details
                .identity
                .as_ref()
                .and_then(|i| i.firmware_revision.as_deref())
        })
        .unwrap_or("");
    let profile = profile(model, firmware)?;
    let mut result = AtaHealth::default();
    let mut find = |ids: &[u8], metric| {
        let attribute = ids
            .iter()
            .find_map(|id| health.attributes.iter().find(|a| a.id == *id))?;
        result.attributes.push(AtaMetricSource {
            id: attribute.id,
            metric,
        });
        Some(attribute)
    };
    let life_id = match profile {
        Profile::Samsung => 177,
        Profile::Micron => 202,
        Profile::Intel => 233,
        Profile::WdSandisk => 230,
        _ => 231,
    };
    result.remaining_life_percent =
        find(&[life_id], AtaMetric::RemainingLife).and_then(|attribute| {
            match profile {
                Profile::KingstonA400 { normalized: false } => raw(attribute, 1)
                    .filter(|value| *value <= 100)
                    .map(|value| value as u8),
                // WD/SanDisk encode the whole percentage used in raw byte 1.
                Profile::WdSandisk => {
                    raw(attribute, 2).map(|value| 100u8.saturating_sub((value >> 8) as u8))
                }
                _ => normalized(attribute),
            }
        });
    if matches!(profile, Profile::Intel)
        && let Some(spare) = find(&[232], AtaMetric::AvailableSpare)
    {
        result.available_spare_percent = normalized(spare);
        result.spare_threshold_percent = spare.threshold.filter(|value| *value <= 100);
    }
    result.temperature_celsius = find(&[194, 190], AtaMetric::Temperature)
        .and_then(|attribute| raw(attribute, 1))
        .filter(|value| *value <= 125)
        .map(|value| value as f32);
    result.power_on_hours = find(&[9], AtaMetric::PowerOnHours)
        .and_then(|attribute| raw(attribute, 4))
        .map(DiskCounter);
    result.power_cycles = find(&[12], AtaMetric::PowerCycles)
        .and_then(|attribute| raw(attribute, 6))
        .map(DiskCounter);
    let unsafe_ids: &[u8] = match profile {
        Profile::Samsung => &[235],
        Profile::Micron | Profile::KingstonUv | Profile::WdSandisk => &[174],
        Profile::Intel => &[174, 192],
        Profile::KingstonA400 { .. } | Profile::KingstonKc600 => &[192],
    };
    result.unsafe_shutdowns = find(unsafe_ids, AtaMetric::UnsafeShutdowns)
        .and_then(|attribute| raw(attribute, 6))
        .map(DiskCounter);
    let error_id = if matches!(profile, Profile::KingstonKc600) {
        160
    } else {
        187
    };
    result.uncorrectable_errors = find(&[error_id], AtaMetric::UncorrectableErrors)
        .and_then(|attribute| raw(attribute, 6))
        .map(DiskCounter);
    let unit = match profile {
        Profile::Samsung | Profile::Micron => 512,
        Profile::Intel | Profile::KingstonKc600 => 32 * 1024 * 1024,
        _ => 1024 * 1024 * 1024,
    };
    let write_ids: &[u8] = match profile {
        Profile::Micron => &[246],
        Profile::Intel => &[241, 225],
        _ => &[241],
    };
    result.host_written_bytes = find(write_ids, AtaMetric::HostWritten)
        .and_then(|attribute| raw(attribute, 6))
        .map(|value| DiskCounter(value * unit));
    // These Micron families expose host writes but no total host reads.
    if !matches!(profile, Profile::Micron) {
        result.host_read_bytes = find(&[242], AtaMetric::HostRead)
            .and_then(|attribute| raw(attribute, 6))
            .map(|value| DiskCounter(value * unit));
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::decode;
    use crate::hardware::disk_details::{AtaAttribute, DiskDetails};
    use serde_json::json;

    fn attribute(id: u8, current: u8, raw: u64) -> AtaAttribute {
        AtaAttribute {
            id,
            flags: 0,
            current,
            worst: current,
            threshold: None,
            raw: raw.to_le_bytes()[..6].try_into().unwrap(),
            failing: false,
        }
    }

    fn details(model: &str, attributes: Vec<AtaAttribute>) -> DiskDetails {
        serde_json::from_value(json!({
            "index":1,"queriedAt":0,"volumes":[],"viaService":true,"queries":[],"solidState":true,
            "identity":{"model":model,"bus":"sata","busCode":11,"removable":false},
            "smartHealth":{"source":"ata","predictFailure":false,"attributes":attributes,"counters":[]}
        })).unwrap()
    }

    #[test]
    fn samsung_uses_normalized_life_and_512_byte_units_even_on_4k_disks() {
        let mut disk = details(
            "Samsung SSD 870 EVO 1TB",
            vec![
                attribute(177, 92, 284),
                attribute(241, 99, 1 << 40),
                attribute(9, 99, 12345),
                attribute(12, 99, 55),
                attribute(194, 65, 35 | (60 << 32)),
                attribute(235, 99, 7),
            ],
        );
        disk.logical_sector_bytes = Some(4096);
        let health = decode(&disk).unwrap();
        assert_eq!(health.remaining_life_percent, Some(92));
        assert_eq!(health.host_written_bytes.unwrap().0, 1 << 49);
        assert!(health.host_read_bytes.is_none());
        assert_eq!(health.power_on_hours.unwrap().0, 12345);
        assert_eq!(health.power_cycles.unwrap().0, 55);
        assert_eq!(health.temperature_celsius, Some(35.));
        assert_eq!(health.unsafe_shutdowns.unwrap().0, 7);
    }

    #[test]
    fn micron_distinguishes_consumed_raw_life_from_remaining_normalized_life() {
        let disk = details(
            "CT1000MX500SSD1",
            vec![
                attribute(202, 7, 93),
                attribute(246, 100, 4_000_000_000),
                attribute(242, 100, 123),
                attribute(174, 100, 8),
            ],
        );
        let health = decode(&disk).unwrap();
        assert_eq!(health.remaining_life_percent, Some(7));
        assert_eq!(health.host_written_bytes.unwrap().0, 2_048_000_000_000);
        assert!(health.host_read_bytes.is_none());
        assert_eq!(health.unsafe_shutdowns.unwrap().0, 8);
    }

    #[test]
    fn intel_uses_32_mib_counters_and_reported_spare_threshold() {
        let mut spare = attribute(232, 98, 500);
        spare.threshold = Some(10);
        let disk = details(
            "INTEL SSDSC2BB480G4",
            vec![
                attribute(233, 75, 0),
                spare,
                attribute(241, 100, 32),
                attribute(225, 100, 900),
                attribute(242, 100, 64),
            ],
        );
        let health = decode(&disk).unwrap();
        assert_eq!(health.remaining_life_percent, Some(75));
        assert_eq!(health.available_spare_percent, Some(98));
        assert_eq!(health.spare_threshold_percent, Some(10));
        assert_eq!(health.host_written_bytes.unwrap().0, 1 << 30);
        assert_eq!(health.host_read_bytes.unwrap().0, 2 << 30);
    }

    #[test]
    fn kingston_profiles_and_firmware_choose_distinct_units_and_life_fields() {
        for (model, unit, life) in [
            ("KINGSTON SA400S37480G", 1 << 30, 12),
            ("KINGSTON SUV400S37240G", 1 << 30, 88),
            ("KINGSTON SKC6001024G", 32 << 20, 88),
        ] {
            let disk = details(model, vec![attribute(231, 88, 12), attribute(241, 100, 10)]);
            let health = decode(&disk).unwrap();
            assert_eq!(health.remaining_life_percent, Some(life));
            assert_eq!(health.host_written_bytes.unwrap().0, 10 * unit);
        }
        let mut disk = details("KINGSTON  SA400S37240G", vec![attribute(231, 88, 12)]);
        disk.identity.as_mut().unwrap().firmware_revision = Some("03070009".into());
        assert_eq!(decode(&disk).unwrap().remaining_life_percent, Some(88));
    }

    #[test]
    fn wd_life_uses_the_documented_raw_byte_and_gib_counters() {
        let disk = details(
            "WDC  WDS100T2B0A-00SM50",
            vec![
                attribute(230, 100, 7 << 8 | 13),
                attribute(241, 100, 9000),
                attribute(242, 100, 12000),
            ],
        );
        let health = decode(&disk).unwrap();
        assert_eq!(health.remaining_life_percent, Some(93));
        assert_eq!(health.host_read_bytes.unwrap().0, 12000 << 30);
    }

    #[test]
    fn unknown_models_and_hdds_keep_vendor_attributes_uninterpreted() {
        for model in [
            "Generic SSD",
            "WDC WD40EZAZ",
            "Samsung SSD 990 PRO 2TB",
            "KINGSTON SNV2S1000G",
        ] {
            assert!(decode(&details(model, vec![attribute(231, 99, 99)])).is_none());
        }
        let mut disk = details("Samsung SSD 870 EVO 1TB", vec![attribute(177, 99, 9)]);
        disk.solid_state = Some(false);
        assert!(decode(&disk).is_none());
    }

    #[test]
    fn exhausted_life_is_zero_and_invalid_values_stay_missing() {
        let disk = details(
            "Samsung SSD 870 EVO 1TB",
            vec![attribute(177, 0, 4000), attribute(241, 100, 0)],
        );
        let health = decode(&disk).unwrap();
        assert_eq!(health.remaining_life_percent, Some(0));
        assert_eq!(health.host_written_bytes.unwrap().0, 0);
        let disk = details(
            "Samsung SSD 870 EVO 1TB",
            vec![
                attribute(177, 255, 0),
                attribute(241, 100, (1 << 48) - 1),
                attribute(194, 100, 255),
            ],
        );
        let health = decode(&disk).unwrap();
        assert!(health.remaining_life_percent.is_none());
        assert!(health.host_written_bytes.is_none());
        assert!(health.temperature_celsius.is_none());
    }

    #[test]
    fn bridge_identity_and_old_service_replies_are_supported() {
        let mut disk = details("USB bridge", vec![attribute(177, 91, 500)]);
        disk.ata_identity = Some(
            serde_json::from_value(json!({
                "model":"Samsung SSD 860 EVO 1TB", "solidState":true
            }))
            .unwrap(),
        );
        assert!(disk.ata_health.is_none());
        disk.refresh_ata_health();
        assert_eq!(disk.ata_health.unwrap().remaining_life_percent, Some(91));
    }
}
