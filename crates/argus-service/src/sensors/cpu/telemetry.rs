use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

use argus_core::hardware::{
    CpuCoreSensors, CpuInfo, CpuMetricKind as Kind, CpuSensorSource as Source, CpuSensors,
};

use super::counters::{ClockCounter, EnergyCounter, amd_clock, intel_vid, legacy_vid, tsc_mhz};
use super::{Affinity, Identity, Module, PciGuard, energy_unit, valid};

struct ThreadSampler {
    group: u16,
    processor: u8,
    clock: ClockCounter,
}

pub(super) struct CoreSampler {
    pub core: u32,
    pub smu_slot: Option<usize>,
    cpuid_core: Option<usize>,
    threads: Vec<ThreadSampler>,
    energy: EnergyCounter,
    efficient: bool,
}

impl CoreSampler {
    pub fn processors(&self) -> impl Iterator<Item = (u16, u8)> + '_ {
        self.threads.iter().map(|t| (t.group, t.processor))
    }

    #[cfg(test)]
    pub fn fixture(core: u32, slot: usize, processor: u8) -> Self {
        Self {
            core,
            smu_slot: Some(slot),
            cpuid_core: Some(core as usize),
            threads: vec![ThreadSampler {
                group: 0,
                processor,
                clock: ClockCounter::default(),
            }],
            energy: EnergyCounter::default(),
            efficient: false,
        }
    }
}

pub(super) struct Telemetry {
    pub cores: Vec<CoreSampler>,
    tsc_mhz: Option<f32>,
    bus_mhz: Option<f32>,
    energy: [EnergyCounter; 4],
    vermeer_mapping_pending: bool,
    mapping_attempt: Option<Instant>,
}

impl Telemetry {
    pub fn new(module: &Module, id: Identity, info: &CpuInfo, processors: &[(u16, u8)]) -> Self {
        let tsc = tsc_mhz();
        let nominal_ratio = if id.intel {
            module
                .msr(0xce)
                .and_then(|v| valid(((v >> 8) & 255) as f64, 1.0, 100.0))
        } else if id.family >= 0x17 {
            module
                .msr(0xc0010064)
                .and_then(|v| amd_clock(id.family, v, 100.0))
                .map(|v| v / 100.0)
        } else {
            None
        };
        let bus_mhz = tsc
            .zip(nominal_ratio)
            .and_then(|(tsc, ratio)| valid((tsc / ratio) as f64, 40.0, 400.0));
        let mut cores: BTreeMap<u32, CoreSampler> = BTreeMap::new();
        for &(group, processor) in processors {
            let Some(thread) = info
                .thread_index(group, processor)
                .and_then(|i| info.threads.get(i))
            else {
                continue;
            };
            let core = cores.entry(thread.core).or_insert_with(|| CoreSampler {
                core: thread.core,
                smu_slot: None,
                cpuid_core: None,
                threads: Vec::new(),
                energy: EnergyCounter::default(),
                efficient: thread.efficient,
            });
            if core.threads.is_empty()
                && !id.intel
                && id.family >= 0x17
                && let Some(_guard) = Affinity::set(group, processor)
            {
                use std::arch::x86_64::__cpuid;
                if __cpuid(0x80000000).eax >= 0x8000001e {
                    let core_id = (__cpuid(0x8000001e).ebx & 255) as usize;
                    core.cpuid_core = Some(core_id);
                    core.smu_slot = Some(core_id);
                }
            }
            core.threads.push(ThreadSampler {
                group,
                processor,
                clock: ClockCounter::default(),
            });
        }
        let mut cores: Vec<_> = cores.into_values().collect();
        let mut slots = HashSet::new();
        if cores
            .iter()
            .filter_map(|c| c.smu_slot)
            .any(|s| !slots.insert(s))
        {
            for core in &mut cores {
                core.smu_slot = None;
            }
        }
        let vermeer_mapping_pending =
            !id.intel && id.family == 0x19 && id.model == 0x21 && cores.len() == 6;
        if vermeer_mapping_pending {
            // Vermeer's PM table retains fused-off lanes; CoreId can be dense.
            // Keep per-core SMU fields unknown until the fuse map is verified.
            for core in &mut cores {
                core.smu_slot = None;
            }
        }
        let mut result = Self {
            cores,
            tsc_mhz: tsc,
            bus_mhz,
            energy: Default::default(),
            vermeer_mapping_pending,
            mapping_attempt: None,
        };
        result.resolve_smu_mapping(module);
        result
    }

