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

/// Chiplet packages whose SMU tables keep fused-off core lanes while CPUID
/// CoreId may number the enabled cores densely. Their per-core SMU fields wait
/// for the fixed topology fuses (ZenStates-Core `GetTopologyFuses`), read
/// through the existing signed module.
#[derive(Clone, Copy, Debug)]
enum TopologyFuses {
    Vermeer,
    GraniteRidge,
}

impl TopologyFuses {
    fn needed(id: Identity, cores: &[CoreSampler]) -> Option<Self> {
        if id.intel {
            return None;
        }
        if id.family == 0x19 && id.model == 0x21 && cores.len() == 6 {
            return Some(Self::Vermeer);
        }
        if id.family != 0x1a || !(0x40..=0x4f).contains(&id.model) {
            return None;
        }
        // A CCD with all eight cores enabled has no lane gap, so its CoreIds
        // are the SMU lanes either way. Only partial CCDs (9900X, 9600X, BIOS
        // downcoring) need the fuse map.
        let mut ccds = BTreeMap::<usize, usize>::new();
        for core_id in cores.iter().filter_map(|c| c.cpuid_core) {
            *ccds.entry(core_id / 8).or_default() += 1;
        }
        ccds.values().any(|n| *n != 8).then_some(Self::GraniteRidge)
    }

    fn ccd_fuses(self) -> (u32, u32) {
        match self {
            Self::Vermeer => (0x5d218, 0x5d21c),
            Self::GraniteRidge => (0x5d3bc, 0x5d3c0),
        }
    }

    /// Core disable fuse of CCD0; CCD n is at `+ (n << 25)`, mask in [7:0].
    fn core_fuse(self) -> u32 {
        match self {
            Self::Vermeer => 0x3008_1d98,
            Self::GraniteRidge => 0x304a_03dc,
        }
    }

    /// Enabled physical CCDs, ascending.
    fn enabled_ccds(self, present: u32, down: u32) -> Option<Vec<u32>> {
        if present == u32::MAX || down == u32::MAX {
            return None;
        }
        let disabled = (present >> 30) | ((down & 0x3f) << 2);
        let (mask, max) = match self {
            // Zen 3 packages mark absent CCDs down; a 6-core part has one.
            Self::Vermeer => (!disabled & 0xff, 1),
            // Zen 4 / Zen 5 desktop: present in [23:22], up to two CCDs.
            Self::GraniteRidge => ((present >> 22) & 3 & !disabled, 2),
        };
        let ccds: Vec<_> = (0..8).filter(|ccd| mask & (1 << ccd) != 0).collect();
        (!ccds.is_empty() && ccds.len() <= max && mask & !3 == 0).then_some(ccds)
    }
}

