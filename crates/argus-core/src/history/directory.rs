use super::*;

pub(super) const MARKER: &str = ".argus-history";

/// Keep the marker pinned against removal while opening destructive writers.
pub(crate) fn owned_directory(dir: &Path) -> io::Result<fs::File> {
    let marker = pin_marker(dir, false)?;
    if dir.join(migration::PUBLISH).try_exists()? {
        return Err(invalid());
    }
    Ok(marker)
}
pub(super) fn pin_marker(dir: &Path, exclusive: bool) -> io::Result<fs::File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let marker = OpenOptions::new()
        .read(true)
        .share_mode(if exclusive { 0 } else { 1 })
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(dir.join(MARKER))?;
    let meta = marker.metadata()?;
    if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(invalid());
    }
    Ok(marker)
}

pub(super) fn mark(dir: &Path) -> io::Result<()> {
    match fs::File::create_new(dir.join(MARKER)) {
        Ok(file) => file.sync_all(),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => pin_marker(dir, false).map(drop),
        Err(e) => Err(e),
    }
}

/// A nonempty custom folder keeps its other files outside Argus storage.
pub fn prepare_directory(default: &Path, selected: Option<&Path>) -> io::Result<PathBuf> {
    let actual = choose_directory(default, selected)?;
    migration::recover(&actual)?;
    Ok(actual)
}
fn choose_directory(default: &Path, selected: Option<&Path>) -> io::Result<PathBuf> {
    let dir = selected.unwrap_or(default);
    if !dir.is_absolute() {
        return Err(invalid());
    }
    fs::create_dir_all(dir)?;
    if dir == default {
        mark(dir)?;
        return Ok(dir.into());
    }
    if marked(dir)? {
        return Ok(dir.into());
    }
    let actual = if fs::read_dir(dir)?.next().transpose()?.is_none() {
        dir.into()
    } else {
        let child = dir.join("Argus History");
        match fs::create_dir(&child) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if !fs::symlink_metadata(&child)?.file_type().is_dir() {
                    return Err(invalid());
                }
                if marked(&child)? {
                    return Ok(child);
                }
                if fs::read_dir(&child)?.next().transpose()?.is_some() {
                    return Err(invalid());
                }
            }
            Err(e) => return Err(e),
        }
        child
    };
    mark(&actual)?;
    Ok(actual)
}

fn marked(dir: &Path) -> io::Result<bool> {
    match pin_marker(dir, false) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

pub fn select_directory(
    current: &Path,
    default: &Path,
    selected: Option<&Path>,
) -> io::Result<PathBuf> {
    distinct(current, selected.unwrap_or(default))?;
    let actual = prepare_directory(default, selected)?;
    validate_directory(current, &actual)?;
    Ok(actual)
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
fn distinct(current: &Path, destination: &Path) -> io::Result<()> {
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
    Ok(())
}

pub fn validate_directory(current: &Path, destination: &Path) -> io::Result<()> {
    distinct(current, destination)?;
    let _owner = owned_directory(destination)?;
    let id = unsafe { windows::Win32::System::Com::CoCreateGuid() }.map_err(io::Error::other)?;
    let probe = destination.join(format!(".argus-write-{id:?}"));
    let mut file = fs::File::create_new(&probe)?;
    let result = file.write_all(b"Argus").and_then(|()| file.sync_all());
    drop(file);
    let cleanup = fs::remove_file(probe);
    result.and(cleanup)
}
