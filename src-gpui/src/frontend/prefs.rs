//! Native preferences keep stable preference keys and preserve unrecognized data.
//!
//! Values change in memory at once; a background thread writes them, so a
//! preference set from the interface (the page, a sort, a pane width) never
//! waits for the disk on the UI thread. Writes keep their order, and the queue
//! is drained before the application quits (`flush`). Each write's outcome
//! goes back to the interface (`outcomes`), which says when saving starts to
//! fail.
use serde_json::Value;
use smol::channel::{Receiver, Sender};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{OnceLock, mpsc},
    time::Duration,
};
pub struct Prefs {
    path: PathBuf,
    values: BTreeMap<String, Value>,
    report: Sender<bool>,
    outcomes: Option<Receiver<bool>>,
    failing: bool,
}
impl Prefs {
    pub fn load(path: PathBuf) -> Self {
        // The values of the shared preferences file (`argus_app::preferences`).
        let values = argus_app::preferences::load(&path).unwrap_or_default();
        let (report, outcomes) = smol::channel::unbounded();
        Self {
            path,
            values,
            report,
            outcomes: Some(outcomes),
            failing: false,
        }
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
        let _ = writer().send(Job::Set(
            self.path.clone(),
            key.into(),
            value,
            self.report.clone(),
        ));
    }
    /// The writer's outcomes, for the interface to take once: true for each
    /// write that reached the disk, false for each that failed.
    pub fn outcomes(&mut self) -> Option<Receiver<bool>> {
        self.outcomes.take()
    }
    /// Notes a write's outcome. True when saving has just started to fail: the
    /// notice shows once, and again only after a write has succeeded meanwhile.
    pub fn record(&mut self, saved: bool) -> bool {
        let started = !saved && !self.failing;
        self.failing = !saved;
        started
    }
}

enum Job {
    Set(PathBuf, String, Value, Sender<bool>),
    Flush(mpsc::SyncSender<()>),
}

/// A file's pending changes and who to tell how writing them went.
struct Batch {
    path: PathBuf,
    entries: Vec<(String, Value)>,
    reports: Vec<Sender<bool>>,
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
                    let mut files: Vec<Batch> = Vec::new();
                    let mut flushed = Vec::new();
                    for job in std::iter::once(job).chain(receiver.try_iter()) {
                        match job {
                            Job::Set(path, key, value, report) => {
                                let index = match files.iter().position(|file| file.path == path) {
                                    Some(index) => index,
                                    None => {
                                        files.push(Batch {
                                            path,
                                            entries: Vec::new(),
                                            reports: Vec::new(),
                                        });
                                        files.len() - 1
                                    }
                                };
                                let file = &mut files[index];
                                file.entries.push((key, value));
                                if !file.reports.iter().any(|r| r.same_channel(&report)) {
                                    file.reports.push(report);
                                }
                            }
                            Job::Flush(done) => flushed.push(done),
                        }
                    }
                    for file in files {
                        let result = argus_app::preferences::set_many(&file.path, file.entries);
                        if let Err(error) = &result {
                            log::warn!(
                                "native preferences write failed: {}",
                                argus_core::diagnostics::failure(&error.to_string())
                            );
                        }
                        for report in file.reports {
                            let _ = report.try_send(result.is_ok());
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

/// Waits (at most `timeout`) until every preference set so far is on disk.
pub fn flush(timeout: Duration) {
    let (done, wait) = mpsc::sync_channel(1);
    if writer().send(Job::Flush(done)).is_ok() {
        let _ = wait.recv_timeout(timeout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "argus-native-prefs-{}-{name}.json",
            std::process::id()
        ))
    }

    #[test]
    fn the_notice_shows_once_per_run_of_failures() {
        let mut prefs = Prefs::load(path("record"));
        assert!(prefs.record(false));
        assert!(!prefs.record(false));
        assert!(!prefs.record(true));
        assert!(!prefs.record(true));
        assert!(prefs.record(false));
    }

    #[test]
    fn write_outcomes_reach_the_interface() {
        let file = path("outcomes");
        // Malformed data is never overwritten, so the write fails.
        std::fs::write(&file, b"{broken").unwrap();
        let mut prefs = Prefs::load(file.clone());
        let outcomes = prefs.outcomes().expect("outcomes");
        assert!(prefs.outcomes().is_none());
        prefs.set(
            "tableSort.services",
            serde_json::json!({"id":"name","desc":true}),
        );
        assert_eq!(outcomes.recv_blocking(), Ok(false));
        assert_eq!(std::fs::read(&file).unwrap(), b"{broken");
        std::fs::remove_file(&file).unwrap();
        prefs.set("services.filter", "running");
        assert_eq!(outcomes.recv_blocking(), Ok(true));
        let saved = argus_app::preferences::load(&file).unwrap();
        assert_eq!(saved["services.filter"], "running");
        std::fs::remove_file(&file).unwrap();
    }
}
