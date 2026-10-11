//! System-wide counters: CPU (total and per logical processor) and memory.

use std::mem::size_of;

use serde::Serialize;
use windows::Wdk::System::SystemInformation::SystemProcessorPerformanceInformation as SPPI_CLASS;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{
    GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows::Win32::System::Threading::{
    ALL_PROCESSOR_GROUPS, GetActiveProcessorCount, GetSystemTimes,
};

use crate::nt::{SystemProcessorPerformanceInformation, query_system_information};
use crate::util::{AlignedBuf, filetime_ticks};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStats {
    /// Total CPU usage, 0–100.
    pub cpu: f32,
    /// Portion of `cpu` spent in kernel mode, 0–100.
    pub cpu_kernel: f32,
    /// Per logical processor usage, 0–100 (first processor group only).
    pub cores: Vec<f32>,
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

#[derive(Clone, Copy, Default)]
struct CpuTimes {
    idle: i64,
    kernel: i64,     // includes idle
    busy_total: i64, // kernel (includes idle) + user
}

pub(crate) struct SystemTracker {
    logical_cpus: u32,
    prev_total: Option<CpuTimes>,
    prev_cores: Vec<SystemProcessorPerformanceInformation>,
    buf: AlignedBuf,
}

impl SystemTracker {
    pub fn new() -> Self {
        let logical_cpus = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) }.max(1);
        let mut t = Self {
            logical_cpus,
            prev_total: None,
            prev_cores: Vec::new(),
            // The kernel requires an exact multiple of the record size; one
            // processor group holds at most 64 logical processors.
            buf: AlignedBuf::with_capacity(64 * size_of::<SystemProcessorPerformanceInformation>()),
        };
        t.prev_total = read_total_times();
        t.prev_cores = t.read_cores();
        t
    }

    pub fn logical_cpus(&self) -> u32 {
        self.logical_cpus
    }

    /// Samples CPU and memory. Process aggregates are filled in by the caller.
    pub fn sample(&mut self) -> SystemStats {
        let mut s = SystemStats {
            logical_cpus: self.logical_cpus,
            ..Default::default()
        };

        if let Some(now) = read_total_times() {
            if let Some(prev) = self.prev_total {
                let total = now.busy_total - prev.busy_total;
                s.cpu = busy_percent(now.idle - prev.idle, total);
                if total > 0 {
                    let kernel_busy = (now.kernel - prev.kernel) - (now.idle - prev.idle);
                    s.cpu_kernel =
                        (kernel_busy.max(0) as f64 / total as f64 * 100.0).min(s.cpu as f64) as f32;
                }
            }
            self.prev_total = Some(now);
        }

        let cores = self.read_cores();
        if cores.len() == self.prev_cores.len() {
            s.cores = cores
                .iter()
                .zip(&self.prev_cores)
                .map(|(n, p)| {
                    let total = (n.kernel_time + n.user_time) - (p.kernel_time + p.user_time);
                    busy_percent(n.idle_time - p.idle_time, total)
                })
                .collect();
        }
        self.prev_cores = cores;

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

    fn read_cores(&mut self) -> Vec<SystemProcessorPerformanceInformation> {
        let Ok(len) = query_system_information(SPPI_CLASS, &mut self.buf) else {
            return Vec::new();
        };
        let n = len / size_of::<SystemProcessorPerformanceInformation>();
        // SAFETY: the buffer holds `n` contiguous, 8-byte aligned records.
        unsafe {
            std::slice::from_raw_parts(
                self.buf.as_ptr() as *const SystemProcessorPerformanceInformation,
                n,
            )
        }
        .to_vec()
    }
}

fn read_total_times() -> Option<CpuTimes> {
    let (mut idle, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    Some(CpuTimes {
        idle: filetime_ticks(idle),
        kernel: filetime_ticks(kernel),
        busy_total: filetime_ticks(kernel) + filetime_ticks(user),
    })
}

fn busy_percent(idle_delta: i64, total_delta: i64) -> f32 {
    if total_delta <= 0 {
        return 0.0;
    }
    let busy = (total_delta - idle_delta).max(0) as f64 / total_delta as f64;
    (busy * 100.0).clamp(0.0, 100.0) as f32
}
