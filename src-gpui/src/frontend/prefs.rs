//! Native preferences keep the original key names and preserve unrecognized data.
//!
//! Values change in memory at once; a background thread writes them, so a
//! preference set from the interface (the page, a sort, a pane width) never
//! waits for the disk on the UI thread. Writes keep their order, and the queue
//! is drained before the application quits (`flush`).
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
pub struct Prefs {
    path: PathBuf,
    values: BTreeMap<String, Value>,
}
impl Prefs {
    pub fn load(path: PathBuf) -> Self {
        // The values of the shared preferences file (`argus_app::preferences`).
        let values = argus_app::preferences::load(&path).unwrap_or_default();
        Self { path, values }
    }
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str, fallback: T) -> T {
        self.values
            .get(key)
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or(fallback)
    }
    pub fn set(&mut self, key: &str, value: impl serde::Serialize) {
        let Ok(value) = serde_json::to_value(value) else {
            return;
        };
        if !argus_app::preferences::valid_key(key) {
            log::warn!("native preference key rejected");
            return;
        }
        self.values.insert(key.into(), value.clone());
        let _ = writer().send(Job::Set(self.path.clone(), key.into(), value));
    }
}

enum Job {
    Set(PathBuf, String, Value),
    Flush(mpsc::SyncSender<()>),
}

/// The preference writer: each wake-up takes every queued change and writes
/// each file once.
fn writer() -> &'static mpsc::Sender<Job> {
    static WRITER: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
    WRITER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("argus-preferences".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    let mut files: Vec<(PathBuf, Vec<(String, Value)>)> = Vec::new();
                    let mut flushed = Vec::new();
                    for job in std::iter::once(job).chain(receiver.try_iter()) {
                        match job {
                            Job::Set(path, key, value) => {
                                match files.iter_mut().find(|(file, _)| *file == path) {
                                    Some((_, entries)) => entries.push((key, value)),
                                    None => files.push((path, vec![(key, value)])),
                                }
                            }
                            Job::Flush(done) => flushed.push(done),
                        }
                    }
                    for (path, entries) in files {
                        let result = argus_app::preferences::set_many(&path, entries);
                        SAVE_FAILED.store(result.is_err(), Ordering::Relaxed);
                        if let Err(error) = result {
                            log::warn!(
                                "native preferences write failed: {}",
                                argus_core::diagnostics::failure(&error.to_string())
                            );
                        }
                    }
                    for done in flushed {
                        let _ = done.send(());
                    }
                }
            })
            .expect("preference writer thread");
        sender
    })
}

static SAVE_FAILED: AtomicBool = AtomicBool::new(false);

/// Whether the latest preference write failed; a later successful one clears it.
pub fn save_failed() -> bool {
    SAVE_FAILED.load(Ordering::Relaxed)
}

/// Waits (at most `timeout`) until every preference set so far is on disk.
pub fn flush(timeout: Duration) {
    let (done, wait) = mpsc::sync_channel(1);
    if writer().send(Job::Flush(done)).is_ok() {
        let _ = wait.recv_timeout(timeout);
    }
}
