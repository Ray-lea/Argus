//! Native preferences keep stable preference keys and preserve unrecognized data.
//!
//! Values change in memory at once; a background thread writes them, so a
//! preference set from the interface (the page, a sort, a pane width) never
//! waits for the disk on the UI thread. Writes keep their order, and the queue
//! is drained before the application quits (`flush`). Each write's outcome
//! goes back to the interface (`outcomes`), which says when saving starts to
//! fail.
//!
//! A preference is saved only when chosen, so one never chosen follows a
//! changed default with no migration. Saved values that have to change with
//! an update go through `MIGRATIONS`, once per profile.
use serde_json::Value;
use smol::channel::{Receiver, Sender};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{OnceLock, mpsc},
    time::Duration,
};

/// How many of `MIGRATIONS` have run on this profile.
const MIGRATED: &str = "migrations";

/// Steps that bring preferences an earlier version saved up to date, in
/// order; each runs once per profile, and the count that ran is saved
/// (`MIGRATED`), so a value chosen afterwards stays. They are for saved
/// values that must change as well: a choice that lost its meaning, or a
/// value an earlier version saved but never restored. Only append: a step's
/// place is its number.
const MIGRATIONS: [fn(&mut Prefs); 1] = [
    // Releases up to 0.5.0 saved the history range a click picked but always
    // opened the page at an hour. The page restores what is saved now, which
    // would open it at whatever was clicked last back then.
    |prefs| prefs.remove("historyRange"),
];

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
    /// Copy the old shared choice once. A stored null keeps following Windows
    /// and distinguishes a new profile from a pre-split preference file.
    pub fn initialize_window_motion(&mut self) {
        if !self.values.contains_key("motion.window") {
            self.set("motion.window", self.get::<Option<bool>>("motion.ui", None));
        }
    }
    /// Runs the steps of `MIGRATIONS` this profile has not run yet.
    pub fn migrate(&mut self) {
        let done = self.get::<usize>(MIGRATED, 0);
        if done >= MIGRATIONS.len() {
            return;
        }
        for step in &MIGRATIONS[done..] {
            step(self);
        }
        self.set(MIGRATED, MIGRATIONS.len());
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
        self.write(key, Some(value));
    }
    /// Forgets a preference: it reads as its default again.
    pub fn remove(&mut self, key: &str) {
        if self.values.remove(key).is_some() {
            self.write(key, None);
        }
    }
    fn write(&self, key: &str, value: Option<Value>) {
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
    /// A value to store, or none to remove the key.
    Set(PathBuf, String, Option<Value>, Sender<bool>),
    Flush(mpsc::SyncSender<()>),
}

/// A file's pending changes and who to tell how writing them went.
struct Batch {
    path: PathBuf,
    entries: Vec<(String, Option<Value>)>,
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
                        let result = argus_app::preferences::apply(&file.path, file.entries);
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

    #[test]
    fn window_motion_migrates_once_and_remains_independent_after_restart() {
        let file = path("window-motion");
        std::fs::write(
            &file,
            br#"{"values":{"motion.ui":false,"motion.chartMode":"balanced"}}"#,
        )
        .unwrap();
        let mut prefs = Prefs::load(file.clone());
        prefs.initialize_window_motion();
        assert_eq!(
            prefs.get::<Option<bool>>("motion.window", None),
            Some(false)
        );
        prefs.set("motion.ui", true);
        prefs.initialize_window_motion();
        assert_eq!(
            prefs.get::<Option<bool>>("motion.window", None),
            Some(false)
        );
        flush(Duration::from_secs(2));
        let mut restored = Prefs::load(file.clone());
        restored.initialize_window_motion();
        assert_eq!(
            restored.get::<Option<bool>>("motion.window", None),
            Some(false)
        );
        assert_eq!(restored.get::<Option<bool>>("motion.ui", None), Some(true));
        assert_eq!(
            restored.get::<String>("motion.chartMode", String::new()),
            "balanced"
        );
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn a_new_profile_keeps_system_window_motion_after_changing_interface_motion() {
        let file = path("window-motion-system");
        let _ = std::fs::remove_file(&file);
        let mut prefs = Prefs::load(file.clone());
        prefs.initialize_window_motion();
        prefs.set("motion.ui", false);
        flush(Duration::from_secs(2));
        let mut restored = Prefs::load(file.clone());
        restored.initialize_window_motion();
        assert!(restored.values.contains_key("motion.window"));
        assert_eq!(restored.get::<Option<bool>>("motion.window", None), None);
        restored.set("motion.window", true);
        assert_eq!(restored.get::<Option<bool>>("motion.ui", None), Some(false));
        flush(Duration::from_secs(2));
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn a_range_left_by_an_earlier_release_reads_as_the_default_once() {
        let file = path("migrate-range");
        std::fs::write(
            &file,
            br#"{"values":{"historyRange":"7d","netUnit":"bits"},"window":{"rect":[0,0,900,700],"maximized":false}}"#,
        )
        .unwrap();
        let mut prefs = Prefs::load(file.clone());
        prefs.migrate();
        assert_eq!(prefs.get::<String>("historyRange", "5m".into()), "5m");
        flush(Duration::from_secs(2));
        let saved = argus_app::preferences::load(&file).unwrap();
        assert!(!saved.contains_key("historyRange"));
        assert_eq!(saved[MIGRATED], MIGRATIONS.len());
        // A range picked afterwards stays across restarts.
        prefs.set("historyRange", "3d");
        flush(Duration::from_secs(2));
        let mut restored = Prefs::load(file.clone());
        restored.migrate();
        assert_eq!(restored.get::<String>("historyRange", "5m".into()), "3d");
        assert_eq!(restored.get::<String>("netUnit", String::new()), "bits");
        assert_eq!(restored.get::<usize>(MIGRATED, 0), MIGRATIONS.len());
        assert!(argus_app::preferences::window(&file).is_some());
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn a_new_profile_runs_no_step_after_its_first_start() {
        let file = path("migrate-new");
        let _ = std::fs::remove_file(&file);
        let mut prefs = Prefs::load(file.clone());
        prefs.migrate();
        prefs.set("historyRange", "1h");
        flush(Duration::from_secs(2));
        let mut restored = Prefs::load(file.clone());
        restored.migrate();
        assert_eq!(restored.get::<String>("historyRange", "5m".into()), "1h");
        // A count from a newer version that has more steps runs none here.
        restored.set(MIGRATED, MIGRATIONS.len() + 3);
        restored.set("historyRange", "7d");
        flush(Duration::from_secs(2));
        let mut newer = Prefs::load(file.clone());
        newer.migrate();
        assert_eq!(newer.get::<String>("historyRange", "5m".into()), "7d");
        assert_eq!(newer.get::<usize>(MIGRATED, 0), MIGRATIONS.len() + 3);
        std::fs::remove_file(file).unwrap();
    }

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
