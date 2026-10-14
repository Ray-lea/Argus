//! Processor identity and topology (clock speeds are in sensors.rs).

use serde::Serialize;
use windows::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationAll, RelationCache, RelationProcessorCore,
    RelationProcessorPackage, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::Win32::System::Threading::{
    ALL_PROCESSOR_GROUPS, GetActiveProcessorCount, IsProcessorFeaturePresent,
    PF_VIRT_FIRMWARE_ENABLED,
};
use windows::core::{PCWSTR, w};

use super::registry::{reg_dword, reg_string};
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
    pub(crate) reference_mhz: u32,
    pub sockets: u32,
    pub cores: u32,
    /// Hybrid CPUs only: cores in the highest efficiency class.
    pub performance_cores: u32,
    pub efficiency_cores: u32,
    pub logical: u32,
    pub l1: u64,
    pub l2: u64,
    pub l3: u64,
    /// Hardware virtualization enabled in firmware.
    pub virtualization: bool,
    /// One entry per logical processor, indexed by processor number.
    pub threads: Vec<CpuThread>,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuThread {
    /// Physical core it belongs to (hyper-threads share one).
    pub core: u32,
    /// An efficiency core of a hybrid CPU (Intel E-core).
    pub efficient: bool,
}

const CPU_KEY: PCWSTR = w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");

pub fn cpu_info() -> CpuInfo {
    let mut info = CpuInfo {
        name: reg_string(CPU_KEY, w!("ProcessorNameString"))
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default(),
        reference_mhz: reg_dword(CPU_KEY, w!("~MHz")).unwrap_or(0),
        logical: unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) },
        virtualization: unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) }.as_bool(),
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
    // (logical processor, core, efficiency class)
    let mut members: Vec<(usize, u32, u8)> = Vec::new();
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
                            members.push((m.Group as usize * 64 + bit, core, p.EfficiencyClass));
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
            }
            _ => {}
        }
        offset += rec.Size as usize;
    }

    let top = core_classes.iter().copied().max().unwrap_or(0);
    let hybrid = core_classes.iter().any(|&c| c != top);
    if hybrid {
        info.performance_cores = core_classes.iter().filter(|&&c| c == top).count() as u32;
        info.efficiency_cores = info.cores - info.performance_cores;
    }
    if let Some(last) = members.iter().map(|m| m.0).max() {
        info.threads = vec![CpuThread::default(); last + 1];
        for (logical, core, class) in members {
            info.threads[logical] = CpuThread {
                core,
                efficient: hybrid && class < top,
            };
        }
    }
    info
}

fn base_frequency(logical: u32) -> (u32, Option<&'static str>) {
    use std::arch::x86_64::__cpuid;
    let cpuid = (__cpuid(0).eax >= 0x16).then(|| __cpuid(0x16).eax & 0xffff);
    if let Some(mhz) = cpuid.filter(|v| (100..=12000).contains(v)) {
        return (mhz, Some("cpuid"));
    }
    // ~MHz is often the invariant TSC/reference frequency (2688 MHz on
    // i5-13500HX), not the nominal processor frequency. Keep it only as
    // the existing PDH performance-counter reference, never as a base clock.
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
