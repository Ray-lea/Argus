//! The registry adapter is deliberately separate: crash/conflict tests never
//! touch IFEO. A journal is durable before the first registry write.

use argus_core::tr;
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Value {
    pub kind: u32,
    pub bytes: Vec<u8>,
}

impl Value {
    pub fn string(text: &str) -> Self {
        Self {
            kind: 1,
            bytes: text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub debugger: Option<Value>,
    pub filtered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Prepared,
    Active,
    Restoring,
    Restored,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub version: u32,
    pub owner: String,
    pub image: String,
    pub before: Observation,
    pub replacement: Value,
    pub phase: Phase,
}

pub trait Store {
    fn observe(&mut self) -> Result<Observation>;
    fn write_debugger(&mut self, value: Option<&Value>) -> Result<()>;
    fn save(&mut self, journal: &Journal) -> Result<()>;
}

pub fn enable(
    store: &mut impl Store,
    owner: String,
    image: String,
    replacement: Value,
) -> Result<Journal> {
    let before = store.observe()?;
    if before.debugger.is_some() || before.filtered {
        return Err(tr!("taskManager.alreadyReplaced"));
    }
    let mut journal = Journal {
        version: 1,
        owner,
        image,
        before,
        replacement,
        phase: Phase::Prepared,
    };
    store.save(&journal)?;
    if store.observe()? != journal.before {
        return Err(tr!("taskManager.changedBeforeEnable"));
    }
    if let Err(error) = store.write_debugger(Some(&journal.replacement)) {
        return match restore(store, &mut journal) {
            Ok(()) => Err(error),
            Err(recovery) => Err(tr!(
                "taskManager.restoreIncomplete",
                error = error,
                recovery = recovery
            )),
        };
    }
    let actual = store.observe()?;
    if actual.filtered || actual.debugger.as_ref() != Some(&journal.replacement) {
        return Err(tr!("taskManager.changedAfterWrite"));
    }
    journal.phase = Phase::Active;
    // A failed commit leaves Prepared on disk; reconciliation restores it.
    if let Err(error) = store.save(&journal) {
        let recovery = restore(store, &mut journal)
            .err()
            .unwrap_or_else(|| tr!("taskManager.restoredOriginal"));
        return Err(tr!(
            "taskManager.commitFailed",
            error = error,
            recovery = recovery
        ));
    }
    Ok(journal)
}

pub fn restore(store: &mut impl Store, journal: &mut Journal) -> Result<()> {
    let actual = store.observe()?;
    if actual == journal.before {
        journal.phase = Phase::Restored;
        return store.save(journal);
    }
    if actual.filtered || actual.debugger.as_ref() != Some(&journal.replacement) {
        return Err(tr!("taskManager.modified"));
    }
    journal.phase = Phase::Restoring;
    store.save(journal)?;
    if store.observe()? != actual {
        return Err(tr!("taskManager.changedBeforeRestore"));
    }
    store.write_debugger(journal.before.debugger.as_ref())?;
    if store.observe()? != journal.before {
        return Err(tr!("taskManager.verifyFailed"));
    }
    journal.phase = Phase::Restored;
    store.save(journal)
}

/// Undoes what Argus wrote, before the service is updated or removed. After a
/// finished restore nothing in the entry is Argus's: a value written since
/// belongs to someone else, stays, and does not hold up the service.
pub fn settle(store: &mut impl Store, journal: &mut Journal) -> Result<()> {
    if journal.phase == Phase::Restored
        && store.observe()?.debugger.as_ref() != Some(&journal.replacement)
    {
        return Ok(());
    }
    restore(store, journal)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Memory {
        live: Observation,
        disk: Option<Journal>,
        writes: usize,
        fail_save: Option<Phase>,
        fail_write: bool,
        observed: usize,
        change_on: Option<usize>,
    }
    impl Store for Memory {
        fn observe(&mut self) -> Result<Observation> {
            self.observed += 1;
            if self.change_on == Some(self.observed) {
                self.live.debugger = Some(Value::string("other.exe"));
            }
            Ok(self.live.clone())
        }
        fn save(&mut self, j: &Journal) -> Result<()> {
            if self.fail_save == Some(j.phase) {
                return Err("disk failure".into());
            }
            self.disk = Some(j.clone());
            Ok(())
        }
        fn write_debugger(&mut self, value: Option<&Value>) -> Result<()> {
            assert!(self.disk.is_some());
            if self.fail_write {
                return Err("registry failure".into());
            }
            self.live.debugger = value.cloned();
            self.writes += 1;
            Ok(())
        }
    }
    fn activate(s: &mut Memory) -> Result<Journal> {
        enable(
            s,
            "owner".into(),
            "ui.exe".into(),
            Value::string("launcher.exe"),
        )
    }
    #[test]
    fn enable_and_restore_are_durable_and_idempotent() {
        let mut s = Memory::default();
        let mut j = activate(&mut s).unwrap();
        assert_eq!(s.disk.as_ref().unwrap().phase, Phase::Active);
        restore(&mut s, &mut j).unwrap();
        restore(&mut s, &mut j).unwrap();
        assert_eq!(s.live, Observation::default());
        assert_eq!(s.writes, 2);
    }
    #[test]
    fn refuses_foreign_values_including_empty_and_filtered_keys() {
        for live in [
            Observation {
                debugger: Some(Value::string("")),
                filtered: false,
            },
            Observation {
                debugger: None,
                filtered: true,
            },
        ] {
            let mut s = Memory {
                live: live.clone(),
                ..Default::default()
            };
            assert!(activate(&mut s).is_err());
            assert_eq!(s.live, live);
            assert!(s.disk.is_none());
        }
    }
    #[test]
    fn backup_failure_never_changes_registry_and_commit_failure_rolls_back() {
        for phase in [Phase::Prepared, Phase::Active] {
            let mut s = Memory {
                fail_save: Some(phase),
                ..Default::default()
            };
            assert!(activate(&mut s).is_err());
            assert_eq!(s.live, Observation::default());
            if phase == Phase::Prepared {
                assert_eq!(s.writes, 0);
            }
        }
    }
    #[test]
    fn interrupted_enable_and_restore_recover_from_journal() {
        for phase in [Phase::Prepared, Phase::Active, Phase::Restoring] {
            let mut s = Memory::default();
            let mut j = activate(&mut s).unwrap();
            j.phase = phase;
            s.save(&j).unwrap();
            let mut recovered = s.disk.clone().unwrap();
            restore(&mut s, &mut recovered).unwrap();
            assert_eq!(s.live, Observation::default());
        }
    }
    #[test]
    fn external_changes_survive_restore_and_enable_races() {
        let mut s = Memory::default();
        let mut j = activate(&mut s).unwrap();
        s.live.debugger = Some(Value {
            kind: 7,
            bytes: vec![1, 2, 3],
        });
        let foreign = s.live.clone();
        assert!(restore(&mut s, &mut j).is_err());
        assert_eq!(s.live, foreign);
        let mut s = Memory {
            change_on: Some(2),
            ..Default::default()
        };
        assert!(activate(&mut s).is_err());
        assert_eq!(s.writes, 0);
    }
    #[test]
    fn a_finished_restore_leaves_later_values_to_their_owner() {
        let foreign = [
            Observation {
                debugger: Some(Value::string("procexp64.exe")),
                filtered: false,
            },
            Observation {
                debugger: None,
                filtered: true,
            },
            Observation::default(),
        ];
        for live in foreign {
            let mut s = Memory::default();
            let mut j = activate(&mut s).unwrap();
            restore(&mut s, &mut j).unwrap();
            s.live = live.clone();
            let writes = s.writes;
            let mut loaded = s.disk.clone().unwrap();
            settle(&mut s, &mut loaded).unwrap();
            assert_eq!((s.live, s.writes), (live, writes));
            assert_eq!(s.disk.unwrap().phase, Phase::Restored);
        }
        // Argus's value back under a finished record is still Argus's to undo.
        let mut s = Memory::default();
        let mut j = activate(&mut s).unwrap();
        restore(&mut s, &mut j).unwrap();
        s.live.debugger = Some(j.replacement.clone());
        settle(&mut s, &mut j).unwrap();
        assert_eq!(s.live, Observation::default());
        // A record that is not finished keeps refusing someone else's value.
        for phase in [Phase::Prepared, Phase::Active, Phase::Restoring] {
            let mut s = Memory::default();
            let mut j = activate(&mut s).unwrap();
            j.phase = phase;
            s.live.debugger = Some(Value::string("procexp64.exe"));
            assert!(settle(&mut s, &mut j).is_err());
            assert_eq!(s.live.debugger, Some(Value::string("procexp64.exe")));
        }
    }
    #[test]
    fn failed_restore_preserves_files_and_ownership() {
        let mut s = Memory::default();
        let mut j = activate(&mut s).unwrap();
        s.fail_write = true;
        assert!(restore(&mut s, &mut j).is_err());
        assert_eq!(s.live.debugger, Some(j.replacement));
        assert_eq!(s.disk.unwrap().phase, Phase::Restoring);
    }
}
