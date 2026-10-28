//! Process enumeration and per-process rates.
//!
//! One `NtQuerySystemInformation(SystemProcessInformation)` call returns every
//! process and thread with their counters, which is far cheaper than Toolhelp,
//! PDH or WMI. Rates are computed from deltas against the previous sample.

use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use windows::Wdk::System::SystemInformation::SystemProcessInformation as SPI_CLASS;
use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{
    GetProcessInformation, GetProcessTimes, OpenProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_LIMITED_INFORMATION, ProcessPowerThrottling,
};
use windows::Win32::System::WindowsProgramming::QueryProcessCycleTime;

use crate::cpu::{CycleBudget, CycleClock, SystemCycles};
use crate::meta::{FileMeta, MetaStore};
use crate::nt::{
    ProcessEntry, ProcessIter, THREAD_STATE_WAITING, WAIT_REASON_SUSPENDED,
    query_system_information,
};
use crate::package::{AppMeta, Identity};
use crate::paths::PathResolver;
use crate::util::{AlignedBuf, OwnedHandle, filetime_ticks, ticks_to_unix_ms};
use crate::webview::{self, Instance, WebViewInfo};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRow {
    pub pid: u32,
    pub ppid: u32,
    /// Unix ms. Together with `pid` this identifies a process instance.
    pub create_time: i64,
    pub name: Arc<str>,
    pub path: Option<Arc<str>>,
    /// A packaged app's display name for its own executable, otherwise the
    /// executable's file description.
    pub description: Option<Arc<str>>,
    pub company: Option<Arc<str>>,
    pub icon: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_view: Option<Arc<WebViewInfo>>,
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
    /// Windows component identity for display grouping, not an action safety check.
    pub windows_process: bool,
}

fn kernel_process(pid: u32, parent: u32, name: &str, has_path: bool) -> bool {
    pid == 4
        || (!has_path
            && parent == 4
            && matches!(
                name,
                "Registry" | "MemCompression" | "Memory Compression" | "Secure System"
            ))
}

struct Tracked {
    seen: bool,
    parent: u32,
    session: u32,
    name: Arc<str>,
    path: Option<Arc<str>>,
    meta: Option<Arc<FileMeta>>,
    cpu_ticks: i64,
    cpu_cycles: u64,
    io: [i64; 3],
    disk: Option<(u64, u64)>,
    /// Query handle, opened once per process instance (None when access is
    /// denied, which is final for that instance).
    handle: Option<OwnedHandle>,
    web_view: Option<Box<webview::State>>,
    package: Option<Box<Package>>,
}

/// A packaged app's process: its identity, then the app once resolved.
struct Package {
    identity: Identity,
    app: Option<Arc<AppMeta>>,
    /// The app's name if this process is the app's own executable.
    own_name: Option<Arc<str>>,
    /// While the app's name is pending, the process shows its executable's
    /// name rather than a description about to change; not for longer than
    /// `PACKAGE_WAIT`, should the shell be slow.
    since: Instant,
}
const PACKAGE_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

pub(crate) struct ProcessTracker {
    buf: AlignedBuf,
    tracked: HashMap<(u32, i64), Tracked>,
    by_pid: HashMap<u32, i64>,
    webviews: Vec<(usize, Instance)>,
    generation: bool,
    paths: PathResolver,
    meta: Arc<MetaStore>,
    last_instant: Option<Instant>,
    last_systime: i64,
    native_disk: bool,
    previous_cycles: Option<SystemCycles>,
    cycle_clock: CycleClock,
}

impl ProcessTracker {
    pub fn new(meta: Arc<MetaStore>) -> Self {
        Self {
            buf: AlignedBuf::with_capacity(512 * 1024),
            tracked: HashMap::new(),
            by_pid: HashMap::new(),
            webviews: Vec::new(),
            generation: false,
            paths: PathResolver::new(),
            meta,
            last_instant: None,
            last_systime: 0,
            native_disk: crate::disk_process::standard_supported(),
            previous_cycles: None,
            cycle_clock: CycleClock::new(),
        }
    }

