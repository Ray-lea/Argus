//! Where Argus keeps what runs with elevated rights: one folder in Program
//! Files, `Argus Privileged` (`Argus Privileged Dev` for a development build),
//! whose programs have fixed names. The service and the task manager
//! replacement live at its top, administrator startup in `runtime`, and the
//! sensor driver's setup in `setup`.
//!
//! Argus 0.5 and earlier gave every copy a name of its own and kept the other
//! two in folders of their own beside it (`LEGACY_RUNTIME`, `LEGACY_SETUP`).
//! Those names and folders are still read, and cleaned up once nothing needs
//! them.
//!
//! A running program cannot be overwritten or deleted, but another name can
//! be linked to it, and the name it ran under then replaced: `swap_in` links
//! the file in place aside, then renames the new one over it. The fixed name
//! names a whole file throughout; after a crash, the old one or the new one.

use std::fs;
use std::path::{Path, PathBuf};

use windows::Win32::Storage::FileSystem::{
    CreateDirectoryW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows::core::PCWSTR;

use crate::security;
use crate::util::{Handle, Result, io, wide, win};

/// Administrator startup's folders, one for each application identifier.
pub(crate) const RUNTIME: &str = "runtime";
/// The sensor driver's setup: its staging, lock and helper reports.
pub(crate) const SETUP: &str = "setup";
/// Where Argus 0.5 and earlier kept administrator startup, beside the
/// service's folder in Program Files.
pub(crate) const LEGACY_RUNTIME: &str = "Argus Runtime";
/// Where Argus 0.5 and earlier kept the sensor driver's setup.
pub(crate) const LEGACY_SETUP: &str = "Argus Sensor Setup";
/// How many replaced programs of one name may still be running.
const ASIDE_LIMIT: u32 = 9;

pub(crate) fn root() -> Result<PathBuf> {
    crate::install::directory()
}

pub(crate) fn runtime() -> Result<PathBuf> {
    Ok(root()?.join(RUNTIME))
}

pub(crate) fn setup() -> Result<PathBuf> {
    Ok(root()?.join(SETUP))
}

fn program_files() -> Result<PathBuf> {
    Ok(root()?.parent().ok_or(87u32)?.to_path_buf())
}

pub(crate) fn legacy_runtime() -> Result<PathBuf> {
    Ok(program_files()?.join(LEGACY_RUNTIME))
}

pub(crate) fn legacy_setup() -> Result<PathBuf> {
    Ok(program_files()?.join(LEGACY_SETUP))
}

/// Makes `path` a folder only administrators and SYSTEM can change, unless
/// it is one already, and pins it once its owner and permissions pass.
pub(crate) fn create_protected(path: &Path) -> Result<Handle> {
    let descriptor = security::directory_descriptor()?;
    if let Err(error) = unsafe {
        CreateDirectoryW(
            PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
            Some(&descriptor.attributes()),
        )
    } {
        let code = win(error);
        if code != 183 {
            return Err(code);
        }
    }
    security::protect_directory(path)
}

/// Pins Program Files and the protected folder, made if it is not there:
/// administrator startup and the sensor driver's setup may come before the
/// service, or stay after it.
pub(crate) fn ensure_root() -> Result<(Handle, Handle)> {
    let root = root()?;
    let parent = security::protect_directory(root.parent().ok_or(87u32)?)?;
    Ok((parent, create_protected(&root)?))
}

/// The name a replaced `<stem>.exe` keeps while it still runs: `<stem>.old.exe`,
/// then `<stem>.old-2.exe` and on when the earlier one runs as well.
pub(crate) fn aside(stem: &str, nth: u32) -> String {
    if nth <= 1 {
        format!("{stem}.old.exe")
    } else {
        format!("{stem}.old-{nth}.exe")
    }
}

/// Whether `name` is one `aside` gives `<stem>.exe`.
pub(crate) fn is_aside(stem: &str, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let Some(rest) = name
        .strip_prefix(&stem.to_ascii_lowercase())
        .and_then(|rest| rest.strip_prefix(".old"))
        .and_then(|rest| rest.strip_suffix(".exe"))
    else {
        return false;
    };
    match rest.strip_prefix('-') {
        None => rest.is_empty(),
        Some(nth) => {
            !nth.starts_with('0')
                && !nth.is_empty()
                && nth.bytes().all(|b| b.is_ascii_digit())
                && nth
                    .parse::<u32>()
                    .is_ok_and(|nth| (2..=ASIDE_LIMIT).contains(&nth))
        }
    }
}

/// Renames `from` to `to`, in place of a file of that name, in one step that
/// reaches the disk before it returns. A scanner that has just opened a new
/// file, or the one it replaces, may hold it for a moment: refused access is
/// retried for up to a second.
pub(crate) fn rename(from: &Path, to: &Path) -> Result<()> {
    let (from, to) = (wide(&from.to_string_lossy()), wide(&to.to_string_lossy()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        let result = unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(win);
        match result {
            Err(5 | 32) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            result => return result,
        }
    }
}

/// Deletes `path`; one already gone is fine.
pub(crate) fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(io(error)),
        _ => Ok(()),
    }
}

