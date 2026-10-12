//! Process enumeration and per-process rates.
//!
//! One `NtQuerySystemInformation(SystemProcessInformation)` call returns every
//! process and thread with their counters, which is far cheaper than Toolhelp,
//! PDH or WMI. Rates are computed from deltas against the previous sample.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use windows::Wdk::System::SystemInformation::SystemProcessInformation as SPI_CLASS;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{
    GetProcessInformation, OpenProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_LIMITED_INFORMATION, ProcessPowerThrottling,
};

use crate::meta::{FileMeta, MetaStore};
use crate::nt::{
    ProcessEntry, ProcessIter, THREAD_STATE_WAITING, WAIT_REASON_SUSPENDED,
    query_system_information,
};
use crate::paths::PathResolver;
use crate::util::{AlignedBuf, OwnedHandle, filetime_ticks, ticks_to_unix_ms};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRow {
    pub pid: u32,
    pub ppid: u32,
    /// Unix ms. Together with `pid` this identifies a process instance.
    pub create_time: i64,
    pub name: Arc<str>,
    pub path: Option<Arc<str>>,
    pub description: Option<Arc<str>>,
    pub company: Option<Arc<str>>,
    pub icon: Option<u32>,
    /// Percent of total machine capacity, 0–100.
    pub cpu: f32,
    /// Total CPU time consumed, ms.
    pub cpu_time: u64,
    /// Private working set (what Task Manager calls "Memory").
    pub private_ws: u64,
    pub working_set: u64,
    /// Private committed bytes.
    pub commit: u64,
    /// I/O bytes per second.
    pub io_read: u64,
    pub io_write: u64,
    pub io_other: u64,
    /// The process's share of its busiest GPU engine, percent.
    pub gpu: f32,
    /// That engine, e.g. `GPU 0 - 3D`; None while the process leaves the GPU idle.
    pub gpu_engine: Option<Arc<str>>,
    /// GPU memory in use, bytes: video memory and system memory.
    pub gpu_dedicated: u64,
    pub gpu_shared: u64,
    /// Network bytes per second, when per-process traffic is measured (ETW).
    pub net_sent: u64,
    pub net_received: u64,
    pub threads: u32,
    pub handles: u32,
    pub session_id: u32,
    pub base_priority: i32,
    pub suspended: bool,
    /// Efficiency mode (EcoQoS) is on, set by the app itself or by a user.
    pub efficiency: bool,
    /// Owns a taskbar window, which puts it under "Apps" (see `app_windows`).
    pub app_window: bool,
}

struct Tracked {
    name: Arc<str>,
    path: Option<Arc<str>>,
    meta: Option<Arc<FileMeta>>,
    cpu_ticks: i64,
    io: [i64; 3],
    /// Query handle, opened once per process instance (None when access is
    /// denied, which is final for that instance).
    handle: Option<OwnedHandle>,
}

pub(crate) struct ProcessTracker {
    buf: AlignedBuf,
    tracked: HashMap<(u32, i64), Tracked>,
    paths: PathResolver,
    meta: Arc<MetaStore>,
    last_instant: Option<Instant>,
    last_systime: i64,
}

impl ProcessTracker {
    pub fn new(meta: Arc<MetaStore>) -> Self {
        Self {
            buf: AlignedBuf::with_capacity(512 * 1024),
            tracked: HashMap::new(),
            paths: PathResolver::new(),
            meta,
            last_instant: None,
            last_systime: 0,
        }
    }

