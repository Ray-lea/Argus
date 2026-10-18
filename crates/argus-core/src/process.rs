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
    /// Disk bytes per second; absent until the disk source has a valid interval.
    pub disk_read: Option<u64>,
    pub disk_write: Option<u64>,
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
    disk: Option<(u64, u64)>,
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
    native_disk: bool,
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
            native_disk: crate::disk_process::standard_supported(),
        }
    }

    pub fn sample(&mut self, logical_cpus: u32) -> Vec<ProcessRow> {
        let now = Instant::now();
        let systime = filetime_ticks(unsafe { GetSystemTimePreciseAsFileTime() });
        let Ok(returned) = query_system_information(SPI_CLASS, &mut self.buf) else {
            return Vec::new();
        };
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
            let disk = self
                .native_disk
                .then(|| crate::disk_process::standard_counters(&self.buf, returned, p))
                .flatten();

            let (mut entry, prev) = match self.tracked.remove(&key) {
                Some(t) => {
                    let prev = (t.cpu_ticks, t.io, t.disk);
                    (t, Some(prev))
                }
                None => {
                    let name: Arc<str> = unsafe { p.image_name.to_string_lossy() }.into();
                    let path = self.paths.image_path(pid).map(Arc::from);
                    // Started since the last sample: everything it consumed is new.
                    let started = elapsed.is_some() && p.create_time > prev_systime;
                    let prev = started.then_some((0, [0; 3], Some((0, 0))));
                    (
                        Tracked {
                            name,
                            path,
                            meta: None,
                            cpu_ticks,
                            io,
                            disk,
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
                (Some((pc, pio, _)), Some(cap), Some(secs)) if cap > 0.0 => {
                    let cpu = ((cpu_ticks - pc).max(0) as f64 / cap * 100.0).min(100.0) as f32;
                    let rate = |i: usize| ((io[i] - pio[i]).max(0) as f64 / secs) as u64;
                    (cpu, [rate(0), rate(1), rate(2)])
                }
                _ => (0.0, [0; 3]),
            };
            entry.cpu_ticks = cpu_ticks;
            entry.io = io;
            let disk_rates = crate::disk_process::interval_rates(
                prev.and_then(|(_, _, disk)| disk),
                disk,
                elapsed,
            );
            entry.disk = disk;

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
                disk_read: disk_rates.map(|v| v.0),
                disk_write: disk_rates.map(|v| v.1),
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

/// A process found by image name, on demand (not part of sampling).
#[derive(Debug, Clone)]
pub struct Found {
    pub pid: u32,
    pub parent_pid: u32,
    /// Unix ms, as `actions::terminate` checks it.
    pub create_time: i64,
    /// Whether the parent still runs. A process under that PID started after
    /// this one only reuses the number.
    pub parent_running: bool,
}

/// Running processes whose image name is `image`, compared without case.
pub fn find(image: &str) -> Vec<Found> {
    find_by(|name| name.eq_ignore_ascii_case(image))
}

/// Running processes whose image name `matches`.
pub fn find_by(matches: impl Fn(&str) -> bool) -> Vec<Found> {
    let mut buf = AlignedBuf::with_capacity(512 * 1024);
    if query_system_information(SPI_CLASS, &mut buf).is_err() {
        return Vec::new();
    }
    let started: HashMap<u32, i64> = ProcessIter::new(&buf)
        .map(|e| {
            (
                e.info.unique_process_id.0 as usize as u32,
                e.info.create_time,
            )
        })
        .collect();
    ProcessIter::new(&buf)
        .filter(|e| matches(&unsafe { e.info.image_name.to_string_lossy() }))
        .map(|e| {
            let parent_pid = e.info.inherited_from_unique_process_id.0 as usize as u32;
            Found {
                pid: e.info.unique_process_id.0 as usize as u32,
                parent_pid,
                create_time: ticks_to_unix_ms(e.info.create_time),
                parent_running: started
                    .get(&parent_pid)
                    .is_some_and(|&created| created <= e.info.create_time),
            }
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_is_found_by_its_name_with_its_path() {
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let image = std::path::Path::new(&system)
            .join("System32")
            .join("PING.EXE");
        let mut child = std::process::Command::new(&image)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let found = find_by(|name| name.eq_ignore_ascii_case("ping.exe"));
        let path = crate::details::image_path_of(pid);
        let _ = child.kill();
        let _ = child.wait();
        let ping = found.iter().find(|p| p.pid == pid).expect("found by name");
        assert!(ping.parent_running);
        assert!(path.is_some_and(|p| p.eq_ignore_ascii_case(&image.to_string_lossy())));
        assert!(find_by(|_| false).is_empty());
    }
}
