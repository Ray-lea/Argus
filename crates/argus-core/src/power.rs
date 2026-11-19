//! Estimated power usage of each process, for the "Power usage" and "Power
//! usage trend" columns that Windows Task Manager also has.
//!
//! Like Task Manager's, the estimate weighs a process's CPU, GPU and disk
//! activity. Windows publishes neither its weights nor where its levels
//! begin, so these are Argus's own: the unit is one logical processor kept
//! busy, a fully busy GPU engine counts as `GPU_WEIGHT` of them and disk
//! transfers as one per `DISK_RATE`. The levels split that scale between an
//! idle process, light background work, an app in use, a process keeping a
//! core busy and a heavy load (`BOUNDS`). They are relative levels, not
//! watts: no per-process energy reading exists without a driver.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::Instant;

use crate::process::ProcessRow;

/// The levels from lowest to highest; `processes.power.<level>` names them.
pub const LEVELS: [&str; 5] = ["veryLow", "low", "moderate", "high", "veryHigh"];

/// Where each level above the lowest begins, in busy logical processors.
const BOUNDS: [f64; 4] = [0.1, 0.3, 0.8, 2.0];

/// A fully busy GPU engine draws about as much as this many busy logical
/// processors: more for a discrete card, less for an integrated one.
const GPU_WEIGHT: f64 = 4.0;

/// Disk transfers at this many bytes a second weigh as much as one busy
/// logical processor.
const DISK_RATE: f64 = 512.0 * 1024.0 * 1024.0;

/// A drive at full speed draws about as much as one or two busy logical
/// processors: faster figures add no more. A process's disk counters can
/// report tens of gigabytes a second over an interval, which no drive moves.
const DISK_LIMIT: f64 = 1.5;

/// The trend averages about the last two minutes in buckets this long; the
/// one filling now counts too, so the window spans 110 to 120 seconds.
const BUCKET_SECS: f64 = 10.0;
const BUCKETS: usize = 12;

/// A trend shows once this many seconds of a process have been averaged.
const TREND_MIN_SECS: f32 = 10.0;

/// One sample stands for at most this long. Past the slowest background
/// cadence (30 s); a longer gap, such as sleep, adds no more weight.
const MAX_SAMPLE_SECS: f64 = 35.0;

/// The index in `LEVELS` of an estimate.
pub fn level(estimate: f64) -> usize {
    BOUNDS
        .iter()
        .take_while(|bound| estimate >= **bound)
        .count()
}

/// A process's estimate from its share of the machine's CPU and of its
/// busiest GPU engine (both percent) and the disk bytes it moves a second.
pub fn estimate(cpu: f32, logical_cpus: u32, gpu: f32, disk_bytes: u64) -> f64 {
    let cpu = f64::from(cpu.clamp(0.0, 100.0)) / 100.0 * f64::from(logical_cpus.max(1));
    let gpu = f64::from(gpu.clamp(0.0, 100.0)) / 100.0 * GPU_WEIGHT;
    cpu + gpu + (disk_bytes as f64 / DISK_RATE).min(DISK_LIMIT)
}

/// The estimates of one process instance over the trend's window.
struct Trend {
    /// Estimate times seconds, and seconds, per bucket at its number modulo
    /// `BUCKETS`.
    buckets: [(f32, f32); BUCKETS],
    /// The number of the latest bucket.
    latest: u64,
    /// The pass that last listed the process.
    seen: u64,
}

impl Trend {
    fn new(bucket: u64) -> Self {
        Self {
            buckets: [(0.0, 0.0); BUCKETS],
            latest: bucket,
            seen: 0,
        }
    }

    /// Empties the buckets that have left the window by `bucket`.
    fn advance(&mut self, bucket: u64) {
        let passed = bucket.saturating_sub(self.latest).min(BUCKETS as u64);
        for step in 1..=passed {
            self.buckets[((self.latest + step) % BUCKETS as u64) as usize] = (0.0, 0.0);
        }
        self.latest = self.latest.max(bucket);
    }

    fn add(&mut self, bucket: u64, estimate: f64, secs: f64) {
        self.advance(bucket);
        let slot = &mut self.buckets[(bucket % BUCKETS as u64) as usize];
        slot.0 += (estimate * secs) as f32;
        slot.1 += secs as f32;
    }

    fn average(&self) -> Option<f32> {
        let (sum, secs) = self
            .buckets
            .iter()
            .fold((0.0, 0.0), |(sum, secs), (s, t)| (sum + s, secs + t));
        (secs >= TREND_MIN_SECS).then(|| sum / secs)
    }
}

/// Fills in each snapshot's estimates and keeps their trends. It runs after
/// the host has added the disk rates the service measures.
#[derive(Default)]
pub struct PowerTracker {
    epoch: Option<Instant>,
    last: Option<Instant>,
    trends: HashMap<(u32, i64), Trend>,
    pass: u64,
}

