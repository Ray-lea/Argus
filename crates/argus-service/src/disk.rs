//! Disk snapshots run on a worker; a slow or unavailable broker cannot stall
//! process sampling. Local elevated hosts use the same read-only query.
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use argus_core::Snapshot;
use argus_core::disk_process::{DiskQuery, DiskRates, DiskSnapshot};

const TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct DiskMonitor {
    query: Mutex<Option<DiskQuery>>,
}

impl DiskMonitor {
    pub fn new() -> Self {
        Self {
            query: Mutex::new(Some(DiskQuery::default())),
        }
    }

    pub fn snapshot(&self) -> Option<DiskSnapshot> {
        self.query.lock().ok()?.as_mut()?.read()
    }

    #[cfg(test)]
    pub fn disabled() -> Self {
        Self {
            query: Mutex::new(None),
        }
    }
}

#[derive(Clone, Default)]
struct Cache {
    frame: Option<Arc<DiskSnapshot>>,
    received: Option<Instant>,
}

pub struct DiskClient {
    tx: Option<mpsc::SyncSender<()>>,
    cache: Arc<Mutex<Cache>>,
    last: Cache,
    thread: Option<JoinHandle<()>>,
    rates: DiskRates,
}

impl DiskClient {
    pub fn spawn() -> Self {
        let mut local = argus_core::privilege::is_elevated().then(DiskQuery::default);
        Self::with_reader(move || {
            if let Some(query) = &mut local {
                return query.read();
            }
            let pid = crate::install::running_pid().ok()??;
            match crate::pipe::request_with_timeout(
                crate::PIPE_NAME,
                pid,
                crate::Operation::DiskSnapshot {},
                TIMEOUT,
            )
            .ok()?
            {
                crate::Reply::Disk(frame) => frame,
                _ => None,
            }
        })
    }

    fn with_reader(mut read: impl FnMut() -> Option<DiskSnapshot> + Send + 'static) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let cache: Arc<Mutex<Cache>> = Arc::default();
        let shared = cache.clone();
        let thread = std::thread::Builder::new()
            .name("argus-disk-client".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    let frame = read().map(Arc::new);
                    *shared.lock().unwrap() = Cache {
                        frame,
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
        }
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
            None
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
            None
        });
        (client, calls, gate)
    }

    fn cache(client: &DiskClient, sampled_at_ms: u64, bytes: u64, age: Duration) {
        *client.cache.lock().unwrap() = Cache {
            frame: Some(Arc::new(DiskSnapshot {
                sampled_at_ms,
                counters: vec![(17, 10, bytes, bytes)],
            })),
            received: Instant::now().checked_sub(age),
        };
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