    pub fn resolve_smu_mapping(&mut self, module: &Module) {
        if !self.vermeer_mapping_pending
            || self
                .mapping_attempt
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(30))
        {
            return;
        }
        self.mapping_attempt = Some(Instant::now());
        let Some(_guard) = PciGuard::acquire() else {
            return;
        };
        let mut fuses = [None; 3];
        let ids: Option<Vec<_>> = self.cores.iter().map(|core| core.cpuid_core).collect();
        let result = (|| {
            fuses[0] = module.smn(0x5d218);
            fuses[1] = module.smn(0x5d21c);
            let ccd = vermeer_ccd(fuses[0]?, fuses[1]?)?;
            // Fixed Vermeer topology fuses, through the existing signed module.
            fuses[2] = module.smn(if ccd == 0 { 0x3008_1d98 } else { 0x3208_1d98 });
            vermeer_core_slots(ids.as_deref()?, fuses[2]?)
        })();
        if let Some(slots) = result {
            for (core, slot) in self.cores.iter_mut().zip(slots) {
                core.smu_slot = Some(slot);
            }
            self.vermeer_mapping_pending = false;
        } else {
            argus_core::diagnostics::warn(
                "AMD SMU core mapping",
                format!(
                    "Vermeer fuse map unavailable or inconsistent; cores={}; core_ids={ids:?}; ccd_fuses={:?}; core_fuse={:?}; per-core SMU fields withheld",
                    self.cores.len(),
                    &fuses[..2],
                    fuses[2]
                ),
            );
        }
    }

    pub fn sample_package(
        &mut self,
        module: &Module,
        id: Identity,
        frame: &mut CpuSensors,
    ) -> Option<u64> {
        frame.metric(Kind::BusMhz, self.bus_mhz, Source::Msr);
        let power_unit = module.msr(if id.intel { 0x606 } else { 0xc0010299 });
        if id.intel {
            self.intel_power(module, id, frame, power_unit);
            if super::intel::ring_supported(id) {
                if let Some(ratio) = module.msr(0x621).and_then(super::intel::ring_ratio) {
                    frame.metric(Kind::RingMultiplier, Some(ratio), Source::Msr);
                    frame.metric(
                        Kind::RingMhz,
                        self.bus_mhz.map(|bus| bus * ratio),
                        Source::Msr,
                    );
                }
                if let Some(limits) = module.msr(0x620) {
                    for (kind, bits) in
                        [(Kind::RingMinMhz, limits >> 8), (Kind::RingMaxMhz, limits)]
                    {
                        frame.metric(
                            kind,
                            super::intel::ring_ratio(bits)
                                .zip(self.bus_mhz)
                                .map(|(ratio, bus)| ratio * bus),
                            Source::Msr,
                        );
                    }
                }
            }
        }
        power_unit
    }

    pub fn sample(
        &mut self,
        module: &Module,
        id: Identity,
        frame: &mut CpuSensors,
        started: Instant,
    ) {
        let power_unit = self.sample_package(module, id, frame);
        self.sample_cores(module, id, frame, started, power_unit);
    }

    pub fn sample_cores(
        &mut self,
        module: &Module,
        id: Identity,
        frame: &mut CpuSensors,
        started: Instant,
        power_unit: Option<u64>,
    ) {
        for core in &mut self.cores {
            if started.elapsed().as_millis() >= 500 {
                break;
            }
            let first = &core.threads[0];
            let Some(_affinity) = Affinity::set(first.group, first.processor) else {
                continue;
            };
            let mut reading = CpuCoreSensors {
                core: core.core,
                group: first.group,
                processor: first.processor,
                efficient: core.efficient,
                ..Default::default()
            };
            if id.intel {
                let status = module.msr(0x198);
                let ratio = status.map(|v| {
                    if matches!(id.model, 0x1a | 0x1e | 0x1f | 0x2e | 0x25 | 0x2c | 0x2f) {
                        v & 255
                    } else {
                        (v >> 8) & 255
                    }
                });
                reading.multiplier = ratio.and_then(|v| valid(v as f64, 1.0, 120.0));
                reading.clock_mhz = reading
                    .multiplier
                    .zip(self.bus_mhz)
                    .and_then(|(ratio, bus)| valid((ratio * bus) as f64, 100.0, 12000.0));
                reading.vid = status.and_then(|v| intel_vid(id, v));
                if id.thermal & 1 != 0 {
                    let target = module.msr(0x1a2).map(|v| ((v >> 16) & 255) as u32);
                    if let Some((status, target)) = module.msr(0x19c).zip(target) {
                        reading.limits = super::intel::limits(status, id.thermal, false);
                        reading.celsius = super::intel_temperature(status, target);
                        if reading.celsius.is_some() {
                            reading.distance_to_limit = Some(((status >> 16) & 127) as f32);
                            reading.thermal_throttling = Some(status & 0x5 != 0);
                        }
                    }
                }
            } else if id.family >= 0x17 {
                if let Some(status) = module.msr(0xc0010293) {
                    reading.clock_mhz = self
                        .bus_mhz
                        .and_then(|bus| amd_clock(id.family, status, bus));
                    reading.multiplier = reading
                        .clock_mhz
                        .zip(self.bus_mhz)
                        .map(|(clock, bus)| clock / bus);
                    if id.family == 0x17
                        || id.family == 0x18
                        || (id.family == 0x19 && id.model < 0x60)
                    {
                        reading.vid =
                            valid(1.55 - ((status >> 14) & 255) as f64 * 0.00625, 0.1, 1.55);
                    }
                }
                reading.watts = core.energy.sample(
                    module.msr(0xc001029a),
                    power_unit.map_or(0.0, |u| energy_unit(id, u as u32)),
                    Instant::now(),
                );
            } else if let Some(status) = module.msr(0xc0010071) {
                reading.vid = legacy_vid(id, status, false);
                frame.metric(
                    Kind::NorthbridgeVidVolts,
                    legacy_vid(id, status, true),
                    Source::Msr,
                );
            }

            if id.intel || id.family >= 0x17 {
                let mut effective = Vec::new();
                let mut c0 = Vec::new();
                for thread in &mut core.threads {
                    let Some(_thread) = Affinity::set(thread.group, thread.processor) else {
                        continue;
                    };
                    let (mperf, aperf) = if id.intel {
                        (0xe7, 0xe8)
                    } else {
                        (0xc00000e7, 0xc00000e8)
                    };
                    let a = module.msr(aperf);
                    let m = module.msr(mperf);
                    let (mhz, activity) = thread.clock.sample(a, m, self.tsc_mhz, Instant::now());
                    if let Some(v) = mhz {
                        effective.push(v);
                    }
                    if let Some(v) = activity {
                        c0.push(v);
                    }
                }
                if effective.len() == core.threads.len() {
                    reading.effective_mhz = mean(&effective);
                }
                if c0.len() == core.threads.len() {
                    reading.c0_percent = mean(&c0);
                }
            }
            frame.cores.push(reading);
        }
        if frame.cores.len() == self.cores.len() {
            frame.metric(
                Kind::AverageMhz,
                complete_mean(frame.cores.iter().map(|c| c.clock_mhz)),
                Source::Msr,
            );
            frame.metric(
                Kind::EffectiveMhz,
                complete_mean(frame.cores.iter().map(|c| c.effective_mhz)),
                Source::Msr,
            );
            if !id.intel {
                let power: Option<Vec<_>> = frame.cores.iter().map(|c| c.watts).collect();
                frame.metric(
                    Kind::CoresWatts,
                    power.filter(|v| !v.is_empty()).map(|v| v.iter().sum()),
                    Source::Msr,
                );
            }
        }
        frame.core_vid = frame
            .cores
            .iter()
            .filter_map(|c| c.vid)
            .reduce(f32::max)
            .or(frame.core_vid);
        let flags: Vec<_> = frame
            .cores
            .iter()
            .filter_map(|c| c.thermal_throttling)
            .collect();
        if !flags.is_empty() {
            frame.thermal_throttling =
                Some(flags.iter().any(|v| *v) || frame.thermal_throttling == Some(true));
        }
    }

    fn intel_power(
        &mut self,
        module: &Module,
        id: Identity,
        frame: &mut CpuSensors,
        unit: Option<u64>,
    ) {
        let Some(unit) = unit else { return };
        let power = 2f64.powi(-((unit & 15) as i32));
        let time = 2f64.powi(-(((unit >> 16) & 15) as i32));
        if let Some(limits) = module.msr(0x610) {
            for (shift, watts, seconds) in [
                (0, Kind::Pl1Watts, Kind::Pl1Seconds),
                (32, Kind::Pl2Watts, Kind::Pl2Seconds),
            ] {
                let (limit, window) = intel_limit(limits >> shift, power, time);
                frame.metric(watts, limit, Source::Msr);
                frame.metric(seconds, window, Source::Msr);
            }
        }
        frame.metric(
            Kind::TdpWatts,
            module
                .msr(0x614)
                .and_then(|v| valid((v & 0x7fff) as f64 * power, 0.1, 2000.0)),
            Source::Msr,
        );
        let domains = [
            (0x639, Kind::CoresWatts),
            (0x641, Kind::GraphicsWatts),
            (0x619, Kind::MemoryWatts),
            (0x64d, Kind::PlatformWatts),
        ];
        for (counter, (register, kind)) in self.energy.iter_mut().zip(domains) {
            let scale =
                if kind == Kind::MemoryWatts && matches!(id.model, 0x3f | 0x4f | 0x55 | 0x56) {
                    1.0 / 65536.0
                } else {
                    energy_unit(id, unit as u32)
                };
            frame.metric(
                kind,
                counter.sample(module.msr(register), scale, Instant::now()),
                Source::Msr,
            );
        }
    }
}

