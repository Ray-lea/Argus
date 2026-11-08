//! Read-only comparison of Windows process sources. No process names or paths
//! are printed. The parent harness bounds each run and alternates source order.
//! cargo run -p argus-core --example process_sources --release -- full 60
//! Modes: full, basic, hybrid, compare. Samples default to 60, interval to 1000 ms.

#[allow(dead_code)]
#[path = "../src/nt.rs"]
mod nt;
#[allow(dead_code)]
#[path = "../src/util.rs"]
mod util;

use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use util::{AlignedBuf, OwnedHandle, filetime_ticks};
use windows::Wdk::System::SystemInformation::{SYSTEM_INFORMATION_CLASS, SystemProcessInformation};
use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX2};
use windows::Win32::System::Threading::{
    ALL_PROCESSOR_GROUPS, GetActiveProcessorCount, GetCurrentProcess, GetCurrentThread,
    GetProcessHandleCount, GetProcessInformation, GetProcessIoCounters, GetProcessTimes,
    GetSystemTimes, GetThreadTimes, IO_COUNTERS, OpenProcess,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION, ProcessPowerThrottling,
    SetProcessInformation,
};

// phnt ntexapi.h at System Informer 837a50b78b28356ee3ef8eb2dc3401f09705eb21.
const BASIC: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(252);
#[repr(C)]
struct BasicProcess {
    next: u32,
    pid: HANDLE,
    parent: HANDLE,
    sequence: u64,
    name: nt::UnicodeString,
}
const _: () = assert!(size_of::<BasicProcess>() == 48);
const _: () = assert!(std::mem::offset_of!(BasicProcess, sequence) == 24);

#[derive(Default)]
struct Record {
    pid: u32,
    parent: u32,
    identity: u64,
    created: Option<u64>,
    cpu: Option<u64>,
    private_ws: Option<u64>,
    working_set: Option<u64>,
    commit: Option<u64>,
    io: Option<[u64; 3]>,
    handles: Option<u32>,
    threads: Option<u32>,
}

#[derive(Default)]
struct Coverage {
    cpu: usize,
    private_ws: usize,
    working_set: usize,
    commit: usize,
    io: usize,
    handles: usize,
    threads: usize,
}

impl Coverage {
    fn of(records: &[Record]) -> Self {
        Self {
            cpu: records.iter().filter(|r| r.cpu.is_some()).count(),
            private_ws: records.iter().filter(|r| r.private_ws.is_some()).count(),
            working_set: records.iter().filter(|r| r.working_set.is_some()).count(),
            commit: records.iter().filter(|r| r.commit.is_some()).count(),
            io: records.iter().filter(|r| r.io.is_some()).count(),
            handles: records.iter().filter(|r| r.handles.is_some()).count(),
            threads: records.iter().filter(|r| r.threads.is_some()).count(),
        }
    }
    fn json(&self) -> Value {
        json!({"cpu":self.cpu,"private_ws":self.private_ws,"working_set":self.working_set,
            "commit":self.commit,"io":self.io,"handles":self.handles,"threads":self.threads})
    }
}

struct Reading {
    rows: Vec<Record>,
    milliseconds: f64,
    native_ms: f64,
    thread_cpu_ms: f64,
    thread_kernel_ms: f64,
    buffer_bytes: usize,
    errors: BTreeMap<String, usize>,
    opened: usize,
    cached_handles: usize,
    started: Instant,
}

