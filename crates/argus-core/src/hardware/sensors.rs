//! Readings Windows publishes as performance counters, collected in one PDH
//! query per sample: CPU clock (overall and per logical processor), ACPI
//! thermal zones, and RAPL power meters. None of them needs a driver or
//! administrator rights; counters a machine lacks are simply absent.

use serde::Serialize;
use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue, PdhOpenQueryW,
};
use windows::core::{PCWSTR, w};

use crate::util::AlignedBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThermalZone {
    /// ACPI name without the `\_TZ.` prefix, e.g. `TZ01`.
    pub name: String,
    pub celsius: f32,
}

/// RAPL power domains, in watts; None where the platform does not meter it.
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

#[derive(Debug, Clone, Default)]
pub struct SensorSample {
    /// Effective clock in MHz, the way Task Manager reports it.
    pub cpu_mhz: Option<u32>,
    /// Per logical processor (index = processor number), MHz; empty if unknown.
    pub core_mhz: Vec<u32>,
    pub thermal: Vec<ThermalZone>,
    pub power: CpuPower,
}

pub(crate) struct Sensors {
    query: PDH_HQUERY,
    base_mhz: u32,
    total_perf: Option<PDH_HCOUNTER>,
    /// MHz per processor (`Actual Frequency`, Windows 11) ...
    core_freq: Option<PDH_HCOUNTER>,
    /// ... or, on older systems, % of base clock per processor.
    core_perf: Option<PDH_HCOUNTER>,
    zones: Option<PDH_HCOUNTER>,
    /// Zones in tenths of a kelvin (`High Precision Temperature`) rather than kelvin.
    zones_precise: bool,
    energy: Option<PDH_HCOUNTER>,
}

// PDH handles are process-wide; the sampler uses them from one thread at a time.
unsafe impl Send for Sensors {}

impl Sensors {
    pub fn new(base_mhz: u32) -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return None;
        }
        let add = |path: PCWSTR| {
            let mut counter = PDH_HCOUNTER::default();
            (unsafe { PdhAddEnglishCounterW(query, path, 0, &mut counter) } == 0).then_some(counter)
        };
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
        // Rate counters need a first collection to have something to diff against.
        unsafe { PdhCollectQueryData(query) };
        Some(Self {
            query,
            base_mhz,
            total_perf,
            core_freq,
            core_perf,
            zones,
            zones_precise,
            energy,
        })
    }

    pub fn sample(&mut self) -> SensorSample {
        let mut out = SensorSample::default();
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return out;
        }
        let base = self.base_mhz as f64;
        out.cpu_mhz = self
            .total_perf
            .and_then(value)
            .filter(|&pct| pct > 0.0)
            .map(|pct| (base * pct / 100.0).round() as u32);

        let per_core = if let Some(c) = self.core_freq {
            array(c)
        } else if let Some(c) = self.core_perf {
            array(c)
                .into_iter()
                .map(|(name, pct)| (name, base * pct / 100.0))
                .collect()
        } else {
            Vec::new()
        };
        for (name, mhz) in per_core {
            // Instances are "group,number"; skip the "_Total" rollups.
            let Some((group, number)) = name.split_once(',') else {
                continue;
            };
            let (Ok(group), Ok(number)) = (group.parse::<usize>(), number.parse::<usize>()) else {
                continue;
            };
            let index = group * 64 + number;
            if out.core_mhz.len() <= index {
                out.core_mhz.resize(index + 1, 0);
            }
            out.core_mhz[index] = mhz.round() as u32;
        }

        if let Some(c) = self.zones {
            for (name, raw) in array(c) {
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
            let mut power = CpuPower::default();
            for (name, milliwatts) in array(c) {
                if milliwatts <= 0.0 || name == "_Total" {
                    continue;
                }
                let watts = (milliwatts / 1000.0) as f32;
                let slot = match name.rsplit('_').next() {
                    Some("PKG") => &mut power.package,
                    Some("PP0") => &mut power.cores,
                    Some("PP1") => &mut power.graphics,
                    Some("DRAM") => &mut power.memory,
                    _ => continue,
                };
                // Several packages add up.
                *slot = Some(slot.unwrap_or(0.0) + watts);
            }
            out.power = power;
        }
        out
    }
}

impl Drop for Sensors {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.query) };
    }
}

fn value(counter: PDH_HCOUNTER) -> Option<f64> {
    let mut v = PDH_FMT_COUNTERVALUE::default();
    (unsafe { PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, None, &mut v) } == 0)
        .then_some(unsafe { v.Anonymous.doubleValue })
}

/// Every instance of a wildcard counter, as (instance name, value).
fn array(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
    let (mut size, mut count) = (0u32, 0u32);
    let status = unsafe {
        PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None)
    };
    if status != PDH_MORE_DATA || size == 0 {
        return Vec::new();
    }
    // The items are followed by the instance name strings they point into.
    let mut buf = AlignedBuf::with_capacity(size as usize);
    let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    if unsafe {
        PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items))
    } != 0
    {
        return Vec::new();
    }
    (0..count as usize)
        .filter_map(|i| {
            let item = unsafe { &*items.add(i) };
            // 0 and 1 are "valid" and "new data"; anything else failed this round.
            if item.FmtValue.CStatus > 1 {
                return None;
            }
            let name = unsafe { item.szName.to_string() }.ok()?;
            Some((name, unsafe { item.FmtValue.Anonymous.doubleValue }))
        })
        .collect()
}
