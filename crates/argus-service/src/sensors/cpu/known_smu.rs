//! Exact-version fields with published hardware measurements. The pinned
//! provenance and license live in vendor/cpu-layouts/manifest.json.
use super::{Identity, telemetry::CoreSampler, valid};
use argus_core::hardware::{CpuMetricKind as Kind, CpuSensorSource as Source, CpuSensors};

pub(super) fn eligible(version: u32, id: Identity, cores: &[CoreSampler]) -> bool {
    let profile = match version {
        0x620205 => Some((0x1a, 0x40..=0x4f, 16, 16)),
        0x620105 => Some((0x1a, 0x40..=0x4f, 8, 8)),
        _ => None,
    };
    profile.is_none_or(|(family, models, count, slots)| {
        !id.intel
            && id.family == family
            && models.contains(&id.model)
            && cores.len() == count
            && cores.iter().all(|c| c.smu_slot.is_some_and(|s| s < slots))
    })
}

pub(super) fn decode(version: u32, data: &[f32], frame: &mut CpuSensors, cores: &[CoreSampler]) {
    if version == 0x380905
        && (cores.len() != 6 || cores.iter().any(|c| c.smu_slot.is_none_or(|s| s >= 8)))
    {
        return;
    }
    let field = |index: usize, low, high| data.get(index).and_then(|v| valid(*v as f64, low, high));
    let globals: &[(Kind, usize, f64, f64)] = match version {
        0x620205 => &[
            (Kind::PptLimitWatts, 2, 1., 2000.),
            (Kind::PptWatts, 3, 0., 2000.),
            (Kind::TdcLimitAmps, 8, 1., 2000.),
            (Kind::TdcAmps, 9, 0., 2000.),
            (Kind::ThermalLimitCelsius, 10, 40., 125.),
            (Kind::EdcLimitAmps, 63, 1., 2000.),
            (Kind::FabricMhz, 71, 50., 10000.),
            (Kind::UncoreMhz, 75, 50., 10000.),
            (Kind::MemoryMhz, 79, 50., 10000.),
            (Kind::SocVoltageSetpoint, 83, 0.1, 2.5),
            (Kind::MiscVoltage, 58, 0.1, 2.5),
            (Kind::VddgIodVolts, 259, 0.1, 2.),
            (Kind::VddgCcdVolts, 261, 0.1, 2.),
            (Kind::VddpVolts, 269, 0.1, 2.),
            (Kind::SocWatts, 21, 0., 500.),
            (Kind::VddioWatts, 22, 0., 500.),
            (Kind::Vdd18Watts, 23, 0., 500.),
        ],
        0x620105 => &[
            (Kind::PptLimitWatts, 2, 1., 2000.),
            (Kind::PptWatts, 3, 0., 2000.),
            (Kind::TdcLimitAmps, 8, 1., 2000.),
            (Kind::TdcAmps, 9, 0., 2000.),
            (Kind::ThermalLimitCelsius, 10, 40., 125.),
            (Kind::EdcLimitAmps, 63, 1., 2000.),
            (Kind::FabricMhz, 71, 50., 10000.),
            (Kind::UncoreMhz, 75, 50., 10000.),
            (Kind::MemoryMhz, 79, 50., 10000.),
            (Kind::SocVoltageSetpoint, 83, 0.1, 2.5),
            (Kind::MemoryIoVolts, 58, 0.1, 2.5),
            (Kind::SocWatts, 21, 0., 500.),
            (Kind::GraphicsWatts, 107, 0., 500.),
            (Kind::GraphicsMhz, 108, 1., 5000.),
        ],
        _ => &[],
    };
    for &(kind, index, low, high) in globals {
        frame.metric(kind, field(index, low, high), Source::Smu);
    }
    // These two tables share a header, but not every rail identity. In
    // particular d[64] (EDC), the SVI3/VRM block and L3 candidates are not
    // promoted to established telemetry from plausible values alone. d[18]
    // is also omitted: it has not been established as a hard VID ceiling.
    let profile = match version {
        0x620205 => (333, 317, 381, 397, 413, None),
        0x620105 => (317, 309, 341, 349, 357, Some(325)),
        0x380905 => (188, 180, 228, 236, 244, Some(212)),
        _ => return,
    };
    if version == 0x620205 {
        frame.core_vid = field(19, 0.1, 2.);
    } else if version == 0x620105 {
        frame.core_vid = field(269, 0.1, 2.);
    }
    for reading in &mut frame.cores {
        let Some(slot) = cores
            .iter()
            .find(|c| c.core == reading.core)
            .and_then(|c| c.smu_slot)
        else {
            continue;
        };
        reading.celsius = field(profile.0 + slot, 1., 125.);
        reading.voltage = field(profile.1 + slot, 0.1, 2.);
        let c0 = field(profile.2 + slot, 0., 100.);
        let c1 = field(profile.3 + slot, 0., 100.);
        let c6 = field(profile.4 + slot, 0., 100.);
        // An intact residency triplet sums to approximately 100%. Reject
        // partial/zero tables instead of displaying convincing false idle data.
        if let (Some(c0), Some(c1), Some(c6)) = (c0, c1, c6)
            && (98.0..=102.0).contains(&(c0 + c1 + c6))
        {
            reading.smu_c0_percent = Some(c0);
            reading.c1_percent = Some(c1);
            reading.c6_percent = Some(c6);
        }
        if let Some(base) = profile.5 {
            reading.clock_mhz = reading
                .clock_mhz
                .or_else(|| field(base + slot, 0.1, 12.).map(|v| v * 1000.));
        }
        // 0x620205 d[349..364] is FIT-related, not live frequency. Keep the
        // independent MSR/Windows clocks and energy counters on this profile.
    }
    let volts: Option<Vec<_>> = frame.cores.iter().map(|c| c.voltage).collect();
    frame.metric(
        Kind::CoreSmuVoltage,
        volts
            .filter(|v| !v.is_empty() && v.len() == cores.len())
            .map(|v| v.iter().sum::<f32>() / v.len() as f32),
        Source::Smu,
    );
}

