//! Processor identity and topology (clock speeds are in sensors.rs).

use serde::Serialize;
use windows::Win32::System::SystemInformation::{
    GROUP_AFFINITY, GetLogicalProcessorInformationEx, RelationAll, RelationCache,
    RelationProcessorCore, RelationProcessorPackage, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::Win32::System::Threading::{
    ALL_PROCESSOR_GROUPS, GetActiveProcessorCount, IsProcessorFeaturePresent,
    PF_VIRT_FIRMWARE_ENABLED,
};
use windows::core::{PCWSTR, w};

use super::registry::reg_string;
use crate::util::AlignedBuf;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    /// Empty when the registry does not say.
    pub name: String,
    pub vendor: String,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub instructions: Vec<&'static str>,
    pub microcode: Option<u32>,
    pub base_mhz: u32,
    pub base_source: Option<&'static str>,
    #[serde(skip)]
    pub(crate) hybrid: bool,
    pub sockets: u32,
    pub cores: u32,
    /// Hybrid CPUs only: cores in the highest efficiency class.
    pub performance_cores: u32,
    /// Hybrid CPUs only: the other cores, less the low-power ones.
    pub efficiency_cores: u32,
    /// Intel LP E-cores (`core_kinds`).
    pub low_power_cores: u32,
    pub logical: u32,
    pub l1: u64,
    pub l2: u64,
    pub l3: u64,
    /// Hardware virtualization enabled in firmware.
    pub virtualization: bool,
    /// Dense logical-processor order, sorted by (group, processor).
    pub threads: Vec<CpuThread>,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuThread {
    pub group: u16,
    pub processor: u8,
    /// Physical core it belongs to (hyper-threads share one).
    pub core: u32,
    /// An efficiency core of a hybrid CPU (Intel E-core or LP E-core).
    pub efficient: bool,
    /// A low-power efficiency core (Intel LP E-core); also `efficient`.
    pub low_power: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoreKind {
    Performance,
    Efficient,
    LowPower,
}

/// Each core's kind from its efficiency class and whether an L3 cache serves
/// it. Windows ranks Intel's E-cores and LP E-cores in one efficiency class.
/// Intel tells them apart by the L3 (CPUID.1A core type Atom, no CPUID.4
/// level 3 cache, given for Meteor Lake): LP E-cores stand outside the cache
/// the other cores share. The cache topology Windows reports carries the same
/// answer without running CPUID on every processor. Other vendors' compact
/// cores keep an L3 of their own and remain efficiency cores.
fn core_kinds(intel: bool, classes: &[u8], l3: &[bool]) -> Vec<CoreKind> {
    let top = classes.iter().copied().max().unwrap_or(0);
    // The L3 tells cores apart only where it serves the performance cores.
    let shared = classes
        .iter()
        .zip(l3)
        .any(|(class, served)| *class == top && *served);
    classes
        .iter()
        .zip(l3)
        .map(|(class, served)| {
            if *class == top {
                CoreKind::Performance
            } else if intel && shared && !served {
                CoreKind::LowPower
            } else {
                CoreKind::Efficient
            }
        })
        .collect()
}

impl CpuInfo {
    pub fn thread_index(&self, group: u16, processor: u8) -> Option<usize> {
        self.threads
            .binary_search_by_key(&(group, processor), |t| (t.group, t.processor))
            .ok()
    }
}

const CPU_KEY: PCWSTR = w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");

fn virtualization_enabled(
    firmware: bool,
    cpuid: impl Fn(u32) -> std::arch::x86_64::CpuidResult,
) -> bool {
    if firmware {
        return true;
    }
    // Hyper-V consumes the extensions in its root partition, where Windows
    // can report PF_VIRT_FIRMWARE_ENABLED as false. The Hv#1 interface's
    // CreatePartitions privilege distinguishes that host from ordinary guests.
    cpuid(1).ecx & (1 << 31) != 0
        && cpuid(0x4000_0000).eax >= 0x4000_0003
        && cpuid(0x4000_0001).eax == u32::from_le_bytes(*b"Hv#1")
        && cpuid(0x4000_0003).ebx & 1 != 0
}

pub fn cpu_info() -> CpuInfo {
    let mut info = CpuInfo {
        name: reg_string(CPU_KEY, w!("ProcessorNameString"))
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default(),
        logical: unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) },
        virtualization: virtualization_enabled(
            unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) }.as_bool(),
            std::arch::x86_64::__cpuid,
        ),
        ..Default::default()
    };
    identify(&mut info);
    let (base, source) = base_frequency(info.logical);
    info.base_mhz = base;
    info.base_source = source;

    let mut len = 0u32;
    let _ = unsafe { GetLogicalProcessorInformationEx(RelationAll, None, &mut len) };
    if len == 0 {
        return info;
    }
    let mut buf = AlignedBuf::with_capacity(len as usize);
    if unsafe {
        GetLogicalProcessorInformationEx(RelationAll, Some(buf.as_mut_ptr().cast()), &mut len)
    }
    .is_err()
    {
        return info;
    }

    let mut core_classes = Vec::new();
    // (group, processor, core)
    let mut members: Vec<(u16, u8, u32)> = Vec::new();
    // (group, mask) of the processors each L3 cache serves
    let mut l3_masks: Vec<(u16, usize)> = Vec::new();
    let mut offset = 0usize;
    while offset < len as usize {
        // SAFETY: records are laid out back to back, each `Size` bytes long.
        let rec = unsafe {
            &*(buf.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX)
        };
        if rec.Size == 0 {
            break;
        }
        match rec.Relationship {
            r if r == RelationProcessorCore => {
                let p = unsafe { &rec.Anonymous.Processor };
                let core = info.cores;
                info.cores += 1;
                core_classes.push(p.EfficiencyClass);
                // GroupMask is a variable-length array of GroupCount entries.
                let masks = unsafe {
                    std::slice::from_raw_parts(p.GroupMask.as_ptr(), p.GroupCount as usize)
                };
                for m in masks {
                    for bit in 0..usize::BITS as usize {
                        if m.Mask & (1usize << bit) != 0 {
                            members.push((m.Group, bit as u8, core));
                        }
                    }
                }
            }
            r if r == RelationProcessorPackage => info.sockets += 1,
            r if r == RelationCache => {
                let cache = unsafe { &rec.Anonymous.Cache };
                let size = cache.CacheSize as u64;
                match cache.Level {
                    1 => info.l1 += size,
                    2 => info.l2 += size,
                    3 => info.l3 += size,
                    _ => {}
                }
                if cache.Level == 3 {
                    // GroupCount was reserved (zero) before caches could span
                    // groups: one mask then. Never read past the record.
                    let masks = unsafe { cache.Anonymous.GroupMasks.as_ptr() };
                    let end = std::ptr::from_ref(rec) as usize + rec.Size as usize;
                    let room =
                        end.saturating_sub(masks as usize) / std::mem::size_of::<GROUP_AFFINITY>();
                    let count = usize::from(cache.GroupCount.max(1)).min(room);
                    // SAFETY: `count` entries lie inside this record.
                    for m in unsafe { std::slice::from_raw_parts(masks, count) } {
                        l3_masks.push((m.Group, m.Mask));
                    }
                }
            }
            _ => {}
        }
        offset += rec.Size as usize;
    }

    let mut served = vec![false; core_classes.len()];
    for (group, processor, core) in &members {
        if l3_masks
            .iter()
            .any(|(g, mask)| g == group && mask & (1usize << processor) != 0)
        {
            served[*core as usize] = true;
        }
    }
    let kinds = core_kinds(info.vendor == "GenuineIntel", &core_classes, &served);
    let count = |kind| kinds.iter().filter(|k| **k == kind).count() as u32;
    // Not hybrid: every core is in the top class, and no kind is counted.
    if kinds.iter().any(|kind| *kind != CoreKind::Performance) {
        info.performance_cores = count(CoreKind::Performance);
        info.efficiency_cores = count(CoreKind::Efficient);
        info.low_power_cores = count(CoreKind::LowPower);
    }
    members.sort_unstable_by_key(|m| (m.0, m.1));
    info.threads = members
        .into_iter()
        .map(|(group, processor, core)| {
            let kind = kinds[core as usize];
            CpuThread {
                group,
                processor,
                core,
                efficient: kind != CoreKind::Performance,
                low_power: kind == CoreKind::LowPower,
            }
        })
        .collect();
    info
}

