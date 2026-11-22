//! The order in which a grant, or the maintenance of one, changes the
//! account's folder and task (`commit`), apart from the doing (`Runtime`), so
//! that the tests can look at what a failure or a crash after each step
//! leaves.
//!
//! Two things hold throughout: the task starts a copy that is there, and the
//! records the launcher reads (`located`) name a copy that is there. Argus 0.5
//! and earlier kept the account's folder in `Program Files\Argus Runtime`:
//! the first grant or maintenance of this version moves it into the protected
//! folder, and only then deletes the old one.

use super::*;

/// What `commit` changes, one step each.
pub(super) trait Steps {
    /// Writes this executable as the account's copy, `IMAGE`. A copy there,
    /// which may be running, keeps another name.
    fn put_image(&mut self) -> Result<()>;
    /// Puts back the copy `put_image` replaced. A new copy stays: a
    /// registration of the task that names it may still reach the disk.
    fn undo_image(&mut self);
    /// Deletes the replaced copy and those no record names, unless they run.
    fn release_image(&mut self);
    /// The launcher, its icon, and the grant record to commit.
    fn prepare(&mut self) -> Result<()>;
    /// Brings an enabled task manager replacement to this build.
    fn follow(&mut self) -> Result<()>;
    /// Makes the task start the account's copy.
    fn register(&mut self) -> Result<()>;
    /// Makes the task start what it started before, or removes a new one.
    fn restore_task(&mut self);
    fn write_launch(&mut self) -> Result<()>;
    /// Commits the grant record (`enable`), or discards it.
    fn commit_grant(&mut self) -> Result<()>;
    /// Deletes the grant record to commit, and with `moving` the launch
    /// record written here.
    fn discard(&mut self, moving: bool);
    /// Whether Argus 0.5 or earlier left a folder of the account's.
    fn legacy(&mut self) -> bool;
    /// Writes the task's registration to disk.
    fn flush(&mut self) -> Result<()>;
    /// Points this account's shortcuts and sign-in entry at the launcher.
    fn links(&mut self);
    /// Deletes what Argus 0.5 or earlier left of the account
    /// (`retire_legacy`).
    fn retire(&mut self);
}

/// `enable`: the task starts the copy, as a grant does; otherwise only the
/// standard launcher's records are kept up (a revoked grant's maintenance).
/// `moving`: the records are still where Argus 0.5 or earlier kept them.
pub(super) fn commit(
    steps: &mut impl Steps,
    enable: bool,
    moving: bool,
    same_account: bool,
) -> Result<()> {
    steps.put_image()?;
    let result = (|| {
        steps.prepare()?;
        steps.follow()?;
        if enable {
            steps.register()?;
        }
        steps.write_launch()?;
        steps.commit_grant()
    })();
    if result.is_err() {
        steps.restore_task();
        steps.undo_image();
        // The records stay where the task names: a launch record written
        // here would otherwise move them half way.
        steps.discard(moving);
        // Preserve a record that may already have committed; status exposes
        // any inconsistency and repair can complete it with explicit consent.
        return result;
    }
    steps.release_image();
    // The task may have named a copy in the folder of Argus 0.5 or earlier:
    // before that goes, the registration has to reach the disk, or a crash
    // could bring back one that names a deleted copy.
    let settled = !steps.legacy() || steps.flush().is_ok();
    if same_account {
        // The grant is committed. Optional shortcut/compatibility migration
        // failures must not report that the working authorization failed.
        steps.links();
    }
    if settled {
        steps.retire();
    }
    Ok(())
}

/// The account's folder and task.
pub(super) struct Runtime<'a> {
    pub id: &'a str,
    pub sid: &'a str,
    pub dir: PathBuf,
    /// This executable, which becomes the copy.
    pub current: PathBuf,
    pub record: Grant,
    /// The grant record before this, with the folder it is in.
    pub previous: Option<(PathBuf, Grant)>,
    pub scheduler: &'a task::Scheduler,
    pub enable: bool,
    pub same_account: bool,
    /// The grant record to commit.
    pub pending: PathBuf,
    pub swapped: Option<crate::layout::Swapped>,
}