pub(super) fn percentages(frame: &mut CpuSensors) {
    for (value, limit, kind) in [
        (Kind::PptWatts, Kind::PptLimitWatts, Kind::PptPercent),
        (Kind::TdcAmps, Kind::TdcLimitAmps, Kind::TdcPercent),
        (Kind::EdcAmps, Kind::EdcLimitAmps, Kind::EdcPercent),
    ] {
        let percent = frame
            .value(value)
            .zip(frame.value(limit))
            .filter(|(_, limit)| *limit > 0.)
            .and_then(|(v, l)| valid(f64::from(v / l * 100.), 0., 1000.));
        frame.metric(kind, percent, Source::Smu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_core::hardware::CpuCoreSensors;
    #[test]
    fn measured_vermeer_fixtures_preserve_disabled_core_slots_and_load_response() {
        let cores: Vec<_> = [0, 1, 4, 5, 6, 7]
            .into_iter()
            .enumerate()
            .map(|(core, slot)| CoreSampler::fixture(core as u32, slot, core as u8 * 2))
            .collect();
        let read = |bytes: &[u8]| {
            let data: Vec<_> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|v| f32::from_le_bytes(*v))
                .collect();
            let mut frame = CpuSensors {
                cores: (0..6)
                    .map(|core| CpuCoreSensors {
                        core,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            };
            decode(0x380905, &data, &mut frame, &cores);
            frame
        };
        let idle = read(include_bytes!(
            "../../../vendor/cpu-layouts/fixtures/vermeer-idle_01.bin"
        ));
        let busy = read(include_bytes!(
            "../../../vendor/cpu-layouts/fixtures/vermeer-core0_load.bin"
        ));
        assert!(
            idle.cores
                .iter()
                .all(|c| c.celsius.is_some() && c.c6_percent.is_some())
        );
        assert!((idle.cores[2].celsius.unwrap() - 38.18113).abs() < 0.001);
        assert!(busy.cores[0].celsius.unwrap() - idle.cores[0].celsius.unwrap() > 20.);
        assert!(busy.cores[0].smu_c0_percent.unwrap() > 80.);
        assert!(busy.cores[0].c6_percent.unwrap() < 15.);
        assert!(idle.cores[0].c6_percent.unwrap() > 60.);
    }
    #[test]
    fn granite_620205_uses_its_own_header_and_never_treats_fit_as_a_clock() {
        let cores: Vec<_> = (0..16)
            .map(|i| CoreSampler::fixture(i, i as usize, i as u8 * 2))
            .collect();
        let mut frame = CpuSensors {
            cores: (0..16)
                .map(|core| CpuCoreSensors {
                    core,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let mut raw = vec![f32::NAN; 613];
        raw[0] = 999.;
        raw[1] = 999.;
        raw[2] = 230.;
        raw[3] = 115.;
        raw[8] = 160.;
        raw[9] = 80.;
        raw[63] = 225.;
        raw[64] = 100.;
        raw[71] = 2000.;
        raw[75] = 3000.;
        raw[79] = 3000.;
        raw[83] = 1.2;
        for slot in 0..16 {
            raw[317 + slot] = 1.1;
            raw[333 + slot] = 50. + slot as f32;
            raw[349 + slot] = 5.;
            raw[381 + slot] = 20.;
            raw[397 + slot] = 10.;
            raw[413 + slot] = 70.;
        }
        decode(0x620205, &raw, &mut frame, &cores);
        percentages(&mut frame);
        assert_eq!(frame.value(Kind::PptWatts), Some(115.));
        assert_eq!(frame.value(Kind::PptPercent), Some(50.));
        assert_eq!(frame.value(Kind::EdcAmps), None);
        assert_eq!(frame.value(Kind::SocVoltageSetpoint), Some(1.2));
        assert_eq!(frame.value(Kind::SocVoltage), None);
        assert_eq!(frame.cores[8].celsius, Some(58.));
        assert_eq!(frame.cores[8].c6_percent, Some(70.));
        assert_eq!(frame.cores[8].clock_mhz, None);
        assert_eq!(frame.cores[8].vid, None);
        let id = Identity {
            intel: false,
            family: 0x1a,
            model: 0x44,
            thermal: 0,
        };
        assert!(eligible(0x620205, id, &cores));
        assert!(!eligible(0x620205, id, &cores[..8]));
        assert!(!eligible(0x620205, Identity { family: 0x19, ..id }, &cores));
    }
}
