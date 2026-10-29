//! System-wide counters: CPU (total and per logical processor) and memory.

use std::mem::size_of;

use serde::Serialize;
use windows::Wdk::System::SystemInformation::NtQuerySystemInformation;
use windows::Wdk::System::SystemInformation::SystemProcessorPerformanceInformation as SPPI_CLASS;
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{
    GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows::Win32::System::Threading::{
    ALL_PROCESSOR_GROUPS, GetActiveProcessorCount, GetActiveProcessorGroupCount,
};

use crate::cpu::{IdleTimes, PROCESSOR_IDLE};
use crate::nt::SystemProcessorPerformanceInformation as Times;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStats {
    /// Total CPU usage, 0–100.
    pub cpu: f32,
    /// Kernel portion reported by the Windows time counters, bounded by `cpu`.
    pub cpu_kernel: f32,
    /// Dense (group, processor) order; None until a valid interval is available.
    pub cores: Vec<Option<f32>>,
    pub logical_cpus: u32,
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_available: u64,
    pub commit_used: u64,
    pub commit_limit: u64,
    pub cached: u64,
    /// Disjoint memory lists; None when the native query is unavailable.
    pub memory_lists: Option<MemoryLists>,
    pub kernel_paged: u64,
    pub kernel_nonpaged: u64,
    pub process_count: u32,
    pub thread_count: u32,
    pub handle_count: u32,
    pub uptime_ms: u64,
    /// Aggregate process I/O, bytes per second.
    pub io_read: u64,
    pub io_write: u64,
    pub io_other: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLists {
    pub modified: u64,
    pub standby: u64,
    /// Free and zeroed pages can both be immediately reused.
    pub free: u64,
}

impl MemoryLists {
    fn from_native(
        lists: &crate::nt::SystemMemoryListInformation,
        page: u64,
        total: u64,
    ) -> Option<Self> {
        if page == 0 || total == 0 {
            return None;
        }
        let modified = (lists.modified_page_count as u64).checked_mul(page)?;
        let standby = lists
            .page_count_by_priority
            .iter()
            .try_fold(0_u64, |sum, count| sum.checked_add(*count as u64))?
            .checked_mul(page)?;
        let free = (lists.free_page_count as u64)
            .checked_add(lists.zero_page_count as u64)?
            .checked_mul(page)?;
        (modified.checked_add(standby)?.checked_add(free)? <= total).then_some(Self {
            modified,
            standby,
            free,
        })
    }
}

fn memory_lists(page: u64, total: u64) -> Option<MemoryLists> {
    let mut lists = crate::nt::SystemMemoryListInformation::default();
    let mut returned = 0;
    let status = unsafe {
        NtQuerySystemInformation(
            crate::nt::SYSTEM_MEMORY_LIST_INFORMATION,
            (&mut lists as *mut crate::nt::SystemMemoryListInformation).cast(),
            size_of_val(&lists) as u32,
            &mut returned,
        )
    };
    if status.is_err() || returned as usize != size_of_val(&lists) {
        return None;
    }
    MemoryLists::from_native(&lists, page, total)
}

type Address = (u16, u8);
type Readings = Vec<(Address, Option<CpuReading>)>;

#[derive(Clone, Copy)]
struct CpuReading {
    times: Times,
    idle: Option<i64>,
}

#[derive(Clone, Copy)]
struct CpuInterval {
    idle: i64,
    kernel: i64,
    total: i64,
    precise: bool,
}

pub(crate) struct SystemTracker {
    logical_cpus: u32,
    previous: BTreeMap<Address, CpuReading>,
    precise_idle_fraction: Option<f64>,
}

impl SystemTracker {
    pub fn new() -> Self {
        let mut tracker = Self {
            logical_cpus: unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) }.max(1),
            previous: BTreeMap::new(),
            precise_idle_fraction: None,
        };
        tracker.update_cpu(read_cores());
        tracker
    }

    pub fn logical_cpus(&self) -> u32 {
        self.logical_cpus
    }

    pub fn precise_idle_fraction(&self) -> Option<f64> {
        self.precise_idle_fraction
    }

    fn update_cpu(&mut self, readings: Readings) -> SystemStats {
        let mut stats = SystemStats::default();
        let mut next = BTreeMap::new();
        let (mut idle, mut kernel, mut total) = (0_u128, 0_u128, 0_u128);
        let mut complete = !readings.is_empty();
        let mut precise = complete;
        self.precise_idle_fraction = None;
        for (address, now) in readings {
            let delta = now
                .zip(self.previous.get(&address))
                .and_then(|(n, p)| interval(n, *p));
            stats
                .cores
                .push(delta.map(|d| usage(d.idle as u128, d.total as u128)));
            if let Some(d) = delta {
                idle += d.idle as u128;
                kernel += d.kernel as u128;
                total += d.total as u128;
                precise &= d.precise;
            } else {
                complete = false;
            }
            if let Some(now) = now {
                next.insert(address, now);
            }
        }
        if complete && total > 0 {
            self.precise_idle_fraction =
                precise.then_some((idle as f64 / total as f64).clamp(0., 1.));
            // Weight groups by elapsed processor time, not by their CPU count or percentage.
            stats.cpu = usage(idle, total);
            // The independent idle counter cannot recover the missing user/kernel
            // split. Keep the observed kernel time; do not label missed user work
            // as kernel work by subtracting the new idle value from KernelTime.
            stats.cpu_kernel = ((kernel as f64 / total as f64 * 100.) as f32).min(stats.cpu);
        }
        self.previous = next;
        self.logical_cpus = stats.cores.len() as u32;
        stats.logical_cpus = self.logical_cpus;
        stats
    }

    /// Samples CPU and memory. Process aggregates are filled in by the caller.
    pub fn sample(&mut self) -> SystemStats {
        let mut s = self.update_cpu(read_cores());

        unsafe {
            let mut ms = MEMORYSTATUSEX {
                dwLength: size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };
            if GlobalMemoryStatusEx(&mut ms).is_ok() {
                s.mem_total = ms.ullTotalPhys;
                s.mem_available = ms.ullAvailPhys;
                s.mem_used = ms.ullTotalPhys.saturating_sub(ms.ullAvailPhys);
            }
            let mut pi = PERFORMANCE_INFORMATION {
                cb: size_of::<PERFORMANCE_INFORMATION>() as u32,
                ..Default::default()
            };
            if GetPerformanceInfo(&mut pi, pi.cb).is_ok() {
                let page = pi.PageSize as u64;
                s.commit_used = pi.CommitTotal as u64 * page;
                s.commit_limit = pi.CommitLimit as u64 * page;
                s.cached = pi.SystemCache as u64 * page;
                s.memory_lists = memory_lists(page, s.mem_total);
                if let Some(lists) = s.memory_lists {
                    // Cached memory includes modified pages, while available
                    // memory includes only standby, free and zeroed pages.
                    s.cached = lists.standby + lists.modified;
                }
                s.kernel_paged = pi.KernelPaged as u64 * page;
                s.kernel_nonpaged = pi.KernelNonpaged as u64 * page;
            }
            s.uptime_ms = GetTickCount64();
        }
        s
    }
}

