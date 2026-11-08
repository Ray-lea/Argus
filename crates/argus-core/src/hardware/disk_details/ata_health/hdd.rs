//! Rotating ATA disks: hours and sector/error counters, without SSD wear estimates.
use super::{AtaHealth, AtaMetric, AtaMetricSource, DiskCounter, DiskDetails, raw};

struct Profile {
    hours_divisor: u128,
    temperature_divisor: Option<f32>,
    host_sectors: bool,
}

fn profile(model: &str) -> Option<Profile> {
    let model = model.to_ascii_uppercase();
    let mut profile = Profile {
        hours_divisor: 1,
        temperature_divisor: Some(1.),
        host_sectors: false,
    };
    let bare = model.strip_prefix("SEAGATE ").unwrap_or(&model);
    if bare.strip_prefix("ST").is_some_and(|part| {
        let family = part.trim_start_matches(|c: char| c.is_ascii_digit());
        family.len() < part.len()
            && ["DM", "VN", "NM", "LM", "DX", "DL", "AS", "NS", "A"]
                .iter()
                .any(|prefix| family.starts_with(prefix))
    }) {
        // Convert the documented 512-byte LBA counters only on 512-byte
        // logical disks. Other sector formats retain their raw attributes.
        profile.host_sectors = true;
        return Some(profile);
    }
    let wd = model.strip_prefix("WDC ").unwrap_or(&model).trim_start();
    if wd.starts_with("WUH72") {
        return Some(profile);
    }
    if wd
        .strip_prefix("WD")
        .is_some_and(|part| part.starts_with(|c: char| c.is_ascii_digit()))
    {
        return Some(profile);
    }
    let hitachi = model
        .strip_prefix("HGST ")
        .or_else(|| model.strip_prefix("HITACHI "))
        .unwrap_or(&model)
        .trim_start();
    if ["HDS", "HDT", "HTS", "HUS", "HUH", "HCC", "HUA"]
        .iter()
        .any(|prefix| hitachi.starts_with(prefix))
    {
        return Some(profile);
    }
    if let Some(part) = model.strip_prefix("TOSHIBA ")
        && [
            "DT01", "DT02", "DT03", "HDW", "HDK", "HDN", "MG0", "MG1", "MD0", "MQ0", "MK",
        ]
        .iter()
        .any(|prefix| part.starts_with(prefix))
    {
        return Some(profile);
    }
    if let Some(part) = model.strip_prefix("SAMSUNG ") {
        if ["HD", "HM", "HN-"]
            .iter()
            .any(|prefix| part.starts_with(prefix))
        {
            // HM early generations use half-minutes; restrict hours to the
            // documented modern SpinPoint SATA families.
            if part.starts_with("HM") {
                if part.ends_with("JI") || part.ends_with("UI") || part.ends_with("HI") {
                    return Some(profile);
                }
                return None;
            }
            return Some(profile);
        }
        if ["SV4012H", "SV0412H", "SV1204H", "SP40A2H"].contains(&part)
            || part.starts_with("SV") && part.ends_with('N')
        {
            profile.hours_divisor = 120;
            if matches!(part, "SV0412H" | "SV1204H") {
                profile.temperature_divisor = Some(10.);
            }
            return Some(profile);
        }
    }
    if let Some(part) = model.strip_prefix("MAXTOR ") {
        if ["6Y", "6B", "6L", "4D", "4G", "5T", "5A", "7Y", "7B", "7L"]
            .iter()
            .any(|prefix| part.starts_with(prefix))
        {
            profile.hours_divisor = if part.starts_with("6L0") { 1 } else { 60 };
            // D540X-4G does not expose a Celsius temperature in attribute 194.
            if part.starts_with("4G") {
                profile.temperature_divisor = None;
            }
            return Some(profile);
        }
        if part.starts_with("STM")
            || part.starts_with("6H")
            || part.starts_with("7H500")
            || part.starts_with("4K")
        {
            return Some(profile);
        }
    }
    if let Some(part) = model.strip_prefix("FUJITSU ") {
        if [
            "MHG", "MHH", "MHJ", "MHK", "MHL", "MHM", "MHN", "MHR", "MHS", "MHT", "MHU", "MHV",
            "MPA", "MPB", "MPC", "MPD", "MPE", "MPF", "MPG",
        ]
        .iter()
        .any(|prefix| part.starts_with(prefix))
        {
            profile.hours_divisor = 3600;
            return Some(profile);
        }
        if part.starts_with("MHW") && part.ends_with("AC") {
            profile.hours_divisor = 60;
            return Some(profile);
        }
        if part.starts_with("MHZ") && part.contains("FFS G2") {
            profile.hours_divisor = 60;
            return Some(profile);
        }
        if part.starts_with("MHY")
            || part.starts_with("MJA")
            || part.starts_with("MHZ")
            || part.starts_with("MHW") && part.contains("BH")
        {
            return Some(profile);
        }
    }
    if model.starts_with("HITACHI_DK") {
        profile.hours_divisor = 60;
        return Some(profile);
    }
    None
}

