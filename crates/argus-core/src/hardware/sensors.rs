//! Windows CPU clocks from PCW, plus PDH thermal, power and idle readings.

use serde::Serialize;
use std::cell::OnceCell;
use std::sync::Arc;
use windows::Win32::System::Performance::PDH_HCOUNTER;
use windows::core::w;

use super::{CpuInfo, clock};
use crate::pdh::{self, Query};
use crate::util::AlignedBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThermalZone {
    /// ACPI name without the `\_TZ.` prefix, e.g. `TZ01`.
    pub name: String,
    pub celsius: f32,
}

/// CPU power domains, in watts; None when no valid reading is available.
/// RAPL meters (`RAPL_Package0_PKG` and so on) give every domain. An ARM64 PC,
/// such as a Snapdragon one, has no RAPL and may name its energy meter rails
/// after Microsoft's EMI rail taxonomy; its `CPU` rails then give the cores
/// (see `CPU_RAILS`). Other rails (`GPU`, `NPU`, ...) are not attributed to a
/// device.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuPower {
    /// The whole package (cores, graphics, uncore).
    pub package: Option<f32>,
    /// CPU cores only.
    pub cores: Option<f32>,
    /// Integrated graphics.
    pub graphics: Option<f32>,
    pub memory: Option<f32>,
}

/// Where `CpuPower` came from, for the diagnostics log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PowerSource {
    Rapl,
    CpuRails,
    None,
}

/// Whether `CPU` energy meter rails may stand in for missing RAPL CPU meters.
/// Not on x64: there the kernel sensors fill a missing Windows RAPL meter from
/// the RAPL MSRs, and package and core power must come from one measurement.
const CPU_RAILS: bool = cfg!(not(target_arch = "x86_64"));

impl CpuPower {
    /// One sample of `\Energy Meter(*)\Power`, in milliwatts per instance.
    fn from_meters(meters: &[(String, f64)], cpu_rails: bool) -> (Self, PowerSource) {
        let mut power = Self::default();
        // Microsoft's taxonomy: a rail named exactly `CPU` is the whole
        // component; without one, the `CPU_*` rails add up.
        let (mut whole, mut parts) = (None, None);
        let mut rapl_cpu = false;
        for (name, milliwatts) in meters {
            let rail = cpu_rail(name).filter(|_| cpu_rails);
            let slot = match (rail, name.rsplit('_').next()) {
                (Some(true), _) => &mut whole,
                (Some(false), _) => &mut parts,
                (None, Some(domain @ ("PKG" | "PP0"))) => {
                    rapl_cpu = true;
                    if domain == "PKG" {
                        &mut power.package
                    } else {
                        &mut power.cores
                    }
                }
                (None, Some("PP1")) => &mut power.graphics,
                (None, Some("DRAM")) => &mut power.memory,
                _ => continue,
            };
            add_watts(slot, name, *milliwatts);
        }
        // A RAPL package or core meter decides, even while its reading is
        // invalid: CPU power never mixes RAPL with another meter.
        if rapl_cpu {
            return (power, PowerSource::Rapl);
        }
        power.cores = whole.or(parts);
        let source = if power.cores.is_some() {
            PowerSource::CpuRails
        } else {
            PowerSource::None
        };
        (power, source)
    }
}

/// `Some(true)` for a rail named `CPU`, `Some(false)` for `CPU_*`, ignoring case.
fn cpu_rail(name: &str) -> Option<bool> {
    if name.eq_ignore_ascii_case("CPU") {
        return Some(true);
    }
    let prefix = name.get(..4)?;
    (prefix.eq_ignore_ascii_case("CPU_") && name.len() > 4).then_some(false)
}

fn add_watts(slot: &mut Option<f32>, name: &str, milliwatts: f64) {
    // PDH already checked counter status. Zero is a valid idle reading,
    // not an absent domain; invalid readings must not turn into zero.
    if !milliwatts.is_finite() || milliwatts < 0.0 || name == "_Total" {
        return;
    }
    // Several packages or rails add up.
    let watts = f64::from(slot.unwrap_or(0.0)) + milliwatts / 1000.0;
    if watts <= f64::from(f32::MAX) {
        *slot = Some(watts as f32);
    }
}