fn read_group(group: u16, count: usize) -> Option<Vec<CpuReading>> {
    if !(1..=64).contains(&count) {
        return None;
    }
    let mut idle = [IdleTimes::default(); 64];
    let mut times = [Times::default(); 64];
    // Query back to back: both are cumulative 100 ns counters, but the legacy
    // IdleTime can include short bursts of real work on supported Windows builds.
    let has_idle = unsafe { crate::cpu::read_group(PROCESSOR_IDLE, group, &mut idle[..count]) };
    if !unsafe { crate::cpu::read_group(SPPI_CLASS, group, &mut times[..count]) } {
        return None;
    }
    Some(
        (0..count)
            .map(|i| CpuReading {
                times: times[i],
                idle: has_idle.then_some(idle[i].idle),
            })
            .collect(),
    )
}

fn read_cores() -> Readings {
    let mut readings = Vec::new();
    for group in 0..unsafe { GetActiveProcessorGroupCount() } {
        let count = unsafe { GetActiveProcessorCount(group) }.min(64) as usize;
        let values = read_group(group, count);
        for processor in 0..count {
            readings.push((
                (group, processor as u8),
                values.as_ref().map(|v| v[processor]),
            ));
        }
    }
    readings
}

fn interval(now: CpuReading, prev: CpuReading) -> Option<CpuInterval> {
    let legacy_idle = now.times.idle_time.checked_sub(prev.times.idle_time)?;
    let kernel = now.times.kernel_time.checked_sub(prev.times.kernel_time)?;
    let user = now.times.user_time.checked_sub(prev.times.user_time)?;
    let total = kernel.checked_add(user)?;
    if legacy_idle < 0 || kernel < legacy_idle || user < 0 || total <= 0 {
        return None;
    }
    let (idle, precise) = match (now.idle, prev.idle) {
        (Some(n), Some(p)) if n >= 0 && p >= 0 => {
            let delta = n.checked_sub(p)?;
            // Some systems return a successful query with a frozen counter.
            // Fall back only when legacy time confirms that this CPU was idle;
            // zero from both sources is valid on a continuously busy CPU.
            if delta == 0 && legacy_idle > 0 {
                (legacy_idle, false)
            } else {
                (delta, true)
            }
        }
        (Some(n), _) | (_, Some(n)) if n < 0 => return None,
        // Use a pair from one source, including during recovery from a failed query.
        _ => (legacy_idle, false),
    };
    (idle >= 0).then_some(CpuInterval {
        idle,
        kernel: kernel - legacy_idle,
        total,
        precise,
    })
}

