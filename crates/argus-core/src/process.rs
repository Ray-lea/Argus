//! Process enumeration and per-process rates.
//!
//! One `NtQuerySystemInformation(SystemProcessInformation)` call returns every
//! process and thread with their counters, which is far cheaper than Toolhelp,
//! PDH or WMI. Rates are computed from deltas against the previous sample.

use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Wdk::System::SystemInformation::SystemProcessInformation as SPI_CLASS;
use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::System::SystemInformation::{GetSystemTimePreciseAsFileTime, GetTickCount64};
use windows::Win32::System::Threading::{
    GetProcessInformation, GetProcessTimes, OpenProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_LIMITED_INFORMATION, ProcessPowerThrottling,
};
use windows::Win32::System::WindowsProgramming::QueryProcessCycleTime;

use crate::cpu::{CycleBudget, CycleClock, CycleUsage, SystemCycles, cycle_usage};
use crate::meta::{FileMeta, MetaStore};
use crate::nt::{
    ProcessEntry, ProcessIter, THREAD_STATE_WAITING, WAIT_REASON_SUSPENDED,
    query_system_information,
};
use crate::package::{AppMeta, Identity};
use crate::paths::PathResolver;
use crate::util::{AlignedBuf, OwnedHandle, filetime_ticks, ticks_to_unix_ms};
use crate::webview::{self, Instance, WebViewInfo};

mod facts;
#[cfg(test)]
mod lifecycle_tests;
mod query;
mod roster;
pub use facts::Facts;
use query::ProcessQuery;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRow {
    /// This row's liveness was selected by the independent roster.
    pub listed_by_roster: bool,
    /// An identity-only row discovered before its first counter snapshot.
    pub counters_pending: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence_number: Option<u64>,
    /// Stable UI identity, also while a new process's creation time is unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_key: Option<Arc<str>>,
    /// The last observation is older than the short retention window.
    /// Display it as an older reading and exclude it from fresh time series.
    pub stale: bool,
    /// A recent reading retained for display while the next query is pending.
    /// It must not be counted again in time series or history averages.
    pub sample_reused: bool,
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
    /// File version from the executable's version resource.
    pub version: Option<Arc<str>>,
    #[serde(flatten)]
    pub facts: Facts,
    pub icon: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_view: Option<Arc<WebViewInfo>>,
    /// Percent of total machine capacity, 0–100.
    pub cpu: f32,
    /// Total CPU time consumed, ms.
    pub cpu_time: u64,
    #[serde(flatten)]
    pub counters: ProcessCounters,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gpu_engines: Vec<crate::gpu_process::GpuEngineUsage>,
    /// Resident GPU memory, bytes; absent while the provider cannot report it.
    pub gpu_dedicated: Option<u64>,
    pub gpu_shared: Option<u64>,
    /// Network bytes per second, when per-process traffic is measured (ETW).
    pub net_sent: u64,
    pub net_received: u64,
    /// Estimated power usage from CPU, GPU and disk activity, in busy
    /// logical processors (`power`); None until the counters arrive.
    pub power: Option<f32>,
    /// That estimate averaged over about the last two minutes; None until
    /// ten seconds of the process have been seen.
    pub power_trend: Option<f32>,
    pub threads: u32,
    pub handles: u32,
    pub session_id: u32,
    pub base_priority: i32,
    pub suspended: bool,
    /// Suspended because Windows froze the process, as it does packaged apps
    /// in the background: Windows thaws it again, and resuming does not.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub frozen: bool,
    /// Efficiency mode (EcoQoS) is on, set by the app itself or by a user.
    pub efficiency: bool,
    /// None when the process can't be queried.
    pub io_priority: Option<crate::actions::IoPriority>,
    pub memory_priority: Option<crate::actions::MemoryPriority>,
    /// Owns a taskbar window, which puts it under "Apps" (see `app_windows`).
    pub app_window: bool,
    /// Windows component identity for display grouping, not an action safety check.
    pub windows_process: bool,
    /// The counters only optional columns show, while one of them is
    /// (`ProcessTracker::set_extra_counters`): for every process, they make
    /// serializing a snapshot about half as costly again.
    #[serde(flatten)]
    pub extra: Option<ProcessExtra>,
}

/// Additional columns from the same system snapshot, without per-process queries.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessCounters {
    /// Accumulated processor time, milliseconds.
    pub cpu_user_time: u64,
    pub cpu_kernel_time: u64,
    pub peak_working_set: u64,
    /// Reserved and committed virtual address ranges, in bytes.
    pub virtual_size: u64,
    /// All page faults reported for this process, including soft faults.
    pub page_faults: u32,
    /// Lifetime transfer totals, bytes, across file, network, and device I/O.
    pub io_read_total: u64,
    pub io_write_total: u64,
    pub io_other_total: u64,
}

