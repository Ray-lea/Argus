//! Fixed SMU telemetry calls only. Table layouts are exact-version matches;
//! no arbitrary mailbox command, address or physical-memory access is exposed.
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use argus_core::hardware::{
    CoreTemperature, CpuMetricKind as Kind, CpuSensorSource as Source, CpuSensors, SmuState,
};
use serde::Deserialize;

use super::telemetry::CoreSampler;
use super::{Identity, Module, PciGuard, known_smu, valid};

const BLOB: &[u8] = include_bytes!("../../../vendor/pawnio/RyzenSMU.bin");

#[derive(Debug, Deserialize)]
struct Layout {
    version: u32,
    bytes: usize,
    fclk: i32,
    uclk: i32,
    mclk: i32,
    soc_voltage: i32,
    vddp: i32,
    vddg_iod: i32,
    vddg_ccd: i32,
    misc_voltage: i32,
    svi3: i32,
    stride: i32,
    rails: usize,
}

fn layouts() -> &'static [Layout] {
    static VALUE: OnceLock<Vec<Layout>> = OnceLock::new();
    VALUE.get_or_init(|| {
        serde_json::from_str(include_str!("../../../vendor/cpu-layouts/amd-smu.json"))
            .expect("embedded SMU layouts")
    })
}

#[derive(Default)]
pub(super) struct Smu {
    module: Option<Module>,
    version: Option<u32>,
    attempt: Option<Instant>,
    resolved: Option<Instant>,
    cached: Option<CachedTable>,
}

struct CachedTable {
    at: Instant,
    version: u32,
    values: Vec<f32>,
}

impl Smu {
    pub fn sample(&mut self, frame: &mut CpuSensors, cores: &[CoreSampler], id: Identity) {
        frame.smu_table_version = self.version;
        frame.smu_state = Some(SmuState::Pending);
        let Some(_pci) = PciGuard::acquire() else {
            self.apply_cached(frame, cores);
            return;
        };
        if self.module.is_none() {
            if self
                .attempt
                .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
            {
                frame.smu_state = Some(SmuState::Unavailable);
                return;
            }
            self.attempt = Some(Instant::now());
            self.module = Module::open(BLOB).ok();
        }
        let Some(module) = &self.module else {
            frame.smu_state = Some(SmuState::Unavailable);
            return;
        };
        if self
            .resolved
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(30))
        {
            let mut info = [0u64; 2];
            if !module.call(b"ioctl_resolve_pm_table", &[], &mut info) || info[0] > u32::MAX as u64
            {
                self.module = None;
                self.resolved = None;
                self.version = None;
                self.cached = None;
                frame.smu_state = Some(SmuState::Unavailable);
                frame.smu_table_version = None;
                return;
            }
            self.version = Some(info[0] as u32);
            self.resolved = Some(Instant::now());
        }
        frame.smu_table_version = self.version;
        let Some(layout) = layouts().iter().find(|l| Some(l.version) == self.version) else {
            self.cached = None;
            frame.smu_state = Some(SmuState::UnsupportedTable);
            return;
        };
        if !known_smu::eligible(layout.version, id, cores) {
            self.cached = None;
            frame.smu_state = Some(SmuState::UnsupportedTable);
            return;
        }
        // The signed module maps at most one page. Large Threadripper tables
        // still have their scalar rail/clock fields within this first page.
        let cells = layout.bytes.min(4096).div_ceil(8);
        let mut raw = [0u64; 512];
        if !module.call(b"ioctl_update_pm_table", &[], &mut [])
            || !module.call(b"ioctl_read_pm_table", &[], &mut raw[..cells])
        {
            frame.smu_state = Some(SmuState::Unavailable);
            self.apply_cached(frame, cores);
            return;
        }
        let values: Vec<f32> = raw[..cells]
            .iter()
            .flat_map(|v| [f32::from_bits(*v as u32), f32::from_bits((v >> 32) as u32)])
            .collect();
        if !values.iter().any(|v| v.is_finite() && *v != 0.0) {
            frame.smu_state = Some(SmuState::Unavailable);
            self.apply_cached(frame, cores);
            return;
        }
        decode(layout, &values, frame, cores);
        frame.smu_state = Some(SmuState::Ready);
        self.cached = Some(CachedTable {
            at: Instant::now(),
            version: layout.version,
            values,
        });
    }

    fn apply_cached(&self, frame: &mut CpuSensors, cores: &[CoreSampler]) -> bool {
        let Some(cached) = &self.cached else {
            return false;
        };
        if cached.at.elapsed() >= Duration::from_secs(3) {
            return false;
        }
        let Some(layout) = layouts().iter().find(|l| l.version == cached.version) else {
            return false;
        };
        decode(layout, &cached.values, frame, cores);
        frame.smu_table_version = Some(cached.version);
        frame.smu_state = Some(SmuState::Ready);
        true
    }
}

