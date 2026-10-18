//! The startup self-test of an Enable action: a randomly named copy of the
//! launcher gets its own IFEO entry; a normal start must be redirected and a
//! debug start must bypass it. It never touches taskmgr.exe's entry. `Probe`
//! keeps what it touches apart from the order and cleanup, which the tests
//! check without IFEO.

use super::registry::{self, RegResult};
use super::state::{Observation, Result, Value};
use super::{Cause, Failure, LAUNCHER, Step, launch};
use crate::{security, util};
use argus_core::tr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Names the test program of a self-test that is not cleaned up yet.
pub const RECORD: &str = "taskmgr-probe.txt";

/// What the self-test touches. Test programs and their entries go by the
/// program's file name.
pub trait Probe {
    /// The test program a self-test recorded and did not clean up.
    fn recorded(&mut self) -> Result<Option<String>>;
    /// Records the test program, then writes it.
    fn prepare(&mut self, name: &str) -> Result<()>;
    /// Deletes the test program, then its record.
    fn forget(&mut self, name: &str) -> Result<()>;
    fn exists(&mut self, name: &str) -> RegResult<bool>;
    fn observe(&mut self, name: &str) -> RegResult<Observation>;
    fn create(&mut self, name: &str) -> RegResult<()>;
    fn write(&mut self, name: &str, debugger: Option<&Value>) -> RegResult<()>;
    fn delete(&mut self, name: &str) -> RegResult<()>;
    /// Starts the test program, bypassing IFEO or not, and returns its exit code.
    fn start(&mut self, name: &str, bypass: bool) -> Result<u32>;
    /// See `registry::refusal`.
    fn refusal(&mut self, error: &registry::Error, name: &str) -> Option<Cause>;
    /// The Debugger value that redirects the test program to the launcher.
    fn debugger(&self) -> Value;
}

/// Runs the self-test, then cleans up. When the cleanup fails too, the
/// self-test's error stays the main one and the cleanup's is added to it.
pub fn run(probe: &mut impl Probe) -> std::result::Result<(), Failure> {
    cleanup(probe, Step::ProbeLeftover)?;
    let name = format!(
        "ArgusTaskmgrProbe-{}.exe",
        super::id().map_err(|e| Failure::new(Step::ProbeFiles, e))?
    );
    match probe.exists(&name) {
        Ok(false) => {}
        Ok(true) => return Err(failed(Step::ProbeCreate, tr!("taskManager.probeNameTaken"))),
        Err(error) => {
            return Err(refused(probe, Step::ProbeCreate, &name, error, |error| {
                tr!("taskManager.selfTestFailed", error = error)
            }));
        }
    }
    probe.prepare(&name).map_err(|error| {
        Failure::new(
            Step::ProbeFiles,
            tr!("taskManager.probeFilesFailed", error = error),
        )
    })?;
    let result = test(probe, &name);
    match (result, cleanup(probe, Step::ProbeCleanup)) {
        (result, Ok(())) => result,
        (Ok(()), Err(cleanup)) => Err(cleanup),
        (Err(failure), Err(cleanup)) => Err(failure.kept(cleanup)),
    }
}

fn test(probe: &mut impl Probe, name: &str) -> std::result::Result<(), Failure> {
    if let Err(error) = probe.create(name) {
        return Err(refused(probe, Step::ProbeCreate, name, error, |error| {
            tr!("taskManager.probeCreateFailed", error = error)
        }));
    }
    let debugger = probe.debugger();
    if let Err(error) = probe.write(name, Some(&debugger)) {
        return Err(refused(probe, Step::ProbeWrite, name, error, |error| {
            tr!("taskManager.probeWriteFailed", error = error)
        }));
    }
    match probe.start(name, false) {
        Ok(86) => {}
        Ok(_) => {
            return Err(failed(
                Step::ProbeRedirect,
                tr!("taskManager.probeNotRedirected"),
            ));
        }
        Err(error) => {
            return Err(Failure::new(
                Step::ProbeRedirect,
                tr!("taskManager.probeStartFailed", error = error),
            ));
        }
    }
    match probe.start(name, true) {
        Ok(0) => Ok(()),
        Ok(_) => Err(failed(
            Step::ProbeBypass,
            tr!("taskManager.probeFallbackFailed"),
        )),
        Err(error) => Err(Failure::new(
            Step::ProbeBypass,
            tr!("taskManager.probeStartFailed", error = error),
        )),
    }
}