    pub fn sample(&mut self, logical_cpus: u32) -> Vec<ProcessRow> {
        let now = Instant::now();
        let systime = filetime_ticks(unsafe { GetSystemTimePreciseAsFileTime() });
        if query_system_information(SPI_CLASS, &mut self.buf).is_err() {
            return Vec::new();
        }
        let elapsed = self
            .last_instant
            .map(|t| now.duration_since(t).as_secs_f64());
        let prev_systime = self.last_systime;
        self.last_instant = Some(now);
        self.last_systime = systime;

        let capacity_ticks = elapsed.map(|s| s * 1e7 * logical_cpus as f64);
        let mut next = HashMap::with_capacity(self.tracked.len() + 32);
        let mut rows = Vec::with_capacity(self.tracked.len() + 32);

        for ProcessEntry { info: p, threads } in ProcessIter::new(&self.buf) {
            let pid = p.unique_process_id.0 as usize as u32;
            if pid == 0 {
                continue; // System Idle Process: it is the absence of load, not a process.
            }
            let key = (pid, p.create_time);
            let cpu_ticks = p.kernel_time + p.user_time;
            let io = [
                p.read_transfer_count,
                p.write_transfer_count,
                p.other_transfer_count,
            ];

            let (mut entry, prev) = match self.tracked.remove(&key) {
                Some(t) => {
                    let prev = (t.cpu_ticks, t.io);
                    (t, Some(prev))
                }
                None => {
                    let name: Arc<str> = unsafe { p.image_name.to_string_lossy() }.into();
                    let path = self.paths.image_path(pid).map(Arc::from);
                    // Started since the last sample: everything it consumed is new.
                    let prev =
                        (elapsed.is_some() && p.create_time > prev_systime).then_some((0, [0; 3]));
                    (
                        Tracked {
                            name,
                            path,
                            meta: None,
                            cpu_ticks,
                            io,
                            handle: unsafe {
                                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                            }
                            .ok()
                            .map(OwnedHandle),
                        },
                        prev,
                    )
                }
            };
            if entry.meta.is_none()
                && let Some(path) = &entry.path
            {
                entry.meta = self.meta.get_or_request(path);
            }

            let (cpu, rates) = match (prev, capacity_ticks, elapsed) {
                (Some((pc, pio)), Some(cap), Some(secs)) if cap > 0.0 => {
                    let cpu = ((cpu_ticks - pc).max(0) as f64 / cap * 100.0).min(100.0) as f32;
                    let rate = |i: usize| ((io[i] - pio[i]).max(0) as f64 / secs) as u64;
                    (cpu, [rate(0), rate(1), rate(2)])
                }
                _ => (0.0, [0; 3]),
            };
            entry.cpu_ticks = cpu_ticks;
            entry.io = io;

            let suspended = !threads.is_empty()
                && threads.iter().all(|t| {
                    t.thread_state == THREAD_STATE_WAITING && t.wait_reason == WAIT_REASON_SUSPENDED
                });
            let meta = entry.meta.as_deref();
            rows.push(ProcessRow {
                pid,
                ppid: p.inherited_from_unique_process_id.0 as usize as u32,
                create_time: ticks_to_unix_ms(p.create_time),
                name: entry.name.clone(),
                path: entry.path.clone(),
                description: meta.and_then(|m| m.description.clone()),
                company: meta.and_then(|m| m.company.clone()),
                icon: meta.and_then(|m| m.icon),
                cpu,
                cpu_time: (cpu_ticks / 10_000) as u64,
                private_ws: p.working_set_private_size as u64,
                working_set: p.working_set_size as u64,
                commit: p.private_page_count as u64,
                io_read: rates[0],
                io_write: rates[1],
                io_other: rates[2],
                gpu: 0.0,
                gpu_engine: None,
                gpu_dedicated: 0,
                gpu_shared: 0,
                net_sent: 0,
                net_received: 0,
                threads: p.number_of_threads,
                handles: p.handle_count,
                session_id: p.session_id,
                base_priority: p.base_priority,
                suspended,
                efficiency: entry.handle.as_ref().is_some_and(|h| efficiency_mode(h.0)),
                app_window: false,
            });
            next.insert(key, entry);
        }
        self.tracked = next;
        rows
    }
}

/// Whether efficiency mode is explicitly on (about 1 µs per process with a
/// cached handle).
fn efficiency_mode(process: HANDLE) -> bool {
    let mut state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ..Default::default()
    };
    unsafe {
        GetProcessInformation(
            process,
            ProcessPowerThrottling,
            (&mut state as *mut PROCESS_POWER_THROTTLING_STATE).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
    .is_ok()
        && state.ControlMask & state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0
}

/// Minimal (pid, ppid, create time) listing, used to walk process trees.
pub(crate) fn list_lineage() -> Vec<(u32, u32, i64)> {
    let mut buf = AlignedBuf::with_capacity(512 * 1024);
    if query_system_information(SPI_CLASS, &mut buf).is_err() {
        return Vec::new();
    }
    ProcessIter::new(&buf)
        .map(|e| {
            (
                e.info.unique_process_id.0 as usize as u32,
                e.info.inherited_from_unique_process_id.0 as usize as u32,
                ticks_to_unix_ms(e.info.create_time),
            )
        })
        .filter(|(pid, ..)| *pid != 0)
        .collect()
}
