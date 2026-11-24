//! Processor identity and topology (clock speeds are in sensors.rs).

use serde::Serialize;
use std::collections::BTreeMap;
use windows::Win32::System::Power::{
    CallNtPowerInformation, PROCESSOR_POWER_INFORMATION, PowerInformationInternal,
    ProcessorInformation, ProcessorInformationEx,
};
use windows::Win32::System::SystemInformation::{
    GROUP_AFFINITY, GetLogicalProcessorInformationEx, RelationAll, RelationCache, RelationNumaNode,
    RelationNumaNodeEx, RelationProcessorCore, RelationProcessorPackage,
    SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::Win32::System::Threading::{
    ALL_PROCESSOR_GROUPS, GetActiveProcessorCount, GetActiveProcessorGroupCount,
    IsProcessorFeaturePresent, PF_VIRT_FIRMWARE_ENABLED,
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
    /// Physical performance cores when more than one core type is present.
    pub performance_cores: u32,
    /// Physical efficiency cores, excluding the low-power ones, on hybrid CPUs.
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
    /// Processor Information instance names use a NUMA node and an index within it.
    #[serde(skip)]
    pub(super) counter_threads: BTreeMap<u32, Vec<Option<usize>>>,
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
/// the other cores share. The cache topology Windows reports gives us that L3
/// membership. Other vendors' compact cores remain efficiency cores.
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

/// Called on a short-lived worker so changing its processor group leaves the
/// caller's affinity intact, including a default affinity spanning groups.
#[cfg(target_arch = "x86_64")]
fn intel_core_type(thread: &CpuThread) -> Option<u8> {
    use std::arch::x86_64::{__cpuid, __cpuid_count};
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadGroupAffinity};
    let affinity = GROUP_AFFINITY {
        Group: thread.group,
        Mask: 1usize << thread.processor,
        ..Default::default()
    };
    if let Err(error) = unsafe { SetThreadGroupAffinity(GetCurrentThread(), &affinity, None) }.ok()
    {
        crate::diagnostics::windows_failure("cpu.topology.affinity", &error);
        return None;
    }
    (__cpuid(0).eax >= 0x1a).then(|| (__cpuid_count(0x1a, 0).eax >> 24) as u8)
}

#[cfg(not(target_arch = "x86_64"))]
fn intel_core_type(_thread: &CpuThread) -> Option<u8> {
    None
}

fn intel_core_classes(
    cores: usize,
    threads: &[CpuThread],
    mut read_type: impl FnMut(&CpuThread) -> Option<u8> + Send,
) -> Option<Vec<u8>> {
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .spawn_scoped(scope, move || {
                let mut classes = vec![None; cores];
                for thread in threads {
                    let class = &mut classes[thread.core as usize];
                    if class.is_none() {
                        // Siblings share a core type. If one cannot be read,
                        // the next sibling can still supply the physical core.
                        *class = match read_type(thread) {
                            Some(0x40) => Some(1), // Intel Core
                            Some(0x20) => Some(0), // Intel Atom
                            _ => None,
                        };
                    }
                }
                // Never treat an unreadable core as either P or E.
                classes.into_iter().collect()
            })
            .inspect_err(|e| {
                crate::diagnostics::warn("cpu.topology.worker", crate::diagnostics::io(e))
            })
            .ok()?;
        worker.join().ok()?
    })
}

impl CpuInfo {
    fn classify_cores(
        &mut self,
        mut classes: Vec<u8>,
        l3: &[bool],
        read_type: impl FnMut(&CpuThread) -> Option<u8> + Send,
    ) {
        let intel = self.vendor == "GenuineIntel";
        let min = classes.iter().copied().min().unwrap_or(0);
        let max = classes.iter().copied().max().unwrap_or(0);
        let source = if intel && self.hybrid && classes.len() > 1 && min == max {
            match intel_core_classes(classes.len(), &self.threads, read_type) {
                Some(detected) => {
                    classes = detected;
                    "CPUID.1A"
                }
                None => "Windows (CPUID.1A incomplete)",
            }
        } else {
            "Windows"
        };
        let kinds = core_kinds(intel, &classes, l3);
        let count = |kind| kinds.iter().filter(|k| **k == kind).count() as u32;
        self.efficiency_cores = count(CoreKind::Efficient);
        self.low_power_cores = count(CoreKind::LowPower);
        self.performance_cores = if self.efficiency_cores + self.low_power_cores > 0 {
            count(CoreKind::Performance)
        } else {
            0
        };
        for thread in &mut self.threads {
            let kind = kinds[thread.core as usize];
            thread.efficient = kind != CoreKind::Performance;
            thread.low_power = kind == CoreKind::LowPower;
        }
        log::info!(
            target: "argus_diagnostics",
            "cpu topology: {} physical, {} logical; windows_efficiency={min}..{max}; cpuid_hybrid={}; source={source}; hybrid cores: P={}, E={}, LP-E={}",
            self.cores,
            self.logical,
            self.hybrid,
            self.performance_cores,
            self.efficiency_cores,
            self.low_power_cores,
        );
    }