impl PowerTracker {
    pub fn apply(&mut self, processes: &mut [ProcessRow], logical_cpus: u32, now: Instant) {
        let epoch = *self.epoch.get_or_insert(now);
        let bucket = (now.saturating_duration_since(epoch).as_secs_f64() / BUCKET_SECS) as u64;
        let secs = self
            .last
            .map(|last| now.saturating_duration_since(last).as_secs_f64())
            .filter(|secs| *secs > 0.0)
            .map(|secs| secs.min(MAX_SAMPLE_SECS));
        self.last = Some(now);
        self.pass += 1;
        for p in processes {
            p.power = None;
            p.power_trend = None;
            if p.counters_pending {
                continue;
            }
            let disk = p.disk_read.unwrap_or(0) + p.disk_write.unwrap_or(0);
            let estimate = estimate(p.cpu, logical_cpus, p.gpu, disk);
            p.power = Some(estimate as f32);
            let trend = match self.trends.entry((p.pid, p.create_time)) {
                Entry::Occupied(entry) => {
                    let trend = entry.into_mut();
                    match secs {
                        // A reading kept from an earlier query counts once.
                        Some(secs) if !p.stale && !p.sample_reused => {
                            trend.add(bucket, estimate, secs)
                        }
                        _ => trend.advance(bucket),
                    }
                    trend
                }
                // A new process's first reading has no interval of its own.
                Entry::Vacant(entry) => entry.insert(Trend::new(bucket)),
            };
            trend.seen = self.pass;
            p.power_trend = trend.average();
        }
        let pass = self.pass;
        self.trends.retain(|_, trend| trend.seen == pass);
    }
}

#[cfg(test)]
mod tests {
    use super::{LEVELS, PowerTracker, estimate, level};
    use crate::process::ProcessRow;
    use std::time::{Duration, Instant};

    fn row(pid: u32, cpu: f32) -> ProcessRow {
        ProcessRow {
            pid,
            create_time: 1,
            cpu,
            ..Default::default()
        }
    }

    #[test]
    fn levels_rise_with_cpu_gpu_and_disk_activity() {
        assert_eq!(LEVELS[level(estimate(0.0, 16, 0.0, 0))], "veryLow");
        // A quarter of one of 16 logical processors.
        assert_eq!(LEVELS[level(estimate(1.5, 16, 0.0, 0))], "low");
        assert_eq!(LEVELS[level(estimate(0.0, 16, 15.0, 0))], "moderate");
        // One logical processor kept busy.
        assert_eq!(LEVELS[level(estimate(6.25, 16, 0.0, 0))], "high");
        assert_eq!(
            LEVELS[level(estimate(0.0, 16, 0.0, 600 * 1024 * 1024))],
            "high"
        );
        // Disk alone stays below the top level, however fast it reads.
        assert_eq!(LEVELS[level(estimate(0.0, 16, 0.0, 40 << 30))], "high");
        assert_eq!(LEVELS[level(estimate(50.0, 16, 80.0, 0))], "veryHigh");
        // Out-of-range readings stay within the scale.
        assert_eq!(estimate(250.0, 4, -5.0, 0), 4.0);
    }

    #[test]
    fn the_trend_averages_about_two_minutes_and_skips_reused_readings() {
        let start = Instant::now();
        let at = |secs: u64| start + Duration::from_secs(secs);
        let mut tracker = PowerTracker::default();
        let mut busy = [row(7, 25.0)];
        tracker.apply(&mut busy, 4, at(0));
        assert_eq!(busy[0].power, Some(1.0));
        // Not before ten seconds have been averaged.
        for second in 1..10 {
            tracker.apply(&mut busy, 4, at(second));
            assert_eq!(busy[0].power_trend, None);
        }
        tracker.apply(&mut busy, 4, at(10));
        assert_eq!(busy[0].power_trend, Some(1.0));
        // Idle from now on: the trend falls while the busy seconds remain
        // in the window, then reaches the idle level.
        let mut idle = [row(7, 0.0)];
        tracker.apply(&mut idle, 4, at(70));
        let partly = idle[0].power_trend.unwrap();
        assert!(partly > 0.0 && partly < 1.0);
        // A reused reading adds nothing: a busy one here would raise it.
        let mut reused = [ProcessRow {
            sample_reused: true,
            ..row(7, 100.0)
        }];
        tracker.apply(&mut reused, 4, at(80));
        assert!(reused[0].power_trend.unwrap() <= partly);
        for second in (81..=140).step_by(10) {
            tracker.apply(&mut idle, 4, at(second));
        }
        assert_eq!(idle[0].power_trend, Some(0.0));
        assert_eq!(level(f64::from(idle[0].power_trend.unwrap())), 0);
    }

    #[test]
    fn pending_counters_and_new_instances_have_no_trend() {
        let start = Instant::now();
        let mut tracker = PowerTracker::default();
        let mut rows = [row(7, 10.0)];
        for second in 0..=20 {
            tracker.apply(&mut rows, 4, start + Duration::from_secs(second));
        }
        assert!(rows[0].power_trend.is_some());
        // The same PID started again is another instance.
        let mut reused_pid = [ProcessRow {
            create_time: 2,
            ..row(7, 10.0)
        }];
        tracker.apply(&mut reused_pid, 4, start + Duration::from_secs(21));
        assert_eq!(reused_pid[0].power_trend, None);
        let mut pending = [ProcessRow {
            counters_pending: true,
            ..row(8, 0.0)
        }];
        tracker.apply(&mut pending, 4, start + Duration::from_secs(22));
        assert_eq!((pending[0].power, pending[0].power_trend), (None, None));
        assert!(tracker.trends.is_empty());
    }
}
