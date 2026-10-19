//! Disk snapshots run on a worker; a slow or unavailable broker cannot stall
//! process sampling. Local elevated hosts use the same read-only query.
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use argus_core::Snapshot;
use argus_core::disk_process::{DiskFailure, DiskQuery, DiskRates, DiskSnapshot};

const TIMEOUT: Duration = Duration::from_secs(2);
/// At most this many distinct failures are handed to the host's log per run.
const MAX_REPORTED: usize = 8;

pub(crate) struct DiskMonitor {
    query: Mutex<Option<DiskQuery>>,
}

impl DiskMonitor {
    pub fn new() -> Self {
        Self {
            query: Mutex::new(Some(DiskQuery::default())),
        }
    }

    /// The counters, or why the query failed. The service keeps no log: the
    /// failure goes back in the reply, and the host logs it
    /// (`DiskClient::new_failure`). None without a query.
    pub fn snapshot(&self) -> Option<Result<DiskSnapshot, DiskFailure>> {
        Some(self.query.lock().ok()?.as_mut()?.read())
    }

    #[cfg(test)]
    pub fn disabled() -> Self {
        Self {
            query: Mutex::new(None),
        }
    }
}

/// Why the host got no disk snapshot, in fixed words and codes fit for its
/// log: nothing about a process, a path or the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskUnavailable {
    /// Elevated, the query in this process failed.
    Local(DiskFailure),
    /// The privileged service's query failed.
    Service(DiskFailure),
    /// The privileged service answered without a snapshot or a failure: it
    /// has no query to run.
    ServiceEmpty,
    /// The privileged service answered something else (a refusal, say).
    ServiceReply,
    /// The request to the privileged service failed (Windows error code).
    Request(u32),
}

impl std::fmt::Display for DiskUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(failure) => write!(f, "local query: {failure}"),
            Self::Service(failure) => write!(f, "privileged service query: {failure}"),
            Self::ServiceEmpty => f.write_str("the privileged service returned no snapshot"),
            Self::ServiceReply => f.write_str("unexpected reply from the privileged service"),
            Self::Request(code) => {
                write!(
                    f,
                    "request to the privileged service failed (os error {code})"
                )
            }
        }
    }
}

/// What a read gives: a snapshot, or why not. `Err(None)`: no source at all
/// (standard user, no privileged service running), which is no failure.
type Read = Result<DiskSnapshot, Option<DiskUnavailable>>;

/// A read through the privileged service.
fn from_service(reply: crate::util::Result<crate::Reply>) -> Read {
    match reply {
        Ok(crate::Reply::Disk(Some(frame))) => Ok(frame),
        Ok(crate::Reply::DiskFailed(failure)) => Err(Some(DiskUnavailable::Service(failure))),
        Ok(crate::Reply::Disk(None)) => Err(Some(DiskUnavailable::ServiceEmpty)),
        Ok(_) => Err(Some(DiskUnavailable::ServiceReply)),
        Err(code) => Err(Some(DiskUnavailable::Request(code))),
    }
}

#[derive(Clone, Default)]
struct Cache {
    frame: Option<Arc<DiskSnapshot>>,
    failure: Option<DiskUnavailable>,
    received: Option<Instant>,
}

pub struct DiskClient {
    tx: Option<mpsc::SyncSender<()>>,
    cache: Arc<Mutex<Cache>>,
    last: Cache,
    thread: Option<JoinHandle<()>>,
    rates: DiskRates,
    /// Failures already handed out by `new_failure`.
    reported: Vec<DiskUnavailable>,
}

impl DiskClient {
    pub fn spawn() -> Self {
        let mut local = argus_core::privilege::is_elevated().then(DiskQuery::default);
        Self::with_reader(move || {
            if let Some(query) = &mut local {
                return query.read().map_err(|e| Some(DiskUnavailable::Local(e)));
            }
            let Ok(Some(pid)) = crate::install::running_pid() else {
                return Err(None);
            };
            from_service(crate::pipe::request_with_timeout(
                crate::PIPE_NAME,
                pid,
                crate::Operation::DiskSnapshot {},
                TIMEOUT,
            ))
        })
    }

