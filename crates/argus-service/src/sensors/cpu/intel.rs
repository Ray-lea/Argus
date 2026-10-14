//! Read-only, architecturally defined thermal/limiting status and client ring clocks.
use super::Identity;
use argus_core::hardware::{CpuLimitReason as Reason, CpuLimitStatus};

pub(super) fn ring_supported(id: Identity) -> bool {
    id.intel
        && id.family == 6
        && matches!(
            id.model,
            0x3c | 0x45
                | 0x46
                | 0x3d
                | 0x47
                | 0x4f
                | 0x56
                | 0x55
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
                | 0xc5
                | 0xc6
                | 0xbd
                | 0xcc
        )
}

pub(super) fn limits(status: u64, thermal_features: u32, package: bool) -> Vec<CpuLimitStatus> {
    let mut bits = vec![
        (0, Reason::Thermal),
        (2, Reason::Prochot),
        (4, Reason::CriticalTemperature),
    ];
    if thermal_features & (1 << 4) != 0 {
        bits.push((10, Reason::Power));
    }
    // HWP feedback bits exist in the per-core register, not PACKAGE_THERM_STATUS.
    if !package && thermal_features & (1 << 7) != 0 {
        bits.extend([(12, Reason::Current), (14, Reason::CrossDomain)]);
    }
    bits.into_iter()
        .map(|(bit, reason)| CpuLimitStatus {
            reason,
            active: status & (1 << bit) != 0,
            occurred: status & (1 << (bit + 1)) != 0,
        })
        .collect()
}

pub(super) fn ring_ratio(value: u64) -> Option<f32> {
    let ratio = value & 127;
    (1..=100).contains(&ratio).then_some(ratio as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_and_latched_limits_are_distinct_and_reserved_bits_stay_hidden() {
        let status = (1 << 3) | (1 << 10) | (1 << 12) | (1 << 15);
        let flags = limits(status, (1 << 4) | (1 << 7), false);
        let find = |reason| flags.iter().find(|f| f.reason == reason).unwrap();
        assert!(!find(Reason::Prochot).active);
        assert!(find(Reason::Prochot).occurred);
        assert!(find(Reason::Power).active);
        assert!(find(Reason::Current).active);
        assert!(find(Reason::CrossDomain).occurred);
        assert!(
            !limits(status, 0, false)
                .iter()
                .any(|f| f.reason == Reason::Power)
        );
        assert!(
            !limits(status, 1 << 7, true)
                .iter()
                .any(|f| f.reason == Reason::Current)
        );
        assert_eq!(ring_ratio(0xffff0000 | 43), Some(43.0));
        assert_eq!(ring_ratio(0), None);
        assert_eq!(ring_ratio(127), None);
    }
}