impl ProcessCounters {
    fn from_snapshot(p: &crate::nt::SystemProcessInformation) -> Self {
        Self {
            cpu_user_time: (p.user_time.max(0) / 10_000) as u64,
            cpu_kernel_time: (p.kernel_time.max(0) / 10_000) as u64,
            peak_working_set: p.peak_working_set_size as u64,
            virtual_size: p.virtual_size as u64,
            page_faults: p.page_fault_count,
            io_read_total: p.read_transfer_count.max(0) as u64,
            io_write_total: p.write_transfer_count.max(0) as u64,
            io_other_total: p.other_transfer_count.max(0) as u64,
        }
    }
}

/// Counters of optional columns, from the same system snapshot without
/// per-process queries.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessExtra {
    /// `cpu` split by the kernel-mode share of the interval's CPU time (of
    /// the lifetime's when the interval has none), percent.
    pub cpu_kernel: f32,
    pub cpu_user: f32,
    /// The working set's pages other processes can map too.
    pub shared_ws: u64,
    /// Peak private committed bytes.
    pub peak_commit: u64,
    pub peak_virtual_size: u64,
    /// Kernel pool usage charged to the process, bytes.
    pub paged_pool: u64,
    pub non_paged_pool: u64,
    /// The page faults resolved by reading from disk.
    pub hard_faults: u32,
    /// Page faults, the hard ones among them, and context switches per second.
    pub page_fault_rate: u64,
    pub hard_fault_rate: u64,
    pub context_switch_rate: u64,
    /// Context switches of the threads it has now: those of exited threads
    /// leave the sum.
    pub context_switches: u64,
    /// I/O operations per second, then over the process's lifetime, across
    /// file, network, and device I/O.
    pub io_read_ops: u64,
    pub io_write_ops: u64,
    pub io_other_ops: u64,
    pub io_read_ops_total: u64,
    pub io_write_ops_total: u64,
    pub io_other_ops_total: u64,
    /// Disk requests per second, absent like `disk_read` and also from a
    /// privileged service that does not report them.
    pub disk_read_ops: Option<u64>,
    pub disk_write_ops: Option<u64>,
    /// Lifetime disk bytes and requests; absent without a disk source.
    pub disk_read_total: Option<u64>,
    pub disk_write_total: Option<u64>,
    pub disk_read_ops_total: Option<u64>,
    pub disk_write_ops_total: Option<u64>,
    /// The most threads the process has had at once.
    pub peak_threads: u32,
    /// Processes in the list started by this instance (`ppid` and a later
    /// creation time).
    pub children: u32,
    /// Time since the instance started, ms; absent while that is unknown.
    pub runtime: Option<u64>,
}

impl ProcessExtra {
    /// What the snapshot and the interval's rates give; the CPU split, the
    /// disk counts and the lineage are filled in later.
    fn of(
        p: &crate::nt::SystemProcessInformation,
        totals: &Totals,
        rates: &Rates,
        private_ws: u64,
    ) -> Self {
        Self {
            shared_ws: (p.working_set_size as u64).saturating_sub(private_ws),
            peak_commit: p.peak_pagefile_usage as u64,
            peak_virtual_size: p.peak_virtual_size as u64,
            paged_pool: p.quota_paged_pool_usage as u64,
            non_paged_pool: p.quota_non_paged_pool_usage as u64,
            hard_faults: p.hard_fault_count,
            page_fault_rate: rates.page_faults,
            hard_fault_rate: rates.hard_faults,
            context_switch_rate: rates.context_switches,
            context_switches: totals.context_switches,
            io_read_ops: rates.io_ops[0],
            io_write_ops: rates.io_ops[1],
            io_other_ops: rates.io_ops[2],
            io_read_ops_total: p.read_operation_count.max(0) as u64,
            io_write_ops_total: p.write_operation_count.max(0) as u64,
            io_other_ops_total: p.other_operation_count.max(0) as u64,
            peak_threads: p.number_of_threads_high_watermark,
            ..Self::default()
        }
    }
}

/// A process instance's cumulative counters, whose differences between two
/// samples give its rates. Kept for every process, so the optional columns
/// have rates as soon as one is shown.
#[derive(Clone, Copy, Default)]
struct Totals {
    /// Kernel and user time, 100 ns.
    kernel_ticks: i64,
    cpu_ticks: i64,
    cycles: u64,
    /// Read, write and other I/O: bytes, then operations.
    io: [i64; 3],
    io_ops: [i64; 3],
    page_faults: u32,
    hard_faults: u32,
    context_switches: u64,
    disk: Option<crate::disk_process::DiskCounts>,
}