/// Removes what a self-test left: its entry, its test program, then the
/// record. An entry someone else changed stays for inspection.
pub fn cleanup(probe: &mut impl Probe, step: Step) -> std::result::Result<(), Failure> {
    let text = |error: String| {
        if step == Step::ProbeLeftover {
            tr!("taskManager.probeLeftoverFailed", error = error)
        } else {
            tr!("taskManager.probeCleanupFailed", error = error)
        }
    };
    let Some(name) = probe
        .recorded()
        .map_err(|error| Failure::new(step, text(error)))?
    else {
        return Ok(());
    };
    if !valid(&name) {
        return Err(Failure::new(
            step,
            text(tr!("taskManager.probeRecordInvalid")),
        ));
    }
    let actual = probe
        .observe(&name)
        .map_err(|error| refused(probe, step, &name, error, text))?;
    let debugger = probe.debugger();
    if actual.filtered || actual.debugger.as_ref().is_some_and(|d| *d != debugger) {
        return Err(Failure::new(step, text(tr!("taskManager.probeModified"))));
    }
    if let Err(error) = probe.write(&name, None).and_then(|()| probe.delete(&name)) {
        return Err(refused(probe, step, &name, error, text));
    }
    probe
        .forget(&name)
        .map_err(|error| Failure::new(step, text(error)))
}

/// A self-test failure that is not an OS error of its own.
fn failed(step: Step, error: String) -> Failure {
    Failure::new(step, tr!("taskManager.selfTestFailed", error = error))
}

/// An IFEO change the self-test could not make, with why when Windows can
/// tell.
fn refused(
    probe: &mut impl Probe,
    step: Step,
    name: &str,
    error: registry::Error,
    text: impl FnOnce(String) -> String,
) -> Failure {
    let cause = probe.refusal(&error, name);
    Failure {
        registry: error.os(),
        ..Failure::refused(step, text(error.into()), cause)
    }
}

fn valid(name: &str) -> bool {
    name.strip_prefix("ArgusTaskmgrProbe-")
        .and_then(|s| s.strip_suffix(".exe"))
        .is_some_and(|s| s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
}

/// The machine: files beside the launcher, entries under IFEO.
pub struct System {
    dir: PathBuf,
    /// Pins the record while a cleanup works with it.
    record: Option<util::Handle>,
}

impl System {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.into(),
            record: None,
        }
    }

    fn key(name: &str) -> String {
        format!("{}\\{name}", registry::IFEO)
    }
}

