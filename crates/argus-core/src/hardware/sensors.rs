//! Readings Windows publishes as performance counters, collected in one PDH
//! query per sample: CPU clock (overall and per logical processor), ACPI
//! thermal zones, and RAPL power meters. None of them needs a driver or
//! administrator rights; counters a machine lacks are simply absent.

use serde::Serialize;
use windows::Win32::System::Performance::PDH_HCOUNTER;
use windows::core::w;

use super::{CpuInfo, CpuThread};
use crate::pdh::{self, Query};
use crate::util::AlignedBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThermalZone {
    /// ACPI name without the `\_TZ.` prefix, e.g. `TZ01`.
    pub name: String,
    pub celsius: f32,
}

/// RAPL power domains, in watts; None when no valid reading is available.
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

impl CpuPower {
    fn add_meter(&mut self, name: &str, milliwatts: f64) {
        // PDH already checked counter status. Zero is a valid idle reading,
        // not an absent domain; invalid readings must not turn into zero.
        if !milliwatts.is_finite() || milliwatts < 0.0 || name == "_Total" {
            return;
        }
        let slot = match name.rsplit('_').next() {
            Some("PKG") => &mut self.package,
            Some("PP0") => &mut self.cores,
            Some("PP1") => &mut self.graphics,
            Some("DRAM") => &mut self.memory,
            _ => return,
        };
        // Several packages add up.
        let watts = f64::from(slot.unwrap_or(0.0)) + milliwatts / 1000.0;
        if watts <= f64::from(f32::MAX) {
            *slot = Some(watts as f32);
        }
    }
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
    query: Query,
    buf: AlignedBuf,
    fallback_mhz: Option<u32>,
    threads: Vec<CpuThread>,
    /// Direct MHz readings for logical processors and their total.
    core_freq: Option<PDH_HCOUNTER>,
    /// Nominal-scaled fallback for a single homogeneous processor package.
    core_perf: Option<PDH_HCOUNTER>,
    zones: Option<PDH_HCOUNTER>,
    /// Zones in tenths of a kelvin (`High Precision Temperature`) rather than kelvin.
    zones_precise: bool,
    energy: Option<PDH_HCOUNTER>,
    idle: [Option<PDH_HCOUNTER>; 3],
}

// PDH handles are process-wide; the sampler uses them from one thread at a time.
unsafe impl Send for Sensors {}

impl Sensors {
    pub fn new(cpu: &CpuInfo) -> Option<Self> {
        let query = Query::open()?;
        let add = |path| query.add(path);
        let core_freq = add(w!(r"\Processor Information(*)\Actual Frequency"));
        let fallback_mhz = performance_reference(cpu);
        let core_perf = fallback_mhz
            .and_then(|_| add(w!(r"\Processor Information(*)\% Processor Performance")));
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
        query.collect();
        Some(Self {
            query,
            buf: AlignedBuf::with_capacity(16 * 1024),
            fallback_mhz,
            threads: cpu.threads.clone(),
            core_freq,
            core_perf,
            zones,
            zones_precise,
            energy,
            idle,
        })
    }