    pub fn thread_index(&self, group: u16, processor: u8) -> Option<usize> {
        self.threads
            .binary_search_by_key(&(group, processor), |t| (t.group, t.processor))
            .ok()
    }

    pub(super) fn counter_index(&self, name: &str) -> Option<usize> {
        let (node, index) = name.split_once(',')?;
        self.counter_threads
            .get(&node.parse().ok()?)?
            .get(index.parse::<usize>().ok()?)?
            .as_ref()
            .copied()
    }

    fn index_numa_threads(&mut self, masks: BTreeMap<(u32, u16), usize>) {
        for ((node, group), mut mask) in masks {
            while mask != 0 {
                let index = self.thread_index(group, mask.trailing_zeros() as u8);
                self.counter_threads.entry(node).or_default().push(index);
                mask &= mask - 1;
            }
        }
    }
}

const CPU_KEY: PCWSTR = w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");

fn virtualization_enabled(firmware: bool, present: bool, leaf: impl Fn(u32) -> [u32; 4]) -> bool {
    if firmware {
        return true;
    }
    // Hyper-V consumes the extensions in its root partition, where Windows
    // can report PF_VIRT_FIRMWARE_ENABLED as false. The Hv#1 interface's
    // CreatePartitions privilege distinguishes that host from ordinary guests.
    present
        && leaf(0x4000_0000)[0] >= 0x4000_0003
        && leaf(0x4000_0001)[0] == u32::from_le_bytes(*b"Hv#1")
        && leaf(0x4000_0003)[1] & 1 != 0
}

fn detect_virtualization() -> bool {
    let firmware = unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) }.as_bool();
    if firmware {
        return true;
    }
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::__cpuid;
        let present = __cpuid(1).ecx & (1 << 31) != 0;
        virtualization_enabled(false, present, |id| {
            let r = __cpuid(id);
            [r.eax, r.ebx, r.ecx, r.edx]
        })
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        use windows::Wdk::System::SystemInformation::{
            NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS,
        };
        // SYSTEM_HYPERVISOR_DETAIL_INFORMATION: seven 16-byte HV_DETAILS leaves
        // corresponding to synthetic hypervisor leaves 0x4000_0000..=0x4000_0006.
        let mut details = [[0u32; 4]; 7];
        let mut returned = 0u32;
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_INFORMATION_CLASS(159),
                details.as_mut_ptr().cast(),
                size_of_val(&details) as u32,
                &mut returned,
            )
        };
        let present = status.0 >= 0 && returned == size_of_val(&details) as u32;
        virtualization_enabled(false, present, |id| {
            details
                .get(id.wrapping_sub(0x4000_0000) as usize)
                .copied()
                .unwrap_or_default()
        })
    }
}