impl Probe for System {
    fn recorded(&mut self) -> Result<Option<String>> {
        let record = self.dir.join(RECORD);
        if !record.exists() {
            return Ok(None);
        }
        self.record = Some(security::protect_directory(&record).map_err(util::message)?);
        if fs::metadata(&record).map_err(|e| e.to_string())?.len() > 128 {
            return Err(tr!("taskManager.probeRecordInvalid"));
        }
        fs::read_to_string(&record)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    fn prepare(&mut self, name: &str) -> Result<()> {
        registry::atomic_file(&self.dir, RECORD, name.as_bytes())?;
        let launcher = fs::read(self.dir.join(LAUNCHER)).map_err(|e| e.to_string())?;
        registry::atomic_file(&self.dir, name, &launcher)
    }

    fn forget(&mut self, name: &str) -> Result<()> {
        let path = self.dir.join(name);
        if path.exists() {
            drop(security::protect_directory(&path).map_err(util::message)?);
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        self.record = None;
        fs::remove_file(self.dir.join(RECORD)).map_err(|e| e.to_string())
    }

    fn exists(&mut self, name: &str) -> RegResult<bool> {
        Ok(registry::open(&Self::key(name), false)?.is_some())
    }

    fn observe(&mut self, name: &str) -> RegResult<Observation> {
        registry::observe(&Self::key(name))
    }

    fn create(&mut self, name: &str) -> RegResult<()> {
        registry::create(&Self::key(name)).map(drop)
    }

    fn write(&mut self, name: &str, debugger: Option<&Value>) -> RegResult<()> {
        registry::write(&Self::key(name), debugger)
    }

    fn delete(&mut self, name: &str) -> RegResult<()> {
        registry::delete(&Self::key(name))
    }

    fn start(&mut self, name: &str, bypass: bool) -> Result<u32> {
        let process = launch::spawn(&self.dir.join(name), &["--probe-child".into()], bypass)?;
        launch::exit_code(&process, Duration::from_secs(5))
    }

    fn refusal(&mut self, error: &registry::Error, name: &str) -> Option<Cause> {
        registry::refusal(error, &Self::key(name))
    }

    fn debugger(&self) -> Value {
        Value::string(&format!(
            "{} --probe-redirect",
            launch::quote(&self.dir.join(LAUNCHER).to_string_lossy())
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::registry::{Call, OsError};
    use super::*;
    use std::collections::HashMap;

    fn denied(call: Call) -> registry::Error {
        registry::Error::Os(OsError { call, code: 5 })
    }

    /// A machine whose files, entries and test program are a few fields.
    #[derive(Default)]
    struct Memory {
        record: Option<String>,
        program: Option<String>,
        entries: HashMap<String, Observation>,
        refuse_create: bool,
        refuse_write: bool,
        refuse_removal: bool,
        refuse_flush: bool,
        redirected: bool,
        cause: Option<Cause>,
    }

    impl Memory {
        fn left_anything(&self) -> bool {
            self.record.is_some() || self.program.is_some() || !self.entries.is_empty()
        }
    }

    impl Probe for Memory {
        fn recorded(&mut self) -> Result<Option<String>> {
            Ok(self.record.clone())
        }
        fn prepare(&mut self, name: &str) -> Result<()> {
            self.record = Some(name.into());
            self.program = Some(name.into());
            Ok(())
        }
        fn forget(&mut self, _: &str) -> Result<()> {
            self.program = None;
            self.record = None;
            Ok(())
        }
        fn exists(&mut self, name: &str) -> RegResult<bool> {
            Ok(self.entries.contains_key(name))
        }
        fn observe(&mut self, name: &str) -> RegResult<Observation> {
            Ok(self.entries.get(name).cloned().unwrap_or_default())
        }
        fn create(&mut self, name: &str) -> RegResult<()> {
            if self.refuse_create {
                return Err(denied(Call::Create(3)));
            }
            self.entries.entry(name.into()).or_default();
            Ok(())
        }
        fn write(&mut self, name: &str, debugger: Option<&Value>) -> RegResult<()> {
            if debugger.map_or(self.refuse_removal, |_| self.refuse_write) {
                return Err(denied(if debugger.is_some() {
                    Call::SetValue
                } else {
                    Call::DeleteValue
                }));
            }
            if let Some(entry) = self.entries.get_mut(name) {
                entry.debugger = debugger.cloned();
            }
            if debugger.is_some() && self.refuse_flush {
                return Err(denied(Call::Flush));
            }
            Ok(())
        }
        fn delete(&mut self, name: &str) -> RegResult<()> {
            if self.refuse_removal {
                return Err(denied(Call::DeleteKey));
            }
            self.entries.remove(name);
            Ok(())
        }
        fn start(&mut self, name: &str, bypass: bool) -> Result<u32> {
            // A redirected start runs the launcher, which exits with 86.
            let entry = self.entries.get(name).and_then(|e| e.debugger.clone());
            Ok(
                if !bypass && self.redirected && entry == Some(self.debugger()) {
                    86
                } else {
                    0
                },
            )
        }
        fn refusal(&mut self, error: &registry::Error, _: &str) -> Option<Cause> {
            if error.denied() { self.cause } else { None }
        }
        fn debugger(&self) -> Value {
            Value::string("launcher --probe-redirect")
        }
    }

    #[test]
    fn a_passing_self_test_leaves_nothing() {
        let mut m = Memory {
            redirected: true,
            ..Default::default()
        };
        run(&mut m).unwrap();
        assert!(!m.left_anything());
    }

    #[test]
    fn a_refused_entry_names_its_step_and_cause_and_leaves_nothing() {
        for (create, write, step, call) in [
            (true, false, Step::ProbeCreate, Call::Create(3)),
            (false, true, Step::ProbeWrite, Call::SetValue),
        ] {
            let mut m = Memory {
                refuse_create: create,
                refuse_write: write,
                redirected: true,
                cause: Some(Cause::Blocked),
                ..Default::default()
            };
            let failure = run(&mut m).unwrap_err();
            assert_eq!(failure.step, step);
            assert_eq!(failure.cause, Some(Cause::Blocked));
            assert_eq!(failure.registry, Some(OsError { call, code: 5 }));
            assert!(failure.kept.is_none());
            assert!(!m.left_anything());
        }
    }

    #[test]
    fn a_failed_start_keeps_its_own_error_after_the_cleanup() {
        let mut m = Memory::default();
        let failure = run(&mut m).unwrap_err();
        assert_eq!(failure.step, Step::ProbeRedirect);
        assert!(failure.kept.is_none());
        assert!(!m.left_anything());
    }

    #[test]
    fn a_refused_cleanup_is_reported_with_the_error_and_retried_next_time() {
        let mut m = Memory {
            refuse_removal: true,
            cause: Some(Cause::Blocked),
            ..Default::default()
        };
        let failure = run(&mut m).unwrap_err();
        assert_eq!(failure.step, Step::ProbeRedirect);
        assert_eq!(failure.cause, None);
        assert_eq!(failure.registry, None);
        assert_eq!(failure.kept_cause, Some(Cause::Blocked));
        assert_eq!(
            failure.kept_registry,
            Some(OsError {
                call: Call::DeleteValue,
                code: 5
            })
        );
        assert!(failure.kept.is_some());
        assert!(m.record.is_some() && m.program.is_some() && m.entries.len() == 1);
        // The next attempt stops at the leftover rather than adding entries.
        assert_eq!(run(&mut m).unwrap_err().step, Step::ProbeLeftover);
        assert_eq!(m.entries.len(), 1);
        m.refuse_removal = false;
        m.redirected = true;
        run(&mut m).unwrap();
        assert!(!m.left_anything());
    }

    #[test]
    fn a_flush_failure_keeps_its_call_even_when_cleanup_also_fails() {
        for refuse_removal in [false, true] {
            let mut m = Memory {
                refuse_flush: true,
                refuse_removal,
                redirected: true,
                ..Default::default()
            };
            let failure = run(&mut m).unwrap_err();
            assert_eq!(failure.step, Step::ProbeWrite);
            assert_eq!(
                failure.registry,
                Some(OsError {
                    call: Call::Flush,
                    code: 5
                })
            );
            assert_eq!(
                failure.kept_registry,
                refuse_removal.then_some(OsError {
                    call: Call::DeleteValue,
                    code: 5
                })
            );
            assert_eq!(m.left_anything(), refuse_removal);
            if refuse_removal {
                m.refuse_removal = false;
                cleanup(&mut m, Step::ProbeLeftover).unwrap();
                assert!(!m.left_anything());
            }
        }
    }

    #[test]
    fn a_changed_or_unknown_leftover_stays_for_inspection() {
        let name = format!("ArgusTaskmgrProbe-{}.exe", super::super::id().unwrap());
        let foreign = Observation {
            debugger: Some(Value::string("other.exe")),
            filtered: false,
        };
        let mut m = Memory {
            record: Some(name.clone()),
            entries: HashMap::from([(name.clone(), foreign.clone())]),
            redirected: true,
            ..Default::default()
        };
        assert_eq!(run(&mut m).unwrap_err().step, Step::ProbeLeftover);
        assert_eq!(m.entries.get(&name), Some(&foreign));
        let mut m = Memory {
            record: Some(r"..\argus.exe".into()),
            redirected: true,
            ..Default::default()
        };
        assert_eq!(run(&mut m).unwrap_err().step, Step::ProbeLeftover);
    }
}
