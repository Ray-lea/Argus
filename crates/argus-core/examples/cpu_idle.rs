//! Compares the CPU usage the collector reports with what Windows' own idle
//! accounting gives over the same seconds: the legacy idle time of
//! `SystemProcessorPerformanceInformation`, PDH's `% Processor Time` (which
//! Task Manager's standard metrics match since Windows 11 24H2) and
//! `% Processor Utility` (its older, frequency-scaled figure).
//! `cargo run -p argus-core --example cpu_idle --release -- [seconds]`

use std::ffi::c_void;
use std::time::Duration;

use argus_core::{Collector, MetaStore};
use windows::Win32::System::Performance::{
    PDH_FMT, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW,
    PdhCollectQueryData, PdhGetFormattedCounterValue, PdhOpenQueryW,
};
use windows::Win32::System::Threading::{GetActiveProcessorCount, GetActiveProcessorGroupCount};
use windows::core::{PCWSTR, w};

/// Values over 100 % kept (utility exceeds it while the clock is boosted).
const PDH_FMT_NOCAP100: PDH_FMT = PDH_FMT(0x0000_8000);

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformationEx(
        class: i32,
        input: *const c_void,
        input_length: u32,
        buffer: *mut c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}

/// `SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION`: idle, kernel (with idle) and
/// user time in 100 ns.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Times {
    idle: i64,
    kernel: i64,
    user: i64,
    dpc: i64,
    interrupt: i64,
    interrupts: u32,
}

/// Idle cycles (`SystemProcessorIdleCycleTimeInformation`) summed over every
/// logical processor, and the number of processors.
fn idle_cycles() -> (u64, u64) {
    let mut sum = (0, 0);
    for group in 0..unsafe { GetActiveProcessorGroupCount() } {
        let count = unsafe { GetActiveProcessorCount(group) }.min(64) as usize;
        let mut cycles = vec![0u64; count];
        let mut returned = 0;
        let status = unsafe {
            NtQuerySystemInformationEx(
                83,
                (&group as *const u16).cast(),
                2,
                cycles.as_mut_ptr().cast(),
                (count * 8) as u32,
                &mut returned,
            )
        };
        if status < 0 {
            continue;
        }
        sum.0 += cycles.iter().sum::<u64>();
        sum.1 += count as u64;
    }
    sum
}

/// Idle, kernel and user time summed over every logical processor.
fn legacy() -> (i64, i64, i64) {
    let mut sum = (0, 0, 0);
    for group in 0..unsafe { GetActiveProcessorGroupCount() } {
        let count = unsafe { GetActiveProcessorCount(group) }.min(64) as usize;
        let mut times = vec![Times::default(); count];
        let mut returned = 0;
        let status = unsafe {
            NtQuerySystemInformationEx(
                8,
                (&group as *const u16).cast(),
                2,
                times.as_mut_ptr().cast(),
                (count * size_of::<Times>()) as u32,
                &mut returned,
            )
        };
        if status < 0 {
            continue;
        }
        for t in &times {
            sum.0 += t.idle;
            sum.1 += t.kernel;
            sum.2 += t.user;
        }
    }
    sum
}

fn counter(query: PDH_HQUERY, path: PCWSTR) -> PDH_HCOUNTER {
    let mut counter = PDH_HCOUNTER::default();
    unsafe { PdhAddEnglishCounterW(query, path, 0, &mut counter) };
    counter
}

fn value(counter: PDH_HCOUNTER) -> f64 {
    let mut value = PDH_FMT_COUNTERVALUE::default();
    let status = unsafe {
        PdhGetFormattedCounterValue(
            counter,
            PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100.0),
            None,
            &mut value,
        )
    };
    if status == 0 {
        unsafe { value.Anonymous.doubleValue }
    } else {
        f64::NAN
    }
}

fn main() {
    let seconds = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(15);
    let mut collector = Collector::new(MetaStore::new());
    let mut query = PDH_HQUERY::default();
    unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) };
    let time = counter(
        query,
        w!(r"\Processor Information(_Total)\% Processor Time"),
    );
    let utility = counter(
        query,
        w!(r"\Processor Information(_Total)\% Processor Utility"),
    );
    let interrupt = counter(
        query,
        w!(r"\Processor Information(_Total)\% Interrupt Time"),
    );
    let dpc = counter(query, w!(r"\Processor Information(_Total)\% DPC Time"));
    // Primes every counter and source: rates need two readings.
    let _ = collector.sample();
    unsafe { PdhCollectQueryData(query) };
    let mut before = legacy();
    // The idle thread's cycles against every processor's invariant TSC ticks:
    // precise, and the idle loop counts as idle, as Windows' time accounting.
    let mut cycles_before = (idle_cycles(), unsafe { core::arch::x86_64::_rdtsc() });
    println!(" s  argus  legacy-idle  pdh-time  pdh-utility  processes  int+dpc  idle-cycles");
    let mut sums = [0f64; 7];
    for second in 1..=seconds {
        std::thread::sleep(Duration::from_secs(1));
        let snapshot = collector.sample();
        unsafe { PdhCollectQueryData(query) };
        let cycles_now = (idle_cycles(), unsafe { core::arch::x86_64::_rdtsc() });
        let capacity = (cycles_now.1 - cycles_before.1) as f64 * cycles_now.0.1 as f64;
        let idle_cycles_used = (cycles_now.0.0 - cycles_before.0.0) as f64;
        cycles_before = cycles_now;
        let cycle_usage = (1. - idle_cycles_used / capacity) * 100.;
        let now = legacy();
        let idle = (now.0 - before.0) as f64;
        let total = ((now.1 - before.1) + (now.2 - before.2)) as f64;
        before = now;
        let legacy_usage = if total > 0. {
            (1. - idle / total) * 100.
        } else {
            f64::NAN
        };
        // What the processes' own CPU time adds up to, as Task Manager's
        // standard metric counts it.
        let processes = snapshot
            .processes
            .iter()
            .map(|p| f64::from(p.cpu))
            .sum::<f64>();
        let row = [
            f64::from(snapshot.system.cpu),
            legacy_usage,
            value(time),
            value(utility),
            processes,
            value(interrupt) + value(dpc),
            cycle_usage,
        ];
        for (sum, v) in sums.iter_mut().zip(row) {
            *sum += v;
        }
        println!(
            "{second:>2}  {:>5.1}  {:>11.1}  {:>8.1}  {:>11.1}  {:>9.1}  {:>7.1}  {:>11.1}",
            row[0], row[1], row[2], row[3], row[4], row[5], row[6]
        );
    }
    let n = f64::from(seconds.max(1));
    println!(
        "avg {:>5.1}  {:>11.1}  {:>8.1}  {:>11.1}  {:>9.1}  {:>7.1}  {:>11.1}",
        sums[0] / n,
        sums[1] / n,
        sums[2] / n,
        sums[3] / n,
        sums[4] / n,
        sums[5] / n,
        sums[6] / n
    );
}