pub(super) fn decode(details: &DiskDetails, model: &str) -> Option<AtaHealth> {
    let profile = profile(model)?;
    let attributes = &details.smart_health.as_ref()?.attributes;
    let mut result = AtaHealth {
        rotational: true,
        ..Default::default()
    };
    let mut read = |ids: &[u8], metric, bytes| {
        let attribute = ids
            .iter()
            .find_map(|id| attributes.iter().find(|a| a.id == *id))?;
        result.attributes.push(AtaMetricSource {
            id: attribute.id,
            metric,
        });
        raw(attribute, bytes)
    };
    result.temperature_celsius = profile.temperature_divisor.and_then(|divisor| {
        read(
            &[194, 190],
            AtaMetric::Temperature,
            if divisor == 10. { 2 } else { 1 },
        )
        .map(|value| value as f32 / divisor)
        .filter(|value| (0. ..=125.).contains(value))
    });
    // Standard HDD hours occupy the low 24 bits; upper bytes can carry
    // vendor auxiliary data. Legacy sub-hour counters use a 32-bit value.
    result.power_on_hours = read(
        &[9],
        AtaMetric::PowerOnHours,
        if profile.hours_divisor == 1 { 3 } else { 4 },
    )
    .map(|value| DiskCounter(value / profile.hours_divisor));
    result.power_cycles = read(&[12], AtaMetric::PowerCycles, 4).map(DiskCounter);
    result.start_stop_count = read(&[4], AtaMetric::StartStopCount, 4).map(DiskCounter);
    result.load_cycle_count = read(&[193], AtaMetric::LoadCycleCount, 4).map(DiskCounter);
    // Attribute 5 commonly packs other counters above the low 16-bit count.
    result.reallocated_sectors = read(&[5], AtaMetric::ReallocatedSectors, 2).map(DiskCounter);
    result.pending_sectors = read(&[197], AtaMetric::PendingSectors, 6).map(DiskCounter);
    result.offline_uncorrectable =
        read(&[198], AtaMetric::OfflineUncorrectable, 6).map(DiskCounter);
    result.interface_crc_errors = read(&[199], AtaMetric::InterfaceCrcErrors, 6).map(DiskCounter);
    if profile.host_sectors
        && let Some(sector_bytes) = details.logical_sector_bytes.filter(|size| *size == 512)
    {
        result.host_read_bytes = read(&[242], AtaMetric::HostRead, 6)
            .map(|value| DiskCounter(value * u128::from(sector_bytes)));
        result.host_written_bytes = read(&[241], AtaMetric::HostWritten, 6)
            .map(|value| DiskCounter(value * u128::from(sector_bytes)));
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::super::decode;
    use super::super::tests::{attribute, details};

    #[test]
    fn rotating_disks_expose_counts_without_fabricating_endurance_or_io_totals() {
        for model in [
            "WDC WD40EZAZ-00SF3B0",
            "ST8000VN004-2M2101",
            "TOSHIBA MG08ACA16TE",
            "HGST HUS726060ALE610",
            "HGST HUH721212ALE600",
            "WDC  WUH721818ALE6L4",
        ] {
            let mut disk = details(
                model,
                vec![
                    attribute(9, 99, (0x40 << 24) | 12345),
                    attribute(12, 100, 88),
                    attribute(4, 100, 97),
                    attribute(193, 99, 850),
                    attribute(5, 100, 0x120000_000003),
                    attribute(197, 100, 2),
                    attribute(198, 100, 1),
                    attribute(199, 200, 6),
                    attribute(1, 80, 0x123456789),
                    attribute(7, 90, 0x123456789),
                ],
            );
            disk.solid_state = Some(false);
            let health = decode(&disk).unwrap();
            assert!(health.rotational);
            assert!(health.remaining_life_percent.is_none());
            assert_eq!(health.power_on_hours.unwrap().0, 12345);
            assert_eq!(health.reallocated_sectors.unwrap().0, 3);
            assert_eq!(health.pending_sectors.unwrap().0, 2);
            assert_eq!(health.offline_uncorrectable.unwrap().0, 1);
            assert_eq!(health.start_stop_count.unwrap().0, 97);
            assert!(health.host_written_bytes.is_none());
            assert!(health.uncorrectable_errors.is_none());
        }
    }

    #[test]
    fn legacy_ata_hours_and_temperature_use_family_specific_units() {
        for (model, ticks) in [
            ("Maxtor 6Y080L0", 600),
            ("SAMSUNG SV0412H", 1200),
            ("FUJITSU MHT2040AT", 36000),
        ] {
            let mut disk = details(
                model,
                vec![attribute(9, 99, ticks), attribute(194, 100, 350)],
            );
            disk.solid_state = None;
            let health = decode(&disk).unwrap();
            assert_eq!(health.power_on_hours.unwrap().0, 10);
            if model.starts_with("SAMSUNG") {
                assert_eq!(health.temperature_celsius, Some(35.));
            }
        }
    }

    #[test]
    fn seagate_host_totals_require_a_known_logical_sector_size() {
        let mut disk = details(
            "ST16000NM001G-2KK103",
            vec![
                attribute(241, 100, 1_000_000),
                attribute(242, 100, 2_000_000),
            ],
        );
        disk.solid_state = Some(false);
        assert!(decode(&disk).unwrap().host_written_bytes.is_none());
        disk.logical_sector_bytes = Some(512);
        assert_eq!(
            decode(&disk).unwrap().host_written_bytes.unwrap().0,
            512_000_000
        );
        disk.logical_sector_bytes = Some(4096);
        assert!(decode(&disk).unwrap().host_written_bytes.is_none());
        disk.solid_state = Some(true);
        assert!(decode(&disk).is_none());
    }
}