fn usage(idle: u128, total: u128) -> f32 {
    // The total time is tick-quantized, while the independent idle counter is
    // precise. Small boundary skew can put idle just above total on an idle CPU.
    (total.saturating_sub(idle) as f64 / total as f64 * 100.) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_lists_are_disjoint_and_pagefile_modified_pages_are_not_counted_twice() {
        let lists = crate::nt::SystemMemoryListInformation {
            zero_page_count: 20,
            free_page_count: 10,
            modified_page_count: 5,
            modified_no_write_page_count: 1,
            page_count_by_priority: [1, 2, 3, 4, 5, 6, 7, 8],
            modified_page_count_page_file: 4,
            ..Default::default()
        };
        let value = MemoryLists::from_native(&lists, 4096, 100 * 4096).unwrap();
        assert_eq!(
            value,
            MemoryLists {
                modified: 5 * 4096,
                standby: 36 * 4096,
                free: 30 * 4096
            }
        );
    }

    #[test]
    fn incomplete_or_overflowing_memory_lists_are_not_presented_as_real_values() {
        let mut lists = crate::nt::SystemMemoryListInformation::default();
        assert!(MemoryLists::from_native(&lists, 0, 4096).is_none());
        assert!(MemoryLists::from_native(&lists, 4096, 0).is_none());
        lists.modified_page_count = 2;
        assert!(MemoryLists::from_native(&lists, 4096, 4096).is_none());
        lists.modified_page_count = usize::MAX;
        assert!(MemoryLists::from_native(&lists, 4096, u64::MAX).is_none());
        lists.modified_page_count = 0;
        lists.page_count_by_priority = [usize::MAX; 8];
        assert!(MemoryLists::from_native(&lists, 4096, u64::MAX).is_none());
    }
    fn times(idle: i64, kernel: i64, user: i64) -> CpuReading {
        CpuReading {
            times: Times {
                idle_time: idle,
                kernel_time: kernel,
                user_time: user,
                ..Default::default()
            },
            idle: None,
        }
    }
    fn precise(legacy_idle: i64, kernel: i64, user: i64, idle: i64) -> CpuReading {
        CpuReading {
            idle: Some(idle),
            ..times(legacy_idle, kernel, user)
        }
    }

    #[test]
    fn short_bursts_are_not_idle_and_are_not_invented_kernel_time() {
        let mut tracker = SystemTracker {
            logical_cpus: 4,
            previous: BTreeMap::new(),
            precise_idle_fraction: None,
        };
        tracker.update_cpu(
            (0..4)
                .map(|p| ((0, p), Some(precise(0, 0, 0, 0))))
                .collect(),
        );
        // One-second native readings from the reproduction: the legacy source
        // reports complete idleness on the two processors doing short bursts.
        let idle = [10_000_000, 10_000_000, 6_669_211, 6_875_819];
        let result = tracker.update_cpu(
            idle.into_iter()
                .enumerate()
                .map(|(p, idle)| ((0, p as u8), Some(precise(10_000_000, 10_000_000, 0, idle))))
                .collect(),
        );
        assert!((result.cpu - 16.137425).abs() < 0.00001);
        assert!((result.cores[2].unwrap() - 33.30789).abs() < 0.00001);
        assert!((result.cores[3].unwrap() - 31.24181).abs() < 0.00001);
        assert_eq!(result.cpu_kernel, 0.);
    }

    #[test]
    fn frozen_independent_idle_falls_back_on_all_sixteen_processors() {
        for frozen in [0, 123_456_789] {
            let mut tracker = SystemTracker {
                logical_cpus: 16,
                previous: BTreeMap::new(),
                precise_idle_fraction: None,
            };
            let readings = |step: i64| {
                (0..16)
                    .map(|p| {
                        (
                            (0, p),
                            Some(precise(
                                step * 8_000_000,
                                step * 9_000_000,
                                step * 1_000_000,
                                frozen,
                            )),
                        )
                    })
                    .collect()
            };
            tracker.update_cpu(readings(0));
            for step in 1..=5 {
                let result = tracker.update_cpu(readings(step));
                assert_eq!(result.cores, [Some(20.); 16]);
                assert_eq!(result.cpu, 20.);
                assert_eq!(result.cpu_kernel, 10.);
                assert_eq!(tracker.precise_idle_fraction(), None);
            }
        }
    }

    #[test]
    fn frozen_idle_fallback_is_per_processor_and_recovers_with_paired_readings() {
        let mut tracker = SystemTracker {
            logical_cpus: 2,
            previous: BTreeMap::new(),
            precise_idle_fraction: None,
        };
        tracker.update_cpu(vec![
            ((0, 0), Some(precise(0, 0, 0, 1000))),
            ((1, 0), Some(precise(0, 0, 0, 2000))),
        ]);
        let result = tracker.update_cpu(vec![
            ((0, 0), Some(precise(80, 90, 10, 1000))),
            ((1, 0), Some(precise(200, 200, 0, 2100))),
        ]);
        assert_eq!(result.cores, [Some(20.), Some(50.)]);
        assert_eq!(result.cpu, 40.);
        // A mixed-source aggregate must not validate the process cycle clock.
        assert_eq!(tracker.precise_idle_fraction(), None);

        let result = tracker.update_cpu(vec![
            ((0, 0), Some(precise(160, 180, 20, 1040))),
            ((1, 0), Some(precise(400, 400, 0, 2200))),
        ]);
        assert_eq!(result.cores, [Some(60.), Some(50.)]);
        assert!((result.cpu - 160. / 3.).abs() < 0.00001);
        assert_eq!(tracker.precise_idle_fraction(), Some(140. / 300.));
    }

    #[test]
    fn zero_idle_delta_on_a_busy_processor_is_still_valid() {
        let mut tracker = SystemTracker {
            logical_cpus: 1,
            previous: BTreeMap::new(),
            precise_idle_fraction: None,
        };
        tracker.update_cpu(vec![((0, 0), Some(precise(100, 200, 300, 1000)))]);
        let result = tracker.update_cpu(vec![((0, 0), Some(precise(100, 240, 360, 1000)))]);
        assert_eq!(result.cores, [Some(100.)]);
        assert_eq!(result.cpu, 100.);
        assert_eq!(result.cpu_kernel, 40.);
        assert_eq!(tracker.precise_idle_fraction(), Some(0.));
    }

    #[test]
    #[ignore = "samples live native CPU counters for three seconds"]
    fn live_frozen_idle_uses_legacy_counters() {
        let mut frozen = SystemTracker::new();
        let mut legacy = SystemTracker::new();
        for step in 0..=3 {
            if step > 0 {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            let readings = read_cores();
            let expected = legacy.update_cpu(
                readings
                    .iter()
                    .map(|&(a, r)| (a, r.map(|r| CpuReading { idle: None, ..r })))
                    .collect(),
            );
            let actual = frozen.update_cpu(
                readings
                    .into_iter()
                    .map(|(a, r)| {
                        (
                            a,
                            r.map(|r| CpuReading {
                                idle: Some(123456),
                                ..r
                            }),
                        )
                    })
                    .collect(),
            );
            if step > 0 {
                println!(
                    "sample {step}: frozen={:.2}% legacy={:.2}% cores={} elevated={}",
                    actual.cpu,
                    expected.cpu,
                    actual.cores.len(),
                    crate::privilege::is_elevated()
                );
                assert!(actual.cores.iter().all(Option::is_some));
                assert_eq!(actual.cores, expected.cores);
                assert_eq!(actual.cpu, expected.cpu);
                assert_eq!(actual.cpu_kernel, expected.cpu_kernel);
                assert_eq!(
                    frozen.precise_idle_fraction(),
                    (expected.cpu == 100.).then_some(0.)
                );
            }
        }
    }

    #[test]
    fn idle_query_recovery_does_not_mix_counter_origins_and_resets_reprime() {
        let mut tracker = SystemTracker {
            logical_cpus: 1,
            previous: BTreeMap::new(),
            precise_idle_fraction: None,
        };
        let sample = |tracker: &mut SystemTracker, r| tracker.update_cpu(vec![((0, 0), Some(r))]);
        sample(&mut tracker, precise(100, 100, 0, 1000));
        assert_eq!(sample(&mut tracker, times(150, 200, 0)).cpu, 50.);
        assert_eq!(sample(&mut tracker, precise(200, 300, 0, 1200)).cpu, 50.);
        assert_eq!(sample(&mut tracker, precise(250, 400, 0, 1225)).cpu, 75.);
        assert_eq!(sample(&mut tracker, precise(300, 500, 0, 10)).cores, [None]);
        assert_eq!(sample(&mut tracker, precise(350, 600, 0, 60)).cpu, 50.);
    }

    #[test]
    fn quantized_total_is_clamped_per_core_but_summed_before_clamping() {
        let mut tracker = SystemTracker {
            logical_cpus: 2,
            previous: BTreeMap::new(),
            precise_idle_fraction: None,
        };
        tracker.update_cpu(vec![
            ((0, 0), Some(precise(0, 0, 0, 0))),
            ((1, 0), Some(precise(0, 0, 0, 0))),
        ]);
        let result = tracker.update_cpu(vec![
            ((0, 0), Some(precise(100, 100, 0, 101))),
            ((1, 0), Some(precise(100, 100, 0, 99))),
        ]);
        assert_eq!(result.cores, [Some(0.), Some(1.)]);
        assert_eq!(result.cpu, 0.);
        assert!(interval(precise(100, 100, 0, -1), precise(0, 0, 0, 0)).is_none());
        assert!(interval(precise(0, i64::MAX, 1, 0), precise(0, 0, 0, 0)).is_none());
    }
    #[test]
    fn unequal_groups_are_dense_and_totals_are_time_weighted() {
        let mut tracker = SystemTracker {
            logical_cpus: 80,
            previous: BTreeMap::new(),
            precise_idle_fraction: None,
        };
        let addresses: Vec<_> = (0..48)
            .map(|p| (0, p))
            .chain((0..32).map(|p| (1, p)))
            .collect();
        tracker.update_cpu(
            addresses
                .iter()
                .map(|&a| (a, Some(times(0, 0, 0))))
                .collect(),
        );
        let result = tracker.update_cpu(
            addresses
                .iter()
                .map(|&a| {
                    (
                        a,
                        Some(if a.0 == 0 {
                            times(100, 100, 0)
                        } else {
                            times(0, 50, 150)
                        }),
                    )
                })
                .collect(),
        );
        assert_eq!(result.cores.len(), 80);
        assert_eq!(result.cores[47], Some(0.));
        assert_eq!(result.cores[48], Some(100.));
        assert!((result.cpu - 400. / 7.).abs() < 0.001);
        assert!((result.cpu_kernel - 100. / 7.).abs() < 0.001);
    }
    #[test]
    fn missing_group_and_counter_reset_do_not_shift_or_reuse_baselines() {
        let mut tracker = SystemTracker {
            logical_cpus: 2,
            previous: BTreeMap::new(),
            precise_idle_fraction: None,
        };
        tracker.update_cpu(vec![
            ((0, 0), Some(times(0, 0, 0))),
            ((1, 0), Some(times(0, 0, 0))),
        ]);
        let result = tracker.update_cpu(vec![((0, 0), None), ((1, 0), Some(times(0, 10, 10)))]);
        assert_eq!(result.cores, [None, Some(100.)]);
        let result = tracker.update_cpu(vec![
            ((0, 0), Some(times(0, 20, 20))),
            ((1, 0), Some(times(0, 0, 0))),
            ((1, 1), Some(times(0, 20, 20))),
        ]);
        assert_eq!(result.cores, [None, None, None]);
    }
    #[test]
    fn native_query_covers_all_local_processors_and_rejects_invalid_group() {
        let readings = read_cores();
        assert_eq!(
            readings.len(),
            unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) } as usize
        );
        assert!(
            readings
                .iter()
                .all(|(_, value)| value.is_some_and(|v| v.idle.is_some()))
        );
        assert!(read_group(u16::MAX, 1).is_none());
    }
}
