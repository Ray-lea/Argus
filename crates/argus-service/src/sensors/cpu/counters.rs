use std::time::Instant;

use super::{Identity, valid};

#[derive(Default)]
pub(super) struct EnergyCounter {
    previous: Option<(u32, Instant, f64)>,
    observed: bool,
}

impl EnergyCounter {
    pub fn sample(&mut self, value: Option<u64>, unit: f64, now: Instant) -> Option<f32> {
        let Some(value) = value.filter(|_| unit.is_finite() && unit > 0.0) else {
            self.previous = None;
            return None;
        };
        self.observed |= value != 0;
        let previous = self.previous.replace((value as u32, now, unit));
        let (before, at, old_unit) = previous?;
        if !self.observed || old_unit != unit {
            return None;
        }
        super::energy_delta(
            before,
            value as u32,
            now.duration_since(at).as_secs_f64(),
            unit,
        )
    }
}

#[derive(Default)]
pub(super) struct ClockCounter {
    previous: Option<(u64, u64, Instant)>,
}

impl ClockCounter {
    pub fn sample(
        &mut self,
        aperf: Option<u64>,
        mperf: Option<u64>,
        tsc_mhz: Option<f32>,
        now: Instant,
    ) -> (Option<f32>, Option<f32>) {
        let (Some(aperf), Some(mperf)) = (aperf, mperf) else {
            self.previous = None;
            return (None, None);
        };
        let Some((a, m, at)) = self.previous.replace((aperf, mperf, now)) else {
            return (None, None);
        };
        let seconds = now.duration_since(at).as_secs_f64();
        if !(0.05..=30.0).contains(&seconds) || aperf < a || mperf < m {
            return (None, None);
        }
        let effective = valid((aperf - a) as f64 / seconds / 1e6, 0.0, 12000.0);
        let activity = tsc_mhz.and_then(|tsc| {
            valid(
                (mperf - m) as f64 / seconds / (tsc as f64 * 1e6) * 100.0,
                0.0,
                105.0,
            )
            .map(|v| v.min(100.0))
        });
        (effective, activity)
    }
}

pub(super) fn tsc_mhz() -> Option<f32> {
    use std::arch::x86_64::{__cpuid, _mm_lfence, _rdtsc};
    let max = __cpuid(0).eax;
    if max >= 0x15 {
        let value = __cpuid(0x15);
        if value.eax != 0 && value.ebx != 0 && value.ecx != 0 {
            return valid(
                value.ecx as f64 * value.ebx as f64 / value.eax as f64 / 1e6,
                100.0,
                10000.0,
            );
        }
    }
    let max_extended = __cpuid(0x80000000).eax;
    if max_extended < 0x80000007 || __cpuid(0x80000007).edx & (1 << 8) == 0 {
        return None;
    }
    unsafe { _mm_lfence() };
    let before = unsafe { _rdtsc() };
    let at = Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(25));
    unsafe { _mm_lfence() };
    let after = unsafe { _rdtsc() };
    valid(
        (after.checked_sub(before)?) as f64 / at.elapsed().as_secs_f64() / 1e6,
        100.0,
        10000.0,
    )
}

pub(super) fn amd_clock(family: u32, status: u64, bus_mhz: f32) -> Option<f32> {
    let ratio = if family == 0x1a {
        (status & 0xfff) as f64 * 0.05
    } else {
        let divisor = (status >> 8) & 0x3f;
        if divisor == 0 {
            return None;
        }
        2.0 * (status & 0xff) as f64 / divisor as f64
    };
    valid(ratio * bus_mhz as f64, 100.0, 12000.0)
}