fn base_frequency(logical: u32) -> (u32, Option<&'static str>) {
    use std::arch::x86_64::__cpuid;
    let cpuid = (__cpuid(0).eax >= 0x16).then(|| __cpuid(0x16).eax & 0xffff);
    if let Some(mhz) = cpuid.filter(|v| (100..=12000).contains(v)) {
        return (mhz, Some("cpuid"));
    }
    // ~MHz is often the invariant TSC/reference frequency (2688 MHz on
    // i5-13500HX). Neither the base clock nor performance scaling uses it.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct PowerInfo {
        number: u32,
        max_mhz: u32,
        current_mhz: u32,
        limit_mhz: u32,
        max_idle: u32,
        idle: u32,
    }
    #[link(name = "PowrProf")]
    unsafe extern "system" {
        fn CallNtPowerInformation(
            level: u32,
            input: *const std::ffi::c_void,
            input_len: u32,
            output: *mut std::ffi::c_void,
            output_len: u32,
        ) -> u32;
    }
    let mut info = vec![PowerInfo::default(); logical.clamp(1, 4096) as usize];
    let status = unsafe {
        CallNtPowerInformation(
            11,
            std::ptr::null(),
            0,
            info.as_mut_ptr().cast(),
            std::mem::size_of_val(info.as_slice()) as u32,
        )
    };
    if status == 0 && (100..=12000).contains(&info[0].max_mhz) {
        (info[0].max_mhz, Some("windows"))
    } else {
        (0, None)
    }
}