    fn with_reader(mut read: impl FnMut() -> Read + Send + 'static) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let cache: Arc<Mutex<Cache>> = Arc::default();
        let shared = cache.clone();
        let thread = std::thread::Builder::new()
            .name("argus-disk-client".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    let (frame, failure) = match read() {
                        Ok(frame) => (Some(Arc::new(frame)), None),
                        Err(failure) => (None, failure),
                    };
                    *shared.lock().unwrap() = Cache {
                        frame,
                        failure,
                        received: Some(Instant::now()),
                    };
                }
            })
            .expect("spawn disk client");
        Self {
            tx: Some(tx),
            cache,
            last: Cache::default(),
            thread: Some(thread),
            rates: DiskRates::default(),
            reported: Vec::new(),
        }
    }

    /// Why the latest read gave no snapshot, the first time each reason
    /// comes up (and only a few of them per run), for the host to log once:
    /// otherwise the disk columns would just stay empty without a trace.
    pub fn new_failure(&mut self) -> Option<DiskUnavailable> {
        let failure = self.last.failure?;
        if self.reported.len() >= MAX_REPORTED || self.reported.contains(&failure) {
            return None;
        }
        self.reported.push(failure);
        Some(failure)
    }

    pub fn enrich(&mut self, snapshot: &mut Snapshot, interval: Duration) {
        if snapshot.processes.iter().any(|p| p.disk_read.is_some()) {
            self.rates = DiskRates::default();
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(());
        }
        if let Ok(cache) = self.cache.try_lock() {
            self.last = cache.clone();
        }
        let frame = self
            .last
            .received
            .filter(|at| at.elapsed() <= interval * 2 + TIMEOUT)
            .and(self.last.frame.as_deref());
        if let Some(frame) = frame {
            self.rates.apply(frame, &mut snapshot.processes);
        } else {
            self.rates = DiskRates::default();
            for process in &mut snapshot.processes {
                process.disk_read = None;
                process.disk_write = None;
            }
        }
    }
}

impl Drop for DiskClient {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_source_is_unavailable_and_worker_never_blocks_sampler() {
        assert!(DiskMonitor::disabled().snapshot().is_none());
        let (entered, wait_entered) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let mut first = true;
        let mut client = DiskClient::with_reader(move || {
            if first {
                first = false;
                entered.send(()).unwrap();
                wait_release.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            Err(None)
        });
        let mut snapshot = Snapshot {
            seq: 0,
            timestamp: 0,
            system: Default::default(),
            hardware: Default::default(),
            processes: Vec::new(),
            process_network: false,
        };
        client.enrich(&mut snapshot, Duration::from_secs(1));
        wait_entered.recv_timeout(Duration::from_secs(2)).unwrap();
        // This must return while the worker is still waiting for release.
        client.enrich(&mut snapshot, Duration::from_secs(1));
        release.send(()).unwrap();
    }

    fn process(pid: u32, disk: Option<u64>) -> argus_core::ProcessRow {
        argus_core::ProcessRow {
            pid,
            ppid: 0,
            create_time: 10,
            name: "example.exe".into(),
            path: None,
            description: None,
            company: None,
            icon: None,
            web_view: None,
            cpu: 0.0,
            cpu_time: 0,
            private_ws: 0,
            working_set: 0,
            commit: 0,
            io_read: 0,
            io_write: 0,
            io_other: 0,
            disk_read: disk,
            disk_write: disk,
            gpu: 0.0,
            gpu_engine: None,
            gpu_dedicated: 0,
            gpu_shared: 0,
            net_sent: 0,
            net_received: 0,
            threads: 1,
            handles: 1,
            session_id: 1,
            base_priority: 8,
            suspended: false,
            efficiency: false,
            app_window: false,
        }
    }

    fn snapshot(processes: Vec<argus_core::ProcessRow>) -> Snapshot {
        Snapshot {
            seq: 0,
            timestamp: 0,
            system: Default::default(),
            hardware: Default::default(),
            processes,
            process_network: false,
        }
    }

    /// A client whose reader blocks until `gate` is dropped, so the cache
    /// holds exactly what a test puts there.
    fn gated() -> (DiskClient, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (called, calls) = mpsc::channel();
        let (gate, wait) = mpsc::channel::<()>();
        let client = DiskClient::with_reader(move || {
            let _ = called.send(());
            let _ = wait.recv_timeout(Duration::from_secs(5));
            Err(None)
        });
        (client, calls, gate)
    }

    fn cache(client: &DiskClient, sampled_at_ms: u64, bytes: u64, age: Duration) {
        *client.cache.lock().unwrap() = Cache {
            frame: Some(Arc::new(DiskSnapshot {
                sampled_at_ms,
                counters: vec![(17, 10, bytes, bytes)],
            })),
            failure: None,
            received: Instant::now().checked_sub(age),
        };
    }

    fn fail(client: &DiskClient, failure: DiskUnavailable) {
        *client.cache.lock().unwrap() = Cache {
            frame: None,
            failure: Some(failure),
            received: Some(Instant::now()),
        };
    }