fn times(thread: bool) -> (u64, u64) {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        if thread {
            GetThreadTimes(
                GetCurrentThread(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
            .expect("own thread times");
        } else {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
            .expect("own process times");
        }
    }
    (filetime_ticks(kernel) as u64, filetime_ticks(user) as u64)
}

fn error(errors: &mut BTreeMap<String, usize>, stage: &str, code: i32) {
    *errors
        .entry(format!("{stage}:0x{:08x}", code as u32))
        .or_default() += 1;
}

struct Full {
    buffer: AlignedBuf,
}
impl Full {
    fn new() -> Self {
        Self {
            buffer: AlignedBuf::with_capacity(512 * 1024),
        }
    }
    fn sample(&mut self) -> Reading {
        let cpu = times(true);
        let started = Instant::now();
        let mut errors = BTreeMap::new();
        let result = nt::query_system_information(SystemProcessInformation, &mut self.buffer);
        let native_ms = started.elapsed().as_secs_f64() * 1000.;
        let rows = match result {
            Ok(_) => nt::ProcessIter::new(&self.buffer)
                .filter_map(|entry| {
                    let p = entry.info;
                    let pid = p.unique_process_id.0 as usize as u32;
                    (pid != 0).then(|| Record {
                        pid,
                        parent: p.inherited_from_unique_process_id.0 as usize as u32,
                        identity: p.create_time as u64,
                        created: Some(p.create_time as u64),
                        cpu: Some((p.kernel_time + p.user_time).max(0) as u64),
                        private_ws: Some(p.working_set_private_size.max(0) as u64),
                        working_set: Some(p.working_set_size as u64),
                        commit: Some(p.private_page_count as u64),
                        io: Some([
                            p.read_transfer_count.max(0) as u64,
                            p.write_transfer_count.max(0) as u64,
                            p.other_transfer_count.max(0) as u64,
                        ]),
                        handles: Some(p.handle_count),
                        threads: Some(p.number_of_threads),
                    })
                })
                .collect(),
            Err(status) => {
                error(&mut errors, "full", status.0);
                Vec::new()
            }
        };
        finish(
            started,
            cpu,
            native_ms,
            self.buffer.len(),
            rows,
            errors,
            0,
            0,
        )
    }
}

fn basic_entries(buffer: &AlignedBuf, returned: usize) -> Option<Vec<Record>> {
    if returned > buffer.len() {
        return None;
    }
    let mut offset = 0usize;
    let mut rows = Vec::new();
    loop {
        if !offset.is_multiple_of(8) || offset.checked_add(size_of::<BasicProcess>())? > returned {
            return None;
        }
        // Native records and their links are validated before dereferencing.
        let p = unsafe { &*buffer.as_ptr().add(offset).cast::<BasicProcess>() };
        let pid = p.pid.0 as usize as u32;
        if pid != 0 {
            rows.push(Record {
                pid,
                parent: p.parent.0 as usize as u32,
                identity: p.sequence,
                ..Default::default()
            });
        }
        if p.next == 0 {
            return Some(rows);
        }
        if (p.next as usize) < size_of::<BasicProcess>() {
            return None;
        }
        offset = offset.checked_add(p.next as usize)?;
    }
}

struct CachedHandle(Result<OwnedHandle, i32>);
struct Basic {
    buffer: AlignedBuf,
    handles: HashMap<(u32, u64), CachedHandle>,
}
impl Basic {
    fn new() -> Self {
        Self {
            buffer: AlignedBuf::with_capacity(128 * 1024),
            handles: HashMap::new(),
        }
    }
    fn sample(&mut self, metrics: bool) -> Reading {
        let cpu = times(true);
        let started = Instant::now();
        let mut errors = BTreeMap::new();
        let result = nt::query_system_information(BASIC, &mut self.buffer);
        let native_ms = started.elapsed().as_secs_f64() * 1000.;
        let mut rows = match result {
            Ok(returned) => basic_entries(&self.buffer, returned).unwrap_or_else(|| {
                error(&mut errors, "basic-layout", 0);
                Vec::new()
            }),
            Err(status) => {
                error(&mut errors, "basic", status.0);
                Vec::new()
            }
        };
        let mut opened = 0;
        if metrics {
            let active: HashSet<_> = rows.iter().map(|p| (p.pid, p.identity)).collect();
            self.handles.retain(|key, _| active.contains(key));
            for row in &mut rows {
                let slot = self
                    .handles
                    .entry((row.pid, row.identity))
                    .or_insert_with(|| {
                        opened += 1;
                        CachedHandle(
                            unsafe {
                                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, row.pid)
                            }
                            .map(OwnedHandle)
                            .map_err(|e| e.code().0),
                        )
                    });
                let handle = match &slot.0 {
                    Ok(handle) => handle.0,
                    Err(code) => {
                        error(&mut errors, "open", *code);
                        continue;
                    }
                };
                let (mut created, mut exited, mut kernel, mut user) = (
                    FILETIME::default(),
                    FILETIME::default(),
                    FILETIME::default(),
                    FILETIME::default(),
                );
                match unsafe {
                    GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user)
                } {
                    Ok(()) => {
                        row.created = Some(filetime_ticks(created) as u64);
                        row.cpu = Some((filetime_ticks(kernel) + filetime_ticks(user)) as u64);
                    }
                    Err(e) => error(&mut errors, "times", e.code().0),
                }
                let mut memory = PROCESS_MEMORY_COUNTERS_EX2 {
                    cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
                    ..Default::default()
                };
                match unsafe {
                    GetProcessMemoryInfo(
                        handle,
                        (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX2).cast(),
                        memory.cb,
                    )
                } {
                    Ok(()) => {
                        row.private_ws = Some(memory.PrivateWorkingSetSize as u64);
                        row.working_set = Some(memory.WorkingSetSize as u64);
                        row.commit = Some(memory.PrivateUsage as u64);
                    }
                    Err(e) => error(&mut errors, "memory-ex2", e.code().0),
                }
                let mut io = IO_COUNTERS::default();
                match unsafe { GetProcessIoCounters(handle, &mut io) } {
                    Ok(()) => {
                        row.io = Some([
                            io.ReadTransferCount,
                            io.WriteTransferCount,
                            io.OtherTransferCount,
                        ])
                    }
                    Err(e) => error(&mut errors, "io", e.code().0),
                }
                let mut handles = 0;
                match unsafe { GetProcessHandleCount(handle, &mut handles) } {
                    Ok(()) => row.handles = Some(handles),
                    Err(e) => error(&mut errors, "handles", e.code().0),
                }
            }
        }
        finish(
            started,
            cpu,
            native_ms,
            self.buffer.len(),
            rows,
            errors,
            opened,
            self.handles.values().filter(|h| h.0.is_ok()).count(),
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn finish(
    started: Instant,
    before: (u64, u64),
    native_ms: f64,
    buffer_bytes: usize,
    rows: Vec<Record>,
    errors: BTreeMap<String, usize>,
    opened: usize,
    cached_handles: usize,
) -> Reading {
    let milliseconds = started.elapsed().as_secs_f64() * 1000.;
    let after = times(true);
    Reading {
        rows,
        milliseconds,
        native_ms,
        thread_cpu_ms: ((after.0 - before.0) + (after.1 - before.1)) as f64 / 10_000.,
        thread_kernel_ms: (after.0 - before.0) as f64 / 10_000.,
        buffer_bytes,
        errors,
        opened,
        cached_handles,
        started,
    }
}

fn own_memory() -> Value {
    let mut memory = PROCESS_MEMORY_COUNTERS_EX2 {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        ..Default::default()
    };
    let mut handles = 0;
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX2).cast(),
            memory.cb,
        )
        .expect("own memory");
        GetProcessHandleCount(GetCurrentProcess(), &mut handles).expect("own handles");
    }
    json!({"private_bytes":memory.PrivateUsage,"working_set":memory.WorkingSetSize,"handles":handles})
}

