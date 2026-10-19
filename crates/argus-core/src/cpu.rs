//! Native CPU counters and checked cycle accounting shared by the samplers.

use std::time::Instant;

use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS};
use windows::Win32::System::Threading::{GetActiveProcessorCount, GetActiveProcessorGroupCount};

use crate::nt::NtQuerySystemInformationEx;

pub(crate) const PROCESSOR_IDLE: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(42);
const IDLE_CYCLES: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(83);
const INTERRUPT_CYCLES: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(108);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct IdleTimes {
    pub idle: i64,
    c1: i64,
    c2: i64,
    c3: i64,
    c1_transitions: u32,
    c2_transitions: u32,
    c3_transitions: u32,
    padding: u32,
}

const _: () = assert!(size_of::<IdleTimes>() == 48);

/// The classes used here have a fixed record size and at most 64 records per group.
///
/// # Safety
/// `T` must be the native, plain-data record type returned by `class`.
pub(crate) unsafe fn read_group<T: Copy>(
    class: SYSTEM_INFORMATION_CLASS,
    group: u16,
    values: &mut [T],
) -> bool {
    if !(1..=64).contains(&values.len()) {
        return false;
    }
    let bytes = size_of_val(values) as u32;
    let mut returned = 0;
    let status = unsafe {
        NtQuerySystemInformationEx(
            class,
            (&group as *const u16).cast(),
            size_of::<u16>() as u32,
            values.as_mut_ptr().cast(),
            bytes,
            &mut returned,
        )
    };
    status.0 >= 0 && returned == bytes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProcessorCycles {
    group: u16,
    processor: u8,
    idle: u64,
    interrupts: u64,
}

pub(crate) struct SystemCycles(Vec<ProcessorCycles>);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SystemCycleDelta {
    pub idle: u128,
    pub interrupts: u128,
}

impl SystemCycleDelta {
    pub fn total(self) -> u128 {
        self.idle + self.interrupts
    }
}

impl SystemCycles {
    pub fn read(logical_cpus: u32) -> Option<Self> {
        let mut values = Vec::with_capacity(logical_cpus.min(4096) as usize);
        let mut idle = [0u64; 64];
        let mut interrupts = [0u64; 64];
        for group in 0..unsafe { GetActiveProcessorGroupCount() } {
            let count = unsafe { GetActiveProcessorCount(group) } as usize;
            if !(1..=64).contains(&count)
                || !unsafe { read_group(IDLE_CYCLES, group, &mut idle[..count]) }
                || !unsafe { read_group(INTERRUPT_CYCLES, group, &mut interrupts[..count]) }
            {
                return None;
            }
            for processor in 0..count {
                values.push(ProcessorCycles {
                    group,
                    processor: processor as u8,
                    idle: idle[processor],
                    interrupts: interrupts[processor],
                });
            }
        }
        (!values.is_empty() && values.len() == logical_cpus as usize).then_some(Self(values))
    }

    /// Idle plus interrupt/DPC cycles. Process cycles are accounted separately.
    pub fn delta(&self, previous: &Self) -> Option<SystemCycleDelta> {
        if self.0.len() != previous.0.len() {
            return None;
        }
        self.0
            .iter()
            .zip(&previous.0)
            .try_fold(SystemCycleDelta::default(), |sum, (n, p)| {
                if (n.group, n.processor) != (p.group, p.processor) {
                    return None;
                }
                Some(SystemCycleDelta {
                    idle: sum.idle + n.idle.checked_sub(p.idle)? as u128,
                    interrupts: sum.interrupts + n.interrupts.checked_sub(p.interrupts)? as u128,
                })
            })
    }
}

pub(crate) struct ClockInterval {
    capacity: f64,
    seconds: f64,
    rate: f64,
}

/// A processor-clock denominator includes processes born and exited between polls.
/// It is used only when Windows selected a user-mode QPC timebase, and its units
/// agree with native cycle counters. No MHz/boost estimate is involved.
pub(crate) struct CycleClock {
    supported: bool,
    previous: Option<(u64, Instant, u32)>,
    rate: Option<f64>,
    verified: bool,
}

impl CycleClock {
    pub fn new() -> Self {
        Self {
            supported: processor_clock_supported(),
            previous: None,
            rate: None,
            verified: false,
        }
    }

    pub fn clear(&mut self) {
        self.previous = None;
        self.rate = None;
        self.verified = false;
    }

    pub fn sample(&mut self, logical_cpus: u32) -> Option<ClockInterval> {
        if !self.supported {
            return None;
        }
        let cycles = unsafe {
            std::arch::x86_64::_mm_lfence();
            std::arch::x86_64::_rdtsc()
        };
        self.record(cycles, Instant::now(), logical_cpus)
    }

    fn record(&mut self, cycles: u64, now: Instant, count: u32) -> Option<ClockInterval> {
        let previous = self.previous.replace((cycles, now, count));
        let interval = previous.and_then(|(before, at, n)| {
            let seconds = now.checked_duration_since(at)?.as_secs_f64();
            let delta = cycles.checked_sub(before)?;
            // Background sampling supports 30 s; leave room for scheduling jitter.
            (count == n && count > 0 && delta > 0 && seconds > 0. && seconds <= 60.).then_some(
                ClockInterval {
                    capacity: delta as f64 * count as f64,
                    seconds,
                    rate: delta as f64 / seconds,
                },
            )
        });
        if interval.is_none() {
            self.rate = None;
            self.verified = false;
        }
        interval
    }

    pub fn capacity(
        &mut self,
        interval: Option<ClockInterval>,
        native: &CycleBudget,
        idle_cycles: Option<u128>,
        idle_fraction: Option<f64>,
    ) -> Option<f64> {
        let interval = interval?;
        if self
            .rate
            .is_some_and(|rate| ((interval.rate / rate) - 1.).abs() > 0.02)
        {
            self.rate = Some(interval.rate);
            self.verified = false;
            return None;
        }
        self.rate = Some(interval.rate);
        // Native queries are adjacent, not atomic. Allow bounded sampling skew.
        if native.known as f64 > interval.capacity * 1.03 {
            self.verified = false;
            return None;
        }
        let tolerance = (0.03125 / interval.seconds).clamp(0.03, 0.25);
        if let Some((idle, fraction)) = idle_cycles.zip(idle_fraction)
            && fraction >= tolerance * 2.
        {
            if ((idle as f64 / interval.capacity) - fraction).abs() > tolerance {
                self.verified = false;
                return None;
            }
            self.verified = true;
        }
        if native
            .total()
            .is_some_and(|value| value >= interval.capacity * 0.97)
        {
            self.verified = true;
        }
        self.verified.then_some(interval.capacity)
    }
}

fn processor_clock_supported() -> bool {
    // SYSTEM_QUERY_PERFORMANCE_COUNTER_INFORMATION, available since Windows 7 SP1.
    // On x64 a QPC without a kernel transition uses the TSC which Windows has
    // checked for invariance and synchronization across processors.
    let mut info = [1u32, 0, 0]; // Version, Flags, ValidFlags.
    let mut returned = 0;
    let status = unsafe {
        NtQuerySystemInformation(
            SYSTEM_INFORMATION_CLASS(124),
            info.as_mut_ptr().cast(),
            size_of_val(&info) as u32,
            &mut returned,
        )
    };
    status.0 >= 0
        && returned == size_of_val(&info) as u32
        && info[0] == 1
        && info[2] & 1 != 0
        && info[1] & 1 == 0
}

/// Native cycles validate the clock; their possibly incomplete sum is never a
/// denominator. Keep the lower bound even when a contributor is unavailable.
pub(crate) struct CycleBudget {
    known: u128,
    complete: bool,
}

impl CycleBudget {
    pub fn new(system: Option<u128>) -> Self {
        Self {
            known: system.unwrap_or_default(),
            complete: system.is_some(),
        }
    }

    pub fn add(&mut self, cycles: Option<u64>) {
        if let Some(cycles) = cycles {
            self.known += cycles as u128;
        } else {
            self.complete = false;
        }
    }

    pub fn total(&self) -> Option<f64> {
        (self.complete && self.known > 0).then_some(self.known as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn clock() -> CycleClock {
        CycleClock {
            supported: true,
            previous: None,
            rate: None,
            verified: false,
        }
    }

    #[test]
    fn a_process_missing_between_polls_cannot_inflate_another_process() {
        let start = Instant::now();
        let mut clock = clock();
        assert!(clock.record(1000, start, 4).is_none());
        let interval = clock.record(1100, start + Duration::from_secs(1), 4);
        let mut native = CycleBudget::new(Some(100));
        native.add(Some(100));
        // Native counters see only half the available cycles. Idle accounting
        // independently verifies the clock even on the first busy interval.
        assert_eq!(
            clock.capacity(interval, &native, Some(100), Some(0.25)),
            Some(400.)
        );
        let interval = clock.record(1200, start + Duration::from_secs(2), 4);
        native.add(None); // An exited protected process has no query handle.
        assert_eq!(
            clock.capacity(interval, &native, Some(100), Some(0.25)),
            Some(400.)
        );
    }

    #[test]
    fn clock_needs_native_validation_and_rejects_conflicting_units() {
        let start = Instant::now();
        let mut clock = clock();
        clock.record(1000, start, 4);
        let interval = clock.record(1100, start + Duration::from_secs(1), 4);
        assert_eq!(
            clock.capacity(interval, &CycleBudget::new(None), None, None),
            None
        );
        let interval = clock.record(1200, start + Duration::from_secs(2), 4);
        assert_eq!(
            clock.capacity(interval, &CycleBudget::new(Some(400)), None, None),
            Some(400.)
        );
        let interval = clock.record(1300, start + Duration::from_secs(3), 4);
        assert_eq!(
            clock.capacity(interval, &CycleBudget::new(Some(200)), Some(80), Some(0.75)),
            None
        );
        let interval = clock.record(1400, start + Duration::from_secs(4), 4);
        let mut incomplete = CycleBudget::new(Some(500));
        incomplete.add(None);
        assert_eq!(
            clock.capacity(interval, &incomplete, Some(300), Some(0.75)),
            None
        );
    }

    #[test]
    fn thirty_second_background_samples_keep_cycle_accounting() {
        let start = Instant::now();
        let mut clock = clock();
        clock.record(1000, start, 4);
        let interval = clock.record(4000, start + Duration::from_secs(30), 4);
        assert_eq!(
            clock.capacity(
                interval,
                &CycleBudget::new(Some(12_000)),
                Some(9000),
                Some(0.75)
            ),
            Some(12_000.)
        );
        let interval = clock.record(4100, start + Duration::from_secs(31), 4);
        assert_eq!(
            clock.capacity(
                interval,
                &CycleBudget::new(Some(400)),
                Some(300),
                Some(0.75)
            ),
            Some(400.)
        );
    }

    #[test]
    fn clock_restarts_after_topology_reset_gap_and_rate_change() {
        let start = Instant::now();
        let mut clock = clock();
        clock.record(1000, start, 4);
        let interval = clock.record(1100, start + Duration::from_secs(1), 4);
        assert_eq!(
            clock.capacity(interval, &CycleBudget::new(Some(400)), None, None),
            Some(400.)
        );
        assert!(
            clock
                .record(1200, start + Duration::from_secs(2), 8)
                .is_none()
        );
        assert!(!clock.verified);
        assert!(
            clock
                .record(100, start + Duration::from_secs(3), 8)
                .is_none()
        );
        assert!(
            clock
                .record(2000, start + Duration::from_secs(90), 8)
                .is_none()
        );
        let interval = clock.record(2100, start + Duration::from_secs(91), 8);
        assert_eq!(
            clock.capacity(interval, &CycleBudget::new(Some(800)), None, None),
            Some(800.)
        );
        let interval = clock.record(2300, start + Duration::from_secs(92), 8);
        assert_eq!(
            clock.capacity(interval, &CycleBudget::new(Some(1600)), None, None),
            None
        );
        clock.clear();
        assert!(
            clock
                .record(2400, start + Duration::from_secs(93), 8)
                .is_none()
        );
        clock.supported = false;
        assert!(clock.sample(8).is_none());
    }

    #[test]
    fn cycle_budget_includes_new_and_exited_processes_without_unsigned_wrap() {
        let mut budget = CycleBudget::new(Some(300));
        budget.add(Some(400)); // Existing process.
        budget.add(Some(100)); // Born since the previous sample.
        budget.add(Some(200)); // Final counter from a retained exited-process handle.
        assert_eq!(budget.total(), Some(1000.));
        assert_eq!(400. / budget.total().unwrap() * 100., 40.);
        budget.add(None);
        assert_eq!(budget.total(), None);
        assert_eq!(CycleBudget::new(Some(0)).total(), None);
        let mut large = CycleBudget::new(Some(u64::MAX as u128));
        large.add(Some(u64::MAX));
        assert!(large.total().unwrap().is_finite());
        assert!(large.total().unwrap() > u64::MAX as f64);
    }

    #[test]
    fn cycles_reject_resets_and_topology_changes_even_if_the_total_increases() {
        let baseline = ProcessorCycles {
            group: 0,
            processor: 0,
            idle: 100,
            interrupts: 10,
        };
        let previous = SystemCycles(vec![baseline]);
        let current = SystemCycles(vec![ProcessorCycles {
            idle: 140,
            interrupts: 30,
            ..baseline
        }]);
        assert_eq!(
            current.delta(&previous),
            Some(SystemCycleDelta {
                idle: 40,
                interrupts: 20
            })
        );
        for changed in [
            ProcessorCycles {
                idle: 99,
                interrupts: 300,
                ..baseline
            },
            ProcessorCycles {
                idle: 300,
                interrupts: 9,
                ..baseline
            },
            ProcessorCycles {
                group: 1,
                ..baseline
            },
            ProcessorCycles {
                processor: 1,
                ..baseline
            },
        ] {
            assert_eq!(SystemCycles(vec![changed]).delta(&previous), None);
        }
        assert_eq!(
            SystemCycles(vec![baseline, baseline]).delta(&previous),
            None
        );
    }

    #[test]
    fn native_cpu_queries_cover_all_groups_and_check_buffer_sizes() {
        use windows::Win32::System::Threading::ALL_PROCESSOR_GROUPS;
        let count = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) };
        assert!(SystemCycles::read(count).is_some());
        assert!(SystemCycles::read(count + 1).is_none());
        assert!(!unsafe { read_group(IDLE_CYCLES, 0, &mut [0u64; 0]) });
        assert!(!unsafe { read_group(IDLE_CYCLES, 0, &mut [0u64; 65]) });
        assert!(!unsafe { read_group(IDLE_CYCLES, u16::MAX, &mut [0u64; 1]) });
    }
}
