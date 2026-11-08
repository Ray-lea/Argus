//! Coalesced snapshot reads. Warm clients use the cache; each source permits
//! only one bounded cold wait, leaving the other IPC workers available.
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(crate) struct Policy {
    pub period: Duration,
    pub max_age: Duration,
    pub idle: Duration,
}

pub(crate) trait Source: 'static {
    type Value: Send + Sync + 'static;
    fn read(&mut self) -> Self::Value;
    fn idle(&mut self);
}

struct State<T> {
    frame: Option<(Instant, Arc<T>)>,
    last_request: Option<Instant>,
    stop: bool,
    requested: bool,
    waiting: bool,
}

impl<T> State<T> {
    fn cached(&self, now: Instant, max_age: Duration) -> Option<Arc<T>> {
        self.frame
            .as_ref()
            .filter(|(at, _)| now.saturating_duration_since(*at) <= max_age)
            .map(|(_, frame)| frame.clone())
    }
}

pub(crate) struct Reader<T> {
    shared: Arc<(Mutex<State<T>>, Condvar)>,
    policy: Policy,
    thread: Option<JoinHandle<()>>,
}

impl<T: Send + Sync + 'static> Reader<T> {
    pub fn spawn<S: Source<Value = T>>(
        name: &'static str,
        policy: Policy,
        make: impl FnOnce() -> S + Send + 'static,
    ) -> Self {
        let shared = Arc::new((
            Mutex::new(State {
                frame: None,
                last_request: None,
                stop: false,
                requested: false,
                waiting: false,
            }),
            Condvar::new(),
        ));
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let mut source = make();
                let mut next = Instant::now();
                loop {
                    let mut state = worker.0.lock().unwrap();
                    if state.stop {
                        break;
                    }
                    let Some(last) = state.last_request else {
                        drop(worker.1.wait(state).unwrap());
                        next = Instant::now();
                        continue;
                    };
                    let now = Instant::now();
                    let idle_at = last + policy.idle;
                    if now >= idle_at {
                        state.frame = None;
                        state.last_request = None;
                        state.requested = false;
                        drop(state);
                        // Closing a native reader may wait for its own I/O. The
                        // cache lock stays free so clients can still get Pending.
                        source.idle();
                        next = Instant::now();
                        continue;
                    }
                    if !state.requested || now < next {
                        let wake = if state.requested {
                            next.min(idle_at)
                        } else {
                            idle_at
                        };
                        drop(
                            worker
                                .1
                                .wait_timeout(state, wake.saturating_duration_since(now))
                                .unwrap(),
                        );
                        continue;
                    }
                    drop(state);
                    let frame = source.read();
                    let completed = Instant::now();
                    let mut state = worker.0.lock().unwrap();
                    if state.stop {
                        break;
                    }
                    state.frame = Some((completed, Arc::new(frame)));
                    state.requested = false;
                    next = completed + policy.period;
                    worker.1.notify_all();
                }
            })
            .inspect_err(|error| {
                argus_core::diagnostics::warn(
                    "snapshot worker creation",
                    format!("worker={name}; {}", argus_core::diagnostics::io(error)),
                );
            })
            .ok();
        Self {
            shared,
            policy,
            thread,
        }
    }

    /// Warm requests return immediately. One cold requester may await the first
    /// sample; additional requesters return Pending and keep IPC slots available.
    pub fn snapshot(&self) -> Option<Arc<T>> {
        self.request(true)
    }

    /// A supplementary source must never delay an independently fresh reading.
    pub fn snapshot_without_wait(&self) -> Option<Arc<T>> {
        self.request(false)
    }

    fn request(&self, wait: bool) -> Option<Arc<T>> {
        let now = Instant::now();
        let mut state = self.shared.0.lock().unwrap();
        if state.stop
            || self
                .thread
                .as_ref()
                .is_none_or(|thread| thread.is_finished())
        {
            return None;
        }
        state.last_request = Some(now);
        let frame = state.cached(now, self.policy.max_age);
        if state
            .frame
            .as_ref()
            .is_none_or(|(at, _)| now.saturating_duration_since(*at) >= self.policy.period)
            && !state.requested
        {
            state.requested = true;
            self.shared.1.notify_all();
        }
        if frame.is_some() || state.waiting || !wait {
            return frame;
        }
        // Below the client's two-second deadline, including connect and framing.
        // A cold read after a long tray interval still gets a useful first frame.
        state.waiting = true;
        let deadline = now + Duration::from_millis(1500);
        let frame = loop {
            let now = Instant::now();
            if state.stop {
                break None;
            }
            if let Some(frame) = state.cached(now, self.policy.max_age) {
                break Some(frame);
            }
            if now >= deadline {
                break None;
            }
            state = self
                .shared
                .1
                .wait_timeout(state, deadline.saturating_duration_since(now))
                .unwrap()
                .0;
        };
        state.waiting = false;
        frame
    }

    /// ETW owners must stop and join their reader before reporting SCM STOPPED.
    pub fn stop_and_join(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    #[cfg(test)]
    pub fn disabled(policy: Policy) -> Self {
        Self {
            shared: Arc::new((
                Mutex::new(State {
                    frame: None,
                    last_request: None,
                    stop: true,
                    requested: false,
                    waiting: false,
                }),
                Condvar::new(),
            )),
            policy,
            thread: None,
        }
    }
}

