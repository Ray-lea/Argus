use super::*;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
use windows::core::PCWSTR;

/// Serializes publication/removal with queries that open a primary and its
/// sidecars separately. The recording thread never takes this lock.
pub(super) static PUBLICATION: std::sync::RwLock<()> = std::sync::RwLock::new(());

// DELETE_ON_CLOSE releases the cross-instance lease on normal exit or a killed
// worker. Exclusive sharing prevents another build from removing its scratch
// primary between flush and publication.
pub(super) fn directory_lease(dir: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::FILE_FLAG_DELETE_ON_CLOSE;
    let path = dir.join(".argus-history-maintenance.lock");
    if let Ok(existing) = fs::symlink_metadata(&path)
        && (!existing.file_type().is_file() || existing.len() != 0)
    {
        return Err(invalid());
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE.0)
        .open(path)
}
pub(super) fn today() -> i64 {
    unix_ms().div_euclid(DAY_MS)
}

/// Retention includes observation storage in the byte budget. Only complete
/// day groups are eviction candidates. Temporary merge outputs are recoverable.
pub fn maintain(dir: &Path, options: HistoryOptions, active_day: i64) -> io::Result<()> {
    maintain_until(dir, options, active_day, || false)
}

pub fn maintain_until(
    dir: &Path,
    options: HistoryOptions,
    active: i64,
    cancelled: impl Fn() -> bool,
) -> io::Result<()> {
    maintain_at(dir, options, today(), active, cancelled)
}
pub(super) fn maintain_at(
    dir: &Path,
    options: HistoryOptions,
    today: i64,
    active: i64,
    cancelled: impl Fn() -> bool,
) -> io::Result<()> {
    let mut groups = BTreeMap::<i64, Vec<(PathBuf, u64)>>::new();
    let _lease = match directory_lease(dir) {
        Ok(lease) => lease,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let age = match options.detail {
        HistoryDetail::Full => None,
        HistoryDetail::Minute7 => Some(7),
        HistoryDetail::Minute30 => Some(30),
    };
    let mut candidates = Vec::new();
    // Reclaim unpublished output and superseded sidecars before accounting.
    // A crash must not make an otherwise in-budget day an eviction candidate.
    for day in days(dir) {
        if cancelled() {
            return Ok(());
        }
        if day == active || day >= today.saturating_sub(1) {
            continue;
        }
        let path = day_path(dir, day);
        remove_if_exists(&path.with_extension("merge-tmp"))?;
        match minute::is_merged(&path) {
            Ok(true) => remove_sidecars(&path)?,
            Ok(false)
                if age.is_some_and(|n| day < today.saturating_sub(n))
                    && minute::is_current(&path) =>
            {
                candidates.push(day)
            }
            Ok(false) => (),
            Err(e) => crate::diagnostics::file_failure("history minute recovery", &path, &e),
        }
    }
    let entries = fs::read_dir(dir)?;
    let mut size = crate::watch::WatchRecorder::storage_bytes(dir);
    for entry in entries {
        let entry = entry?;
        if let Some(day) = owned_day(&entry.file_name())
            && entry.file_type()?.is_file()
        {
            let len = entry.metadata()?.len();
            size = size.saturating_add(len);
            groups.entry(day).or_default().push((entry.path(), len));
        }
    }
    for (&day, files) in &groups {
        if cancelled() {
            return Ok(());
        }
        if day == active || day == today {
            continue;
        }
        let expired = options
            .limit
            .days
            .is_some_and(|n| day <= today.saturating_sub(i64::from(n)));
        if expired || options.limit.bytes.is_some_and(|limit| size > limit) {
            let _publish = PUBLICATION.write().unwrap_or_else(|e| e.into_inner());
            // Remove the primary first. A live writer denies deletion of its primary.
            let primary = day_path(dir, day);
            let ordered = files
                .iter()
                .filter(|f| f.0 == primary)
                .chain(files.iter().filter(|f| f.0 != primary));
            for (file, len) in ordered {
                fs::remove_file(file)?;
                size = size.saturating_sub(*len);
            }
            forget(dir);
        }
    }
    for day in candidates {
        if cancelled() {
            return Ok(());
        }
        let path = day_path(dir, day);
        if path.try_exists()?
            && let Err(e) = minute::merge(dir, day, options.recording.processes)
        {
            crate::diagnostics::file_failure("history minute merge", &path, &e);
        }
    }
    Ok(())
}

pub(super) fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
pub(super) fn remove_sidecars(path: &Path) -> io::Result<()> {
    for ext in ["proc", "idx", "roll"] {
        remove_if_exists(&path.with_extension(ext))?;
    }
    Ok(())
}

// Resolve existing junctions even when the destination itself does not exist.
fn resolved(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    let absolute = std::path::absolute(path)?;
    let mut missing = Vec::new();
    let mut base = absolute.as_path();
    let mut real = loop {
        match base.canonicalize() {
            Ok(real) => break real,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                missing.push(base.file_name().ok_or_else(invalid)?);
                base = base.parent().ok_or_else(invalid)?;
            }
            Err(e) => return Err(e),
        }
    };
    for part in missing.iter().rev() {
        real.push(part);
    }
    Ok(real)
}
pub fn validate_directory(current: &Path, destination: &Path) -> io::Result<()> {
    let resolve = |p: &Path| -> io::Result<PathBuf> {
        Ok(PathBuf::from(
            resolved(p)?.to_str().ok_or_else(invalid)?.to_lowercase(),
        ))
    };
    let a = resolve(current)?;
    let b = resolve(destination)?;
    if a.starts_with(&b) || b.starts_with(&a) {
        return Err(invalid());
    }
    fs::create_dir_all(destination)?;
    let id = unsafe { windows::Win32::System::Com::CoCreateGuid() }.map_err(io::Error::other)?;
    let probe = destination.join(format!(".argus-write-{id:?}"));
    let mut file = fs::File::create_new(&probe)?;
    let result = file.write_all(b"Argus").and_then(|()| file.sync_all());
    drop(file);
    let cleanup = fs::remove_file(probe);
    result.and(cleanup)
}