/// What `swap_in` replaced, under the name it keeps until it has exited.
#[derive(Debug)]
pub(crate) struct Swapped {
    pub aside: Option<PathBuf>,
}

impl Swapped {
    /// Puts back the file that `target` named before.
    pub(crate) fn undo(&self, target: &Path) -> Result<()> {
        match &self.aside {
            Some(aside) => rename(aside, target),
            None => remove(target),
        }
    }

    /// Deletes the replaced file's other name, unless it still runs: a later
    /// sweep deletes it then.
    pub(crate) fn release(self) {
        if let Some(aside) = self.aside {
            let _ = fs::remove_file(aside);
        }
    }
}

/// Moves the whole file `staged` to `target` (`<stem>.exe`), which may be a
/// program that is running. A file `target` names keeps an `aside` name
/// first, which `Swapped` undoes or releases.
pub(crate) fn swap_in(staged: &Path, target: &Path, stem: &str) -> Result<Swapped> {
    let dir = target.parent().ok_or(87u32)?;
    let aside = match fs::symlink_metadata(target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(io(error)),
        Ok(_) => Some(link_aside(dir, target, stem)?),
    };
    if let Err(code) = rename(staged, target) {
        if let Some(aside) = &aside {
            let _ = fs::remove_file(aside);
        }
        return Err(code);
    }
    Ok(Swapped { aside })
}