impl Totals {
    fn of(
        p: &crate::nt::SystemProcessInformation,
        threads: &[crate::nt::SystemThreadInformation],
        disk: Option<crate::disk_process::DiskCounts>,
    ) -> Self {
        Self {
            kernel_ticks: p.kernel_time,
            cpu_ticks: p.kernel_time + p.user_time,
            cycles: p.cycle_time,
            io: [
                p.read_transfer_count,
                p.write_transfer_count,
                p.other_transfer_count,
            ],
            io_ops: [
                p.read_operation_count,
                p.write_operation_count,
                p.other_operation_count,
            ],
            page_faults: p.page_fault_count,
            hard_faults: p.hard_fault_count,
            context_switches: threads.iter().map(|t| u64::from(t.context_switches)).sum(),
            disk,
        }
    }

    /// What a process started since the previous sample had then: nothing.
    fn started(&self) -> Self {
        Self {
            disk: self.disk.map(|disk| crate::disk_process::DiskCounts {
                operations: disk.operations.map(|_| (0, 0)),
                ..Default::default()
            }),
            ..Self::default()
        }
    }

    /// The kernel-mode share of the CPU time since `previous`, or of the
    /// lifetime's when there was none in between.
    fn kernel_share(&self, previous: Option<&Self>) -> f32 {
        let share = |kernel: i64, total: i64| {
            (total > 0).then(|| kernel.clamp(0, total) as f32 / total as f32)
        };
        previous
            .and_then(|p| {
                share(
                    self.kernel_ticks - p.kernel_ticks,
                    self.cpu_ticks - p.cpu_ticks,
                )
            })
            .or_else(|| share(self.kernel_ticks, self.cpu_ticks))
            .unwrap_or(0.)
    }
}

/// Per-second rates of a process's counters over one interval.
#[derive(Default)]
struct Rates {
    io: [u64; 3],
    io_ops: [u64; 3],
    page_faults: u64,
    hard_faults: u64,
    context_switches: u64,
}

impl Rates {
    fn between(previous: &Totals, now: &Totals, secs: f64) -> Self {
        // Bytes round down; events to the nearest whole one. A count that
        // went down (threads exited with their switches) counts as none.
        let bytes = |now: i64, before: i64| ((now - before).max(0) as f64 / secs) as u64;
        let events =
            |now: u64, before: u64| (now.saturating_sub(before) as f64 / secs).round() as u64;
        let ops = |i: usize| {
            events(
                now.io_ops[i].max(0) as u64,
                previous.io_ops[i].max(0) as u64,
            )
        };
        Self {
            io: [0, 1, 2].map(|i| bytes(now.io[i], previous.io[i])),
            io_ops: [ops(0), ops(1), ops(2)],
            page_faults: events(now.page_faults.into(), previous.page_faults.into()),
            hard_faults: events(now.hard_faults.into(), previous.hard_faults.into()),
            context_switches: events(now.context_switches, previous.context_switches),
        }
    }
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
    sequence_number: Option<u64>,
    seen: bool,
    parent: u32,
    session: u32,
    name: Arc<str>,
    path: Option<Arc<str>>,
    meta: Option<Arc<FileMeta>>,
    totals: Totals,
    /// Query handle, opened once per running instance and released when its
    /// snapshot has no threads. Access denial is final for that instance.
    handle: Option<OwnedHandle>,
    /// Read on the facts thread, up to `facts::READS` times while incomplete;
    /// an instance without a query handle has none.
    facts: Facts,
    fact_reads: u8,
    fact_pending: bool,
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

fn cache_expired(observed: Option<Instant>, now: Instant) -> bool {
    observed.is_none_or(|at| now.saturating_duration_since(at) > Duration::from_secs(10))
}

pub(crate) struct ProcessTracker {
    roster: roster::Roster,
    counted: Option<(Instant, Instant)>,
    query: Option<ProcessQuery>,
    last_rows: Vec<ProcessRow>,
    tracked: HashMap<(u32, i64), Tracked>,
    by_pid: HashMap<u32, i64>,
    webviews: Vec<(usize, Instance)>,
    generation: bool,
    paths: PathResolver,
    meta: Arc<MetaStore>,
    facts: Option<facts::Reader>,
    last_instant: Option<Instant>,
    last_aligned: bool,
    last_systime: i64,
    native_disk: bool,
    /// Rows carry `ProcessExtra`.
    extra: bool,
    previous_cycles: Option<SystemCycles>,
    cycle_clock: CycleClock,
    /// Total and per-processor usage from idle cycles over the latest
    /// interval, where the processor clock checked out.
    cycle_usage: Option<CycleUsage>,
}

impl ProcessTracker {
    pub fn new(meta: Arc<MetaStore>) -> Self {
        Self {
            roster: roster::Roster::new(),
            counted: None,
            query: ProcessQuery::new(),
            last_rows: Vec::new(),
            tracked: HashMap::new(),
            by_pid: HashMap::new(),
            webviews: Vec::new(),
            generation: false,
            paths: PathResolver::new(),
            meta,
            facts: facts::Reader::new(),
            last_instant: None,
            last_aligned: false,
            last_systime: 0,
            native_disk: crate::disk_process::standard_supported(),
            extra: false,
            previous_cycles: None,
            cycle_clock: CycleClock::new(),
            cycle_usage: None,
        }
    }