pub(super) fn rename_file(
    source: &Path,
    destination: &Path,
    flags: MOVE_FILE_FLAGS,
) -> windows::core::Result<()> {
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let a = wide(source);
    let b = wide(destination);
    unsafe { MoveFileExW(PCWSTR(a.as_ptr()), PCWSTR(b.as_ptr()), flags) }
}
fn transfer(source: &Path, destination: &Path) -> io::Result<()> {
    // No REPLACE_EXISTING or COPY_ALLOWED: a collision never overwrites data,
    // and a cross-volume move goes through the verified copy below.
    let renamed = rename_file(source, destination, MOVE_FILE_FLAGS(0));
    if let Err(err) = renamed {
        if err.code()
            != windows::core::HRESULT::from_win32(
                windows::Win32::Foundation::ERROR_NOT_SAME_DEVICE.0,
            )
        {
            return Err(err.into());
        }
    } else {
        return Ok(());
    }

    let mut input = fs::File::open(source)?;
    let size = input.metadata()?.len();
    let mut out = fs::File::create_new(destination)?;
    let copied = (|| {
        if io::copy(&mut input, &mut out)? != size || out.metadata()?.len() != size {
            return Err(invalid());
        }
        out.sync_all()
    })();
    drop((out, input));
    let result = copied.and_then(|()| fs::remove_file(source));
    if result.is_err() {
        remove_if_exists(destination)?;
    }
    result
}

/// The caller has closed both recorder handles. Include the settings write
/// in the transaction so a persistence failure also restores the old files.
pub fn move_records<T>(
    source: &Path,
    destination: &Path,
    save: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let _source_lease = if source.try_exists()? {
        Some(directory_lease(source)?)
    } else {
        None
    };
    let _destination_lease = directory_lease(destination)?;
    let mut moved = Vec::new();
    let result = (|| {
        let mut names = Vec::new();
        match fs::read_dir(source) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            entries => {
                for entry in entries? {
                    let entry = entry?;
                    let name = entry.file_name();
                    if owned_day(&name).is_some()
                        || matches!(
                            name.to_str(),
                            Some("process-watch.jsonl" | "program-watch.v1.bin")
                        )
                    {
                        if !entry.file_type()?.is_file() {
                            return Err(invalid());
                        }
                        names.push(name);
                    }
                }
            }
        }
        names.sort();
        for name in names {
            transfer(&source.join(&name), &destination.join(&name))?;
            moved.push(name);
        }
        save()
    })();
    if result.is_err() {
        for name in moved.into_iter().rev() {
            let original = source.join(&name);
            if let Err(e) = transfer(&destination.join(&name), &original) {
                crate::diagnostics::file_failure("history move rollback", &original, &e);
            }
        }
    }
    forget(source);
    forget(destination);
    result
}