fn system_times() -> [u64; 3] {
    let (mut idle, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }
        .expect("system times");
    [
        filetime_ticks(idle) as u64,
        filetime_ticks(kernel) as u64,
        filetime_ticks(user) as u64,
    ]
}

fn standard_environment() -> Value {
    assert!(
        !argus_core::privilege::is_elevated(),
        "benchmark requires a standard token"
    );
    let throttle = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    };
    let mut checked = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ..Default::default()
    };
    unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&throttle as *const PROCESS_POWER_THROTTLING_STATE).cast(),
            size_of_val(&throttle) as u32,
        )
        .expect("disable own EcoQoS");
        GetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&mut checked as *mut PROCESS_POWER_THROTTLING_STATE).cast(),
            size_of_val(&checked) as u32,
        )
        .expect("check own EcoQoS");
    }
    assert_eq!(
        checked.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        0
    );
    #[repr(C)]
    struct Version {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        text: [u16; 128],
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(version: *mut Version) -> i32;
    }
    let mut version = Version {
        size: size_of::<Version>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        text: [0; 128],
    };
    let status = unsafe { RtlGetVersion(&mut version) };
    json!({"event":"environment","elevated":false,"eco_qos":false,"release":!cfg!(debug_assertions),
        "windows_build":version.build,"version_status":status,"logical_cpus":unsafe {GetActiveProcessorCount(ALL_PROCESSOR_GROUPS)},
        "field_notes":{"basic":"identity only","hybrid":"CPU time, EX2 private working set, generic IO and handles; no disk-only bytes, cycle time, thread count or suspension state"}})
}