pub(super) struct Telemetry {
    pub cores: Vec<CoreSampler>,
    tsc_mhz: Option<f32>,
    bus_mhz: Option<f32>,
    energy: [EnergyCounter; 4],
    mapping_pending: Option<TopologyFuses>,
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
            #[cfg(target_arch = "x86_64")]
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
        let mapping_pending = TopologyFuses::needed(id, &cores);
        if mapping_pending.is_some() {
            // The PM table retains fused-off lanes; CoreId can be dense.
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
            mapping_pending,
            mapping_attempt: None,
        };
        result.resolve_smu_mapping(module);
        result
    }

    pub fn resolve_smu_mapping(&mut self, module: &Module) {
        let Some(topology) = self.mapping_pending else {
            return;
        };
        if self
            .mapping_attempt
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(30))
        {
            return;
        }
        self.mapping_attempt = Some(Instant::now());
        let Some(_guard) = PciGuard::acquire() else {
            return;
        };
        let (present, down) = topology.ccd_fuses();
        let ccd_fuses = [module.smn(present), module.smn(down)];
        let mut core_fuses = Vec::new();
        let ids: Option<Vec<_>> = self.cores.iter().map(|core| core.cpuid_core).collect();
        let result = (|| {
            let ccds = topology.enabled_ccds(ccd_fuses[0]?, ccd_fuses[1]?)?;
            for ccd in ccds {
                core_fuses.push(module.smn(topology.core_fuse() + (ccd << 25)));
            }
            let disabled: Option<Vec<_>> = core_fuses.iter().copied().collect();
            core_slots(ids.as_deref()?, &disabled?)
        })();
        if let Some(slots) = result {
            for (core, slot) in self.cores.iter_mut().zip(slots) {
                core.smu_slot = Some(slot);
            }
            self.mapping_pending = None;
        } else {
            argus_core::diagnostics::warn(
                "AMD SMU core mapping",
                format!(
                    "{topology:?} fuse map unavailable or inconsistent; cores={}; core_ids={ids:?}; ccd_fuses={ccd_fuses:x?}; core_fuses={core_fuses:x?}; per-core SMU fields withheld",
                    self.cores.len(),
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

/// SMU lane of each core from its CPUID CoreId. `disabled` holds the core
/// disable fuse of each enabled CCD in physical order: the n-th CoreId group
/// (CoreId / 8) is the n-th enabled CCD, whose eight lanes start at n * 8.
/// Within a CCD the CoreIds either name the physical slots or number the
/// enabled slots densely; any other layout or count is rejected.
pub(super) fn core_slots(ids: &[usize], disabled: &[u32]) -> Option<Vec<usize>> {
    let mut groups: Vec<_> = ids.iter().map(|id| id / 8).collect();
    groups.sort_unstable();
    groups.dedup();
    if ids.is_empty() || ids.iter().any(|id| *id >= 16) || groups.len() != disabled.len() {
        return None;
    }
    let mut lanes = vec![0; ids.len()];
    for (ccd, (group, fuse)) in groups.into_iter().zip(disabled).enumerate() {
        if *fuse == u32::MAX {
            return None;
        }
        let slots: Vec<_> = (0..8).filter(|slot| fuse & (1 << slot) == 0).collect();
        let members: Vec<_> = (0..ids.len()).filter(|i| ids[*i] / 8 == group).collect();
        let mut ordered: Vec<_> = members.iter().map(|i| ids[*i] % 8).collect();
        ordered.sort_unstable();
        let physical = ordered == slots;
        if !physical && ordered != (0..slots.len()).collect::<Vec<_>>() {
            return None;
        }
        for i in members {
            let local = ids[i] % 8;
            lanes[i] = ccd * 8 + if physical { local } else { slots[local] };
        }
    }
    Some(lanes)
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
        let vermeer = TopologyFuses::Vermeer;
        assert_eq!(vermeer.enabled_ccds(0x80400000, 0x3f), Some(vec![0]));
        assert_eq!(vermeer.enabled_ccds(0x40400000, 0x3f), Some(vec![1]));
        assert_eq!(vermeer.enabled_ccds(0, 0), None);
        assert_eq!(vermeer.enabled_ccds(u32::MAX, 0), None);
        assert_eq!(
            core_slots(&[0, 1, 2, 3, 4, 5], &[0x330]),
            Some(vec![0, 1, 2, 3, 6, 7])
        );
        assert_eq!(
            core_slots(&[0, 1, 2, 3, 4, 5], &[0x30c]),
            Some(vec![0, 1, 4, 5, 6, 7])
        );
        assert_eq!(
            core_slots(&[7, 0, 1, 4, 5, 6], &[0x30c]),
            Some(vec![7, 0, 1, 4, 5, 6])
        );
        assert_eq!(
            core_slots(&[8, 9, 10, 11, 12, 13], &[0x330]),
            Some(vec![0, 1, 2, 3, 6, 7])
        );
        for ids in [
            &[0, 1, 2, 3, 4, 4][..],
            &[0, 1, 2, 3, 8, 9],
            &[0, 1, 2, 3, 5, 7],
        ] {
            assert_eq!(core_slots(ids, &[0x330]), None);
        }
        assert_eq!(core_slots(&[0, 1, 2, 3, 4, 5], &[0]), None);
        assert_eq!(core_slots(&[0, 1, 2, 3, 4, 5], &[u32::MAX]), None);
    }

    #[test]
    fn granite_ridge_partial_ccds_map_each_ccd_to_its_own_lanes() {
        let granite = TopologyFuses::GraniteRidge;
        // Upstream readings: Granite Ridge 9600X and Raphael 7950X.
        assert_eq!(granite.enabled_ccds(0x024BEBA3, 0), Some(vec![0]));
        assert_eq!(granite.enabled_ccds(0x00E97051, 0), Some(vec![0, 1]));
        assert_eq!(granite.enabled_ccds(0x40E97051, 0), Some(vec![1]));
        assert_eq!(granite.enabled_ccds(0, 0), None);
        assert_eq!(granite.enabled_ccds(0x00E97051, u32::MAX), None);
        // 9900X3D-like 2 x 6: dense CoreIds per CCD, different fused slots.
        let dense = [0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13];
        assert_eq!(
            core_slots(&dense, &[0x0c, 0x81]),
            Some(vec![0, 1, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14])
        );
        // CoreIds that already name the physical slots stay where they are.
        let sparse = [0, 1, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14];
        assert_eq!(core_slots(&sparse, &[0x0c, 0x81]), Some(sparse.to_vec()));
        // Windows core order need not follow CoreId order.
        assert_eq!(
            core_slots(&[8, 0, 9, 1], &[0xfc, 0xfc]),
            Some(vec![8, 0, 9, 1])
        );
        assert_eq!(
            core_slots(&[0, 1, 2, 3, 4, 5], &[0x0c]),
            Some(vec![0, 1, 4, 5, 6, 7])
        );
        for (ids, fuses) in [
            // Fuses disagree with the cores Windows reports (BIOS downcoring).
            (&dense[..], &[0, 0][..]),
            // One CCD per fuse, not a CoreId range spanning both.
            (
                &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11][..],
                &[0x0c, 0x81][..],
            ),
            (&dense[..], &[0x0c][..]),
            (&dense[..], &[0x0c, u32::MAX][..]),
            (
                &[0, 1, 2, 3, 4, 5, 16, 17, 18, 19, 20, 21][..],
                &[0x0c, 0x81][..],
            ),
        ] {
            assert_eq!(core_slots(ids, fuses), None);
        }
    }

    #[test]
    fn only_granite_ridge_ccds_with_fused_cores_wait_for_the_fuse_map() {
        let id = Identity {
            intel: false,
            family: 0x1a,
            model: 0x44,
            thermal: 0,
        };
        let cores = |ids: &[usize]| -> Vec<_> {
            ids.iter()
                .enumerate()
                .map(|(core, id)| {
                    let mut sampler = CoreSampler::fixture(core as u32, *id, core as u8 * 2);
                    sampler.cpuid_core = Some(*id);
                    sampler
                })
                .collect()
        };
        let full: Vec<_> = (0..16).collect();
        let twelve = [0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13];
        assert!(TopologyFuses::needed(id, &cores(&full)).is_none());
        assert!(TopologyFuses::needed(id, &cores(&full[..8])).is_none());
        assert!(matches!(
            TopologyFuses::needed(id, &cores(&twelve)),
            Some(TopologyFuses::GraniteRidge)
        ));
        assert!(matches!(
            TopologyFuses::needed(id, &cores(&full[..6])),
            Some(TopologyFuses::GraniteRidge)
        ));
        let raphael = Identity {
            family: 0x19,
            model: 0x61,
            ..id
        };
        assert!(TopologyFuses::needed(raphael, &cores(&twelve)).is_none());
        let vermeer = Identity {
            family: 0x19,
            model: 0x21,
            ..id
        };
        assert!(matches!(
            TopologyFuses::needed(vermeer, &cores(&full[..6])),
            Some(TopologyFuses::Vermeer)
        ));
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