    #[test]
    fn each_failure_is_reported_once_with_no_personal_data() {
        let (mut client, _calls, gate) = gated();
        let sample = |client: &mut DiskClient| {
            let mut rows = snapshot(vec![process(17, None)]);
            client.enrich(&mut rows, Duration::from_secs(1));
            assert_eq!(rows.processes[0].disk_read, None);
            client.new_failure()
        };
        // No source at all (no service, not elevated) is no failure.
        assert_eq!(sample(&mut client), None);
        fail(&client, DiskUnavailable::ServiceEmpty);
        assert_eq!(sample(&mut client), Some(DiskUnavailable::ServiceEmpty));
        assert_eq!(sample(&mut client), None);
        let local = DiskUnavailable::Local(DiskFailure::Query(0xC000_0022_u32 as i32));
        fail(&client, local);
        assert_eq!(sample(&mut client), Some(local));
        fail(&client, DiskUnavailable::ServiceEmpty);
        assert_eq!(sample(&mut client), None);
        assert_eq!(
            local.to_string(),
            "local query: query failed (NTSTATUS 0xC0000022)"
        );
        // The service's own failure, as its reply carries it.
        let service = DiskUnavailable::Service(DiskFailure::Truncated);
        fail(&client, service);
        assert_eq!(sample(&mut client), Some(service));
        assert_eq!(
            service.to_string(),
            "privileged service query: truncated record"
        );
        // A service that keeps failing in new ways fills only a few lines.
        for code in 0..20 {
            fail(&client, DiskUnavailable::Request(code));
            sample(&mut client);
        }
        assert_eq!(client.reported.len(), MAX_REPORTED);
        drop(gate);
    }

    #[test]
    fn service_replies_map_to_snapshots_or_reasons() {
        let failure = DiskFailure::Query(0xC000_0022_u32 as i32);
        let reply = crate::protocol::execute(
            crate::Operation::DiskSnapshot {},
            &crate::protocol::Context::testing(),
            1,
        );
        assert_eq!(
            from_service(Ok(reply)).unwrap_err(),
            Some(DiskUnavailable::ServiceEmpty)
        );
        assert_eq!(
            from_service(Ok(crate::Reply::DiskFailed(failure))).unwrap_err(),
            Some(DiskUnavailable::Service(failure))
        );
        assert_eq!(
            from_service(Ok(crate::Reply::Done)).unwrap_err(),
            Some(DiskUnavailable::ServiceReply)
        );
        assert_eq!(
            from_service(Err(crate::util::INCOMPATIBLE)).unwrap_err(),
            Some(DiskUnavailable::Request(crate::util::INCOMPATIBLE))
        );
        let frame = DiskSnapshot {
            sampled_at_ms: 5,
            counters: vec![(17, 10, 1, 2)],
        };
        let Ok(back) = from_service(Ok(crate::Reply::Disk(Some(frame)))) else {
            panic!("a snapshot is a snapshot");
        };
        assert_eq!(back.counters, [(17, 10, 1, 2)]);
    }

    #[test]
    fn native_counters_skip_the_service() {
        let (mut client, calls, gate) = gated();
        let mut rows = snapshot(vec![process(17, Some(4096)), process(18, None)]);
        client.enrich(&mut rows, Duration::from_secs(1));
        assert_eq!(rows.processes[0].disk_read, Some(4096));
        assert_eq!(rows.processes[1].disk_read, None);
        assert!(calls.recv_timeout(Duration::from_millis(100)).is_err());
        drop(gate);
    }

    #[test]
    fn service_snapshots_become_rates_until_they_go_stale() {
        let (mut client, calls, gate) = gated();
        // Each sample is a fresh collector snapshot without native counters.
        let sample = |client: &mut DiskClient| {
            let mut rows = snapshot(vec![process(17, None)]);
            client.enrich(&mut rows, Duration::from_secs(1));
            (rows.processes[0].disk_read, rows.processes[0].disk_write)
        };
        // The first snapshot is only a baseline.
        cache(&client, 1000, 0, Duration::ZERO);
        assert_eq!(sample(&mut client), (None, None));
        calls.recv_timeout(Duration::from_secs(2)).unwrap();
        cache(&client, 2000, 4096, Duration::ZERO);
        assert_eq!(sample(&mut client), (Some(4096), Some(4096)));
        // Older than two intervals plus the request timeout: not reused.
        cache(&client, 3000, 8192, Duration::from_secs(5));
        assert_eq!(sample(&mut client), (None, None));
        drop(gate);
    }
}
