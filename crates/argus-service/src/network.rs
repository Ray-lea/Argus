//! One leased ETW reader; cumulative snapshots never consume another client's
//! traffic. Idle clients let the session close, including after a GUI crash.

use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use argus_core::net_process::NetworkTracker;
use serde::{Deserialize, Serialize};
use windows::Win32::System::Com::CoCreateGuid;

const IDLE: Duration = Duration::from_secs(15);
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

enum Control {
    Read(mpsc::Sender<Option<NetworkFrame>>),
    Stop,
}

pub(crate) struct NetworkMonitor {
    tx: SyncSender<Control>,
    thread: Option<JoinHandle<()>>,
}

impl NetworkMonitor {
    pub fn spawn() -> Self {
        Self::with_factory(|| NetworkTracker::start(SESSION))
    }

    fn with_factory(factory: impl Fn() -> Option<NetworkTracker> + Send + 'static) -> Self {
        let (tx, rx) = mpsc::sync_channel(8);
        let thread = std::thread::Builder::new()
            .name("argus-service-network".into())
            .spawn(move || {
                let mut tracker: Option<NetworkTracker> = None;
                let mut counters: Option<Counters> = None;
                let mut last_request: Option<Instant> = None;
                let mut last_attempt: Option<Instant> = None;
                loop {
                    let control = match last_request {
                        Some(last) => rx.recv_timeout(IDLE.saturating_sub(last.elapsed())),
                        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    };
                    match control {
                        Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {
                            tracker = None;
                            counters = None;
                            last_request = None;
                            last_attempt = None;
                        }
                        Ok(Control::Read(reply)) => {
                            last_request = Some(Instant::now());
                            if tracker.as_ref().is_some_and(|t| !t.is_active()) {
                                tracker = None;
                                counters = None;
                            }
                            if tracker.is_none()
                                && last_attempt.is_none_or(|last| last.elapsed() >= RETRY)
                            {
                                last_attempt = Some(Instant::now());
                                counters = Counters::new();
                                if counters.is_some() {
                                    tracker = factory();
                                }
                            }
                            let frame = match (&mut tracker, &mut counters) {
                                (Some(t), Some(c)) => c.update(t.take_bytes(), Instant::now()),
                                _ => None,
                            };
                            if frame.is_none() {
                                tracker = None;
                                counters = None;
                            }
                            let _ = reply.send(frame);
                        }
                    }
                }
                // Tracker drop stops and joins the ETW consumer before SCM STOPPED.
            })
            .expect("spawn service network reader");
        Self {
            tx,
            thread: Some(thread),
        }
    }

    pub fn snapshot(&self) -> Option<NetworkFrame> {
        let (tx, rx) = mpsc::channel();
        self.tx.try_send(Control::Read(tx)).ok()?;
        rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
    }

    #[cfg(test)]
    pub fn disabled() -> Self {
        Self::with_factory(|| None)
    }
}

impl Drop for NetworkMonitor {
    fn drop(&mut self) {
        let _ = self.tx.send(Control::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
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
}
