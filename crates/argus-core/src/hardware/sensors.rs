//! Readings Windows publishes as performance counters, collected in one PDH
//! query per sample: CPU clock (overall and per logical processor), ACPI
//! thermal zones, and RAPL power meters. None of them needs a driver or
//! administrator rights; counters a machine lacks are simply absent.

use serde::Serialize;
use windows::Win32::System::Performance::PDH_HCOUNTER;
use windows::core::w;

use super::CpuThread;
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
    /// Effective clock in MHz, the way Task Manager reports it.
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
    base_mhz: u32,
    threads: Vec<CpuThread>,
    total_perf: Option<PDH_HCOUNTER>,
    /// MHz per processor (`Actual Frequency`, Windows 11) ...
    core_freq: Option<PDH_HCOUNTER>,
    /// ... or, on older systems, % of base clock per processor.
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
    pub fn new(base_mhz: u32, threads: &[CpuThread]) -> Option<Self> {
        let query = Query::open()?;
        let add = |path| query.add(path);
        let total_perf = (base_mhz > 0)
            .then(|| {
                add(w!(
                    r"\Processor Information(_Total)\% Processor Performance"
                ))
            })
            .flatten();
        let core_freq = add(w!(r"\Processor Information(*)\Actual Frequency"));
        let core_perf = if core_freq.is_none() && base_mhz > 0 {
            add(w!(r"\Processor Information(*)\% Processor Performance"))
        } else {
            None
        };
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
            base_mhz,
            threads: threads.to_vec(),
            total_perf,
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
        let base = self.base_mhz as f64;
        out.cpu_mhz = self
            .total_perf
            .and_then(pdh::value)
            .filter(|&pct| pct > 0.0)
            .map(|pct| (base * pct / 100.0).round() as u32);

        let per_core = if let Some(c) = self.core_freq {
            pdh::array(c, &mut self.buf)
        } else if let Some(c) = self.core_perf {
            pdh::array(c, &mut self.buf)
                .into_iter()
                .map(|(name, pct)| (name, base * pct / 100.0))
                .collect()
        } else {
            Vec::new()
        };
        out.core_mhz.resize(self.threads.len(), 0);
        out.idle.resize_with(self.threads.len(), CpuIdle::default);
        for (name, mhz) in per_core {
            // Instances are "group,number"; skip the "_Total" rollups.
            let Some(index) = processor_index(&name, &self.threads) else {
                continue;
            };
            if !mhz.is_finite() || !(0.0..=12000.0).contains(&mhz) {
                continue;
            }
            out.core_mhz[index] = mhz.round() as u32;
        }
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