    pub fn sample(&mut self) -> SensorSample {
        let mut out = SensorSample::default();
        if !self.query.collect() {
            return out;
        }
        let actual = self
            .core_freq
            .map(|c| pdh::array_uncapped(c, &mut self.buf))
            .unwrap_or_default();
        let performance = self
            .core_perf
            .map(|c| pdh::array_uncapped(c, &mut self.buf))
            .unwrap_or_default();
        (out.cpu_mhz, out.core_mhz) =
            clocks(&self.threads, &actual, &performance, self.fallback_mhz);
        out.idle.resize_with(self.threads.len(), CpuIdle::default);
        for (state, counter) in self.idle.iter().enumerate() {
            let Some(counter) = counter else { continue };
            for (name, value) in pdh::array(*counter, &mut self.buf) {
                let Some(index) = processor_index(&name, &self.threads) else {
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

        if let Some(c) = self.zones {
            for (name, raw) in pdh::array(c, &mut self.buf) {
                let kelvin = if self.zones_precise { raw / 10.0 } else { raw };
                let celsius = (kelvin - 273.15) as f32;
                // Zero kelvin or 0 °C means "not reported".
                if celsius > 1.0 && celsius < 150.0 {
                    let name = name.trim_start_matches(r"\_TZ.").to_string();
                    out.thermal.push(ThermalZone { name, celsius });
                }
            }
        }

        if let Some(c) = self.energy {
            for (name, milliwatts) in pdh::array(c, &mut self.buf) {
                out.power.add_meter(&name, milliwatts);
            }
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

fn valid_mhz(value: f64) -> Option<u32> {
    (value.is_finite() && value > 0.0 && value <= 12000.0)
        .then(|| value.round() as u32)
        .filter(|value| *value > 0)
}

fn clocks(
    threads: &[CpuThread],
    actual: &[(String, f64)],
    performance: &[(String, f64)],
    fallback_mhz: Option<u32>,
) -> (Option<u32>, Vec<u32>) {
    let mut cores = vec![0; threads.len()];
    let mut total = None;
    for (name, value) in actual {
        if name == "_Total" {
            total = valid_mhz(*value);
        } else if let Some(index) = processor_index(name, threads) {
            cores[index] = valid_mhz(*value).unwrap_or(0);
        }
    }
    let direct_complete = !cores.is_empty() && cores.iter().all(|value| *value > 0);
    let mut fallback_total = None;
    if let Some(base) = fallback_mhz {
        for (name, value) in performance {
            let mhz = valid_mhz(f64::from(base) * value / 100.0);
            if name == "_Total" {
                fallback_total = mhz;
            } else if let Some(index) = processor_index(name, threads)
                && cores[index] == 0
            {
                cores[index] = mhz.unwrap_or(0);
            }
        }
    }
    let complete = !cores.is_empty() && cores.iter().all(|value| *value > 0);
    if complete {
        let low = cores.iter().min().copied().unwrap();
        let high = cores.iter().max().copied().unwrap();
        // A total cannot exceed every contributing logical processor. This
        // also prevents a broken rollup from overriding valid MHz readings.
        if direct_complete {
            total = total.filter(|value| *value >= low.saturating_sub(1) && *value <= high + 1);
        }
        let sum: u64 = cores.iter().map(|value| u64::from(*value)).sum();
        let average = ((sum + cores.len() as u64 / 2) / cores.len() as u64) as u32;
        total = total.or(if direct_complete {
            Some(average)
        } else {
            fallback_total.or(Some(average))
        });
    }
    (total.or(fallback_total), cores)
}

fn processor_index(name: &str, threads: &[CpuThread]) -> Option<usize> {
    let (group, number) = name.split_once(',')?;
    let (group, number) = (group.parse::<u16>().ok()?, number.parse::<u8>().ok()?);
    threads
        .binary_search_by_key(&(group, number), |t| (t.group, t.processor))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(threads: u8) -> CpuInfo {
        CpuInfo {
            base_mhz: 2500,
            sockets: 1,
            logical: u32::from(threads),
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
    fn hybrid_mhz_readings_do_not_use_a_package_reference_or_performance_percentage() {
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
        let (total, per_core) = clocks(
            &cpu.threads,
            &actual,
            &performance,
            performance_reference(&cpu),
        );
        assert_eq!(total, Some(4227));
        assert_eq!(per_core[0], 4800);
        assert_eq!(per_core[12], 3800);
        assert_eq!(per_core[20], 2500);
        assert_eq!(
            clocks(&cpu.threads, &[], &performance, performance_reference(&cpu)).0,
            None
        );
    }

    #[test]
    fn homogeneous_fallback_preserves_turbo_and_fills_only_missing_readings() {
        let cpu = cpu(2);
        let performance = values(&[("0,0", 160.0), ("0,1", 120.0), ("_Total", 140.0)]);
        assert_eq!(performance_reference(&cpu), Some(2500));
        assert_eq!(
            clocks(&cpu.threads, &[], &performance, performance_reference(&cpu)),
            (Some(3500), vec![4000, 3000])
        );
        let actual = values(&[("0,0", 3800.0), ("0,1", 2800.0), ("_Total", 3300.0)]);
        assert_eq!(
            clocks(
                &cpu.threads,
                &actual,
                &performance,
                performance_reference(&cpu)
            ),
            (Some(3300), vec![3800, 2800])
        );
        let partial = values(&[("0,0", 3800.0)]);
        assert_eq!(
            clocks(
                &cpu.threads,
                &partial,
                &performance,
                performance_reference(&cpu)
            ),
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
        let actual = values(&[
            ("1,2", 1000.0),
            ("0,0", 4000.0),
            ("0,_Total", 9000.0),
            ("1,_Total", 8000.0),
            ("1,0", 2000.0),
            ("0,1", 3000.0),
            ("_Total", 9999.0),
        ]);
        assert_eq!(
            clocks(&threads, &actual, &[], None),
            (Some(2500), vec![4000, 3000, 2000, 1000])
        );
        assert_eq!(
            clocks(&threads, &values(&[("0,0", 4000.0)]), &[], None),
            (None, vec![4000, 0, 0, 0])
        );
        assert_eq!(
            clocks(&threads, &values(&[("_Total", 3500.0)]), &[], None).0,
            Some(3500)
        );
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
                    &cpu.threads,
                    &values(&[("0,0", value), ("_Total", value)]),
                    &[],
                    None
                ),
                (None, vec![0])
            );
        }
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            assert_eq!(
                clocks(
                    &cpu.threads,
                    &[],
                    &values(&[("0,0", value), ("_Total", value)]),
                    Some(2500)
                ),
                (None, vec![0])
            );
        }
    }

    #[test]
    #[ignore = "read-only CPU clock counters using the current account"]
    fn live_cpu_clocks() {
        let cpu = super::super::cpu_info();
        let query = Query::open().unwrap();
        let actual = query.add(w!(r"\Processor Information(*)\Actual Frequency"));
        let nominal = query.add(w!(r"\Processor Information(*)\Processor Frequency"));
        let performance = query.add(w!(r"\Processor Information(*)\% Processor Performance"));
        let mut buf = AlignedBuf::with_capacity(16 * 1024);
        query.collect();
        std::thread::sleep(std::time::Duration::from_millis(600));
        query.collect();
        println!(
            "base_mhz={}; performance_reference={:?}",
            cpu.base_mhz,
            performance_reference(&cpu)
        );
        for (name, counter) in [
            ("actual", actual),
            ("nominal", nominal),
            ("performance", performance),
        ] {
            println!("{name}: {:?}", counter.map(|c| pdh::array(c, &mut buf)));
        }
        let mut sensors = Sensors::new(&cpu).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(600));
        let started = std::time::Instant::now();
        let sample = sensors.sample();
        println!(
            "sample_us={}; selected_mhz={:?}; cores={:?}",
            started.elapsed().as_micros(),
            sample.cpu_mhz,
            sample.core_mhz
        );
        if let Some(counter) = sensors.core_freq {
            let values = pdh::array_uncapped(counter, &mut sensors.buf);
            if let Some(total) = values
                .iter()
                .find(|(name, _)| name == "_Total")
                .and_then(|(_, value)| valid_mhz(*value))
            {
                let low = sample
                    .core_mhz
                    .iter()
                    .filter(|value| **value > 0)
                    .min()
                    .copied();
                let high = sample.core_mhz.iter().max().copied();
                if low
                    .zip(high)
                    .is_some_and(|(low, high)| total >= low && total <= high)
                {
                    assert_eq!(sample.cpu_mhz, Some(total));
                    println!("selected aggregate matches the same PDH collection: {total} MHz");
                }
            }
        }
    }

    #[test]
    fn idle_power_is_a_reading_and_does_not_toggle_availability() {
        let values = [0.0, 250.0, 0.0, 125.0, 0.0].map(|milliwatts| {
            let mut power = CpuPower::default();
            power.add_meter("0_PP1", milliwatts);
            power.graphics
        });
        assert_eq!(
            values,
            [Some(0.0), Some(0.25), Some(0.0), Some(0.125), Some(0.0)]
        );
    }

    #[test]
    fn power_domains_sum_valid_meters_without_inventing_missing_data() {
        let mut power = CpuPower::default();
        assert_eq!(power.graphics, None);
        power.add_meter("0_PKG", 12_000.0);
        power.add_meter("1_PKG", 8_000.0);
        power.add_meter("0_PP0", 0.0);
        power.add_meter("0_DRAM", 0.0);
        power.add_meter("_Total", 999_999.0);
        power.add_meter("unknown", 123.0);
        assert_eq!(power.package, Some(20.0));
        assert_eq!(power.cores, Some(0.0));
        assert_eq!(power.memory, Some(0.0));
        assert_eq!(power.graphics, None);
    }

    #[test]
    fn invalid_power_values_do_not_poison_a_valid_domain() {
        for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            let mut power = CpuPower::default();
            power.add_meter("0_PP1", value);
            assert_eq!(power.graphics, None);
            power.add_meter("0_PKG", 10_000.0);
            power.add_meter("1_PKG", value);
            assert_eq!(power.package, Some(10.0));
        }
    }
    #[test]
    fn idle_instances_preserve_groups_and_ignore_rollups() {
        let threads: Vec<_> = [(0, 0), (0, 1), (1, 0), (1, 2)]
            .into_iter()
            .map(|(group, processor)| CpuThread {
                group,
                processor,
                ..Default::default()
            })
            .collect();
        assert_eq!(processor_index("1,2", &threads), Some(3));
        assert_eq!(processor_index("1,1", &threads), None);
        for value in ["_Total", "0,_Total", "0,64", "9999,1", "0,-1"] {
            assert_eq!(processor_index(value, &threads), None);
        }
    }
}
