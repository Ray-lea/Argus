//! Bounded diagnostics for the native host; only reviewed Argus records are kept.
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
};
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
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