fn percentiles(values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let p = |q: f64| {
        sorted[((q * sorted.len() as f64).ceil() as usize)
            .saturating_sub(1)
            .min(sorted.len() - 1)]
    };
    json!({"n":sorted.len(),"p50":p(0.5),"p95":p(0.95),"p99":p(0.99),"max":sorted.last(),"mean":sorted.iter().sum::<f64>()/sorted.len() as f64})
}

fn bench(mode: &str, samples: usize, interval: Duration) {
    let mut full = Full::new();
    let mut basic = Basic::new();
    let mut elapsed = Vec::new();
    let mut cpu = Vec::new();
    let mut previous: Option<Instant> = None;
    let mut system = system_times();
    let mut accounted_cpu = times(false);
    for index in 0..samples + 3 {
        if let Some(previous) = previous {
            std::thread::sleep(interval.saturating_sub(previous.elapsed()));
        }
        let reading = match mode {
            "full" => full.sample(),
            "basic" => basic.sample(false),
            _ => basic.sample(true),
        };
        let tick = previous.map(|p| reading.started.duration_since(p).as_secs_f64() * 1000.);
        previous = Some(reading.started);
        let now = system_times();
        // Account across the sleep boundary too: Windows may charge a short
        // burst only when this thread is switched out after printing its row.
        let process_cpu = times(false);
        let process_cpu_ms = ((process_cpu.0 - accounted_cpu.0) + (process_cpu.1 - accounted_cpu.1))
            as f64
            / 10_000.;
        accounted_cpu = process_cpu;
        let total = now[1].saturating_sub(system[1]) + now[2].saturating_sub(system[2]);
        let busy = (total > 0).then(|| {
            100. * (total.saturating_sub(now[0].saturating_sub(system[0]))) as f64 / total as f64
        });
        system = now;
        let warmup = index < 3;
        if !warmup {
            elapsed.push(reading.milliseconds);
            cpu.push(process_cpu_ms);
        }
        println!(
            "{}",
            json!({"event":"sample","mode":mode,"index":index,"warmup":warmup,
            "wall_ms":reading.milliseconds,"native_ms":reading.native_ms,"thread_cpu_ms":reading.thread_cpu_ms,
            "thread_kernel_ms":reading.thread_kernel_ms,"interval_ms":tick,"system_busy_pct":busy,
                "process_cpu_ms":process_cpu_ms,
            "processes":reading.rows.len(),"coverage":Coverage::of(&reading.rows).json(),"errors":reading.errors,
            "opened":reading.opened,"cached_handles":reading.cached_handles,"buffer_bytes":reading.buffer_bytes,"own":own_memory()})
        );
        if reading.rows.is_empty() {
            break;
        }
    }
    println!(
        "{}",
        json!({"event":"summary","mode":mode,"wall_ms":percentiles(&elapsed),"process_cpu_ms":percentiles(&cpu)})
    );
}