pub(super) fn intel_vid(id: Identity, status: u64) -> Option<f32> {
    // PERF_STATUS's high word is a VID on these known Core families. Never
    // fall back to its low word: that contains the frequency ratio, not VID.
    if !matches!(
        id.model,
        0x2a | 0x2d
            | 0x3a
            | 0x3c
            | 0x3f
            | 0x45
            | 0x46
            | 0x3d
            | 0x47
            | 0x4f
            | 0x56
            | 0x4e
            | 0x5e
            | 0x8e
            | 0x9e
            | 0x66
            | 0x7d
            | 0x7e
            | 0x8c
            | 0x8d
            | 0xa5
            | 0xa6
            | 0xa7
            | 0x97
            | 0x9a
            | 0xb7
            | 0xba
            | 0xbf
            | 0xaa
            | 0xac
            | 0xb5
            | 0xc5
            | 0xc6
            | 0xbd
    ) {
        return None;
    }
    valid(((status >> 32) & 0xffff) as f64 / 8192.0, 0.1, 2.0)
}

pub(super) fn legacy_vid(id: Identity, status: u64, northbridge: bool) -> Option<f32> {
    let svi2 = (id.family == 0x15 && id.model >= 0x10) || id.family == 0x16;
    let vid = if northbridge {
        (status >> if svi2 { 24 } else { 25 }) & if svi2 { 255 } else { 127 }
    } else {
        ((status >> 9) & 127) | if svi2 { (status >> 13) & 128 } else { 0 }
    };
    valid(
        1.55 - vid as f64 * if svi2 { 0.00625 } else { 0.0125 },
        0.1,
        1.55,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn counters_reprime_after_gaps_reset_and_unit_changes() {
        let at = Instant::now();
        let mut energy = EnergyCounter::default();
        assert_eq!(energy.sample(Some(u32::MAX as u64 - 99), 0.125, at), None);
        assert_eq!(
            energy.sample(Some(100), 0.125, at + Duration::from_secs(1)),
            Some(25.0)
        );
        assert_eq!(
            energy.sample(None, 0.125, at + Duration::from_secs(2)),
            None
        );
        assert_eq!(
            energy.sample(Some(300), 0.125, at + Duration::from_secs(3)),
            None
        );
        assert_eq!(
            energy.sample(Some(400), 0.25, at + Duration::from_secs(4)),
            None
        );
        assert_eq!(
            energy.sample(Some(400), 0.25, at + Duration::from_secs(5)),
            Some(0.0)
        );
        let mut clock = ClockCounter::default();
        assert_eq!(
            clock.sample(Some(0), Some(0), Some(4000.0), at),
            (None, None)
        );
        assert_eq!(
            clock.sample(
                Some(500_000_000),
                Some(400_000_000),
                Some(4000.0),
                at + Duration::from_secs(1)
            ),
            (Some(500.0), Some(10.0))
        );
        assert_eq!(
            clock.sample(
                Some(500_000_000),
                Some(400_000_000),
                Some(4000.0),
                at + Duration::from_secs(2)
            ),
            (Some(0.0), Some(0.0))
        );
        assert_eq!(
            clock.sample(Some(0), Some(0), Some(4000.0), at + Duration::from_secs(3)),
            (None, None)
        );
        assert_eq!(
            clock.sample(
                Some(500_000_000),
                Some(400_000_000),
                Some(4000.0),
                at + Duration::from_secs(34)
            ),
            (None, None)
        );
    }

    #[test]
    fn zen5_frequency_uses_the_twelve_bit_fid_and_vid_never_uses_ratio_bits() {
        assert_eq!(amd_clock(0x1a, 1100, 100.0), Some(5500.0));
        assert_eq!(amd_clock(0x19, 180 | (8 << 8), 100.0), Some(4500.0));
        assert_eq!(amd_clock(0x19, 180, 100.0), None);
        let id = Identity {
            intel: true,
            family: 6,
            model: 0xba,
            thermal: 0,
        };
        assert_eq!(intel_vid(id, 42 << 8), None);
        assert_eq!(
            intel_vid(id, 9830u64 << 32).map(|v| (v * 100.0).round()),
            Some(120.0)
        );
    }
}