    /// CPU usage from idle cycles over the latest process interval, when the
    /// processor clock checked out for it (`cpu::cycle_usage`).
    pub fn cycle_usage(&self) -> Option<&CycleUsage> {
        self.cycle_usage.as_ref()
    }

    /// Whether rows carry the counters only optional columns show
    /// (`ProcessRow::extra`), from the next sample on.
    pub fn set_extra_counters(&mut self, enabled: bool) {
        self.extra = enabled;
    }

    /// When the latest accepted native counter query started.
    pub fn sample_started(&self) -> Option<Instant> {
        self.counted.map(|(started, _)| started)
    }

    pub fn sample(
        &mut self,
        logical_cpus: u32,
        precise_idle_fraction: Option<f64>,
    ) -> Vec<ProcessRow> {
        let counters = self.sample_counters(logical_cpus, precise_idle_fraction);
        self.roster.refresh();
        let mut rows = self.roster.merge(counters, self.counted, |identity| {
            let created = identity_time(identity.pid, identity.sequence).unwrap_or(-1);
            ProcessRow {
                pid: identity.pid,
                ppid: identity.parent,
                name: identity.name.clone(),
                create_time: created,
                counters_pending: true,
                sample_reused: true,
                windows_process: kernel_process(
                    identity.pid,
                    identity.parent,
                    &identity.name,
                    false,
                ),
                ..Default::default()
            }
        });
        if self.extra {
            let now = ticks_to_unix_ms(filetime_ticks(unsafe { GetSystemTimePreciseAsFileTime() }));
            count_lineage(&mut rows, now, unsafe { GetTickCount64() });
        }
        rows
    }

