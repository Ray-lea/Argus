//! Where the day files are: their names, the days a folder holds, opening
//! and reading them within bounds, and clearing them.
use super::*;

pub(super) fn day_path(dir: &Path, day: i64) -> PathBuf {
    dir.join(format!("{}.hist", day_name(day)))
}

/// `YYYY-MM-DD` of a day number (days since the Unix epoch), proleptic Gregorian.
fn day_name(day: i64) -> String {
    // Howard Hinnant's civil_from_days.
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Day number of a `YYYY-MM-DD` name (the inverse of `day_name`).
/// The day a file in the history folder holds, if it is a day file.
pub(super) fn day_of(file_name: &OsStr) -> Option<i64> {
    day_number(file_name.to_str()?.strip_suffix(".hist")?)
}

pub(super) fn owned_day(name: &OsStr) -> Option<i64> {
    let text = name.to_str()?;
    day_of(name).or_else(|| {
        day_number(
            text.strip_suffix(".proc")
                .or_else(|| text.strip_suffix(".roll"))
                .or_else(|| text.strip_suffix(".idx"))
                .or_else(|| text.strip_suffix(".measure"))
                .or_else(|| text.strip_suffix(".merge-tmp"))?,
        )
    })
}

pub(super) fn day_number(name: &str) -> Option<i64> {
    if name.len() != 10
        || name.as_bytes()[4] != b'-'
        || name.as_bytes()[7] != b'-'
        || !name
            .bytes()
            .enumerate()
            .all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit())
    {
        return None;
    }
    let mut it = name.split('-');
    let (y, m, d): (i64, i64, i64) = (
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    );
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let day = era * 146_097 + doe - 719_468;
    (day_name(day) == name).then_some(day)
}

/// Deletes every day file in the folder. Nothing may be recording into it
/// (`HistoryRecorder::clear` closes its file first).
pub fn clear(dir: &Path) -> io::Result<()> {
    forget(dir);
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let _lease = maintenance::directory_lease(dir)?;
    for entry in entries {
        let entry = entry?;
        if owned_day(&entry.file_name()).is_some() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Day files in the folder, oldest first.
pub(super) fn days(dir: &Path) -> Vec<i64> {
    #[cfg(test)]
    if let Some(day) = test_files::day(dir) {
        return vec![day];
    }
    let mut days: Vec<i64> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| day_of(&e.file_name()))
        .collect();
    days.sort_unstable();
    days
}

pub(super) fn open_writable(path: &Path) -> io::Result<File> {
    #[cfg(test)]
    if let Some(file) = test_files::open(path, true) {
        return file;
    }
    use std::os::windows::fs::OpenOptionsExt;
    let file = OpenOptions::new()
        .share_mode(1) // One writer per file. Readers remain available throughout recording.
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    #[cfg(test)]
    let file = File::Disk(file);
    Ok(file)
}

pub(super) fn history_read<T>(path: &Path, result: io::Result<T>) -> Option<T> {
    result
        .inspect_err(|error| {
            if !matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::UnexpectedEof
            ) {
                crate::diagnostics::file_failure("history read", path, error);
            }
        })
        .ok()
}

pub(super) fn read_bounded(file: &mut File) -> io::Result<Vec<u8>> {
    let len = file.metadata()?.len();
    if len > MAX_FILE_BYTES as u64 {
        return Err(invalid());
    }
    let mut bytes = vec![0; len as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}
pub(super) fn read_slice(path: &Path, offset: u64, len: usize) -> Option<Vec<u8>> {
    if len > MAX_FILE_BYTES {
        return None;
    }
    let mut f = history_read(path, File::open(path))?;
    if offset.checked_add(len as u64)? > f.metadata().ok()?.len() {
        return None;
    }
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = vec![0; len];
    history_read(path, f.read_exact(&mut bytes))?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{proc, snapshot, temp_dir};

    #[test]
    fn day_names_round_trip() {
        for day in [0, 19_000, 20_737, -1, 11_016] {
            assert_eq!(day_number(&day_name(day)), Some(day));
        }
        assert_eq!(day_name(20_737), "2026-10-11");
    }

    #[test]
    fn clears_and_records_afresh() {
        let dir = temp_dir("clear");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..11 {
            rec.record(&snapshot(
                base + i * 1000,
                5.0,
                vec![proc(1, "a.exe", 1.0, 1 << 20)],
            ));
        }
        assert!(usage(&dir).bytes > 0);
        rec.clear().unwrap();
        assert!(days(&dir).is_empty());
        let cleared = usage(&dir);
        assert_eq!((cleared.days, cleared.bytes, cleared.first), (0, 0, None));
        assert!(cleared.estimate.bytes_per_day > 0);
        for i in 20..31 {
            rec.record(&snapshot(
                base + i * 1000,
                6.0,
                vec![proc(2, "b.exe", 1.0, 1 << 20)],
            ));
        }
        drop(rec);
        // A new file with its own name table.
        let m = moment(&dir, base + 20_000).unwrap();
        assert_eq!(m.processes[0].name, "b.exe");
        assert_eq!(usage(&dir).first, Some(base + 20_000));
        clear(&dir).unwrap();
        assert!(days(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
