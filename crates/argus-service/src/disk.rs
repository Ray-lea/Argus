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
}
