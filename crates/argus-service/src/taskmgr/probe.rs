//! Checks that a private copy of the launcher loads, exits normally, and can
//! be started under a debugger and detached. The self-test does not create
//! IFEO entries: permission to redirect an unrelated probe is not permission
//! to replace Task Manager. Old probe entries are still cleaned up safely.

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

/// What the self-test touches. Registry mutations only clean up old probes.
pub trait Probe {
    /// The test program a self-test recorded and did not clean up.
    fn recorded(&mut self) -> Result<Option<String>>;
    /// Records the test program, then writes it.
    fn prepare(&mut self, name: &str) -> Result<()>;
    /// Deletes the test program, then its record.
    fn forget(&mut self, name: &str) -> Result<()>;
    fn exists(&mut self, name: &str) -> RegResult<bool>;
    fn observe(&mut self, name: &str) -> RegResult<Observation>;
    fn clear_debugger(&mut self, name: &str) -> RegResult<()>;
    fn delete(&mut self, name: &str) -> RegResult<()>;
    /// Starts the test program, bypassing IFEO or not, and returns its exit code.
    fn start(&mut self, name: &str, bypass: bool) -> Result<u32>;
    /// See `registry::refusal`.
    fn refusal(&mut self, error: &registry::Error, name: &str) -> Option<Cause>;
    /// The Debugger value used by older versions' temporary IFEO entries.
    fn legacy_debugger(&self) -> Value;
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
        Ok(true) => return Err(failed(Step::ProbeFiles, tr!("taskManager.probeNameTaken"))),
        Err(error) => {
            return Err(refused(probe, Step::ProbeFiles, &name, error, |error| {
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
    match probe.start(name, false) {
        Ok(0) => {}
        Ok(_) => {
            return Err(failed(
                Step::ProbeStart,
                tr!("taskManager.probeUnexpectedExit"),
            ));
        }
        Err(error) => {
            return Err(Failure::new(
                Step::ProbeStart,
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

/// Removes an old IFEO entry if present, the test program, then the record.
/// An entry someone else changed stays for inspection.
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
    if probe
        .exists(&name)
        .map_err(|error| refused(probe, step, &name, error, text))?
    {
        let actual = probe
            .observe(&name)
            .map_err(|error| refused(probe, step, &name, error, text))?;
        let debugger = probe.legacy_debugger();
        if actual.filtered || actual.debugger.as_ref().is_some_and(|d| *d != debugger) {
            return Err(Failure::new(step, text(tr!("taskManager.probeModified"))));
        }
        if actual.debugger.is_some() {
            probe
                .clear_debugger(&name)
                .map_err(|error| refused(probe, step, &name, error, text))?;
        }
        probe
            .delete(&name)
            .map_err(|error| refused(probe, step, &name, error, text))?;
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

/// The machine: files beside the launcher and cleanup of old IFEO entries.
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

    fn clear_debugger(&mut self, name: &str) -> RegResult<()> {
        registry::write(&Self::key(name), None)
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

    fn legacy_debugger(&self) -> Value {
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
        refuse_removal: bool,
        refuse_flush: bool,
        refuse_files: bool,
        start_error: Option<bool>,
        normal_exit: u32,
        debug_exit: u32,
        starts: Vec<bool>,
        registry_calls: Vec<Call>,
        cause: Option<Cause>,
    }

    impl Memory {
        fn left_anything(&self) -> bool {
            self.record.is_some() || self.program.is_some() || !self.entries.is_empty()
        }

        fn legacy() -> Self {
            let name = format!("ArgusTaskmgrProbe-{}.exe", super::super::id().unwrap());
            let mut m = Self {
                record: Some(name.clone()),
                program: Some(name.clone()),
                ..Default::default()
            };
            m.entries.insert(
                name,
                Observation {
                    debugger: Some(m.legacy_debugger()),
                    filtered: false,
                },
            );
            m
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
            if self.refuse_files {
                return Err(util::message(5));
            }
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
        fn clear_debugger(&mut self, name: &str) -> RegResult<()> {
            self.registry_calls.push(Call::DeleteValue);
            if self.refuse_removal {
                return Err(denied(Call::DeleteValue));
            }
            if let Some(entry) = self.entries.get_mut(name) {
                entry.debugger = None;
            }
            if self.refuse_flush {
                return Err(denied(Call::Flush));
            }
            Ok(())
        }
        fn delete(&mut self, name: &str) -> RegResult<()> {
            self.registry_calls.push(Call::DeleteKey);
            if self.refuse_removal {
                return Err(denied(Call::DeleteKey));
            }
            self.entries.remove(name);
            Ok(())
        }
        fn start(&mut self, name: &str, bypass: bool) -> Result<u32> {
            assert_eq!(self.program.as_deref(), Some(name));
            self.starts.push(bypass);
            if self.start_error == Some(bypass) {
                return Err(util::message(5));
            }
            Ok(if bypass {
                self.debug_exit
            } else {
                self.normal_exit
            })
        }
        fn refusal(&mut self, error: &registry::Error, _: &str) -> Option<Cause> {
            if error.denied() { self.cause } else { None }
        }
        fn legacy_debugger(&self) -> Value {
            Value::string("launcher --probe-redirect")
        }
    }

    #[test]
    fn standalone_checks_do_not_require_writable_ifeo_probe_entries() {
        let mut m = Memory {
            refuse_removal: true,
            ..Default::default()
        };
        run(&mut m).unwrap();
        assert_eq!(m.starts, [false, true]);
        assert!(m.registry_calls.is_empty());
        assert!(!m.left_anything());
    }

    #[test]
    fn either_start_must_succeed_before_enabling() {
        for debug in [false, true] {
            for os_error in [false, true] {
                let mut m = Memory {
                    start_error: os_error.then_some(debug),
                    normal_exit: if !debug && !os_error { 13 } else { 0 },
                    debug_exit: if debug && !os_error { 13 } else { 0 },
                    ..Default::default()
                };
                let failure = run(&mut m).unwrap_err();
                assert_eq!(
                    failure.step,
                    if debug {
                        Step::ProbeBypass
                    } else {
                        Step::ProbeStart
                    }
                );
                assert_eq!(
                    m.starts,
                    if debug {
                        vec![false, true]
                    } else {
                        vec![false]
                    }
                );
                assert!(failure.kept.is_none());
                assert!(m.registry_calls.is_empty());
                assert!(!m.left_anything());
            }
        }
    }

    #[test]
    fn a_failed_file_cleanup_keeps_the_start_error_and_is_retried_first() {
        let mut m = Memory {
            normal_exit: 13,
            refuse_files: true,
            ..Default::default()
        };
        let failure = run(&mut m).unwrap_err();
        assert_eq!(failure.step, Step::ProbeStart);
        assert!(failure.kept.is_some());
        assert!(m.record.is_some() && m.program.is_some());
        assert_eq!(run(&mut m).unwrap_err().step, Step::ProbeLeftover);
        assert_eq!(m.starts, [false]);
        m.refuse_files = false;
        m.normal_exit = 0;
        run(&mut m).unwrap();
        assert!(!m.left_anything());
        assert!(m.registry_calls.is_empty());
    }

    #[test]
    fn legacy_probe_entries_are_cleaned_before_the_new_checks() {
        for empty in [false, true] {
            let mut m = Memory::legacy();
            if empty {
                m.entries.values_mut().next().unwrap().debugger = None;
            }
            run(&mut m).unwrap();
            assert_eq!(
                m.registry_calls,
                if empty {
                    vec![Call::DeleteKey]
                } else {
                    vec![Call::DeleteValue, Call::DeleteKey]
                }
            );
            assert_eq!(m.starts, [false, true]);
            assert!(!m.left_anything());
        }
    }

    #[test]
    fn a_refused_legacy_cleanup_keeps_its_call_and_prevents_a_new_probe() {
        for flush in [false, true] {
            let mut m = Memory {
                refuse_removal: !flush,
                refuse_flush: flush,
                cause: Some(Cause::Blocked),
                ..Memory::legacy()
            };
            let failure = run(&mut m).unwrap_err();
            assert_eq!(failure.step, Step::ProbeLeftover);
            assert_eq!(failure.cause, Some(Cause::Blocked));
            assert_eq!(
                failure.registry,
                Some(OsError {
                    call: if flush {
                        Call::Flush
                    } else {
                        Call::DeleteValue
                    },
                    code: 5,
                })
            );
            assert!(m.starts.is_empty());
            assert!(m.record.is_some() && m.program.is_some() && m.entries.len() == 1);
            m.refuse_removal = false;
            m.refuse_flush = false;
            run(&mut m).unwrap();
            assert!(!m.left_anything());
        }
    }

    #[test]
    fn a_changed_or_unknown_leftover_stays_for_inspection() {
        for foreign in [
            Observation {
                debugger: Some(Value::string("other.exe")),
                filtered: false,
            },
            Observation {
                debugger: None,
                filtered: true,
            },
        ] {
            let mut m = Memory::legacy();
            let name = m.record.clone().unwrap();
            m.entries.insert(name.clone(), foreign.clone());
            assert_eq!(run(&mut m).unwrap_err().step, Step::ProbeLeftover);
            assert_eq!(m.entries.get(&name), Some(&foreign));
            assert!(m.registry_calls.is_empty() && m.starts.is_empty());
        }
        let mut m = Memory {
            record: Some(r"..\argus.exe".into()),
            ..Default::default()
        };
        assert_eq!(run(&mut m).unwrap_err().step, Step::ProbeLeftover);
        assert!(m.registry_calls.is_empty() && m.starts.is_empty());
    }

    #[test]
    #[ignore = "requires a built release launcher in ARGUS_TEST_LAUNCHER"]
    fn release_launcher_runs_normally_and_after_debug_detachment() {
        struct Files(PathBuf);
        impl Drop for Files {
            fn drop(&mut self) {
                let _ = fs::remove_file(self.0.join(LAUNCHER));
                let _ = fs::remove_dir(&self.0);
            }
        }
        let source = std::env::var_os("ARGUS_TEST_LAUNCHER").expect("release launcher path");
        let dir = std::env::temp_dir().join(format!(
            "argus launcher probe-{}",
            super::super::id().unwrap()
        ));
        fs::create_dir(&dir).unwrap();
        let files = Files(dir.clone());
        fs::copy(source, dir.join(LAUNCHER)).unwrap();
        let mut system = System::new(&dir);
        for debug in [false, true] {
            assert_eq!(system.start(LAUNCHER, debug).unwrap(), 0);
        }
        drop(files);
        assert!(!dir.exists());
    }
}