fn vermeer_ccd(present: u32, down: u32) -> Option<u32> {
    if present == u32::MAX || down == u32::MAX {
        return None;
    }
    let disabled = (present >> 30) | ((down & 0x3f) << 2);
    let enabled = !disabled & 0xff;
    (enabled.count_ones() == 1 && enabled & !3 == 0).then(|| enabled.trailing_zeros())
}

pub(super) fn vermeer_core_slots(ids: &[usize], disabled: u32) -> Option<Vec<usize>> {
    if ids.len() != 6 || disabled == u32::MAX {
        return None;
    }
    let slots: Vec<_> = (0..8).filter(|slot| disabled & (1 << slot) == 0).collect();
    if slots.len() != ids.len() || ids.iter().any(|id| *id >= 16 || id / 8 != ids[0] / 8) {
        return None;
    }
    let mut ordered: Vec<_> = ids.iter().map(|id| id % 8).collect();
    ordered.sort_unstable();
    if ordered == slots {
        Some(ids.iter().map(|id| id % 8).collect())
    } else if ordered == (0..ids.len()).collect::<Vec<_>>() {
        Some(ids.iter().map(|id| slots[id % 8]).collect())
    } else {
        None
    }
}

fn mean(values: &[f32]) -> Option<f32> {
    (!values.is_empty()).then(|| values.iter().sum::<f32>() / values.len() as f32)
}