impl Steps for Runtime<'_> {
    fn put_image(&mut self) -> Result<()> {
        let pending = self.dir.join(format!("image-{}.exe", guid()?));
        let copied = (|| {
            let mut input = fs::File::open(&self.current).map_err(io)?;
            let mut output = crate::install::new_protected_file(&pending)?;
            std::io::copy(&mut input, &mut output).map_err(io)?;
            output.sync_all().map_err(io)?;
            drop(output);
            crate::layout::swap_in(&pending, &self.dir.join(IMAGE), STEM)
        })();
        if copied.is_err() {
            let _ = fs::remove_file(&pending);
        }
        self.swapped = Some(copied?);
        Ok(())
    }

    fn undo_image(&mut self) {
        if let Some(swapped) = self.swapped.take()
            && swapped.aside.is_some()
        {
            let _ = swapped.undo(&self.dir.join(IMAGE));
        }
    }

    fn release_image(&mut self) {
        if let Some(swapped) = self.swapped.take() {
            swapped.release();
        }
        sweep(&self.dir, Some(IMAGE));
    }

    fn prepare(&mut self) -> Result<()> {
        if let Some(bytes) = crate::taskmgr::launcher_asset() {
            put_launcher(&self.dir, bytes)?;
            let icon = self.dir.join(format!("icon-{}.ico", guid()?));
            let mut file = crate::install::new_protected_file(&icon)?;
            file.write_all(include_bytes!("../../../../resources/icons/icon.ico"))
                .map_err(io)?;
            file.sync_all().map_err(io)?;
            drop(file);
            replace(&icon, &self.dir.join(ICON))?;
        }
        // Prepare the complete replacement before changing the authoritative task.
        let mut output = crate::install::new_protected_file(&self.pending)?;
        output
            .write_all(&serde_json::to_vec(&self.record).map_err(|_| 13u32)?)
            .map_err(io)?;
        output.sync_all().map_err(io)
    }

    fn follow(&mut self) -> Result<()> {
        // An older replacement copy must also learn the new startup policy.
        if self.id == "dev.argus.taskmanager" {
            let service = crate::install::directory()?;
            if service.exists() {
                let _guard = security::protect_directory(&service)?;
                crate::taskmgr::follow_update(&service).map_err(|error| {
                    launch_failure("administrator task manager copy refresh", error)
                })?;
            }
        }
        Ok(())
    }

    fn register(&mut self) -> Result<()> {
        self.scheduler
            .register(self.id, self.sid, &self.dir.join(IMAGE))
    }

    fn restore_task(&mut self) {
        let _ = match &self.previous {
            Some((old_dir, old)) => {
                self.scheduler
                    .register(self.id, self.sid, &old_dir.join(&old.image))
            }
            None => self.scheduler.remove(self.id, self.sid),
        };
    }

    fn write_launch(&mut self) -> Result<()> {
        write_launch(&self.dir, &self.record)
    }

    fn commit_grant(&mut self) -> Result<()> {
        if self.enable {
            replace(&self.pending, &self.dir.join("grant.json"))
        } else {
            fs::remove_file(&self.pending).map_err(io)
        }
    }

    fn discard(&mut self, moving: bool) {
        let _ = fs::remove_file(&self.pending);
        if moving {
            let _ = fs::remove_file(self.dir.join("launch.json"));
        }
    }

    fn legacy(&mut self) -> bool {
        legacy_directory(self.id, self.sid).is_ok_and(|legacy| legacy.exists())
    }

    fn flush(&mut self) -> Result<()> {
        util::stage(
            "administrator task registration flush",
            crate::install::flush_key(
                windows::Win32::System::Registry::HKEY_LOCAL_MACHINE,
                TASK_CACHE,
            ),
        )
    }

    fn links(&mut self) {
        let moved = legacy_directory(self.id, self.sid).map(|legacy| legacy.join(LAUNCHER));
        let moved = moved.as_ref().ok().filter(|moved| moved.is_file());
        let _ = util::stage(
            "administrator shortcut refresh",
            links::retarget(
                self.id,
                &self.record.source,
                &self.dir.join(LAUNCHER),
                moved.map(PathBuf::as_path),
            ),
        );
    }

    fn retire(&mut self) {
        retire_legacy(self.id, self.sid, self.same_account);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap};

    /// A copy of Argus 0.5 or earlier, in the folder it kept.
    const LEGACY: &str = "legacy/argus-admin-12345678-1234-1234-1234-123456789abc.exe";
    const NEW: &str = "new/argus-admin.exe";
    const ASIDE: &str = "new/argus-admin.old.exe";
    const REFUSED: u32 = 0x00c0_ffee;

    /// The account's folders and task in memory. Each step is checked
    /// against what a crash at that moment would leave.
    #[derive(Default)]
    struct World {
        /// Each file and which build it holds: "new" is this one.
        files: BTreeMap<&'static str, &'static str>,
        task: Option<&'static str>,
        /// Every registration of the task the disk may hold.
        disk: Vec<Option<&'static str>>,
        /// The copy each folder's records name.
        records: Option<&'static str>,
        legacy_records: Option<&'static str>,
        granted: bool,
        /// Whether the grant is on, as `commit` is told.
        enable: bool,
        pending: bool,
        /// What `put_image` did: None before, Some(true) when it kept a copy aside.
        swapped: Option<bool>,
        previous: Option<&'static str>,
        refused: Vec<(&'static str, usize)>,
        calls: HashMap<&'static str, usize>,
        steps: Vec<&'static str>,
    }

    impl World {
        /// A grant of Argus 0.5 or earlier.
        fn legacy() -> Self {
            Self {
                files: [(LEGACY, "old")].into(),
                task: Some(LEGACY),
                disk: vec![Some(LEGACY)],
                legacy_records: Some(LEGACY),
                previous: Some(LEGACY),
                enable: true,
                ..Default::default()
            }
        }
        /// A grant of this version, made by an earlier build.
        fn current() -> Self {
            Self {
                files: [(NEW, "old")].into(),
                task: Some(NEW),
                disk: vec![Some(NEW)],
                records: Some(NEW),
                granted: true,
                previous: Some(NEW),
                enable: true,
                ..Default::default()
            }
        }
        /// A revoked grant of Argus 0.5 or earlier: only the standard
        /// launcher's records, no task.
        fn revoked() -> Self {
            Self {
                files: [(LEGACY, "old")].into(),
                legacy_records: Some(LEGACY),
                ..Default::default()
            }
        }

        fn refusing(mut self, step: &'static str, nth: usize) -> Self {
            self.refused.push((step, nth));
            self
        }

        fn step(&mut self, name: &'static str) -> Result<()> {
            self.steps.push(name);
            let nth = self.calls.entry(name).or_default();
            *nth += 1;
            if self.refused.contains(&(name, *nth)) {
                return Err(REFUSED);
            }
            Ok(())
        }

        /// The copy the launcher's records name, where `located` reads them.
        fn located(&self) -> Option<&'static str> {
            self.records.or(self.legacy_records)
        }

        /// After a crash now, the task starts one of `disk`, and the
        /// launcher reads `located`: each must name a copy that is there.
        fn check(&self) {
            for image in self.disk.iter().flatten().chain(self.located().as_ref()) {
                assert!(
                    self.files.contains_key(image),
                    "a crash after {:?} leaves {image} named and missing",
                    self.steps.last()
                );
            }
        }
    }

    impl Steps for World {
        fn put_image(&mut self) -> Result<()> {
            self.step("put_image")?;
            let aside = self.files.get(NEW).copied();
            if let Some(build) = aside {
                self.files.insert(ASIDE, build);
            }
            self.files.insert(NEW, "new");
            self.swapped = Some(aside.is_some());
            self.check();
            Ok(())
        }
        fn undo_image(&mut self) {
            self.steps.push("undo_image");
            if self.swapped.take() == Some(true) {
                let build = self.files.remove(ASIDE).unwrap();
                self.files.insert(NEW, build);
            }
            self.check();
        }
        fn release_image(&mut self) {
            self.steps.push("release_image");
            self.files.remove(ASIDE);
            self.check();
        }
        fn prepare(&mut self) -> Result<()> {
            self.step("prepare")?;
            self.pending = true;
            Ok(())
        }
        fn follow(&mut self) -> Result<()> {
            self.step("follow")
        }
        fn register(&mut self) -> Result<()> {
            self.step("register")?;
            self.task = Some(NEW);
            self.disk.push(self.task);
            self.check();
            Ok(())
        }
        fn restore_task(&mut self) {
            if self.step("restore_task").is_ok() {
                self.task = self.previous;
                self.disk.push(self.task);
            }
        }
        fn write_launch(&mut self) -> Result<()> {
            self.step("write_launch")?;
            self.records = Some(NEW);
            self.check();
            Ok(())
        }
        fn commit_grant(&mut self) -> Result<()> {
            self.step("commit_grant")?;
            self.pending = false;
            self.granted |= self.enable;
            Ok(())
        }
        fn discard(&mut self, moving: bool) {
            self.steps.push("discard");
            self.pending = false;
            if moving {
                self.records = None;
            }
            self.check();
        }
        fn legacy(&mut self) -> bool {
            self.files.contains_key(LEGACY) || self.legacy_records.is_some()
        }
        fn flush(&mut self) -> Result<()> {
            self.step("flush")?;
            self.disk = vec![self.task];
            Ok(())
        }
        fn links(&mut self) {
            self.steps.push("links");
        }
        fn retire(&mut self) {
            self.steps.push("retire");
            if self.records.is_some() {
                self.files.remove(LEGACY);
                self.legacy_records = None;
            }
            self.check();
        }
    }

    #[test]
    fn a_grant_of_an_earlier_version_moves_into_the_protected_folder() {
        let mut world = World::legacy();
        assert_eq!(commit(&mut world, true, true, true), Ok(()));
        assert_eq!(
            world.steps,
            [
                "put_image",
                "prepare",
                "follow",
                "register",
                "write_launch",
                "commit_grant",
                "release_image",
                "flush",
                "links",
                "retire",
            ]
        );
        assert_eq!(world.files, BTreeMap::from([(NEW, "new")]));
        assert_eq!((world.task, world.located()), (Some(NEW), Some(NEW)));
        assert!(world.granted && !world.pending);
        assert!(world.legacy_records.is_none());
    }

    #[test]
    fn a_grant_in_place_replaces_its_copy_and_keeps_the_task_as_it_is() {
        let mut world = World::current();
        assert_eq!(commit(&mut world, true, false, true), Ok(()));
        assert!(!world.steps.contains(&"flush"));
        assert_eq!(world.files, BTreeMap::from([(NEW, "new")]));
        assert_eq!(
            world.disk.iter().flatten().collect::<Vec<_>>(),
            [&NEW, &NEW]
        );
    }

    #[test]
    fn a_revoked_grant_moves_without_a_task() {
        let mut world = World::revoked();
        assert_eq!(commit(&mut world, false, true, false), Ok(()));
        assert!(!world.steps.contains(&"register") && !world.steps.contains(&"links"));
        assert_eq!(world.task, None);
        assert_eq!(world.located(), Some(NEW));
        assert!(!world.granted);
    }

    #[test]
    fn without_the_flush_the_earlier_folder_stays_for_the_next_time() {
        let mut world = World::legacy().refusing("flush", 1);
        assert_eq!(commit(&mut world, true, true, true), Ok(()));
        assert!(!world.steps.contains(&"retire"));
        assert!(world.files.contains_key(LEGACY));
        assert_eq!(world.located(), Some(NEW));
        // The next maintenance finds the records moved and the old folder
        // still there: it writes the task to disk before deleting it.
        world.refused.clear();
        world.steps.clear();
        assert_eq!(commit(&mut world, true, false, true), Ok(()));
        assert!(world.steps.contains(&"flush") && world.steps.contains(&"retire"));
        assert!(!world.files.contains_key(LEGACY));
    }

    #[test]
    fn a_failed_grant_leaves_the_task_and_records_where_they_were() {
        type Make = fn() -> World;
        let worlds: [(Make, bool, bool); 3] = [
            (World::legacy, true, true),
            (World::current, true, false),
            (World::revoked, false, true),
        ];
        let steps = [
            "put_image",
            "prepare",
            "follow",
            "register",
            "write_launch",
            "commit_grant",
            "restore_task",
            "flush",
        ];
        for (index, (make, enable, moving)) in worlds.into_iter().enumerate() {
            for step in steps {
                let before = make();
                let mut world = make().refusing(step, 1);
                // `check` looked at each step on the way.
                let result = commit(&mut world, enable, moving, true);
                let case = format!("world {index}, {step}: {:?}", world.steps);
                if result.is_ok() {
                    assert_eq!(world.located(), Some(NEW), "{case}");
                    assert_eq!(world.files.get(NEW), Some(&"new"), "{case}");
                    if enable {
                        assert_eq!(world.task, Some(NEW), "{case}");
                    }
                    continue;
                }
                assert_eq!(result, Err(REFUSED), "{case}");
                // Back to what was there: the task, the records and the
                // copy they name, holding the earlier build.
                assert_eq!(world.task, before.task, "{case}");
                assert_eq!(world.located(), before.located(), "{case}");
                if let Some(image) = before.located() {
                    assert_eq!(world.files.get(image), Some(&"old"), "{case}");
                }
                assert!(!world.pending, "{case}");
                assert!(!world.files.contains_key(ASIDE), "{case}");
            }
        }

        // The task could not be put back: the new copy it may start stays.
        let mut world = World::legacy()
            .refusing("commit_grant", 1)
            .refusing("restore_task", 1);
        assert_eq!(commit(&mut world, true, true, true), Err(REFUSED));
        assert_eq!(world.task, Some(NEW));
        assert!(world.files.contains_key(NEW) && world.files.contains_key(LEGACY));
    }
}
