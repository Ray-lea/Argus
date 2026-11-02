//! Sub-second delivery while a reader is sampling quickly. Native flushes run
//! off the sampler thread; slower readers leave the ordinary ETW timer in use.
use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, ControlTraceW, EVENT_TRACE_CONTROL_FLUSH,
};
use windows::core::PCWSTR;

use super::Properties;

const PERIOD: Duration = Duration::from_millis(250);
const FAST_READ: Duration = Duration::from_millis(750);
const LEASE: Duration = Duration::from_millis(1500);

struct State {
    stopped: bool,
    last_read: Option<Instant>,
    until: Option<Instant>,
}

impl State {
    fn new(now: Instant) -> Self {
        Self {
            stopped: false,
            last_read: None,
            until: Some(now + LEASE),
        }
    }

    fn fast(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| until > now)
    }

    fn observe(&mut self, now: Instant) -> bool {
        let was_fast = self.fast(now);
        if self
            .last_read
            .replace(now)
            .is_some_and(|last| now.saturating_duration_since(last) < FAST_READ)
        {
            self.until = Some(now + LEASE);
        }
        !was_fast && self.fast(now)
    }
}

pub(super) struct Flush {
    shared: Arc<(Mutex<State>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Flush {
    pub fn new(session: u64) -> io::Result<Self> {
        Self::with_callback(move || {
            let mut properties = Properties::new("");
            let result = unsafe {
                ControlTraceW(
                    CONTROLTRACE_HANDLE { Value: session },
                    PCWSTR::null(),
                    properties.as_mut(),
                    EVENT_TRACE_CONTROL_FLUSH,
                )
            };
            if result != ERROR_SUCCESS {
                crate::diagnostics::warn("network ETW flush", crate::diagnostics::code(result.0));
            }
        })
    }

    fn with_callback(mut flush: impl FnMut() + Send + 'static) -> io::Result<Self> {
        let shared = Arc::new((Mutex::new(State::new(Instant::now())), Condvar::new()));
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("argus-etw-flush".into())
            .spawn(move || {
                let (lock, wake) = &*worker;
                let mut state = lock.lock().unwrap();
                let mut next = Instant::now();
                while !state.stopped {
                    let now = Instant::now();
                    if !state.fast(now) {
                        state.until = None;
                        state = wake.wait(state).unwrap();
                        next = Instant::now();
                        continue;
                    }
                    if now < next {
                        let wait = next
                            .min(state.until.unwrap())
                            .saturating_duration_since(now);
                        state = wake.wait_timeout(state, wait).unwrap().0;
                        continue;
                    }
                    drop(state);
                    flush();
                    next = Instant::now() + PERIOD;
                    state = lock.lock().unwrap();
                }
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn observe_read(&self) {
        let (lock, wake) = &*self.shared;
        if lock.lock().unwrap().observe(Instant::now()) {
            wake.notify_one();
        }
    }

    pub fn shutdown(&mut self) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap().stopped = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Flush {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    #[test]
    fn fast_delivery_expires_when_readers_slow_down_or_stop() {
        let at = Instant::now();
        let mut state = State::new(at);
        state.observe(at);
        state.observe(at + Duration::from_secs(1));
        assert!(!state.fast(at + Duration::from_secs(2)));
        assert!(state.observe(at + Duration::from_millis(1500)));
        assert!(state.fast(at + Duration::from_millis(2500)));
        state.observe(at + Duration::from_millis(2000));
        assert!(state.fast(at + Duration::from_millis(3000)));
        assert!(!state.fast(at + Duration::from_secs(4)));
        state.observe(at + Duration::from_secs(5));
        assert!(!state.fast(at + Duration::from_secs(5)));
    }

    #[test]
    fn a_native_flush_does_not_block_reads_and_finishes_before_shutdown_returns() {
        let (entered, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let flusher = Flush::with_callback(move || {
            count.fetch_add(1, Ordering::SeqCst);
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
        })
        .unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let state = flusher.shared.clone();
        let (read, observed) = mpsc::channel();
        let (stopped, finished) = mpsc::channel();
        let owner = std::thread::spawn(move || {
            flusher.observe_read();
            read.send(()).unwrap();
            drop(flusher);
            stopped.send(()).unwrap();
        });
        observed.recv_timeout(Duration::from_secs(1)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !state.0.lock().unwrap().stopped && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(state.0.lock().unwrap().stopped);
        assert!(finished.try_recv().is_err());
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        owner.join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