    fn sample_counters(
        &mut self,
        logical_cpus: u32,
        precise_idle_fraction: Option<f64>,
    ) -> Vec<ProcessRow> {
        let poll_started = Instant::now();
        self.cycle_usage = None;
        if let Some(reader) = &self.facts {
            for (key, facts) in reader.answers() {
                if let Some(entry) = self.tracked.get_mut(&key) {
                    entry.facts.merge(facts);
                    entry.fact_pending = false;
                }
            }
        }
        let Some((buf, returned, observed)) = self
            .query
            .as_mut()
            .and_then(|query| query.read(logical_cpus))
        else {
            // Keep both counter baselines until another real observation arrives.
            self.last_aligned = false;
            let stale = cache_expired(self.last_instant, poll_started);
            return self
                .last_rows
                .iter()
                .cloned()
                .map(|mut row| {
                    row.stale = stale;
                    row.sample_reused = true;
                    row
                })
                .collect();
        };
        let now = observed.clock.at;
        self.counted = Some((observed.started, now));
        let systime = observed.systime;
        let mut cycles = observed.cycles;
        if observed.logical_cpus != logical_cpus {
            self.previous_cycles = None;
            self.cycle_clock.clear();
            cycles = None;
        }
        let clock = self.cycle_clock.sample(logical_cpus, &observed.clock);
        // The system's idle fraction spans one tick, whereas the recovered
        // process rates span the whole gap since their last successful query.
        let aligned = now >= poll_started;
        let precise_idle_fraction = (aligned && self.last_aligned)
            .then_some(precise_idle_fraction)
            .flatten();
        self.last_aligned = aligned;
        let system_delta = cycles
            .as_ref()
            .zip(self.previous_cycles.as_ref())
            .and_then(|(n, p)| n.delta(p));
        let idle_deltas = cycles
            .as_ref()
            .zip(self.previous_cycles.as_ref())
            .and_then(|(n, p)| n.idle_deltas(p));
        let mut budget = CycleBudget::new(system_delta.map(|d| d.total()));
        self.previous_cycles = cycles;
        let elapsed = self
            .last_instant
            .map(|t| now.duration_since(t).as_secs_f64());
        let prev_systime = self.last_systime;
        self.last_instant = Some(now);
        self.last_systime = systime;
        let stale = cache_expired(Some(now), Instant::now());

        let capacity_ticks = elapsed.map(|s| s * 1e7 * logical_cpus as f64);
        self.generation = !self.generation;
        let mut rows = Vec::with_capacity(self.tracked.len() + 32);
        let mut cycle_deltas = Vec::with_capacity(self.tracked.len() + 32);
        let mut kernel_shares = Vec::with_capacity(self.tracked.len() + 32);
        self.webviews.clear();

        for ProcessEntry { info: p, threads } in ProcessIter::new(buf) {
            let pid = p.unique_process_id.0 as usize as u32;
            if pid == 0 {
                continue; // System Idle Process: it is the absence of load, not a process.
            }
            let key = (pid, p.create_time);
            let disk = self
                .native_disk
                .then(|| crate::disk_process::standard_counters(buf, returned, p))
                .flatten();
            let totals = Totals::of(p, threads, disk);

            let (entry, prev) = match self.tracked.entry(key) {
                Entry::Occupied(slot) => {
                    let t = slot.into_mut();
                    let prev = t.totals;
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
                    let prev = started.then(|| totals.started());
                    // Exited objects can remain in native snapshots while
                    // another owner holds a handle. Do not prolong that lifetime.
                    let handle = (p.number_of_threads != 0)
                        .then(|| query_handle(pid, p.create_time))
                        .flatten();
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
                            sequence_number: handle
                                .as_ref()
                                .and_then(|handle| roster::sequence(handle.0)),
                            seen: self.generation,
                            parent: p.inherited_from_unique_process_id.0 as usize as u32,
                            session: p.session_id,
                            name,
                            path,
                            meta: None,
                            totals,
                            handle,
                            facts: Facts::default(),
                            fact_reads: 0,
                            fact_pending: false,
                            web_view: runtime,
                            package,
                        }),
                        prev,
                    )
                }
            };
            entry.seen = self.generation;
            if p.number_of_threads == 0 {
                // Release our reference before the next snapshot; otherwise it
                // can keep this zero-thread record present indefinitely. Its
                // final CPU and I/O counters still come from this snapshot.
                entry.handle = None;
            }
            if let Some(runtime) = entry.web_view.as_mut() {
                if let Some(line) = &entry.facts.command_line {
                    runtime.learn_command_line(line.clone());
                }
                runtime.query(entry.handle.as_ref().map(|h| h.0));
                self.webviews.push((rows.len(), key));
                // The WebView parser reads the line on this thread anyway.
                if entry.facts.command_line.is_none() {
                    entry.facts.command_line = runtime.command_line().cloned();
                }
            }
            if entry.handle.is_some()
                && !entry.fact_pending
                && entry.fact_reads < facts::READS
                && !entry.facts.complete()
                && self
                    .facts
                    .as_ref()
                    .is_some_and(|reader| reader.request(key, entry.facts.clone()))
            {
                entry.fact_reads += 1;
                entry.fact_pending = true;
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

            let (cpu, rates) = match (&prev, capacity_ticks, elapsed) {
                (Some(previous), Some(cap), Some(secs)) if cap > 0.0 => {
                    let cpu = ((totals.cpu_ticks - previous.cpu_ticks).max(0) as f64 / cap * 100.0)
                        .min(100.0) as f32;
                    (cpu, Rates::between(previous, &totals, secs))
                }
                _ => (0.0, Rates::default()),
            };
            let delta = prev
                .and_then(|previous| p.cycle_time.checked_sub(previous.cycles))
                .filter(|_| p.cycle_time != 0 || totals.cpu_ticks == 0);
            budget.add(delta);
            cycle_deltas.push(delta);
            kernel_shares.push(totals.kernel_share(prev.as_ref()));
            entry.totals = totals;
            let disk_rates = crate::disk_process::interval_rates(
                prev.and_then(|previous| previous.disk),
                disk,
                elapsed,
            );

            let suspended = all_suspended(threads);
            let meta = entry.meta.as_deref();
            let private_ws = p.working_set_private_size.max(0) as u64;
            let mut row = ProcessRow {
                listed_by_roster: false,
                counters_pending: false,
                sequence_number: entry.sequence_number,
                instance_key: entry
                    .sequence_number
                    .map(|sequence| format!("{pid}:seq:{sequence}").into()),
                stale,
                sample_reused: false,
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
                version: meta.and_then(|m| m.version.clone()),
                facts: entry.facts.clone(),
                icon: meta.and_then(|m| m.icon),
                web_view: None,
                cpu,
                cpu_time: (totals.cpu_ticks.max(0) / 10_000) as u64,
                counters: ProcessCounters::from_snapshot(p),
                private_ws,
                working_set: p.working_set_size as u64,
                commit: p.private_page_count as u64,
                io_read: rates.io[0],
                io_write: rates.io[1],
                io_other: rates.io[2],
                // Filled from `disk` below.
                disk_read: None,
                disk_write: None,
                gpu: 0.0,
                gpu_engine: None,
                gpu_engines: Vec::new(),
                gpu_dedicated: None,
                gpu_shared: None,
                net_sent: 0,
                net_received: 0,
                // The host estimates them once the disk rates are in.
                power: None,
                power_trend: None,
                threads: p.number_of_threads,
                handles: p.handle_count,
                session_id: p.session_id,
                base_priority: p.base_priority,
                suspended,
                // Only fully suspended processes can be frozen: one query for
                // the few of them, none for the rest.
                frozen: suspended && entry.handle.as_ref().is_some_and(|h| frozen(h.0)),
                efficiency: entry.handle.as_ref().is_some_and(|h| efficiency_mode(h.0)),
                io_priority: entry
                    .handle
                    .as_ref()
                    .and_then(|h| crate::actions::io_priority(h.0)),
                memory_priority: entry
                    .handle
                    .as_ref()
                    .and_then(|h| crate::actions::memory_priority(h.0)),
                app_window: false,
                windows_process: meta.is_some_and(|m| m.windows_component)
                    || kernel_process(pid, entry.parent, &entry.name, entry.path.is_some()),
                extra: self
                    .extra
                    .then(|| ProcessExtra::of(p, &totals, &rates, private_ws)),
            };
            crate::disk_process::fill(&mut row, disk, disk_rates);
            rows.push(row);
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
                    .and_then(|last| last.checked_sub(removed.totals.cycles)),
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
        // The exited processes have let go of their executables' metadata.
        self.meta.sweep();
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
            self.cycle_usage = idle_deltas
                .as_deref()
                .and_then(|idle| cycle_usage(capacity, idle));
        }
        for (row, kernel) in rows.iter_mut().zip(kernel_shares) {
            if let Some(extra) = row.extra.as_mut() {
                extra.cpu_kernel = row.cpu * kernel;
                extra.cpu_user = row.cpu - extra.cpu_kernel;
            }
        }
        self.last_rows.clone_from(&rows);
        rows
    }
}

