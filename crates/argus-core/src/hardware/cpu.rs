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
    pub name: String,
    pub base_mhz: u32,
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
            .unwrap_or_else(|| "处理器".into()),
        base_mhz: reg_dword(CPU_KEY, w!("~MHz")).unwrap_or(0),
        logical: unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) },
        virtualization: unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) }.as_bool(),
        ..Default::default()
    };

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
