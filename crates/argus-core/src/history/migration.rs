use super::directory::{MARKER, mark, pin_marker};
use super::maintenance::rename_file;
use super::*;
use windows::Win32::Storage::FileSystem::MOVE_FILE_FLAGS;

const STAGING: &str = ".argus-history-copy";
pub(super) const PUBLISH: &str = ".argus-history-publish";

pub(super) fn recover(dir: &Path) -> io::Result<()> {
    let staging = dir.join(STAGING);
    if staging.try_exists()? || dir.join(PUBLISH).try_exists()? {
        let _pin = pin_marker(dir, true)?;
        if staging.try_exists()? {
            empty_stage(&staging)?;
        }
        publish(dir, |_| ())?;
    }
    Ok(())
}

fn record_name(name: &OsStr) -> bool {
    owned_day(name).is_some() || name == crate::watch::FILE_NAME
}

fn record_names(dir: &Path) -> io::Result<Vec<std::ffi::OsString>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if record_name(&entry.file_name()) {
            if !entry.file_type()?.is_file() {
                return Err(invalid());
            }
            names.push(entry.file_name());
        }
    }
    names.sort();
    Ok(names)
}

// A private subfolder holds only copies. A killed copy can be discarded up
// to the atomic rename to PUBLISH; after it, every remaining file is complete.
fn finish_stage(
    path: &Path,
    mut finish_file: impl FnMut(&OsStr) -> io::Result<()>,
) -> io::Result<()> {
    if !fs::symlink_metadata(path)?.file_type().is_dir() {
        return Err(invalid());
    }
    // Either mkdir or removal of the marker may have been the final operation.
    if fs::read_dir(path)?.next().transpose()?.is_some() {
        let pin = pin_marker(path, true)?;
        for name in record_names(path)? {
            finish_file(&name)?;
        }
        drop(pin);
        fs::remove_file(path.join(MARKER))?;
    }
    fs::remove_dir(path)
}

fn empty_stage(path: &Path) -> io::Result<()> {
    finish_stage(path, |name| fs::remove_file(path.join(name)))
}

fn publish(dest: &Path, mut after_file: impl FnMut(&OsStr)) -> io::Result<()> {
    let stage = dest.join(PUBLISH);
    if !stage.try_exists()? {
        return Ok(());
    }
    finish_stage(&stage, |name| {
        rename_file(&stage.join(name), &dest.join(name), MOVE_FILE_FLAGS(0))?;
        after_file(name);
        Ok(())
    })
}

pub fn move_records<T>(
    source: &Path,
    destination: &Path,
    save: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    move_with(source, destination, save, |_| ())
}

pub(super) fn move_with<T>(
    source: &Path,
    destination: &Path,
    save: impl FnOnce() -> io::Result<T>,
    after_file: impl FnMut(&OsStr),
) -> io::Result<T> {
    let _dest = pin_marker(destination, true)?;
    if destination.join(PUBLISH).try_exists()? {
        return Err(invalid());
    }
    if !source.try_exists()? {
        return save();
    }
    let _source = pin_marker(source, true)?;
    if source.join(PUBLISH).try_exists()? {
        return Err(invalid());
    }
    let names = record_names(source)?;
    let existing = record_names(destination)?;
    let target_days: std::collections::BTreeSet<_> =
        existing.iter().filter_map(|n| owned_day(n)).collect();
    for name in &names {
        if destination.join(name).try_exists()?
            || owned_day(name).is_some_and(|day| target_days.contains(&day))
        {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
    }
    let stage = destination.join(STAGING);
    if stage.try_exists()? {
        empty_stage(&stage)?;
    }
    fs::create_dir(&stage)?;
    mark(&stage)?;
    let prepared = (|| -> io::Result<()> {
        for name in &names {
            let mut input = fs::File::open(source.join(name))?;
            let len = input.metadata()?.len();
            let mut output = fs::File::create_new(stage.join(name))?;
            if io::copy(&mut input, &mut output)? != len || output.metadata()?.len() != len {
                return Err(invalid());
            }
            output.sync_all()?;
        }
        rename_file(&stage, &destination.join(PUBLISH), MOVE_FILE_FLAGS(0))?;
        Ok(())
    })();
    if let Err(e) = prepared {
        let _ = empty_stage(&stage);
        return Err(e);
    }
    publish(destination, after_file)?;
    forget(destination);
    let result = save();
    // Both locations are complete at the settings commit. A crash during
    // cleanup leaves duplicates on the inactive side only.
    let redundant = if result.is_ok() { source } else { destination };
    for name in names {
        let path = redundant.join(name);
        if let Err(e) = fs::remove_file(&path) {
            crate::diagnostics::file_failure("history move cleanup", &path, &e);
        }
    }
    forget(source);
    result
}