/// Each listed instance's children and its time since it started, `now`
/// being the wall clock and `uptime` the time since system startup, in ms.
/// A child is a later instance naming it as parent: a reused PID's earlier
/// owner is no parent of later processes. A creation time of 0 is system
/// startup; a negative one is unknown.
fn count_lineage(rows: &mut [ProcessRow], now: i64, uptime: u64) {
    let started: HashMap<u32, i64> = rows
        .iter()
        .filter(|row| row.create_time >= 0)
        .map(|row| (row.pid, row.create_time))
        .collect();
    let mut children: HashMap<u32, u32> = HashMap::new();
    for row in rows
        .iter()
        .filter(|row| row.create_time >= 0 && row.ppid != row.pid)
    {
        if started
            .get(&row.ppid)
            .is_some_and(|&parent| parent <= row.create_time)
        {
            *children.entry(row.ppid).or_default() += 1;
        }
    }
    for row in rows {
        let created = row.create_time;
        let Some(extra) = row.extra.as_mut() else {
            continue;
        };
        extra.children = children.get(&row.pid).copied().unwrap_or(0);
        extra.runtime = match created {
            0 => Some(uptime),
            created if created > 0 => Some(now.saturating_sub(created).max(0) as u64),
            _ => None,
        };
    }
}

fn process_cycles(process: HANDLE) -> Option<u64> {
    let mut cycles = 0;
    unsafe { QueryProcessCycleTime(process, &mut cycles) }
        .is_ok()
        .then_some(cycles)
}

fn identity_time(pid: u32, sequence: u64) -> Option<i64> {
    let handle =
        OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    if roster::sequence(handle.0) != Some(sequence) {
        return None;
    }
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }.ok()?;
    Some(ticks_to_unix_ms(filetime_ticks(created)))
}