fn at(table: &[f32], offset: i32, low: f64, high: f64) -> Option<f32> {
    if offset < 0 || offset % 4 != 0 {
        return None;
    }
    table
        .get(offset as usize / 4)
        .and_then(|v| valid(*v as f64, low, high))
}

fn decode(layout: &Layout, table: &[f32], frame: &mut CpuSensors, cores: &[CoreSampler]) {
    for (kind, offset, low, high) in [
        (Kind::FabricMhz, layout.fclk, 50.0, 10000.0),
        (Kind::UncoreMhz, layout.uclk, 50.0, 10000.0),
        (Kind::MemoryMhz, layout.mclk, 50.0, 10000.0),
        (Kind::SocVoltage, layout.soc_voltage, 0.1, 2.5),
        (Kind::MiscVoltage, layout.misc_voltage, 0.1, 2.5),
        (Kind::VddpVolts, layout.vddp, 0.1, 2.0),
        (Kind::VddgIodVolts, layout.vddg_iod, 0.1, 2.0),
        (Kind::VddgCcdVolts, layout.vddg_ccd, 0.1, 2.0),
    ] {
        frame.metric(kind, at(table, offset, low, high), Source::Smu);
    }
    if layout.svi3 >= 0 {
        let rails = [
            [
                Kind::CoreRailVid,
                Kind::CoreVoltage,
                Kind::CoreCurrent,
                Kind::CoreVrmCelsius,
                Kind::CoreRailWatts,
            ],
            [
                Kind::SecondCoreRailVid,
                Kind::SecondCoreVoltage,
                Kind::SecondCoreCurrent,
                Kind::SecondCoreVrmCelsius,
                Kind::SecondCoreRailWatts,
            ],
            [
                Kind::SocVid,
                Kind::SocVoltage,
                Kind::SocCurrent,
                Kind::SocVrmCelsius,
                Kind::SocRailWatts,
            ],
            [
                Kind::MiscVid,
                Kind::MiscVoltage,
                Kind::MiscCurrent,
                Kind::MiscVrmCelsius,
                Kind::MiscRailWatts,
            ],
        ];
        let order: &[usize] = match layout.rails {
            2 => &[0, 2],
            3 => &[0, 2, 3],
            4 => &[0, 1, 2, 3],
            _ => &[],
        };
        for (index, rail) in order.iter().enumerate() {
            let offset = layout.svi3 + index as i32 * layout.stride;
            let kinds = rails[*rail];
            let volts = at(table, offset + 4, 0.1, 2.5);
            let amps = at(table, offset + 8, 0.0, 2000.0);
            frame.metric(kinds[0], at(table, offset, 0.1, 2.5), Source::Svi3);
            frame.metric(kinds[1], volts, Source::Svi3);
            frame.metric(kinds[2], amps, Source::Svi3);
            if layout.stride == 20 {
                frame.metric(kinds[3], at(table, offset + 16, 1.0, 150.0), Source::Svi3);
            }
            frame.metric(
                kinds[4],
                volts
                    .zip(amps)
                    .and_then(|(v, a)| valid((v * a) as f64, 0.0, 2000.0)),
                Source::Svi3,
            );
        }
    }
    // Named fields corroborated by the pinned LibreHardwareMonitor decoder.
    let fields: &[(Kind, usize, f64, f64)] = match layout.version {
        0x621202 => &[
            (Kind::PptLimitWatts, 0, 1.0, 2000.0),
            (Kind::PptWatts, 1, 0.0, 2000.0),
            (Kind::PptFastLimitWatts, 2, 1.0, 2000.0),
            (Kind::PptSlowWatts, 3, 0.0, 2000.0),
            (Kind::PptFastWatts, 26, 0.0, 2000.0),
            (Kind::TdcLimitAmps, 8, 1.0, 2000.0),
            (Kind::TdcAmps, 9, 0.0, 2000.0),
            (Kind::EdcLimitAmps, 63, 1.0, 2000.0),
            (Kind::EdcAmps, 51, 0.0, 2000.0),
            (Kind::ThermalLimitCelsius, 10, 40.0, 125.0),
            (Kind::SocWatts, 21, 0.0, 500.0),
            (Kind::L3Ccd1Celsius, 595, 1.0, 125.0),
            (Kind::L3Ccd2Celsius, 596, 1.0, 125.0),
        ],
        0x540004 => &[
            (Kind::PptWatts, 3, 0.0, 2000.0),
            (Kind::CoresWatts, 20, 0.0, 2000.0),
            (Kind::SocWatts, 21, 0.0, 500.0),
            (Kind::MiscWatts, 22, 0.0, 500.0),
            (Kind::TdcAmps, 48, 0.0, 2000.0),
            (Kind::EdcAmps, 49, 0.0, 2000.0),
            (Kind::IoDieCelsius, 211, 1.0, 125.0),
            (Kind::L3Ccd1Celsius, 539, 1.0, 125.0),
            (Kind::L3Ccd2Celsius, 540, 1.0, 125.0),
        ],
        0x380805 => &[
            (Kind::TdcAmps, 3, 0.0, 1000.0),
            (Kind::SocCelsius, 127, 1.0, 125.0),
        ],
        0x240903 => &[
            (Kind::TdcAmps, 15, 0.0, 1000.0),
            (Kind::EdcAmps, 21, 0.0, 1000.0),
            (Kind::SocCelsius, 115, 1.0, 125.0),
        ],
        0x1e0004 => &[
            (Kind::TdcAmps, 7, 0.0, 1000.0),
            (Kind::EdcAmps, 11, 0.0, 1000.0),
            (Kind::SocCurrent, 66, 0.0, 1000.0),
            (Kind::SocWatts, 67, 0.0, 500.0),
        ],
        _ => &[],
    };
    for &(kind, index, low, high) in fields {
        if kind == Kind::CoresWatts && frame.value(kind).is_some() {
            continue;
        }
        frame.metric(kind, at(table, index as i32 * 4, low, high), Source::Smu);
    }
    known_smu::decode(layout.version, table, frame, cores);
    known_smu::percentages(frame);
    for reading in &mut frame.cores {
        let Some(core) = cores.iter().find(|c| c.core == reading.core) else {
            continue;
        };
        let Some(slot) = core.smu_slot else { continue };
        if layout.version == 0x621202 && slot < 16 {
            reading.celsius = at(table, (333 + slot) as i32 * 4, 1.0, 125.0);
            reading.vid = at(table, (317 + slot) as i32 * 4, 0.1, 2.0);
            reading.watts = reading
                .watts
                .or_else(|| at(table, (301 + slot) as i32 * 4, 0.0, 300.0));
            reading.clock_mhz = reading
                .clock_mhz
                .or_else(|| at(table, (349 + slot) as i32 * 4, 0.1, 12.0).map(|v| v * 1000.0));
        } else if layout.version == 0x1e0004 && slot < 4 {
            reading.celsius = at(table, (108 + slot) as i32 * 4, 1.0, 125.0);
        }
        if let Some(celsius) = reading.celsius {
            for (group, processor) in core.processors() {
                frame.core_temperatures.push(CoreTemperature {
                    group,
                    processor,
                    celsius,
                });
            }
        }
    }
    frame.core_voltage = frame.value(Kind::CoreVoltage).or(frame.core_voltage);
    frame.core_vid = frame
        .cores
        .iter()
        .filter_map(|c| c.vid)
        .reduce(f32::max)
        .or(frame.core_vid);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_bus_contention_keeps_readings_but_expired_cache_is_not_reused() {
        let mut values = vec![f32::NAN; 613];
        values[49] = 1.21;
        let mut smu = Smu {
            cached: Some(CachedTable {
                at: Instant::now(),
                version: 0x621202,
                values,
            }),
            ..Default::default()
        };
        let mut frame = CpuSensors::default();
        assert!(smu.apply_cached(&mut frame, &[]));
        assert_eq!(frame.core_voltage, Some(1.21));
        smu.cached.as_mut().unwrap().at = Instant::now() - Duration::from_secs(4);
        let mut frame = CpuSensors::default();
        assert!(!smu.apply_cached(&mut frame, &[]));
        assert!(frame.core_voltage.is_none());
    }

    #[test]
    fn disabled_core_gaps_do_not_shift_smu_temperature_or_vid_to_another_core() {
        use argus_core::hardware::CpuCoreSensors;
        let layout = layouts().iter().find(|l| l.version == 0x621202).unwrap();
        let mut table = vec![f32::NAN; layout.bytes.div_ceil(4)];
        table[333] = 54.0;
        table[341] = 47.0;
        table[317] = 1.20;
        table[325] = 1.10;
        let cores = [CoreSampler::fixture(0, 0, 0), CoreSampler::fixture(1, 8, 2)];
        let mut frame = CpuSensors {
            cores: vec![
                CpuCoreSensors::default(),
                CpuCoreSensors {
                    core: 1,
                    processor: 2,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        decode(layout, &table, &mut frame, &cores);
        assert_eq!(frame.cores[0].celsius, Some(54.0));
        assert_eq!(frame.cores[1].celsius, Some(47.0));
        assert_eq!(frame.cores[1].vid, Some(1.10));
        assert_eq!(frame.core_temperatures[1].processor, 2);
        assert_eq!(frame.core_temperatures[1].celsius, 47.0);
    }

    #[test]
    fn table_layouts_are_bounded_unique_and_never_fall_back_to_candidates() {
        let mut versions = std::collections::HashSet::new();
        for layout in layouts() {
            assert!(versions.insert(layout.version));
            assert!((64..=64 * 1024).contains(&layout.bytes));
            for offset in [
                layout.fclk,
                layout.uclk,
                layout.mclk,
                layout.soc_voltage,
                layout.vddp,
                layout.vddg_iod,
                layout.vddg_ccd,
                layout.misc_voltage,
            ] {
                assert!(
                    offset == -1
                        || (offset >= 0
                            && offset % 4 == 0
                            && (offset as usize + 4) <= layout.bytes.min(4096))
                );
            }
            assert!(matches!(layout.stride, 0 | 16 | 20));
        }
        assert!(
            !layouts()
                .iter()
                .any(|l| matches!(l.version, 0x620 | 0x650005 | 0x621201))
        );
    }

    #[test]
    fn zen5_svi3_separates_measured_voltage_vid_current_and_temperature() {
        let layout = layouts().iter().find(|l| l.version == 0x621202).unwrap();
        let mut table = vec![f32::NAN; layout.bytes.div_ceil(4)];
        table[0] = 230.0;
        table[1] = 101.0;
        table[8] = 160.0;
        table[9] = 81.0;
        table[63] = 225.0;
        table[10] = 95.0;
        table[48] = 1.25;
        table[49] = 1.21;
        table[50] = 82.0;
        table[51] = 110.0;
        table[52] = 45.0;
        table[53] = 1.20;
        table[54] = 1.18;
        table[55] = 12.0;
        table[57] = 42.0;
        table[58] = 1.10;
        table[59] = 1.08;
        table[60] = 2.0;
        table[62] = 39.0;
        let mut frame = CpuSensors::default();
        decode(layout, &table, &mut frame, &[]);
        assert_eq!(frame.core_voltage, Some(1.21));
        assert_eq!(frame.value(Kind::CoreRailVid), Some(1.25));
        assert_eq!(frame.value(Kind::CoreVrmCelsius), Some(45.0));
        assert_eq!(frame.value(Kind::SocVoltage), Some(1.18));
        assert_eq!(frame.value(Kind::MiscVoltage), Some(1.08));
        assert_eq!(frame.value(Kind::PptLimitWatts), Some(230.0));
        assert_eq!(frame.value(Kind::EdcAmps), Some(110.0));
        assert_eq!(frame.value(Kind::MemoryMhz), None);
        assert!(frame.metrics.iter().all(|m| m.value.is_finite()));
    }
}