    pub fn sample(
        &mut self,
        logical_cpus: u32,
        precise_idle_fraction: Option<f64>,
    ) -> Vec<ProcessRow> {
        let now = Instant::now();
        let systime = filetime_ticks(unsafe { GetSystemTimePreciseAsFileTime() });
        let cycles = SystemCycles::read(logical_cpus);
        let clock = self.cycle_clock.sample(logical_cpus);
        let Ok(returned) = query_system_information(SPI_CLASS, &mut self.buf) else {
            // The process baselines still span the failed interval. Re-prime the
            // cycle budget before using it again, so both sides cover one window.
            self.previous_cycles = None;
            self.cycle_clock.clear();
            return Vec::new();
        };
        let system_delta = cycles
            .as_ref()
            .zip(self.previous_cycles.as_ref())
            .and_then(|(n, p)| n.delta(p));
        let mut budget = CycleBudget::new(system_delta.map(|d| d.total()));
        self.previous_cycles = cycles;
        let elapsed = self
            .last_instant
            .map(|t| now.duration_since(t).as_secs_f64());
        let prev_systime = self.last_systime;
        self.last_instant = Some(now);
        self.last_systime = systime;

        let capacity_ticks = elapsed.map(|s| s * 1e7 * logical_cpus as f64);
        self.generation = !self.generation;
        let mut rows = Vec::with_capacity(self.tracked.len() + 32);
        let mut cycle_deltas = Vec::with_capacity(self.tracked.len() + 32);
        self.webviews.clear();

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

            let (entry, prev) = match self.tracked.entry(key) {
                Entry::Occupied(slot) => {
                    let t = slot.into_mut();
                    let prev = (t.cpu_ticks, t.cpu_cycles, t.io, t.disk);
                    (t, Some(prev))
                }
                Entry::Vacant(slot) => {
                    let name: Arc<str> = unsafe { p.image_name.to_string_lossy() }.into();
                    let path = self.paths.image_path(pid).map(Arc::from);
                    let runtime =
                        webview::is_webview(&name).then(|| Box::new(webview::State::default()));
                    self.by_pid.insert(pid, p.create_time);
                    // Started since the last sample: everything it consumed is new.
                    let started = elapsed.is_some() && p.create_time > prev_systime;
                    let prev = started.then_some((0, 0, [0; 3], Some((0, 0))));
                    let handle = query_handle(pid, p.create_time);
                    let package = handle
                        .as_ref()
                        .and_then(|h| crate::package::identity(h.0))
                        .map(|identity| {
                            Box::new(Package {
                                identity,
                                app: None,
                                own_name: None,
                                since: now,
                            })
                        });
                    (
                        slot.insert(Tracked {
                            seen: self.generation,
                            parent: p.inherited_from_unique_process_id.0 as usize as u32,
                            session: p.session_id,
                            name,
                            path,
                            meta: None,
                            cpu_ticks,
                            cpu_cycles: p.cycle_time,
                            io,
                            disk,
                            handle,
                            web_view: runtime,
                            package,
                        }),
                        prev,
                    )
                }
            };
            entry.seen = self.generation;
            if let Some(runtime) = entry.web_view.as_mut() {
                runtime.query(entry.handle.as_ref().map(|h| h.0));
                self.webviews.push((rows.len(), key));
            }
            if entry.meta.is_none()
                && let Some(path) = &entry.path
            {
                entry.meta = self.meta.get_or_request(path);
            }
            if let Some(package) = entry.package.as_mut()
                && package.app.is_none()
                && let Some(app) = self.meta.app_or_request(&package.identity)
            {
                package.own_name = app.name_for(entry.path.as_deref());
                package.app = Some(app);
            }

            let (cpu, rates) = match (prev, capacity_ticks, elapsed) {
                (Some((pc, _, pio, _)), Some(cap), Some(secs)) if cap > 0.0 => {
                    let cpu = ((cpu_ticks - pc).max(0) as f64 / cap * 100.0).min(100.0) as f32;
                    let rate = |i: usize| ((io[i] - pio[i]).max(0) as f64 / secs) as u64;
                    (cpu, [rate(0), rate(1), rate(2)])
                }
                _ => (0.0, [0; 3]),
            };
            let delta = prev
                .and_then(|(_, previous, _, _)| p.cycle_time.checked_sub(previous))
                .filter(|_| p.cycle_time != 0 || cpu_ticks == 0);
            budget.add(delta);
            cycle_deltas.push(delta);
            entry.cpu_ticks = cpu_ticks;
            entry.cpu_cycles = p.cycle_time;
            entry.io = io;
            let disk_rates = crate::disk_process::interval_rates(
                prev.and_then(|(_, _, _, disk)| disk),
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
                description: match entry.package.as_deref() {
                    Some(p) if p.app.is_none() && now.duration_since(p.since) < PACKAGE_WAIT => {
                        None
                    }
                    Some(Package {
                        own_name: Some(name),
                        ..
                    }) => Some(name.clone()),
                    _ => meta.and_then(|m| m.description.clone()),
                },
                company: meta.and_then(|m| m.company.clone()),
                icon: meta.and_then(|m| m.icon),
                web_view: None,
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
                windows_process: meta.is_some_and(|m| m.windows_component)
                    || kernel_process(pid, entry.parent, &entry.name, entry.path.is_some()),
            });
        }
        // A retained handle keeps the old instance queryable after exit and PID
        // reuse. Include its last interval before releasing the handle. Never
        // subtract sums of cumulative process counters: exits can make them fall.
        self.tracked.retain(|key, removed| {
            if removed.seen == self.generation {
                return true;
            }
            budget.add(
                removed
                    .handle
                    .as_ref()
                    .and_then(|handle| process_cycles(handle.0))
                    .and_then(|last| last.checked_sub(removed.cpu_cycles)),
            );
            if self.by_pid.get(&key.0) == Some(&key.1) {
                self.by_pid.remove(&key.0);
            }
            false
        });
        // Reuse the steady-state table without retaining a process storm's peak.
        let reserve = self.tracked.len() + 32;
        if self.tracked.capacity() > reserve * 2 {
            self.tracked.shrink_to(reserve);
        }
        if self.by_pid.capacity() > reserve * 2 {
            self.by_pid.shrink_to(reserve);
        }
        for i in 0..self.webviews.len() {
            let (index, key) = self.webviews[i];
            let get = |pid| {
                let created = *self.by_pid.get(&pid)?;
                let entry = self.tracked.get(&(pid, created))?;
                Some(webview::Node {
                    instance: (pid, created),
                    parent: entry.parent,
                    session: entry.session,
                    name: &entry.name,
                    path: entry.path.as_ref(),
                    runtime: entry.web_view.as_deref(),
                })
            };
            let context = webview::resolve(get(key.0).unwrap(), get);
            if context.browser != key
                && let Some(browser) = self.tracked.get_mut(&context.browser)
                && let Some(runtime) = browser.web_view.as_mut()
            {
                runtime.learn_context(&context);
            }
            let entry = self.tracked.get_mut(&key).unwrap();
            // A WebView2 process inherits its host's package identity: the host app's name.
            let app = entry
                .package
                .as_ref()
                .and_then(|p| p.app.as_ref()?.name.clone());
            let runtime = entry.web_view.as_mut().unwrap();
            rows[index].web_view = Some(runtime.view(context, &self.meta, app));
        }
        if self.webviews.capacity() > (self.webviews.len() + 8) * 2 {
            self.webviews.shrink_to(self.webviews.len() + 8);
        }
        if let Some(capacity) = self.cycle_clock.capacity(
            clock,
            &budget,
            system_delta.map(|d| d.idle),
            precise_idle_fraction,
        ) {
            for (row, delta) in rows.iter_mut().zip(cycle_deltas) {
                if let Some(delta) = delta
                    && delta as f64 <= capacity * 1.03
                {
                    row.cpu = (delta as f64 / capacity * 100.).clamp(0., 100.) as f32;
                }
            }
        }
        rows
    }
}