/// Fault injection is limited to development/test binaries. The host only
/// exposes it to isolated --test instances; production has no delay hook.
#[cfg(debug_assertions)]
pub mod testing {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    static DELAY: AtomicU64 = AtomicU64::new(0);
    static DELAY_ACTIVE: AtomicBool = AtomicBool::new(false);
    pub fn delay_next_counter_reply(milliseconds: u64) {
        DELAY.store(milliseconds.min(30_000), Ordering::Relaxed);
    }
    pub(crate) fn take_delay() -> u64 {
        DELAY.swap(0, Ordering::Relaxed)
    }
    pub fn counter_reply_delayed() -> bool {
        DELAY_ACTIVE.load(Ordering::Relaxed)
    }
    pub(crate) fn active(value: bool) {
        DELAY_ACTIVE.store(value, Ordering::Relaxed);
    }
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
/// `PROCESS_EXTENDED_BASIC_INFORMATION`, the extended form of
/// `ProcessBasicInformation` that `Size` selects.
#[repr(C)]
#[derive(Default)]
struct ExtendedBasicInformation {
    size: usize,
    exit_status: i32,
    peb_base_address: usize,
    affinity_mask: usize,
    base_priority: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
    flags: u32,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<ExtendedBasicInformation>() == 64);

/// The `IsFrozen` bit of `PROCESS_EXTENDED_BASIC_INFORMATION::Flags`.
const IS_FROZEN: u32 = 1 << 4;

/// Whether Windows froze the process (deep freeze), as Process Lifetime
/// Management does to packaged apps in the background. Its threads read as
/// suspended, yet `NtResumeProcess` does not thaw it: the freeze is not a
/// suspend count. Needs `PROCESS_QUERY_LIMITED_INFORMATION`; false when the
/// query fails.
pub(crate) fn frozen(process: HANDLE) -> bool {
    let mut info = ExtendedBasicInformation {
        size: size_of::<ExtendedBasicInformation>(),
        ..Default::default()
    };
    let status = unsafe {
        windows::Wdk::System::Threading::NtQueryInformationProcess(
            process,
            windows::Wdk::System::Threading::PROCESSINFOCLASS(0),
            (&mut info as *mut ExtendedBasicInformation).cast(),
            size_of::<ExtendedBasicInformation>() as u32,
            std::ptr::null_mut(),
        )
    };
    status.0 >= 0 && info.flags & IS_FROZEN != 0
}

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

fn thread_suspended(t: &crate::nt::SystemThreadInformation) -> bool {
    t.thread_state == THREAD_STATE_WAITING && t.wait_reason == WAIT_REASON_SUSPENDED
}

/// Whether every thread is waiting in the suspended state, as the list shows it.
fn all_suspended(threads: &[crate::nt::SystemThreadInformation]) -> bool {
    !threads.is_empty() && threads.iter().all(thread_suspended)
}

/// How many of a process's threads are suspended right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Suspension {
    None,
    Partial,
    All,
}

