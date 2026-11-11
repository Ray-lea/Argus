//! Bounded diagnostics for the native host; only reviewed Argus records are kept.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use time::{OffsetDateTime, UtcOffset};

fn rotate(file: &mut File, path: &Path) -> std::io::Result<()> {
    file.flush()?;
    for index in (1..3).rev() {
        let source = path.with_extension(format!("{index}.log"));
        if source.exists() {
            std::fs::copy(source, path.with_extension(format!("{}.log", index + 1)))?;
        }
    }
    // Preserve the current records before truncating, including format upgrades.
    std::fs::copy(path, path.with_extension("1.log"))?;
    *file = File::create(path)?;
    Ok(())
}

fn open_current_log(path: &Path) -> std::io::Result<(File, Option<std::io::Error>)> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut archive_error = None;
    let mut prefix = Vec::new();
    if let Ok(reader) = File::open(path)
        && reader.take(64).read_to_end(&mut prefix).is_ok()
        && !prefix.is_empty()
    {
        let end = prefix
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(prefix.len());
        let current = std::str::from_utf8(&prefix[..end])
            .ok()
            .is_some_and(|value| {
                OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).is_ok()
            });
        if !current {
            archive_error = rotate(&mut file, path).err();
        }
    }
    Ok((file, archive_error))
}

fn timestamp(now: OffsetDateTime) -> String {
    let offset = now.offset().whole_seconds();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}{}{:02}:{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond(),
        if offset < 0 { '-' } else { '+' },
        offset.unsigned_abs() / 3600,
        offset.unsigned_abs() / 60 % 60,
    )
}

struct Logger {
    file: Mutex<File>,
    path: PathBuf,
    startup_trace: bool,
}
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.target().starts_with("argus")
            && metadata.level() <= log::Level::Info
            && (metadata.target() != "argus_startup" || self.startup_trace)
    }
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut file = self.file.lock().unwrap();
        if file.metadata().is_ok_and(|m| m.len() > 1024 * 1024) {
            let _ = rotate(&mut file, &self.path);
        }
        let now = OffsetDateTime::now_utc();
        // Resolve the offset for each record so a long-running host follows
        // daylight-saving and time-zone changes without ambiguous local times.
        let offset = UtcOffset::local_offset_at(now).unwrap_or(UtcOffset::UTC);
        let now = timestamp(now.to_offset(offset));
        let written = writeln!(file, "{now} {} {}", record.level(), record.args());
        if self.startup_trace {
            // A process flush alone leaves records in the Windows file cache.
            // Stop before the next driver call if a diagnostic record fails.
            if written.and_then(|_| file.sync_data()).is_err() {
                drop(file);
                eprintln!("Startup diagnostic log could not be persisted");
                std::process::exit(74);
            }
        } else {
            let _ = file.flush();
        }
    }
    fn flush(&self) {
        let _ = self.file.lock().unwrap().flush();
    }
}
pub fn init(directory: PathBuf, startup_trace: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("argus.log");
    let (file, archive_error) = open_current_log(&path)?;
    log::set_boxed_logger(Box::new(Logger {
        file: Mutex::new(file),
        path: path.clone(),
        startup_trace,
    }))?;
    log::set_max_level(log::LevelFilter::Info);
    if let Some(error) = archive_error {
        argus_core::diagnostics::file_failure("diagnostic log format archive", &path, &error);
    }
    std::panic::set_hook(Box::new(|panic| {
        if let Some(at) = panic.location() {
            log::error!(
                "panic at {}:{}",
                argus_core::diagnostics::source_file(at.file()),
                at.line()
            );
        } else {
            log::error!("panic (payload omitted)");
        }
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::format_description::well_known::Rfc3339;

    #[test]
    fn legacy_formats_are_archived_once_without_rewriting_records() {
        let directory = std::env::temp_dir().join(format!(
            "argus-log-format-{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("argus.log");
        let backup = path.with_extension("1.log");
        for previous in [
            "1793536516 INFO previous\n",
            "[2026-10-19][10:19:57][INFO] previous\n",
        ] {
            std::fs::write(&path, previous).unwrap();
            let (mut file, error) = open_current_log(&path).unwrap();
            assert!(error.is_none());
            assert_eq!(std::fs::read_to_string(&backup).unwrap(), previous);
            assert_eq!(file.metadata().unwrap().len(), 0);
            let current = "2026-11-01T04:39:36.824-08:00 INFO current\n";
            file.write_all(current.as_bytes()).unwrap();
            drop(file);
            let (file, error) = open_current_log(&path).unwrap();
            assert!(error.is_none());
            drop(file);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), current);
            assert_eq!(std::fs::read_to_string(&backup).unwrap(), previous);
            std::fs::remove_file(&backup).unwrap();
        }
        // An archive destination error preserves the log and allows startup.
        std::fs::write(&path, "1793536516 INFO previous\n").unwrap();
        std::fs::create_dir(&backup).unwrap();
        let (mut file, error) = open_current_log(&path).unwrap();
        assert!(error.is_some());
        file.write_all(b"current\n").unwrap();
        drop(file);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "1793536516 INFO previous\ncurrent\n"
        );
        std::fs::remove_dir(&backup).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn timestamps_keep_milliseconds_and_unambiguous_offsets() {
        let utc = OffsetDateTime::parse("2026-11-01T02:03:04.005Z", &Rfc3339).unwrap();
        for (offset, expected) in [
            (UtcOffset::UTC, "2026-11-01T02:03:04.005+00:00"),
            (
                UtcOffset::from_hms(8, 0, 0).unwrap(),
                "2026-11-01T10:03:04.005+08:00",
            ),
            (
                UtcOffset::from_hms(-8, 0, 0).unwrap(),
                "2026-10-31T18:03:04.005-08:00",
            ),
            (
                UtcOffset::from_hms(5, 45, 0).unwrap(),
                "2026-11-01T07:48:04.005+05:45",
            ),
            (
                UtcOffset::from_hms(0, -30, 0).unwrap(),
                "2026-11-01T01:33:04.005-00:30",
            ),
        ] {
            let formatted = timestamp(utc.to_offset(offset));
            assert_eq!(formatted, expected);
            assert_eq!(OffsetDateTime::parse(&formatted, &Rfc3339).unwrap(), utc);
        }
    }

    #[test]
    fn repeated_local_hour_remains_distinct_across_daylight_saving() {
        let before = OffsetDateTime::parse("2026-11-01T01:30:00.000-07:00", &Rfc3339).unwrap();
        let after = OffsetDateTime::parse("2026-11-01T01:30:00.000-08:00", &Rfc3339).unwrap();
        assert_ne!(timestamp(before), timestamp(after));
        assert_eq!(after - before, time::Duration::hours(1));
    }
}