fn process_cycles(process: HANDLE) -> Option<u64> {
    let mut cycles = 0;
    unsafe { QueryProcessCycleTime(process, &mut cycles) }
        .is_ok()
        .then_some(cycles)
}

fn query_handle(pid: u32, create_time: i64) -> Option<OwnedHandle> {
    let handle =
        OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }.ok()?;
    (filetime_ticks(created) == create_time).then_some(handle)
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
    fn only_kernel_owned_imageless_instances_use_the_special_system_category() {
        assert!(kernel_process(4, 0, "System", false));
        assert!(kernel_process(120, 4, "Registry", false));
        assert!(kernel_process(124, 4, "MemCompression", false));
        assert!(!kernel_process(120, 4, "Registry", true));
        assert!(!kernel_process(120, 8, "Registry", false));
        assert!(!kernel_process(120, 4, "service.exe", false));
    }
    use std::os::windows::process::CommandExt;

    #[test]
    fn query_handle_checks_process_identity_and_retains_final_cycles() {
        use std::os::windows::io::AsRawHandle;
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let image = std::path::Path::new(&system)
            .join("System32")
            .join("PING.EXE");
        let mut child = std::process::Command::new(image)
            .args(["-n", "30", "127.0.0.1"])
            .creation_flags(0x0800_0000)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe {
            GetProcessTimes(
                HANDLE(child.as_raw_handle()),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        }
        .unwrap();
        let identity = filetime_ticks(created);
        let matched = query_handle(child.id(), identity);
        let wrong_instance = query_handle(child.id(), identity + 1);
        let before = matched.as_ref().and_then(|h| process_cycles(h.0));
        let _ = child.kill();
        child.wait().unwrap();
        // The fixture is stopped before assertions, even when a native query failed.
        assert!(wrong_instance.is_none());
        let final_cycles = process_cycles(matched.unwrap().0).unwrap();
        assert!(final_cycles >= before.unwrap());
    }

    #[test]
    fn a_child_is_found_by_its_name_with_its_path() {
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let image = std::path::Path::new(&system)
            .join("System32")
            .join("PING.EXE");
        let mut child = std::process::Command::new(&image)
            .args(["-n", "30", "127.0.0.1"])
            .creation_flags(0x0800_0000)
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