/// The suspension of the process instance `pid` + `create_time` (Unix ms; 0
/// matches any instance), read from a fresh snapshot rather than the sampled
/// list. `Ok(None)` means the instance is not in the list (it exited); a
/// failed snapshot is an error, so it is never mistaken for an exit.
pub(crate) fn suspension(
    pid: u32,
    create_time: i64,
) -> Result<Option<Suspension>, windows::Win32::Foundation::NTSTATUS> {
    let mut buf = AlignedBuf::with_capacity(512 * 1024);
    query_system_information(SPI_CLASS, &mut buf)?;
    Ok(ProcessIter::new(&buf)
        .find(|e| {
            e.info.unique_process_id.0 as usize as u32 == pid
                && (create_time == 0 || ticks_to_unix_ms(e.info.create_time) == create_time)
        })
        .map(|e| {
            if all_suspended(e.threads) {
                Suspension::All
            } else if e.threads.iter().any(thread_suspended) {
                Suspension::Partial
            } else {
                Suspension::None
            }
        }))
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
    #[test]
    #[ignore = "owned child fixture for watch identity and launch regressions"]
    fn liveness_child() {
        use std::io::Read;
        let _ = std::io::stdin().read(&mut [0u8]);
    }

    #[test]
    fn pending_queries_keep_recent_readings_but_expire_a_stuck_source() {
        let at = std::time::Instant::now();
        assert!(!super::cache_expired(
            Some(at),
            at + std::time::Duration::from_secs(4)
        ));
        assert!(super::cache_expired(
            Some(at),
            at + std::time::Duration::from_secs(11)
        ));
        assert!(super::cache_expired(None, at));
    }
    use super::*;

    #[test]
    fn extra_counters_keep_native_units_and_clamp_invalid_signed_values() {
        // This native record contains only integer fields and nullable pointers.
        let mut snapshot: crate::nt::SystemProcessInformation = unsafe { std::mem::zeroed() };
        snapshot.user_time = 12_345_678;
        snapshot.kernel_time = 45_678_901;
        snapshot.peak_working_set_size = 12_345;
        snapshot.virtual_size = 67_890;
        snapshot.page_fault_count = 456;
        snapshot.read_transfer_count = 9_876_543;
        snapshot.write_transfer_count = 123;
        snapshot.other_transfer_count = -1;
        snapshot.hard_fault_count = 45;
        snapshot.read_operation_count = 77;
        snapshot.write_operation_count = 8;
        snapshot.other_operation_count = -3;
        snapshot.number_of_threads_high_watermark = 31;
        snapshot.peak_pagefile_usage = 4096;
        snapshot.peak_virtual_size = 99_999;
        snapshot.quota_paged_pool_usage = 512;
        snapshot.quota_non_paged_pool_usage = 64;
        let mut threads: [crate::nt::SystemThreadInformation; 2] = unsafe { std::mem::zeroed() };
        threads[0].context_switches = u32::MAX;
        threads[1].context_switches = 2;
        snapshot.working_set_size = 10_000;
        let counters = ProcessCounters::from_snapshot(&snapshot);
        assert_eq!(counters.cpu_user_time, 1234);
        assert_eq!(counters.cpu_kernel_time, 4567);
        assert_eq!(counters.peak_working_set, 12_345);
        assert_eq!(counters.virtual_size, 67_890);
        assert_eq!(counters.page_faults, 456);
        assert_eq!(counters.io_read_total, 9_876_543);
        assert_eq!(counters.io_write_total, 123);
        assert_eq!(counters.io_other_total, 0);
        let totals = Totals::of(&snapshot, &threads, None);
        // Summed wide: one thread's count alone fills 32 bits.
        assert_eq!(totals.context_switches, u64::from(u32::MAX) + 2);
        let extra = ProcessExtra::of(&snapshot, &totals, &Rates::default(), 7_000);
        assert_eq!(extra.shared_ws, 3_000);
        assert_eq!(extra.hard_faults, 45);
        assert_eq!(
            [
                extra.io_read_ops_total,
                extra.io_write_ops_total,
                extra.io_other_ops_total
            ],
            [77, 8, 0]
        );
        assert_eq!(extra.peak_threads, 31);
        assert_eq!(extra.peak_commit, 4096);
        assert_eq!(extra.peak_virtual_size, 99_999);
        assert_eq!((extra.paged_pool, extra.non_paged_pool), (512, 64));
        assert_eq!(extra.context_switches, totals.context_switches);
        // More private than in the working set: a reading between two updates.
        assert_eq!(
            ProcessExtra::of(&snapshot, &totals, &Rates::default(), 20_000).shared_ws,
            0
        );
        snapshot.user_time = -1;
        assert_eq!(ProcessCounters::from_snapshot(&snapshot).cpu_user_time, 0);
    }

    #[test]
    fn rates_count_events_to_the_nearest_and_never_go_negative() {
        let before = Totals {
            io: [1000, 0, 5],
            io_ops: [10, 0, 0],
            page_faults: 100,
            hard_faults: 4,
            context_switches: 900,
            ..Totals::default()
        };
        let after = Totals {
            io: [4001, 2, 1],
            io_ops: [13, 1, 0],
            page_faults: 103,
            hard_faults: 4,
            // A thread with many switches exited.
            context_switches: 500,
            ..Totals::default()
        };
        let rates = Rates::between(&before, &after, 2.0);
        assert_eq!(rates.io, [1500, 1, 0]);
        assert_eq!(rates.io_ops, [2, 1, 0]);
        assert_eq!((rates.page_faults, rates.hard_faults), (2, 0));
        assert_eq!(rates.context_switches, 0);
    }

    #[test]
    fn kernel_share_follows_the_interval_then_the_lifetime() {
        let at = |kernel, total| Totals {
            kernel_ticks: kernel,
            cpu_ticks: total,
            ..Totals::default()
        };
        assert_eq!(at(30, 100).kernel_share(Some(&at(10, 20))), 0.25);
        // No CPU time in between: the lifetime's share.
        assert_eq!(at(30, 100).kernel_share(Some(&at(30, 100))), 0.3);
        assert_eq!(at(30, 100).kernel_share(None), 0.3);
        assert_eq!(at(0, 0).kernel_share(None), 0.);
        // A process started since the last sample starts from nothing.
        assert_eq!(at(5, 10).kernel_share(Some(&at(5, 10).started())), 0.5);
    }

    #[test]
    fn lineage_counts_later_children_and_time_since_start() {
        let row = |pid, ppid, create_time| ProcessRow {
            pid,
            ppid,
            create_time,
            extra: Some(ProcessExtra::default()),
            ..ProcessRow::default()
        };
        let mut rows = [
            // Started with the system.
            row(4, 0, 0),
            row(100, 4, 1_000),
            row(200, 100, 2_000),
            row(201, 100, 3_000),
            // An older instance than PID 100's: PID reuse, no child of it.
            row(202, 100, 500),
            // Identity known, creation time not yet.
            row(203, 100, -1),
            row(300, 300, 4_000),
        ];
        // A row without the optional counters still has its children counted.
        rows[3].extra = None;
        count_lineage(&mut rows, 10_000, 60_000);
        let extra = |row: &ProcessRow| row.extra.clone().unwrap_or_default();
        let children = rows
            .iter()
            .map(|row| extra(row).children)
            .collect::<Vec<_>>();
        assert_eq!(children, [1, 2, 0, 0, 0, 0, 0]);
        assert_eq!(extra(&rows[0]).runtime, Some(60_000));
        assert_eq!(extra(&rows[1]).runtime, Some(9_000));
        assert_eq!(extra(&rows[5]).runtime, None);
    }

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