fn identify(info: &mut CpuInfo) {
    use std::arch::x86_64::{__cpuid, __cpuid_count};
    let vendor = __cpuid(0);
    let bytes: Vec<_> = [vendor.ebx, vendor.edx, vendor.ecx]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    info.vendor = String::from_utf8_lossy(&bytes).into_owned();
    if vendor.eax < 1 {
        return;
    }
    let id = __cpuid(1);
    let family = (id.eax >> 8) & 15;
    info.family = family
        + if family == 15 {
            (id.eax >> 20) & 255
        } else {
            0
        };
    info.model = (id.eax >> 4) & 15
        | if matches!(family, 6 | 15) {
            (id.eax >> 12) & 0xf0
        } else {
            0
        };
    info.stepping = id.eax & 15;
    let mut add = |bits: u32, items: &[(u8, &'static str)]| {
        info.instructions.extend(
            items
                .iter()
                .filter(|(bit, _)| bits & (1 << bit) != 0)
                .map(|(_, name)| *name),
        );
    };
    add(id.edx, &[(25, "SSE"), (26, "SSE2")]);
    add(
        id.ecx,
        &[
            (0, "SSE3"),
            (9, "SSSE3"),
            (19, "SSE4.1"),
            (20, "SSE4.2"),
            (28, "AVX"),
            (12, "FMA"),
            (25, "AES"),
            (29, "F16C"),
            (23, "POPCNT"),
            (1, "PCLMULQDQ"),
        ],
    );
    if vendor.eax >= 7 {
        let bits = __cpuid_count(7, 0);
        // CPUID.7.0:EDX[15] also covers systems whose topology API omits classes.
        info.hybrid = bits.edx & (1 << 15) != 0;
        add(
            bits.ebx,
            &[
                (5, "AVX2"),
                (3, "BMI1"),
                (8, "BMI2"),
                (19, "ADX"),
                (29, "SHA"),
                (16, "AVX-512F"),
                (17, "AVX-512DQ"),
                (28, "AVX-512CD"),
                (30, "AVX-512BW"),
                (31, "AVX-512VL"),
            ],
        );
        add(
            bits.ecx,
            &[(11, "AVX-512VNNI"), (9, "VAES"), (10, "VPCLMULQDQ")],
        );
        add(
            bits.edx,
            &[
                (23, "AVX-512FP16"),
                (24, "AMX-TILE"),
                (22, "AMX-BF16"),
                (25, "AMX-INT8"),
            ],
        );
        if bits.eax >= 1 {
            add(
                __cpuid_count(7, 1).eax,
                &[(4, "AVX-VNNI"), (5, "AVX-512BF16")],
            );
        }
    }
    if __cpuid(0x80000000).eax >= 0x80000001 {
        add(__cpuid(0x80000001).ecx, &[(5, "LZCNT"), (6, "SSE4a")]);
    }
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RegGetValueW};
    let mut data = [0u8; 64];
    let mut size = data.len() as u32;
    if unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            CPU_KEY,
            w!("Update Revision"),
            RRF_RT_REG_BINARY,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .is_ok()
    {
        let offset = if size == 8 {
            4
        } else if size == 4 {
            0
        } else {
            return;
        };
        info.microcode = Some(u32::from_le_bytes(
            data[offset..offset + 4].try_into().unwrap(),
        ))
        .filter(|v| *v != 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(cores, efficiency class, served by an L3)` runs, counted by kind.
    fn kinds(intel: bool, runs: &[(usize, u8, bool)]) -> (usize, usize, usize) {
        let (classes, l3): (Vec<_>, Vec<_>) = runs
            .iter()
            .flat_map(|(cores, class, l3)| std::iter::repeat_n((*class, *l3), *cores))
            .unzip();
        let kinds = core_kinds(intel, &classes, &l3);
        let count = |kind| kinds.iter().filter(|k| **k == kind).count();
        (
            count(CoreKind::Performance),
            count(CoreKind::Efficient),
            count(CoreKind::LowPower),
        )
    }

    #[test]
    fn low_power_cores_are_the_efficiency_cores_outside_the_shared_l3() {
        // Core Ultra 7 155H: Windows gives E and LP E-cores one class.
        assert_eq!(
            kinds(true, &[(6, 1, true), (8, 0, true), (2, 0, false)]),
            (6, 8, 2)
        );
        // Three classes would change nothing: the L3 decides.
        assert_eq!(
            kinds(true, &[(6, 2, true), (8, 1, true), (2, 0, false)]),
            (6, 8, 2)
        );
        // Core Ultra 200V: all four efficiency cores are low-power ones.
        assert_eq!(kinds(true, &[(4, 1, true), (4, 0, false)]), (4, 0, 4));
        // Core i5-13500HX: the E-cores share the L3.
        assert_eq!(kinds(true, &[(6, 1, true), (8, 0, true)]), (6, 8, 0));
    }

    #[test]
    fn cores_are_not_low_power_without_a_shared_l3_to_tell_them_apart() {
        // One class: not hybrid, with or without an L3.
        assert_eq!(kinds(true, &[(16, 0, true)]), (16, 0, 0));
        assert_eq!(kinds(true, &[(4, 0, false)]), (4, 0, 0));
        // No L3 reported for any core: nothing to compare against.
        assert_eq!(kinds(true, &[(2, 1, false), (4, 0, false)]), (2, 4, 0));
        // Another vendor's compact cores are not Intel's LP E-cores.
        assert_eq!(kinds(false, &[(4, 1, true), (8, 0, false)]), (4, 8, 0));
        assert_eq!(kinds(false, &[(4, 1, true), (8, 0, true)]), (4, 8, 0));
    }

    #[test]
    fn virtualization_distinguishes_hyper_v_hosts_from_guests() {
        assert!(virtualization_enabled(true, |_| panic!(
            "firmware already available"
        )));
        for (present, max_leaf, interface, privileges, expected) in [
            (
                true,
                0x4000_000c,
                u32::from_le_bytes(*b"Hv#1"),
                2865663,
                true,
            ),
            (
                true,
                0x4000_000a,
                u32::from_le_bytes(*b"Hv#1"),
                32768,
                false,
            ),
            (false, 0x4000_000c, u32::from_le_bytes(*b"Hv#1"), 1, false),
            (true, 0x4000_0001, u32::from_le_bytes(*b"Hv#1"), 1, false),
            (true, 0x4000_000c, 0, 1, false),
        ] {
            let read = |leaf| {
                let mut value = std::arch::x86_64::CpuidResult {
                    eax: 0,
                    ebx: 0,
                    ecx: 0,
                    edx: 0,
                };
                match leaf {
                    1 => value.ecx = u32::from(present) << 31,
                    0x4000_0000 => value.eax = max_leaf,
                    0x4000_0001 => value.eax = interface,
                    0x4000_0003 => value.ebx = privileges,
                    _ => panic!("unexpected CPUID leaf"),
                }
                value
            };
            assert_eq!(virtualization_enabled(false, read), expected);
        }
    }
}