/// Gives the file `target` names a free `aside` name as well. One taken by a
/// program that has exited is deleted first; one still running is skipped.
fn link_aside(dir: &Path, target: &Path, stem: &str) -> Result<PathBuf> {
    let mut last = 32;
    for nth in 1..=ASIDE_LIMIT {
        let path = dir.join(aside(stem, nth));
        if let Err(code) = remove(&path) {
            last = code;
            continue;
        }
        match fs::hard_link(target, &path) {
            Ok(()) => return Ok(path),
            // Taken meanwhile: try the next one.
            Err(error) if error.raw_os_error() == Some(183) => last = 183,
            Err(error) => return Err(io(error)),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("argus-layout-{}", crate::taskmgr::id().unwrap()));
            fs::create_dir(&dir).unwrap();
            Self(dir)
        }
        fn names(&self) -> Vec<String> {
            let mut names: Vec<_> = fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A copy of a system console program in `dir`, started and waiting: a
    /// program that is running, as an interface copy is during an update.
    /// Its image is mapped by the time `spawn` returns.
    fn running(dir: &Path, name: &str) -> (PathBuf, Child) {
        let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        let path = dir.join(name);
        fs::copy(system.join("PING.EXE"), &path).unwrap();
        let child = Command::new(&path)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap();
        (path, child)
    }

    #[test]
    fn aside_names_are_numbered_and_recognized() {
        assert_eq!(aside("argus-taskmgr-ui", 1), "argus-taskmgr-ui.old.exe");
        assert_eq!(aside("argus-taskmgr-ui", 3), "argus-taskmgr-ui.old-3.exe");
        for nth in 1..=ASIDE_LIMIT {
            assert!(is_aside("argus-admin", &aside("argus-admin", nth)));
        }
        assert!(is_aside("argus-admin", "ARGUS-ADMIN.OLD.EXE"));
        for name in [
            "argus-admin.exe",
            "argus-admin.old",
            "argus-admin.old-1.exe",
            "argus-admin.old-02.exe",
            "argus-admin.old-10.exe",
            "argus-admin.old-x.exe",
            "argus-admin.old.exe.tmp",
            "argus-service.old.exe",
            "other-argus-admin.old.exe",
        ] {
            assert!(!is_aside("argus-admin", name), "{name}");
        }
    }

    #[test]
    fn a_file_that_is_not_running_is_replaced_and_its_aside_released() {
        let scratch = Scratch::new();
        let (target, staged) = (scratch.0.join("app.exe"), scratch.0.join("app.new"));
        // Nothing in place yet: nothing to keep aside.
        fs::write(&staged, b"one").unwrap();
        let swapped = swap_in(&staged, &target, "app").unwrap();
        assert!(swapped.aside.is_none());
        swapped.release();
        assert_eq!(scratch.names(), ["app.exe"]);

        fs::write(&staged, b"two").unwrap();
        let swapped = swap_in(&staged, &target, "app").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"two");
        let aside = swapped.aside.clone().unwrap();
        assert_eq!(fs::read(&aside).unwrap(), b"one");
        swapped.release();
        assert_eq!(scratch.names(), ["app.exe"]);

        // Undone: the earlier file is back under its name.
        fs::write(&staged, b"three").unwrap();
        let swapped = swap_in(&staged, &target, "app").unwrap();
        swapped.undo(&target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"two");
        assert_eq!(scratch.names(), ["app.exe"]);
    }

    #[test]
    fn a_running_program_is_replaced_and_keeps_its_aside_until_it_exits() {
        let scratch = Scratch::new();
        let (target, mut child) = running(&scratch.0, "app.exe");
        let old = fs::read(&target).unwrap();
        // What a plain replacement meets, and why the aside is there.
        let staged = scratch.0.join("app.new");
        fs::write(&staged, b"new").unwrap();
        assert_eq!(rename(&staged, &target), Err(5));

        let swapped = swap_in(&staged, &target, "app").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        let aside = swapped.aside.clone().unwrap();
        assert_eq!(aside, scratch.0.join("app.old.exe"));
        assert_eq!(fs::read(&aside).unwrap(), old);
        swapped.release();
        assert!(aside.exists(), "the running program's file stays");

        // Another update meanwhile: the running aside is skipped.
        fs::write(&staged, b"newer").unwrap();
        let swapped = swap_in(&staged, &target, "app").unwrap();
        assert_eq!(
            swapped.aside.as_deref(),
            Some(&*scratch.0.join("app.old-2.exe"))
        );
        swapped.release();
        assert_eq!(scratch.names(), ["app.exe", "app.old.exe"]);

        // Undoing while the replaced program runs brings it back in place.
        fs::write(&staged, b"newest").unwrap();
        let swapped = swap_in(&staged, &target, "app").unwrap();
        swapped.undo(&target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"newer");

        assert!(fs::remove_file(&aside).is_err());
        child.kill().unwrap();
        child.wait().unwrap();
        fs::remove_file(&aside).unwrap();
        assert_eq!(scratch.names(), ["app.exe"]);
    }

    #[test]
    fn a_failed_move_leaves_the_target_as_it_was() {
        let scratch = Scratch::new();
        let target = scratch.0.join("app.exe");
        fs::write(&target, b"old").unwrap();
        let missing = scratch.0.join("missing.new");
        assert!(swap_in(&missing, &target, "app").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert_eq!(scratch.names(), ["app.exe"]);
    }
}