pub fn cpu_info() -> CpuInfo {
    let mut info = CpuInfo {
        name: reg_string(CPU_KEY, w!("ProcessorNameString"))
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default(),
        logical: unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) },
        virtualization: detect_virtualization(),
        ..Default::default()
    };
    identify(&mut info);
    let (base, source) = base_frequency();
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
    // (group, mask) of the processors each L3 cache serves
    let mut l3_masks: Vec<(u16, usize)> = Vec::new();
    let mut numa_masks = BTreeMap::<(u32, u16), usize>::new();
    let mut offset = 0usize;
    while offset < len as usize {
        // SAFETY: records are laid out back to back, each `Size` bytes long.
        let rec = unsafe {
            &*(buf.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX)
        };
        if rec.Size == 0 || rec.Size as usize > len as usize - offset {
            break;
        }
        // GroupCount was reserved (zero) on systems reporting one mask per
        // cache or NUMA node. Bound the variable array by this record.
        let masks_in_record = |start: *const GROUP_AFFINITY, groups: u16| {
            let end = std::ptr::from_ref(rec) as usize + rec.Size as usize;
            let room = end.saturating_sub(start as usize) / size_of::<GROUP_AFFINITY>();
            unsafe { std::slice::from_raw_parts(start, usize::from(groups.max(1)).min(room)) }
        };
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
                            info.threads.push(CpuThread {
                                group: m.Group,
                                processor: bit as u8,
                                core,
                                ..Default::default()
                            });
                        }
                    }
                }
            }
            r if r == RelationProcessorPackage => info.sockets += 1,
            r if r == RelationNumaNode || r == RelationNumaNodeEx => {
                let node = unsafe { &rec.Anonymous.NumaNode };
                for mask in masks_in_record(
                    unsafe { node.Anonymous.GroupMasks.as_ptr() },
                    node.GroupCount,
                ) {
                    *numa_masks.entry((node.NodeNumber, mask.Group)).or_default() |= mask.Mask;
                }
            }
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
                    for m in masks_in_record(
                        unsafe { cache.Anonymous.GroupMasks.as_ptr() },
                        cache.GroupCount,
                    ) {
                        l3_masks.push((m.Group, m.Mask));
                    }
                }
            }
            _ => {}
        }
        offset += rec.Size as usize;
    }

    let mut served = vec![false; core_classes.len()];
    for thread in &info.threads {
        if l3_masks
            .iter()
            .any(|(g, mask)| *g == thread.group && mask & (1usize << thread.processor) != 0)
        {
            served[thread.core as usize] = true;
        }
    }
    info.classify_cores(core_classes, &served, intel_core_type);
    info.threads
        .sort_unstable_by_key(|t| (t.group, t.processor));
    info.index_numa_threads(numa_masks);
    info
}

fn cpuid_base_frequency() -> Option<u32> {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::__cpuid;
        (__cpuid(0).eax >= 0x16).then(|| __cpuid(0x16).eax & 0xffff)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        None
    }
}

fn base_frequency() -> (u32, Option<&'static str>) {
    if let Some(mhz) = cpuid_base_frequency().filter(|v| (100..=12000).contains(v)) {
        return (mhz, Some("cpuid"));
    }
    match processor_power_info(0)
        .and_then(|info| info.first().map(|p| p.MaxMhz))
        .filter(|mhz| (100..=12000).contains(mhz))
    {
        Some(mhz) => (mhz, Some("windows")),
        None => (0, None),
    }
}

fn processor_power_info(group: u16) -> Option<Vec<PROCESSOR_POWER_INFORMATION>> {
    let count = unsafe { GetActiveProcessorCount(group) };
    if !(1..=64).contains(&count) {
        return None;
    }
    let (level, input, len) = if unsafe { GetActiveProcessorGroupCount() } == 1 {
        (ProcessorInformation, None, 0)
    } else {
        (
            ProcessorInformationEx,
            Some(std::ptr::from_ref(&group).cast()),
            size_of::<u16>() as u32,
        )
    };
    let mut info = vec![PROCESSOR_POWER_INFORMATION::default(); count as usize];
    let status = unsafe {
        CallNtPowerInformation(
            level,
            input,
            len,
            Some(info.as_mut_ptr().cast()),
            size_of_val(info.as_slice()) as u32,
        )
    };
    status.is_ok().then_some(info)
}

/// The nominal MHz of this processor's Windows performance domain.
fn branded_frequency(thread: &CpuThread) -> Option<u32> {
    // PowerInternalProcessorBrandedFrequency, version 1. The processor-specific
    // request includes a PROCESSOR_NUMBER after the two header fields.
    #[repr(C)]
    struct Request {
        kind: u32,
        version: u32,
        group: u16,
        processor: u8,
        reserved: u8,
    }
    let request = Request {
        kind: 43,
        version: 1,
        group: thread.group,
        processor: thread.processor,
        reserved: 0,
    };
    let mut reply = [1u32, 0]; // Version, NominalFrequency (MHz).
    let status = unsafe {
        CallNtPowerInformation(
            PowerInformationInternal,
            Some(std::ptr::from_ref(&request).cast()),
            size_of_val(&request) as u32,
            Some(reply.as_mut_ptr().cast()),
            size_of_val(&reply) as u32,
        )
    };
    (status.is_ok() && reply[0] == 1 && (100..=12000).contains(&reply[1])).then_some(reply[1])
}

