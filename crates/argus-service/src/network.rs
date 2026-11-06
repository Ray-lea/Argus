//! One leased ETW reader; cumulative snapshots never consume another client's
//! traffic. Idle clients let the session close, including after a GUI crash.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use argus_core::net_process::NetworkTracker;
use serde::{Deserialize, Serialize};
use windows::Win32::System::Com::CoCreateGuid;

const IDLE: Duration = Duration::from_secs(15);
const POLICY: crate::snapshot::Policy = crate::snapshot::Policy {
    period: Duration::from_millis(250),
    max_age: Duration::from_secs(2),
    idle: IDLE,
};
const RETRY: Duration = Duration::from_secs(30);
const RETAIN: Duration = Duration::from_secs(30);
pub(crate) const MAX_COUNTERS: usize = 8192;
pub(crate) const SESSION: &str = if cfg!(debug_assertions) {
    "Argus Service Network (dev)"
} else {
    "Argus Service Network"
};

/// Tuples keep a bounded, large process set comfortably below the reply limit:
/// PID, counter generation, first observation (us), sent bytes, received bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkFrame {
    pub epoch: String,
    pub elapsed_us: u64,
    pub counters: Vec<(u32, u64, u64, u64, u64)>,
}

struct Counter {
    generation: u64,
    first_us: u64,
    last: Instant,
    sent: u64,
    received: u64,
}

struct Counters {
    epoch: String,
    start: Instant,
    next: u64,
    values: HashMap<u32, Counter>,
}

impl Counters {
    fn new() -> Option<Self> {
        Some(Self {
            epoch: format!("{:?}", unsafe { CoCreateGuid() }.ok()?),
            start: Instant::now(),
            next: 0,
            values: HashMap::new(),
        })
    }

    fn update(&mut self, deltas: HashMap<u32, (u64, u64)>, now: Instant) -> Option<NetworkFrame> {
        self.values
            .retain(|_, entry| now.duration_since(entry.last) <= RETAIN);
        let elapsed_us = now
            .duration_since(self.start)
            .as_micros()
            .min(u64::MAX as u128) as u64;
        for (pid, (sent, received)) in deltas {
            if !self.values.contains_key(&pid) && self.values.len() == MAX_COUNTERS {
                return None;
            }
            let entry = self.values.entry(pid).or_insert_with(|| {
                self.next += 1;
                Counter {
                    generation: self.next,
                    first_us: elapsed_us,
                    last: now,
                    sent: 0,
                    received: 0,
                }
            });
            entry.sent = entry.sent.checked_add(sent)?;
            entry.received = entry.received.checked_add(received)?;
            entry.last = now;
        }
        Some(NetworkFrame {
            epoch: self.epoch.clone(),
            elapsed_us,
            counters: self
                .values
                .iter()
                .map(|(&pid, c)| (pid, c.generation, c.first_us, c.sent, c.received))
                .collect(),
        })
    }
}

pub(crate) struct NetworkMonitor {
    reader: crate::snapshot::Reader<Option<NetworkFrame>>,
}

impl NetworkMonitor {
    pub fn spawn() -> Self {
        Self::with_factory(|| NetworkTracker::start(SESSION))
    }

    fn with_factory(factory: impl Fn() -> Option<NetworkTracker> + Send + 'static) -> Self {
        Self {
            reader: crate::snapshot::Reader::spawn("argus-service-network", POLICY, move || {
                NativeSource {
                    factory,
                    tracker: None,
                    counters: None,
                    last_attempt: None,
                }
            }),
        }
    }

    pub fn snapshot(&self) -> Option<NetworkFrame> {
        self.reader.snapshot().and_then(|frame| (*frame).clone())
    }

    #[cfg(test)]
    pub fn disabled() -> Self {
        Self {
            reader: crate::snapshot::Reader::disabled(POLICY),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_reader(read: impl FnMut() -> Option<NetworkFrame> + Send + 'static) -> Self {
        Self {
            reader: crate::snapshot::Reader::spawn(
                "network-test",
                crate::snapshot::Policy {
                    period: Duration::from_millis(10),
                    ..POLICY
                },
                move || crate::snapshot::tests::Callback(read),
            ),
        }
    }
}

impl Drop for NetworkMonitor {
    fn drop(&mut self) {
        self.reader.stop_and_join();
    }
}

struct NativeSource<F> {
    factory: F,
    tracker: Option<NetworkTracker>,
    counters: Option<Counters>,
    last_attempt: Option<Instant>,
}

impl<F: Fn() -> Option<NetworkTracker> + 'static> crate::snapshot::Source for NativeSource<F> {
    type Value = Option<NetworkFrame>;
    fn read(&mut self) -> Self::Value {
        if self
            .tracker
            .as_ref()
            .is_some_and(|tracker| !tracker.is_active())
        {
            self.tracker = None;
            self.counters = None;
        }
        if self.tracker.is_none() && self.last_attempt.is_none_or(|last| last.elapsed() >= RETRY) {
            self.last_attempt = Some(Instant::now());
            self.counters = Counters::new();
            if self.counters.is_some() {
                self.tracker = (self.factory)();
            }
        }
        let frame = match (&mut self.tracker, &mut self.counters) {
            (Some(tracker), Some(counters)) => {
                counters.update(tracker.take_bytes(), Instant::now())
            }
            _ => None,
        };
        if frame.is_none() {
            self.tracker = None;
            self.counters = None;
        }
        frame
    }
    fn idle(&mut self) {
        self.tracker = None;
        self.counters = None;
        self.last_attempt = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_do_not_consume_each_others_bytes_and_expired_pids_get_new_generation() {
        let mut c = Counters::new().unwrap();
        let now = c.start + Duration::from_secs(1);
        let a = c.update(HashMap::from([(42, (100, 200))]), now).unwrap();
        let b = c
            .update(HashMap::new(), now + Duration::from_secs(1))
            .unwrap();
        assert_eq!(a.counters, b.counters);
        let d = c
            .update(
                HashMap::from([(42, (20, 40))]),
                now + RETAIN + Duration::from_secs(1),
            )
            .unwrap();
        assert_ne!(a.counters[0].1, d.counters[0].1);
        assert_eq!((d.counters[0].3, d.counters[0].4), (20, 40));
    }

    #[test]
    fn denied_trace_returns_unavailable_without_elevation() {
        assert!(NetworkMonitor::disabled().snapshot().is_none());
    }

    #[test]
    fn thirty_days_of_expiring_network_processes_do_not_accumulate_counters() {
        let mut counters = Counters::new().unwrap();
        for hour in 0..720u32 {
            let now = counters.start + Duration::from_secs(u64::from(hour) * 3600);
            let deltas = (0..128).map(|i| (hour * 128 + i, (100, 200))).collect();
            let frame = counters.update(deltas, now).unwrap();
            assert_eq!(frame.counters.len(), 128);
            let expired = counters
                .update(HashMap::new(), now + RETAIN + Duration::from_secs(1))
                .unwrap();
            assert!(expired.counters.is_empty());
            assert!(counters.values.capacity() <= 512);
        }
    }
}