impl<T> Reader<T> {
    fn stop(&self) {
        self.shared.0.lock().unwrap().stop = true;
        self.shared.1.notify_all();
    }
}

impl<T> Drop for Reader<T> {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{Policy, Reader, Source, State};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };

    pub(crate) struct Callback<T, F: FnMut() -> T + 'static>(pub F);
    impl<T: Send + Sync + 'static, F: FnMut() -> T + 'static> Source for Callback<T, F> {
        type Value = T;
        fn read(&mut self) -> T {
            (self.0)()
        }
        fn idle(&mut self) {}
    }

    pub(crate) const POLICY: Policy = Policy {
        period: Duration::from_millis(10),
        max_age: Duration::from_secs(3),
        idle: Duration::from_secs(15),
    };

    #[test]
    fn slow_refresh_is_coalesced_and_does_not_block_cached_reads() {
        let reads = Arc::new(AtomicUsize::new(0));
        let count = reads.clone();
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut reader = Reader::spawn("snapshot-test", POLICY, move || {
            Callback(move || {
                if count.fetch_add(1, Ordering::SeqCst) == 1 {
                    entered.send(()).unwrap();
                    gate.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                62u32
            })
        });
        assert_eq!(reader.snapshot().as_deref(), Some(&62));
        std::thread::sleep(POLICY.period * 2);
        assert_eq!(reader.snapshot().as_deref(), Some(&62));
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let begin = Instant::now();
        for _ in 0..10_000 {
            assert_eq!(reader.snapshot().as_deref(), Some(&62));
        }
        let elapsed = begin.elapsed();
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        release.send(()).unwrap();
        reader.stop_and_join();
        assert!(elapsed < Duration::from_millis(250));
    }

    #[test]
    fn requests_do_not_make_an_expired_frame_fresh() {
        let at = Instant::now();
        let state = State {
            frame: Some((at, Arc::new(5))),
            last_request: Some(at + POLICY.max_age),
            stop: false,
            requested: false,
            waiting: false,
        };
        assert_eq!(
            state.cached(at + POLICY.max_age, POLICY.max_age).as_deref(),
            Some(&5)
        );
        assert!(
            state
                .cached(
                    at + POLICY.max_age + Duration::from_millis(1),
                    POLICY.max_age
                )
                .is_none()
        );
    }

    #[test]
    fn a_cold_sample_has_one_waiter_and_leaves_other_callers_available() {
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut reader = Reader::spawn("snapshot-cold-test", POLICY, move || {
            Callback(move || {
                entered.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                7
            })
        });
        std::thread::scope(|scope| {
            let first = scope.spawn(|| reader.snapshot());
            started.recv_timeout(Duration::from_secs(2)).unwrap();
            let start = Instant::now();
            for _ in 0..1000 {
                assert!(reader.snapshot().is_none());
            }
            assert!(start.elapsed() < Duration::from_millis(100));
            release.send(()).unwrap();
            assert_eq!(first.join().unwrap().as_deref(), Some(&7));
        });
        reader.stop_and_join();
    }

    #[test]
    fn the_lease_does_not_sample_when_clients_stop_requesting() {
        let reads = Arc::new(AtomicUsize::new(0));
        let count = reads.clone();
        let mut reader = Reader::spawn("snapshot-cadence-test", POLICY, move || {
            Callback(move || count.fetch_add(1, Ordering::Relaxed))
        });
        assert!(reader.snapshot().is_some());
        std::thread::sleep(POLICY.period * 4);
        assert_eq!(reads.load(Ordering::Relaxed), 1);
        reader.stop_and_join();
    }

    #[test]
    fn idle_releases_the_source_and_a_new_reader_starts_it_again() {
        struct IdleSource(mpsc::Sender<&'static str>);
        impl Source for IdleSource {
            type Value = u32;
            fn read(&mut self) -> u32 {
                self.0.send("read").unwrap();
                1
            }
            fn idle(&mut self) {
                self.0.send("idle").unwrap();
            }
        }
        let (tx, rx) = mpsc::channel();
        let mut reader = Reader::spawn(
            "snapshot-idle-test",
            Policy {
                period: Duration::from_secs(1),
                idle: Duration::from_millis(20),
                ..POLICY
            },
            move || IdleSource(tx),
        );
        assert_eq!(reader.snapshot().as_deref(), Some(&1));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "read");
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "idle");
        assert_eq!(reader.snapshot().as_deref(), Some(&1));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "read");
        reader.stop_and_join();
    }
}