/// One line per process with every energy meter and thermal zone Windows
/// reports, the raw readings included, so a report from an unfamiliar PC
/// shows which rails and zones there are and which ones were skipped.
fn log_sources(meters: &[(String, f64)], zones: &[(String, f32)], source: PowerSource) {
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let source = match source {
        PowerSource::Rapl => "RAPL",
        PowerSource::CpuRails => "CPU rails",
        PowerSource::None => "none",
    };
    log::info!(
        target: "argus_diagnostics",
        "sensor sources: cpu power={source}; energy meters: {}; thermal zones: {}",
        source_list(meters, |mw| format!("{mw:.0} mW")),
        source_list(zones, |c| format!("{c:.1} C")),
    );
}

/// `name=value, ...` without `_Total`, at most 64 entries of bounded names.
fn source_list<T>(items: &[(String, T)], value: impl Fn(&T) -> String) -> String {
    const MAX: usize = 64;
    let items: Vec<_> = items.iter().filter(|(name, _)| name != "_Total").collect();
    if items.is_empty() {
        return "none".into();
    }
    let mut out = Vec::with_capacity(items.len().min(MAX) + 1);
    for (raw, v) in items.iter().take(MAX) {
        // Names come from firmware and drivers: bound and escape them.
        let mut name = String::new();
        for c in raw.chars().take(64) {
            if c.is_control() {
                name.extend(c.escape_default());
            } else {
                name.push(c);
            }
        }
        out.push(format!("{name}={}", value(v)));
    }
    if items.len() > MAX {
        out.push(format!("{} more", items.len() - MAX));
    }
    out.join(", ")
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuIdle {
    pub c1: Option<f32>,
    pub c2: Option<f32>,
    pub c3: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct SensorSample {
    /// Overall clock in MHz; direct Windows readings take priority.
    pub cpu_mhz: Option<u32>,
    /// Dense logical-processor order, matching CpuInfo::threads; 0 if unknown.
    pub core_mhz: Vec<u32>,
    pub thermal: Vec<ThermalZone>,
    pub power: CpuPower,
    pub idle: Vec<CpuIdle>,
}

pub(crate) struct Sensors {
    raw_clocks: Option<clock::Query>,
    query: Option<Query>,
    buf: AlignedBuf,
    nominal_mhz: OnceCell<Vec<Option<u32>>>,
    cpu: Arc<CpuInfo>,
    /// Direct MHz readings for logical processors and their total.
    core_freq: Option<PDH_HCOUNTER>,
    /// Performance relative to each processor's nominal frequency.
    core_perf: Option<PDH_HCOUNTER>,
    zones: Option<PDH_HCOUNTER>,
    /// Zones in tenths of a kelvin (`High Precision Temperature`) rather than kelvin.
    zones_precise: bool,
    energy: Option<PDH_HCOUNTER>,
    idle: [Option<PDH_HCOUNTER>; 3],
    /// Samples with PDH data so far, saturating; picks when to log sources.
    ready_samples: u8,
}

// PDH handles are process-wide; the sampler uses them from one thread at a time.
unsafe impl Send for Sensors {}

impl Sensors {
    pub fn new(cpu: Arc<CpuInfo>) -> Self {
        let raw_clocks = clock::Query::open(&cpu);
        let query = Query::open();
        let add = |path| query.as_ref()?.add(path);
        let core_freq = if raw_clocks.is_none() {
            add(w!(r"\Processor Information(*)\Actual Frequency"))
        } else {
            None
        };
        let core_perf = add(w!(r"\Processor Information(*)\% Processor Performance"));
        let mut zones_precise = true;
        let mut zones = add(w!(
            r"\Thermal Zone Information(*)\High Precision Temperature"
        ));
        if zones.is_none() {
            zones_precise = false;
            zones = add(w!(r"\Thermal Zone Information(*)\Temperature"));
        }
        let energy = add(w!(r"\Energy Meter(*)\Power"));
        let idle = [
            add(w!(r"\Processor Information(*)\% C1 Time")),
            add(w!(r"\Processor Information(*)\% C2 Time")),
            add(w!(r"\Processor Information(*)\% C3 Time")),
        ];
        // Rate counters need a first collection to have something to diff against.
        if let Some(query) = &query {
            query.collect();
        }
        Self {
            raw_clocks,
            query,
            buf: AlignedBuf::with_capacity(16 * 1024),
            nominal_mhz: OnceCell::new(),
            cpu,
            core_freq,
            core_perf,
            zones,
            zones_precise,
            energy,
            idle,
            ready_samples: 0,
        }
    }

    pub fn sample(&mut self) -> SensorSample {
        let mut out = SensorSample::default();
        let pdh_ready = self.query.as_ref().is_some_and(Query::collect);
        let measured = if let Some(raw) = &mut self.raw_clocks {
            raw.sample(&self.cpu)
        } else {
            let actual = self
                .core_freq
                .filter(|_| pdh_ready)
                .map(|c| pdh::array_uncapped(c, &mut self.buf))
                .unwrap_or_default();
            pdh_clocks(&self.cpu, &actual)
        };
        (out.cpu_mhz, out.core_mhz) = finish_clocks(
            &self.cpu,
            measured,
            || {
                self.nominal_mhz
                    .get_or_init(|| {
                        super::cpu::nominal_frequencies(
                            &self.cpu.threads,
                            performance_reference(&self.cpu),
                        )
                    })
                    .as_slice()
            },
            || {
                self.core_perf
                    .filter(|_| pdh_ready)
                    .map(|c| pdh::array_uncapped(c, &mut self.buf))
                    .unwrap_or_default()
            },
        );
        if !pdh_ready {
            return out;
        }
        out.idle
            .resize_with(self.cpu.threads.len(), CpuIdle::default);
        for (state, counter) in self.idle.iter().enumerate() {
            let Some(counter) = counter else { continue };
            for (name, value) in pdh::array(*counter, &mut self.buf) {
                let Some(index) = self.cpu.counter_index(&name) else {
                    continue;
                };
                if !value.is_finite() || !(0.0..=100.01).contains(&value) {
                    continue;
                }
                out.idle
                    .resize_with(out.idle.len().max(index + 1), CpuIdle::default);
                let reading = Some(value.min(100.0) as f32);
                match state {
                    0 => out.idle[index].c1 = reading,
                    1 => out.idle[index].c2 = reading,
                    _ => out.idle[index].c3 = reading,
                }
            }
        }

        let zones: Vec<(String, f32)> = self
            .zones
            .map(|c| pdh::array(c, &mut self.buf))
            .unwrap_or_default()
            .into_iter()
            .map(|(name, raw)| {
                let kelvin = if self.zones_precise { raw / 10.0 } else { raw };
                (name, (kelvin - 273.15) as f32)
            })
            .collect();
        for (name, celsius) in &zones {
            // Zero kelvin or 0 °C means "not reported"; unused zones on some
            // firmware read a constant -40 °C.
            if *celsius > 1.0 && *celsius < 150.0 {
                let name = name.trim_start_matches(r"\_TZ.").to_string();
                out.thermal.push(ThermalZone {
                    name,
                    celsius: *celsius,
                });
            }
        }

        let meters = self
            .energy
            .map(|c| pdh::array(c, &mut self.buf))
            .unwrap_or_default();
        let (power, source) = CpuPower::from_meters(&meters, CPU_RAILS);
        out.power = power;
        // The second collection: rate counters have had a baseline by then.
        self.ready_samples = self.ready_samples.saturating_add(1);
        if self.ready_samples == 2 {
            log_sources(&meters, &zones, source);
        }
        out
    }
}

fn performance_reference(cpu: &CpuInfo) -> Option<u32> {
    // A performance percentage has a nominal-performance denominator, not
    // the registry's TSC/reference MHz. Hybrid classes and multiple packages
    // do not establish one common frequency reference for this conversion.
    (cpu.sockets == 1
        && !cpu.hybrid
        && cpu.efficiency_cores == 0
        && !cpu.threads.is_empty()
        && cpu.threads.len() == cpu.logical as usize
        && !cpu.threads.iter().any(|thread| thread.efficient)
        && (100..=12000).contains(&cpu.base_mhz))
    .then_some(cpu.base_mhz)
}

pub(super) fn valid_mhz(value: f64) -> Option<u32> {
    (value.is_finite() && value > 0.0 && value <= 12000.0)
        .then(|| value.round() as u32)
        .filter(|value| *value > 0)
}

/// Check a reported total against complete readings, or derive their mean.
fn clock_total(total: Option<u32>, cores: &[u32], logical: u32) -> Option<u32> {
    if cores.is_empty() || cores.len() != logical as usize || cores.contains(&0) {
        return total;
    }
    if let Some(mhz) = total
        && cores.iter().any(|v| *v <= mhz + 1)
        && cores.iter().any(|v| *v >= mhz.saturating_sub(1))
    {
        return Some(mhz);
    }
    let sum: u64 = cores.iter().map(|v| u64::from(*v)).sum();
    Some(((sum + cores.len() as u64 / 2) / cores.len() as u64) as u32)
}

fn pdh_clocks(cpu: &CpuInfo, actual: &[(String, f64)]) -> (Option<u32>, Vec<u32>) {
    let mut cores = vec![0; cpu.threads.len()];
    let mut total = None;
    for (name, value) in actual {
        if name == "_Total" {
            total = valid_mhz(*value);
        } else if let Some(index) = cpu.counter_index(name) {
            cores[index] = valid_mhz(*value).unwrap_or(0);
        }
    }
    (clock_total(total, &cores, cpu.logical), cores)
}

fn finish_clocks<'a>(
    cpu: &CpuInfo,
    (total, mut cores): (Option<u32>, Vec<u32>),
    references: impl FnOnce() -> &'a [Option<u32>],
    performance: impl FnOnce() -> Vec<(String, f64)>,
) -> (Option<u32>, Vec<u32>) {
    let full_topology = !cores.is_empty() && cores.len() == cpu.logical as usize;
    if full_topology && !cores.contains(&0) {
        return (
            total.or_else(|| clock_total(None, &cores, cpu.logical)),
            cores,
        );
    }

    let percentages = performance();
    if percentages.is_empty() {
        return (total, cores);
    }
    let nominal_mhz = references();
    // A system-wide percentage has a MHz denominator only when every
    // processor reports the same nominal frequency.
    let common = nominal_mhz.first().copied().flatten().filter(|base| {
        full_topology
            && nominal_mhz.len() == cores.len()
            && nominal_mhz.iter().all(|v| *v == Some(*base))
    });
    let mut fallback_total = None;
    for (name, value) in percentages {
        let scaled = |base: u32| valid_mhz(f64::from(base) * value / 100.0);
        if name == "_Total" {
            fallback_total = common.and_then(scaled);
        } else if let Some(index) = cpu.counter_index(&name)
            && cores[index] == 0
            && let Some(base) = nominal_mhz.get(index).copied().flatten()
        {
            cores[index] = scaled(base).unwrap_or(0);
        }
    }
    (
        total.or_else(|| clock_total(fallback_total, &cores, cpu.logical)),
        cores,
    )
}

impl super::Hardware {
    /// Fill missing Windows clocks from a fresh sensor frame before the
    /// client adds Windows readings to that frame's per-core details.
    pub fn supplement_cpu_clocks(&mut self) {
        let Some(frame) = self
            .cpu_sensors
            .as_ref()
            .filter(|s| s.state == super::SensorState::Ready)
        else {
            return;
        };
        let threads = &self.cpu.threads;
        if threads.is_empty()
            || frame.cores.is_empty()
            || (self.core_mhz.len() == threads.len() && !self.core_mhz.contains(&0))
        {
            return;
        }
        let mut physical = vec![0; self.cpu.cores as usize];
        for core in &frame.cores {
            if let Some(mhz) = core.clock_mhz.and_then(|v| valid_mhz(f64::from(v)))
                && let Some(index) = self.cpu.thread_index(core.group, core.processor)
                && threads[index].core == core.core
                && let Some(slot) = physical.get_mut(core.core as usize)
            {
                *slot = mhz;
            }
        }
        if !physical.iter().any(|mhz| *mhz > 0) {
            return;
        }
        self.core_mhz.resize(threads.len(), 0);
        for (mhz, thread) in self.core_mhz.iter_mut().zip(threads) {
            if *mhz == 0
                && let Some(clock) = physical.get(thread.core as usize)
            {
                *mhz = *clock;
            }
        }
        self.cpu_mhz = self
            .cpu_mhz
            .or_else(|| clock_total(None, &self.core_mhz, self.cpu.logical));
    }
}

#[cfg(test)]
mod tests {
    use super::super::CpuThread;
    use super::*;

    fn clocks(
        cpu: &CpuInfo,
        actual: &[(String, f64)],
        nominal: &[Option<u32>],
        performance: impl FnOnce() -> Vec<(String, f64)>,
    ) -> (Option<u32>, Vec<u32>) {
        finish_clocks(cpu, pdh_clocks(cpu, actual), || nominal, performance)
    }

    fn cpu(threads: u8) -> CpuInfo {
        CpuInfo {
            base_mhz: 2500,
            sockets: 1,
            logical: u32::from(threads),
            counter_threads: [(0, (0..usize::from(threads)).map(Some).collect())].into(),
            threads: (0..threads)
                .map(|processor| CpuThread {
                    group: 0,
                    processor,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    fn values(items: &[(&str, f64)]) -> Vec<(String, f64)> {
        items
            .iter()
            .map(|(name, value)| ((*name).into(), *value))
            .collect()
    }

    #[test]
    fn hybrid_direct_mhz_takes_priority_and_missing_references_stay_unknown() {
        let mut cpu = cpu(22);
        cpu.base_mhz = 3000;
        cpu.efficiency_cores = 10;
        for thread in &mut cpu.threads[12..] {
            thread.efficient = true;
        }
        let mut actual: Vec<_> = (0..22)
            .map(|index| {
                (
                    format!("0,{index}"),
                    if index < 12 {
                        4800.0
                    } else if index < 20 {
                        3800.0
                    } else {
                        2500.0
                    },
                )
            })
            .collect();
        actual.push(("_Total".into(), 4227.27));
        let performance = values(&[("_Total", 300.0), ("0,0", 342.85)]);
        assert_eq!(performance_reference(&cpu), None);
        let (total, per_core) = clocks(&cpu, &actual, &[None; 22], || {
            panic!("all direct readings are present")
        });
        assert_eq!(total, Some(4227));
        assert_eq!(per_core[0], 4800);
        assert_eq!(per_core[12], 3800);
        assert_eq!(per_core[20], 2500);
        assert_eq!(clocks(&cpu, &[], &[None; 22], || performance).0, None);
    }

    #[test]
    fn windows_10_hybrid_clocks_use_each_processors_nominal_mhz() {
        let mut cpu = cpu(32);
        cpu.hybrid = true;
        cpu.base_mhz = 2400;
        cpu.efficiency_cores = 16;
        for thread in &mut cpu.threads[16..] {
            thread.efficient = true;
        }
        let mut nominal = vec![Some(2400); 32];
        nominal[16..].fill(Some(1800));
        let mut performance: Vec<_> = (0..32).rev().map(|i| (format!("0,{i}"), 150.0)).collect();
        performance.extend(values(&[("_Total", 300.0), ("0,_Total", 300.0)]));
        let missing = values(&[("_Total", 0.0), ("0,0", 0.0), ("0,31", f64::NAN)]);
        let (total, cores) = clocks(&cpu, &missing, &nominal, || performance.clone());
        assert_eq!(total, Some(3150));
        assert_eq!(cores[..16], [3600; 16]);
        assert_eq!(cores[16..], [2700; 16]);

        nominal[31] = None;
        let (total, cores) = clocks(&cpu, &[], &nominal, || performance.clone());
        assert_eq!(total, None);
        assert_eq!(cores[31], 0);
        let direct = values(&[("0,31", 3500.0)]);
        let (total, cores) = clocks(&cpu, &direct, &nominal, || performance.clone());
        assert_eq!(total, Some(3175));
        assert_eq!(cores[31], 3500);

        // A shared reference reported by every processor also covers a provider
        // that only returns the system-wide percentage (e.g. parked threads).
        assert_eq!(
            clocks(&cpu, &[], &[Some(2400); 32], || values(&[(
                "_Total", 150.0
            )])),
            (Some(3600), vec![0; 32])
        );
    }

    #[test]
    fn homogeneous_fallback_preserves_turbo_and_fills_only_missing_readings() {
        let cpu = cpu(2);
        let performance = values(&[("0,0", 160.0), ("0,1", 120.0), ("_Total", 140.0)]);
        assert_eq!(performance_reference(&cpu), Some(2500));
        let references = [performance_reference(&cpu); 2];
        assert_eq!(
            clocks(&cpu, &[], &references, || performance.clone()),
            (Some(3500), vec![4000, 3000])
        );
        let actual = values(&[("0,0", 3800.0), ("0,1", 2800.0), ("_Total", 3300.0)]);
        assert_eq!(
            clocks(&cpu, &actual, &references, || performance.clone()),
            (Some(3300), vec![3800, 2800])
        );
        let partial = values(&[("0,0", 3800.0)]);
        assert_eq!(
            clocks(&cpu, &partial, &references, || performance),
            (Some(3500), vec![3800, 3000])
        );
        let mut mixed = cpu;
        mixed.sockets = 2;
        assert_eq!(performance_reference(&mixed), None);
        mixed.sockets = 1;
        mixed.hybrid = true;
        assert_eq!(performance_reference(&mixed), None);
    }

    #[test]
    fn groups_and_rollups_do_not_double_count_or_hide_missing_clocks() {
        let threads = [(0, 0), (0, 1), (1, 0), (1, 2)]
            .into_iter()
            .map(|(group, processor)| CpuThread {
                group,
                processor,
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let mut cpu = CpuInfo {
            threads,
            counter_threads: [(0, vec![Some(0), Some(1)]), (1, vec![Some(2), Some(3)])].into(),
            ..cpu(4)
        };
        let actual = values(&[
            ("1,1", 1000.0),
            ("0,0", 4000.0),
            ("0,_Total", 9000.0),
            ("1,_Total", 8000.0),
            ("1,0", 2000.0),
            ("0,1", 3000.0),
            ("_Total", 9999.0),
        ]);
        assert_eq!(
            clocks(&cpu, &actual, &[None; 4], Vec::new),
            (Some(2500), vec![4000, 3000, 2000, 1000])
        );
        assert_eq!(
            clocks(&cpu, &values(&[("0,0", 4000.0)]), &[None; 4], Vec::new),
            (None, vec![4000, 0, 0, 0])
        );
        assert_eq!(
            clocks(&cpu, &values(&[("_Total", 3500.0)]), &[None; 4], Vec::new).0,
            Some(3500)
        );
        let performance = || {
            values(&[
                ("1,1", 200.0),
                ("0,0", 100.0),
                ("1,0", 150.0),
                ("0,1", 125.0),
                ("1,_Total", 5.0),
                ("_Total", 250.0),
            ])
        };
        let nominal = [Some(2000), Some(2000), Some(1500), Some(1500)];
        assert_eq!(
            clocks(&cpu, &[], &nominal, performance),
            (Some(2438), vec![2000, 2500, 2250, 3000])
        );
        cpu.logical = 5;
        assert_eq!(clocks(&cpu, &[], &[Some(2000); 4], performance).0, None);
    }

    #[test]
    fn sensor_clocks_fill_smt_siblings_only_for_a_matching_physical_core() {
        use super::super::{CpuCoreSensors, CpuSensors, Hardware, SensorState};
        let mut cpu = cpu(3);
        cpu.cores = 2;
        cpu.threads[2].core = 1;
        let mut hw = Hardware {
            cpu: Arc::new(cpu),
            core_mhz: vec![4200, 0, 0],
            cpu_sensors: Some(CpuSensors {
                cores: vec![
                    CpuCoreSensors {
                        core: 1,
                        processor: 0,
                        clock_mhz: Some(9000.),
                        ..Default::default()
                    },
                    CpuCoreSensors {
                        core: 0,
                        processor: 1,
                        clock_mhz: Some(4000.),
                        ..Default::default()
                    },
                    CpuCoreSensors {
                        core: 1,
                        processor: 2,
                        clock_mhz: Some(f32::NAN),
                        effective_mhz: Some(250.),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        };
        hw.supplement_cpu_clocks();
        assert_eq!(hw.core_mhz, [4200, 0, 0]);
        hw.cpu_sensors.as_mut().unwrap().state = SensorState::Ready;
        hw.supplement_cpu_clocks();
        assert_eq!(hw.core_mhz, [4200, 4000, 0]);
        assert_eq!(hw.cpu_mhz, None);
        hw.cpu_sensors.as_mut().unwrap().cores[2].clock_mhz = Some(2600.);
        hw.supplement_cpu_clocks();
        assert_eq!(hw.core_mhz, [4200, 4000, 2600]);
        assert_eq!(hw.cpu_mhz, Some(3600));
        hw.core_mhz.clear();
        hw.cpu_mhz = Some(3550);
        hw.supplement_cpu_clocks();
        assert_eq!(hw.core_mhz, [4000, 4000, 2600]);
        assert_eq!(hw.cpu_mhz, Some(3550));
    }

    #[test]
    fn invalid_clocks_and_percentages_stay_unavailable() {
        let cpu = cpu(1);
        for value in [
            0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            12001.0,
            f64::MAX,
        ] {
            assert_eq!(
                clocks(
                    &cpu,
                    &values(&[("0,0", value), ("_Total", value)]),
                    &[None],
                    Vec::new
                ),
                (None, vec![0])
            );
        }
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            assert_eq!(
                clocks(&cpu, &[], &[Some(2500)], || values(&[
                    ("0,0", value),
                    ("_Total", value)
                ])),
                (None, vec![0])
            );
        }
    }

    #[test]
    #[ignore = "read-only CPU clock counters using the current account"]
    fn live_cpu_clocks() {
        let cpu = Arc::new(super::super::cpu_info());
        let mut sensors = Sensors::new(cpu.clone());
        let named_query = Query::open();
        let named = named_query
            .as_ref()
            .and_then(|q| q.add(w!(r"\Processor Information(*)\Actual Frequency")));
        if let Some(q) = &named_query {
            q.collect();
        }
        println!("{}", cpu.name);
        let mut saved = None;
        for path in [
            "PCW",
            "PCW without PDH",
            "PCW without PDH",
            "performance percentage",
        ] {
            if path == "PCW without PDH" && saved.is_none() {
                saved = sensors.query.take();
            }
            if path == "performance percentage" {
                sensors.query = saved.take();
                sensors.raw_clocks = None;
                sensors.core_freq = None;
            }
            std::thread::sleep(std::time::Duration::from_millis(600));
            let started = std::time::Instant::now();
            let sample = sensors.sample();
            let elapsed = started.elapsed();
            let reference = named
                .filter(|_| named_query.as_ref().is_some_and(Query::collect))
                .and_then(|c| {
                    pdh::array_uncapped(c, &mut sensors.buf)
                        .into_iter()
                        .find(|(n, _)| n == "_Total")
                        .and_then(|(_, v)| valid_mhz(v))
                });
            let cores = sample.core_mhz.iter().filter(|v| **v > 0).count();
            println!(
                "{path}: sample_us={}; selected={:?}; named_pdh={reference:?}; cores={cores}/{}",
                elapsed.as_micros(),
                sample.cpu_mhz,
                cpu.logical
            );
            if path != "performance percentage" {
                let [(n0, b0), (n1, b1)] =
                    sensors.raw_clocks.as_ref().unwrap().total_pair().unwrap();
                let delta = u128::from(n1.checked_sub(n0).unwrap());
                let base = u128::from(b1.wrapping_sub(b0));
                assert_ne!(base, 0);
                let mhz = ((delta + base / 2) / base) as u32;
                println!("_Total: delta33={delta} delta34={base} rounded_mhz={mhz}");
                assert_eq!(sample.cpu_mhz, Some(mhz));
            } else if let Some(perf) = sensors.core_perf {
                for (name, percent) in pdh::array_uncapped(perf, &mut sensors.buf) {
                    if let Some(index) = cpu.counter_index(&name)
                        && let Some(base) = sensors.nominal_mhz.get().and_then(|refs| refs[index])
                    {
                        assert_eq!(
                            sample.core_mhz[index],
                            valid_mhz(f64::from(base) * percent / 100.0).unwrap_or(0)
                        );
                    }
                }
            }
        }
    }

    /// With `CPU` rails allowed, as on ARM64; RAPL behaves the same either way.
    fn power(meters: &[(&str, f64)]) -> (CpuPower, PowerSource) {
        CpuPower::from_meters(&values(meters), true)
    }

    #[test]
    fn idle_power_is_a_reading_and_does_not_toggle_availability() {
        let values = [0.0, 250.0, 0.0, 125.0, 0.0]
            .map(|milliwatts| power(&[("RAPL_Package0_PP1", milliwatts)]).0.graphics);
        assert_eq!(
            values,
            [Some(0.0), Some(0.25), Some(0.0), Some(0.125), Some(0.0)]
        );
    }

    #[test]
    fn power_domains_sum_valid_meters_without_inventing_missing_data() {
        let (power, source) = power(&[
            ("RAPL_Package0_PKG", 12_000.0),
            ("RAPL_Package1_PKG", 8_000.0),
            ("RAPL_Package0_PP0", 0.0),
            ("RAPL_Package0_DRAM", 0.0),
            ("_Total", 999_999.0),
            ("unknown", 123.0),
        ]);
        assert_eq!(source, PowerSource::Rapl);
        assert_eq!(power.package, Some(20.0));
        assert_eq!(power.cores, Some(0.0));
        assert_eq!(power.memory, Some(0.0));
        assert_eq!(power.graphics, None);
    }

    #[test]
    fn invalid_power_values_do_not_poison_a_valid_domain() {
        for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            let (p, _) = power(&[
                ("RAPL_Package0_PP1", value),
                ("RAPL_Package0_PKG", 10_000.0),
                ("RAPL_Package1_PKG", value),
            ]);
            assert_eq!(p.graphics, None);
            assert_eq!(p.package, Some(10.0));
            let (p, source) = power(&[("CPU_CLUSTER_0", value), ("CPU_CLUSTER_1", value)]);
            assert_eq!((p.cores, source), (None, PowerSource::None));
        }
    }

    #[test]
    fn cpu_rails_follow_microsofts_rail_taxonomy() {
        // An exact `CPU` rail is the whole component.
        let (p, source) = power(&[
            ("CPU_CLUSTER_0", 1_000.0),
            ("CPU", 3_000.0),
            ("GPU", 2_000.0),
        ]);
        assert_eq!((p.cores, source), (Some(3.0), PowerSource::CpuRails));
        // Otherwise the `CPU_*` rails add up, in any case.
        let (p, _) = power(&[
            ("cpu_cluster_0", 1_500.0),
            ("CPU_CLUSTER_1", 0.0),
            ("CPU_CLUSTER_2", f64::NAN),
            ("npu", 900.0),
            ("soc", 900.0),
            ("system", 9_000.0),
        ]);
        assert_eq!(p.cores, Some(1.5));
        assert_eq!((p.package, p.graphics, p.memory), (None, None, None));
        assert_eq!(power(&[("CPU_CLUSTER_0", 0.0)]).0.cores, Some(0.0));
        for name in ["CPU_", "CPUS", "GPU_CPU", "MCPU", "_Total"] {
            assert_eq!(power(&[(name, 1_000.0)]).1, PowerSource::None, "{name}");
        }
    }

    #[test]
    fn rapl_cpu_meters_keep_other_rails_out() {
        let (p, source) = power(&[("RAPL_Package0_PKG", 10_000.0), ("CPU", 4_000.0)]);
        assert_eq!((p.package, p.cores), (Some(10.0), None));
        assert_eq!(source, PowerSource::Rapl);
        // A RAPL meter decides the source even while its reading is invalid,
        // so the source cannot flip from one sample to the next.
        let (p, source) = power(&[("RAPL_Package0_PP0", f64::NAN), ("CPU", 4_000.0)]);
        assert_eq!((p.cores, source), (None, PowerSource::Rapl));
        // Graphics or memory meters alone do not measure the CPU.
        let (p, source) = power(&[("RAPL_Package0_DRAM", 1_000.0), ("CPU_CLUSTER_0", 2_000.0)]);
        assert_eq!((p.memory, p.cores), (Some(1.0), Some(2.0)));
        assert_eq!(source, PowerSource::CpuRails);
    }

    #[test]
    fn x64_reads_rapl_suffixes_and_ignores_cpu_rails() {
        let meters = values(&[
            ("CPU", 4_000.0),
            ("CPU_CLUSTER_0", 1_000.0),
            ("RAPL_Package0_DRAM", 1_000.0),
        ]);
        let (p, source) = CpuPower::from_meters(&meters, false);
        assert_eq!(
            (p.cores, p.memory, source),
            (None, Some(1.0), PowerSource::None)
        );
        // Any `_PKG` suffix is a package meter here, as before CPU rails.
        let (p, source) = CpuPower::from_meters(&values(&[("CPU_PKG", 7_000.0)]), false);
        assert_eq!((p.package, source), (Some(7.0), PowerSource::Rapl));
        assert_eq!(CPU_RAILS, cfg!(target_arch = "aarch64"));
    }

    #[test]
    fn logged_sources_are_bounded_and_escaped() {
        let none: [(String, f64); 1] = [("_Total".into(), 1.0)];
        assert_eq!(source_list(&none, |v| v.to_string()), "none");
        let named = [
            (r"\_SB.TZ0".to_string(), 41.3_f32),
            ("bad\nname\u{7}".into(), -40.0),
            ("x".repeat(100), 1.0),
        ];
        let line = source_list(&named, |c| format!("{c:.1} C"));
        assert_eq!(
            line,
            format!(
                r"\_SB.TZ0=41.3 C, bad\nname\u{{7}}=-40.0 C, {}=1.0 C",
                "x".repeat(64)
            )
        );
        let many: Vec<(String, f64)> = (0..70).map(|i| (format!("CPU_{i}"), 0.0)).collect();
        let line = source_list(&many, |_| "0".into());
        assert!(line.starts_with("CPU_0=0, CPU_1=0"));
        assert!(line.ends_with("CPU_63=0, 6 more"), "{line}");
    }
    #[test]
    fn idle_instances_use_numa_node_indices_and_ignore_rollups() {
        let mut cpu = cpu(4);
        cpu.counter_threads = [(0, vec![Some(0), Some(2)]), (7, vec![Some(1), Some(3)])].into();
        assert_eq!(cpu.counter_index("7,1"), Some(3));
        assert_eq!(cpu.counter_index("0,1"), Some(2));
        for value in ["_Total", "0,_Total", "0,64", "9999,1", "0,-1"] {
            assert_eq!(cpu.counter_index(value), None);
        }
    }
}
