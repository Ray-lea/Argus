//! System-wide counters: CPU (total and per logical processor) and memory.

use std::mem::size_of;

use serde::Serialize;
use windows::Wdk::System::SystemInformation::SystemProcessorPerformanceInformation as SPPI_CLASS;
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{
    GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows::Win32::System::Threading::{
    ALL_PROCESSOR_GROUPS, GetActiveProcessorCount, GetActiveProcessorGroupCount,
};

use crate::nt::{NtQuerySystemInformationEx, SystemProcessorPerformanceInformation as Times};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStats {
    /// Total CPU usage, 0–100.
    pub cpu: f32,
    /// Portion of `cpu` spent in kernel mode, 0–100.
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

type Address = (u16, u8);
type Readings = Vec<(Address, Option<Times>)>;

pub(crate) struct SystemTracker {
    logical_cpus: u32,
    previous: BTreeMap<Address, Times>,
}

impl SystemTracker {
    pub fn new() -> Self {
        let mut tracker = Self {
            logical_cpus: unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) }.max(1),
            previous: BTreeMap::new(),
        };
        tracker.update_cpu(read_cores());
        tracker
    }

    pub fn logical_cpus(&self) -> u32 {
        self.logical_cpus
    }

    fn update_cpu(&mut self, readings: Readings) -> SystemStats {
        let mut stats = SystemStats::default();
        let mut next = BTreeMap::new();
        let (mut busy, mut kernel, mut total) = (0_i64, 0_i64, 0_i64);
        let mut complete = !readings.is_empty();
        for (address, now) in readings {
            let delta = now
                .zip(self.previous.get(&address))
                .and_then(|(n, p)| interval(n, *p));
            stats
                .cores
                .push(delta.map(|(b, _, t)| b as f32 / t as f32 * 100.));
            if let Some((b, k, t)) = delta {
                busy += b;
                kernel += k;
                total += t;
            } else {
                complete = false;
            }
            if let Some(now) = now {
                next.insert(address, now);
            }
        }
        if complete && total > 0 {
            // Weight groups by elapsed processor time, not by their CPU count or percentage.
            stats.cpu = (busy as f64 / total as f64 * 100.) as f32;
            stats.cpu_kernel = (kernel as f64 / total as f64 * 100.) as f32;
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
                s.kernel_paged = pi.KernelPaged as u64 * page;
                s.kernel_nonpaged = pi.KernelNonpaged as u64 * page;
            }
            s.uptime_ms = GetTickCount64();
        }
        s
    }
}

fn read_group(group: u16, count: usize) -> Option<Vec<Times>> {
    if !(1..=64).contains(&count) {
        return None;
    }
    let mut values = vec![Times::default(); count];
    let bytes = (size_of::<Times>() * count) as u32;
    let mut returned = 0;
    let status = unsafe {
        NtQuerySystemInformationEx(
            SPPI_CLASS,
            (&group as *const u16).cast(),
            2,
            values.as_mut_ptr().cast(),
            bytes,
            &mut returned,
        )
    };
    (status.0 >= 0 && returned == bytes).then_some(values)
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

fn interval(now: Times, prev: Times) -> Option<(i64, i64, i64)> {
    let idle = now.idle_time.checked_sub(prev.idle_time)?;
    let kernel = now.kernel_time.checked_sub(prev.kernel_time)?;
    let user = now.user_time.checked_sub(prev.user_time)?;
    let total = kernel.checked_add(user)?;
    if idle < 0 || kernel < idle || user < 0 || total <= 0 {
        return None;
    }
    Some((total - idle, kernel - idle, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn times(idle: i64, kernel: i64, user: i64) -> Times {
        Times {
            idle_time: idle,
            kernel_time: kernel,
            user_time: user,
            ..Default::default()
        }
    }
    #[test]
    fn unequal_groups_are_dense_and_totals_are_time_weighted() {
        let mut tracker = SystemTracker {
            logical_cpus: 80,
            previous: BTreeMap::new(),
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
        assert!(readings.iter().all(|(_, value)| value.is_some()));
        assert!(read_group(u16::MAX, 1).is_none());
    }
}