fn comparison(before: &Reading, middle: &Reading, after: &Reading) -> Value {
    let first: HashMap<_, _> = before.rows.iter().map(|r| (r.pid, r)).collect();
    let last: HashMap<_, _> = after.rows.iter().map(|r| (r.pid, r)).collect();
    let listed: HashMap<_, _> = middle.rows.iter().map(|r| (r.pid, r)).collect();
    let mut stable_identities = 0usize;
    let mut listed_stable_identities = 0usize;
    let mut basic_parent_mismatches = 0usize;
    for a in &before.rows {
        if a.pid == std::process::id() {
            continue;
        }
        let Some(b) = last.get(&a.pid).filter(|b| b.created == a.created) else {
            continue;
        };
        stable_identities += 1;
        if let Some(m) = listed.get(&a.pid) {
            listed_stable_identities += 1;
            basic_parent_mismatches += usize::from(a.parent == b.parent && m.parent != a.parent);
        }
    }
    let mut paired = 0usize;
    let mut identity_mismatch = 0usize;
    let mut cpu_bounds = 0usize;
    let mut io_bounds = 0usize;
    let mut parents = 0usize;
    let mut stable = [0usize; 3];
    let mut equal = [0usize; 3];
    let mut differences: [Vec<f64>; 3] = Default::default();
    for m in &middle.rows {
        if m.pid == std::process::id() {
            continue;
        }
        let Some((a, b)) = first.get(&m.pid).zip(last.get(&m.pid)) else {
            continue;
        };
        if a.created != b.created || m.created.is_none() {
            continue;
        }
        if a.created != m.created {
            identity_mismatch += 1;
            continue;
        }
        paired += 1;
        if a.parent == b.parent && m.parent != a.parent {
            parents += 1;
        }
        if let Some((low, value, high)) = a.cpu.zip(m.cpu).zip(b.cpu).map(|((a, m), b)| (a, m, b))
            && !(low..=high).contains(&value)
        {
            cpu_bounds += 1;
        }
        if let Some((a, m, b)) = a.io.zip(m.io).zip(b.io).map(|((a, m), b)| (a, m, b))
            && (0..3).any(|i| !(a[i]..=b[i]).contains(&m[i]))
        {
            io_bounds += 1;
        }
        for (i, (a, m, b)) in [
            (a.private_ws, m.private_ws, b.private_ws),
            (a.working_set, m.working_set, b.working_set),
            (a.commit, m.commit, b.commit),
        ]
        .into_iter()
        .enumerate()
        {
            if let Some((a, m, b)) = a.zip(m).zip(b).map(|((a, m), b)| (a, m, b)) {
                let midpoint = a as f64 / 2. + b as f64 / 2.;
                differences[i].push((m as f64 - midpoint).abs() / midpoint.max(4096.) * 100.);
                if a == b {
                    stable[i] += 1;
                    equal[i] += usize::from(a == m);
                }
            }
        }
    }
    json!({"paired":paired,"identity_mismatches":identity_mismatch,"parent_mismatches":parents,
        "stable_identities":stable_identities,"listed_stable_identities":listed_stable_identities,"basic_parent_mismatches":basic_parent_mismatches,
        "cpu_outside_bracket":cpu_bounds,"io_outside_bracket":io_bounds,"memory_stable":stable,"memory_equal":equal,
        "private_ws_difference_pct":percentiles(&differences[0]),"working_set_difference_pct":percentiles(&differences[1]),"commit_difference_pct":percentiles(&differences[2]),
        "coverage":Coverage::of(&middle.rows).json(),"listed":middle.rows.len(),"bracket_ms":after.started.duration_since(before.started).as_secs_f64()*1000.+after.milliseconds,
        "source_ms":[before.milliseconds,middle.milliseconds,after.milliseconds]})
}

fn compare(samples: usize, interval: Duration) {
    let mut full = Full::new();
    let mut basic = Basic::new();
    for index in 0..samples {
        let started = Instant::now();
        let before = full.sample();
        let middle = basic.sample(true);
        let after = full.sample();
        println!(
            "{}",
            json!({"event":"comparison","index":index,"values":comparison(&before,&middle,&after)})
        );
        std::thread::sleep(interval.saturating_sub(started.elapsed()));
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("full");
    assert!(
        matches!(mode, "full" | "basic" | "hybrid" | "compare"),
        "mode: full, basic, hybrid, compare"
    );
    let samples = args
        .get(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(60)
        .clamp(1, 600);
    let interval = args
        .get(2)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1000)
        .clamp(250, 30000);
    println!("{}", standard_environment());
    println!(
        "{}",
        json!({"event":"run","mode":mode,"samples":samples,"interval_ms":interval,
        "timestamp":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),"process_cpu_before":times(false)})
    );
    if mode == "compare" {
        compare(samples, Duration::from_millis(interval));
    } else {
        bench(mode, samples, Duration::from_millis(interval));
    }
}

#[cfg(test)]
mod tests {
    use super::{AlignedBuf, BasicProcess, basic_entries};
    #[test]
    fn basic_records_validate_alignment_and_links() {
        let mut buffer = AlignedBuf::with_capacity(96);
        assert!(basic_entries(&buffer, 47).is_none());
        assert!(basic_entries(&buffer, 97).is_none());
        unsafe {
            (*buffer.as_mut_ptr().cast::<BasicProcess>()).next = 40;
        }
        assert!(basic_entries(&buffer, 96).is_none());
        unsafe {
            (*buffer.as_mut_ptr().cast::<BasicProcess>()).next = 49;
        }
        assert!(basic_entries(&buffer, 96).is_none());
        unsafe {
            (*buffer.as_mut_ptr().cast::<BasicProcess>()).next = 48;
        }
        assert!(basic_entries(&buffer, 96).unwrap().is_empty());
    }
}