fn complete_mean(values: impl Iterator<Item = Option<f32>>) -> Option<f32> {
    mean(&values.collect::<Option<Vec<_>>>()?)
}

fn intel_limit(value: u64, power: f64, time: f64) -> (Option<f32>, Option<f32>) {
    if value & (1 << 15) == 0 {
        return (None, None);
    }
    let window = (value >> 17) & 127;
    let seconds = 2f64.powi((window & 31) as i32) * (1.0 + ((window >> 5) & 3) as f64 / 4.0) * time;
    (
        valid((value & 0x7fff) as f64 * power, 0.1, 2000.0),
        valid(seconds, 0.000001, 1e9),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vermeer_fuses_map_dense_and_sparse_ids_without_guessing_disabled_lanes() {
        assert_eq!(vermeer_ccd(0x80400000, 0x3f), Some(0));
        assert_eq!(vermeer_ccd(0x40400000, 0x3f), Some(1));
        assert_eq!(vermeer_ccd(0, 0), None);
        assert_eq!(vermeer_ccd(u32::MAX, 0), None);
        assert_eq!(
            vermeer_core_slots(&[0, 1, 2, 3, 4, 5], 0x330),
            Some(vec![0, 1, 2, 3, 6, 7])
        );
        assert_eq!(
            vermeer_core_slots(&[0, 1, 2, 3, 4, 5], 0x30c),
            Some(vec![0, 1, 4, 5, 6, 7])
        );
        assert_eq!(
            vermeer_core_slots(&[7, 0, 1, 4, 5, 6], 0x30c),
            Some(vec![7, 0, 1, 4, 5, 6])
        );
        assert_eq!(
            vermeer_core_slots(&[8, 9, 10, 11, 12, 13], 0x330),
            Some(vec![0, 1, 2, 3, 6, 7])
        );
        for ids in [
            &[0, 1, 2, 3, 4, 4][..],
            &[0, 1, 2, 3, 8, 9],
            &[0, 1, 2, 3, 5, 7],
        ] {
            assert_eq!(vermeer_core_slots(ids, 0x330), None);
        }
        assert_eq!(vermeer_core_slots(&[0, 1, 2, 3, 4, 5], 0), None);
        assert_eq!(vermeer_core_slots(&[0, 1, 2, 3, 4, 5], u32::MAX), None);
    }

    #[test]
    fn power_limits_honor_enable_bits_and_fractional_time_windows() {
        let pl1 = 200u64 | (1 << 15) | ((10 | (2 << 5)) << 17);
        assert_eq!(
            intel_limit(pl1, 0.125, 1.0 / 1024.0),
            (Some(25.0), Some(1.5))
        );
        assert_eq!(
            intel_limit(pl1 & !(1 << 15), 0.125, 1.0 / 1024.0),
            (None, None)
        );
        assert_eq!(complete_mean([Some(1000.0), None].into_iter()), None);
        assert_eq!(
            complete_mean([Some(0.0), Some(1000.0)].into_iter()),
            Some(500.0)
        );
    }
}