fn specified_mhz(
    info: &PROCESSOR_POWER_INFORMATION,
    processor: u8,
    system_index: usize,
) -> Option<u32> {
    let number = info.Number as usize;
    ((number == usize::from(processor) || number == system_index)
        && (100..=12000).contains(&info.MaxMhz))
    .then_some(info.MaxMhz)
}

pub(super) fn nominal_frequencies(
    threads: &[CpuThread],
    package_mhz: Option<u32>,
) -> Vec<Option<u32>> {
    let mut frequencies = Vec::with_capacity(threads.len());
    for group in threads.chunk_by(|a, b| a.group == b.group) {
        let power = std::cell::LazyCell::new(|| processor_power_info(group[0].group));
        for (index, thread) in group.iter().enumerate() {
            // The legacy power query returns one array per group. Match the
            // indexed record before using MaxMhz; CurrentMhz is a requested P-state.
            let mhz = branded_frequency(thread).or_else(|| {
                let info = power.as_ref()?.get(index)?;
                specified_mhz(info, thread.processor, frequencies.len())
            });
            frequencies.push(mhz.or(package_mhz));
        }
    }
    frequencies
}

/// Parses Windows' ARM64 processor `Identifier` string, e.g.
/// `"ARMv8 (64-bit) Family 8 Model D4B Revision 0"`.
#[cfg_attr(not(target_arch = "aarch64"), allow(dead_code))]
fn parse_arm64_identifier(text: &str) -> Option<(u32, u32, u32)> {
    let mut family = None;
    let mut model = None;
    let mut revision = None;
    let mut words = text.split_whitespace();
    while let Some(word) = words.next() {
        if word.eq_ignore_ascii_case("Family") {
            family = words.next().and_then(|v| {
                v.parse::<u32>()
                    .ok()
                    .or_else(|| u32::from_str_radix(v, 16).ok())
            });
        } else if word.eq_ignore_ascii_case("Model") {
            model = words.next().and_then(|v| {
                u32::from_str_radix(v.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
            });
        } else if word.eq_ignore_ascii_case("Revision") {
            revision = words.next().and_then(|v| {
                u32::from_str_radix(v.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
            });
        }
    }
    Some((family?, model?, revision?))
}

#[cfg_attr(not(target_arch = "aarch64"), allow(dead_code))]
fn arm64_instructions(mut has_feature: impl FnMut(u32) -> bool) -> Vec<&'static str> {
    // WinNT.h PROCESSOR_FEATURE_ID constants for ARMv8/ARMv9 user-mode extensions.
    const FEATURES: &[(u32, &[&str])] = &[
        (29, &["ARMv8", "NEON"]), // PF_ARM_V8_INSTRUCTIONS_AVAILABLE
        (30, &["AES", "SHA"]),    // PF_ARM_V8_CRYPTO_INSTRUCTIONS_AVAILABLE
        (31, &["CRC32"]),         // PF_ARM_V8_CRC32_INSTRUCTIONS_AVAILABLE
        (34, &["LSE"]),           // PF_ARM_V81_ATOMIC_INSTRUCTIONS_AVAILABLE
        (43, &["DotProd"]),       // PF_ARM_V82_DP_INSTRUCTIONS_AVAILABLE
        (44, &["JSCVT"]),         // PF_ARM_V83_JSCVT_INSTRUCTIONS_AVAILABLE
        (45, &["LRCPC"]),         // PF_ARM_V83_LRCPC_INSTRUCTIONS_AVAILABLE
        (46, &["SVE"]),           // PF_ARM_SVE_INSTRUCTIONS_AVAILABLE
        (47, &["SVE2"]),          // PF_ARM_SVE2_INSTRUCTIONS_AVAILABLE
        (62, &["LSE2"]),          // PF_ARM_LSE2_AVAILABLE
        (64, &["SHA3"]),          // PF_ARM_SHA3_INSTRUCTIONS_AVAILABLE
        (65, &["SHA512"]),        // PF_ARM_SHA512_INSTRUCTIONS_AVAILABLE
        (67, &["FP16"]),          // PF_ARM_V82_FP16_INSTRUCTIONS_AVAILABLE
        (68, &["BF16"]),          // PF_ARM_V86_BF16_INSTRUCTIONS_AVAILABLE
        (69, &["I8MM"]),          // PF_ARM_V86_I8MM_INSTRUCTIONS_AVAILABLE
    ];
    let mut out = Vec::new();
    for &(feature, names) in FEATURES {
        if has_feature(feature) {
            out.extend_from_slice(names);
        }
    }
    out
}

fn identify(info: &mut CpuInfo) {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::{__cpuid, __cpuid_count};
        let vendor = __cpuid(0);
        let bytes: Vec<_> = [vendor.ebx, vendor.edx, vendor.ecx]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        info.vendor = String::from_utf8_lossy(&bytes).into_owned();
        if vendor.eax >= 1 {
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
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        use windows::Win32::System::SystemInformation::{GetNativeSystemInfo, SYSTEM_INFO};
        use windows::Win32::System::Threading::PROCESSOR_FEATURE_ID;
        info.vendor = reg_string(CPU_KEY, w!("VendorIdentifier"))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();
        if let Some((family, model, stepping)) =
            reg_string(CPU_KEY, w!("Identifier")).and_then(|s| parse_arm64_identifier(&s))
        {
            info.family = family;
            info.model = model;
            info.stepping = stepping;
        } else {
            let mut sys = SYSTEM_INFO::default();
            unsafe { GetNativeSystemInfo(&mut sys) };
            info.family = u32::from(sys.wProcessorLevel);
            info.model = u32::from(sys.wProcessorRevision >> 8);
            info.stepping = u32::from(sys.wProcessorRevision & 0xff);
        }
        info.instructions = arm64_instructions(|feature| {
            unsafe { IsProcessorFeaturePresent(PROCESSOR_FEATURE_ID(feature)) }.as_bool()
        });
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

    #[test]
    fn numa_counter_indices_span_groups_and_preserve_missing_members() {
        let mut cpu = CpuInfo {
            threads: [(0, 2), (0, 6), (1, 0), (1, 4)]
                .map(|(group, processor)| CpuThread {
                    group,
                    processor,
                    ..Default::default()
                })
                .to_vec(),
            ..Default::default()
        };
        cpu.index_numa_threads([((7, 0), 0b1010100), ((7, 1), 0b10000), ((3, 1), 1)].into());
        assert_eq!(cpu.counter_index("7,0"), Some(0));
        assert_eq!(cpu.counter_index("7,1"), None);
        assert_eq!(cpu.counter_index("7,2"), Some(1));
        assert_eq!(cpu.counter_index("7,3"), Some(3));
        assert_eq!(cpu.counter_index("3,0"), Some(2));
        assert_eq!(cpu.counter_index("7,_Total"), None);
    }

    #[test]
    fn legacy_nominal_mhz_uses_the_matching_processors_specified_frequency() {
        let mut info = PROCESSOR_POWER_INFORMATION {
            Number: 35,
            MaxMhz: 2400,
            CurrentMhz: 800,
            MhzLimit: 1800,
            ..Default::default()
        };
        assert_eq!(specified_mhz(&info, 3, 35), Some(2400));
        assert_eq!(specified_mhz(&info, 2, 34), None);
        info.Number = 3;
        assert_eq!(specified_mhz(&info, 3, 35), Some(2400));
        info.MaxMhz = 0;
        assert_eq!(specified_mhz(&info, 3, 35), None);
    }

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

    fn counts(cpu: &CpuInfo) -> (u32, u32, u32) {
        (
            cpu.performance_cores,
            cpu.efficiency_cores,
            cpu.low_power_cores,
        )
    }

    #[test]
    fn uniform_windows_class_recovers_six_p_and_eight_e_cores_and_their_threads() {
        // The 13600KF's 14 cores / 20 threads, with overlapping processor
        // numbers in two groups so a group-blind lookup cannot pass.
        let threads = (0..12)
            .map(|p| (1, p, 8 + u32::from(p / 2)))
            .chain((0..8).map(|p| (0, p, u32::from(p))))
            .map(|(group, processor, core)| CpuThread {
                group,
                processor,
                core,
                ..Default::default()
            })
            .collect();
        let mut cpu = CpuInfo {
            vendor: "GenuineIntel".into(),
            hybrid: true,
            cores: 14,
            logical: 20,
            threads,
            ..Default::default()
        };
        let mut reads = 0;
        cpu.classify_cores(vec![0; 14], &[true; 14], |t| {
            reads += 1;
            match (t.group, t.processor) {
                (1, 0) => None, // The other SMT sibling is accessible.
                (1, _) => Some(0x40),
                (0, _) => Some(0x20),
                _ => unreachable!(),
            }
        });
        assert_eq!(reads, 15);
        assert_eq!(counts(&cpu), (6, 8, 0));
        assert!(
            cpu.threads
                .iter()
                .all(|t| t.efficient == (t.group == 0) && !t.low_power)
        );
    }

    #[test]
    fn cpuid_fallback_requires_complete_known_types_and_preserves_l3_classification() {
        let mut cpu = CpuInfo {
            vendor: "GenuineIntel".into(),
            hybrid: true,
            threads: (0..3)
                .map(|processor| CpuThread {
                    processor,
                    core: u32::from(processor),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let l3 = [true, true, false];
        for types in [
            [Some(0x40), Some(0x20), None],
            [Some(0x40), Some(0x20), Some(0x30)],
            [Some(0x40); 3],
            [Some(0x20); 3],
        ] {
            cpu.classify_cores(vec![7; 3], &l3, |t| types[t.processor as usize]);
            assert_eq!(counts(&cpu), (0, 0, 0));
            assert!(cpu.threads.iter().all(|t| !t.efficient && !t.low_power));
        }
        cpu.classify_cores(vec![7; 3], &l3, |t| {
            Some(if t.processor == 0 { 0x40 } else { 0x20 })
        });
        assert_eq!(counts(&cpu), (1, 1, 1));
        assert!(cpu.threads[2].efficient && cpu.threads[2].low_power);
        // Distinct Windows classes already carry the answer.
        let mut reads = 0;
        let mut unused = |_: &CpuThread| {
            reads += 1;
            None
        };
        cpu.classify_cores(vec![3, 3, 2], &l3, &mut unused);
        assert_eq!(counts(&cpu), (2, 0, 1));
        for (vendor, hybrid) in [("AuthenticAMD", true), ("GenuineIntel", false)] {
            cpu.vendor = vendor.into();
            cpu.hybrid = hybrid;
            cpu.classify_cores(vec![0; 3], &l3, &mut unused);
            assert!(cpu.threads.iter().all(|t| !t.efficient && !t.low_power));
        }
        assert_eq!(reads, 0);
    }

    #[test]
    fn virtualization_distinguishes_hyper_v_hosts_from_guests() {
        assert!(virtualization_enabled(true, false, |_| panic!(
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
            let read = |leaf| match leaf {
                0x4000_0000 => [max_leaf, 0, 0, 0],
                0x4000_0001 => [interface, 0, 0, 0],
                0x4000_0003 => [0, privileges, 0, 0],
                _ => panic!("unexpected hypervisor leaf"),
            };
            assert_eq!(virtualization_enabled(false, present, read), expected);
        }
    }

    #[test]
    fn parses_arm64_registry_identifiers_and_feature_sets() {
        assert_eq!(
            parse_arm64_identifier("ARMv8 (64-bit) Family 8 Model 001 Revision 100"),
            Some((8, 0x001, 0x100))
        );
        assert_eq!(
            parse_arm64_identifier("ARMv8 (64-bit) Family 8 Model D4B Revision 0"),
            Some((8, 0xd4b, 0))
        );
        assert_eq!(parse_arm64_identifier("GenuineIntel Family 6"), None);
        let features = arm64_instructions(|id| matches!(id, 29 | 30 | 34 | 43 | 68));
        assert_eq!(
            features,
            vec!["ARMv8", "NEON", "AES", "SHA", "LSE", "DotProd", "BF16"]
        );
    }
}
