//! Shared throughput-axis steps. Samples and returned limits stay in bytes/s.

use super::{GIB, KIB, MIB};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::frontend) enum RateScale {
    Disk,
    Network(bool),
}

impl RateScale {
    pub(in crate::frontend) fn bits(self) -> bool {
        matches!(self, Self::Network(true))
    }

    fn steps(self, kilo: bool) -> &'static [f64] {
        match (self, kilo) {
            (Self::Disk, true) => &[100., 200., 500.],
            (Self::Disk, false) => &[1., 10., 50., 100., 200., 500.],
            (Self::Network(_), true) => &[100., 500.],
            (Self::Network(_), false) => &[1., 5., 10., 50., 100., 500.],
        }
    }
}

fn units(bits: bool) -> [(f64, &'static str); 6] {
    if bits {
        [
            (1e3 / 8., "Kbps"),
            (1e6 / 8., "Mbps"),
            (1e9 / 8., "Gbps"),
            (1e12 / 8., "Tbps"),
            (1e15 / 8., "Pbps"),
            (1e18 / 8., "Ebps"),
        ]
    } else {
        [
            (KIB, "KB/s"),
            (MIB, "MB/s"),
            (GIB, "GB/s"),
            (GIB * KIB, "TB/s"),
            (GIB * MIB, "PB/s"),
            (GIB * GIB, "EB/s"),
        ]
    }
}

/// The first configured step that contains the observed peak, with a 100 K floor.
pub(in crate::frontend) fn ceiling(peak: f64, scale: RateScale) -> f64 {
    let peak = if peak.is_finite() { peak.max(0.) } else { 0. };
    for (index, (size, _)) in units(scale.bits()).into_iter().enumerate() {
        for step in scale.steps(index == 0) {
            let top = step * size;
            if top >= peak {
                return top;
            }
        }
    }
    // The steps cover all u64 byte counters. Keep synthetic larger inputs finite.
    peak
}

pub(in crate::frontend) fn label(top: f64, bits: bool) -> String {
    let units = units(bits);
    let (size, unit) = units
        .iter()
        .rev()
        .find(|(size, _)| top >= *size)
        .unwrap_or(&units[0]);
    format!("{} {unit}", (top / size).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_uses_requested_steps_and_repeats_above_gigabytes() {
        let steps = [
            100. * KIB,
            200. * KIB,
            500. * KIB,
            MIB,
            10. * MIB,
            50. * MIB,
            100. * MIB,
            200. * MIB,
            500. * MIB,
            GIB,
            10. * GIB,
            50. * GIB,
            100. * GIB,
            200. * GIB,
            500. * GIB,
            GIB * KIB,
            10. * GIB * KIB,
        ];
        assert_eq!(ceiling(0., RateScale::Disk), steps[0]);
        for (index, &top) in steps.iter().enumerate() {
            assert_eq!(ceiling(top, RateScale::Disk), top);
            assert_eq!(ceiling(top - 1., RateScale::Disk), top);
            if let Some(&next) = steps.get(index + 1) {
                assert_eq!(ceiling(top + 1., RateScale::Disk), next);
            }
        }
        assert_eq!(label(steps[15], false), "1 TB/s");
    }

    #[test]
    fn network_bits_and_bytes_share_numbers_with_correct_throughput_conversion() {
        let numbers = [100., 500., 1., 5., 10., 50., 100., 500., 1., 5., 10.];
        let unit_indices = [0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2];
        for bits in [false, true] {
            let units = units(bits);
            let scale = RateScale::Network(bits);
            assert_eq!(ceiling(0., scale), 100. * units[0].0);
            for (index, (&number, &unit)) in numbers.iter().zip(&unit_indices).enumerate() {
                let top = number * units[unit].0;
                assert_eq!(ceiling(top, scale), top);
                assert_eq!(ceiling(top - 1., scale), top);
                assert_eq!(label(top, bits), format!("{} {}", number, units[unit].1));
                if index + 1 < numbers.len() {
                    assert_eq!(
                        ceiling(top + 1., scale),
                        numbers[index + 1] * units[unit_indices[index + 1]].0
                    );
                }
            }
        }
        // A 20,000-byte/s peak fits 100 KB/s, but needs the 500 Kbps step in bit mode.
        assert_eq!(ceiling(20_000., RateScale::Network(false)), 100. * KIB);
        assert_eq!(ceiling(20_000., RateScale::Network(true)), 500_000. / 8.);
        assert_eq!(ceiling(2. * MIB, RateScale::Disk), 10. * MIB);
        assert_eq!(ceiling(2. * MIB, RateScale::Network(false)), 5. * MIB);
    }

    #[test]
    fn rate_limits_handle_unknown_idle_and_full_counter_range() {
        for scale in [
            RateScale::Disk,
            RateScale::Network(false),
            RateScale::Network(true),
        ] {
            let floor = ceiling(0., scale);
            for peak in [-1., f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
                assert_eq!(ceiling(peak, scale), floor);
            }
            for peak in [
                1.,
                100_001.,
                1e6,
                1e9,
                1e12,
                1e15,
                u64::MAX as f64,
                f64::MAX,
            ] {
                let top = ceiling(peak, scale);
                assert!(top.is_finite() && top >= peak);
            }
        }
    }
}
