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
pub(super) fn directory_lease(dir: &Path) -> io::Result<(fs::File, fs::File)> {
    Ok((owned_directory(dir)?, lock_directory(dir)?))
}
fn lock_directory(dir: &Path) -> io::Result<fs::File> {
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
pub fn maintain_at(
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
    if let Err(e) = crate::watch::WatchRecorder::prune_saved(dir) {
        crate::diagnostics::file_failure(
            "observation retention",
            &dir.join(crate::watch::FILE_NAME),
            &e,
        );
    }
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
