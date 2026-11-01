//! Bounded diagnostics for the native host; only reviewed Argus records are kept.
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
};
use time::{OffsetDateTime, UtcOffset};

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
}
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.target().starts_with("argus") && metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut file = self.file.lock().unwrap();
        if file.metadata().is_ok_and(|m| m.len() > 1024 * 1024) {
            let _ = file.flush();
            for index in (1..3).rev() {
                let _ = std::fs::copy(
                    self.path.with_extension(format!("{index}.log")),
                    self.path.with_extension(format!("{}.log", index + 1)),
                );
            }
            let _ = std::fs::copy(&self.path, self.path.with_extension("1.log"));
            if let Ok(next) = File::create(&self.path) {
                *file = next;
            }
        }
        let now = OffsetDateTime::now_utc();
        // Resolve the offset for each record so a long-running host follows
        // daylight-saving and time-zone changes without ambiguous local times.
        let offset = UtcOffset::local_offset_at(now).unwrap_or(UtcOffset::UTC);
        let now = timestamp(now.to_offset(offset));
        let _ = writeln!(file, "{now} {} {}", record.level(), record.args());
        let _ = file.flush();
    }
    fn flush(&self) {
        let _ = self.file.lock().unwrap().flush();
    }
}
pub fn init(directory: PathBuf) -> anyhow::Result<()> {
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("argus.log");
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    log::set_boxed_logger(Box::new(Logger {
        file: Mutex::new(file),
        path,
    }))?;
    log::set_max_level(log::LevelFilter::Info);
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
