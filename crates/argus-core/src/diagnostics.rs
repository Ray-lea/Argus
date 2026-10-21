//! Public diagnostic logs accept classifications, never free-form errors.
//! Redacting a few path patterns cannot protect arbitrary exception payloads.

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Numeric failures must be recorded before they become localized UI text.
pub fn code(value: u32) -> String {
    let name = match value {
        0x2000_0001 => "unsafe path",
        0x2000_0002 => "different service owner",
        0x2000_0003 => "incompatible protocol",
        0x2000_0004 => "foreign service",
        0x2000_0005 => "task manager replacement active",
        0x2000_0006 => "task manager replacement conflict",
        0x2000_0007 => "task manager replacement update failed",
        0x2000_0101 => "driver download failed",
        0x2000_0102 => "driver package invalid",
        0x2000_0103 => "driver signature invalid",
        0x2000_0104 => "foreign driver",
        0x2000_0105 => "driver not ready",
        0x2000_0106 => "unsafe driver path",
        0x2000_0107 => "writable driver store",
        0x2000_0108 => "unsafe installer staging path",
        0x2000_0109 => "unsupported legacy installation layout",
        0x2000_010a => "driver device disabled",
        3010 | 1641 => "restart required",
        _ => category(value).unwrap_or("native failure"),
    };
    format!("{name}; code={value} (0x{value:08x})")
}

pub fn io(error: &std::io::Error) -> String {
    format!(
        "kind={:?}; {}",
        error.kind(),
        error
            .raw_os_error()
            .map(|value| code(value as u32))
            .unwrap_or_else(|| "native code unavailable".into())
    )
}

#[track_caller]
pub fn windows_failure(event: &'static str, error: &windows::core::Error) {
    warn(event, code(error.code().0 as u32));
}

pub fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut current = Some(error);
    let mut causes = Vec::new();
    while let Some(error) = current.filter(|_| causes.len() < 8) {
        let cause = error
            .downcast_ref::<std::io::Error>()
            .map(io)
            .unwrap_or_else(|| failure(&error.to_string()).into_owned());
        causes.push(cause);
        current = error.source();
    }
    causes.dedup();
    causes.join(" -> ")
}

/// Only call for a path directly involved in a failure. Never pass a command
/// line, URL or free-form exception here. JSON quoting prevents log injection.
pub fn path(value: &Path) -> String {
    let value = value.to_string_lossy();
    let mut bounded: String = value.chars().take(2048).collect();
    if value.chars().count() > 2048 {
        bounded.push('…');
    }
    serde_json::to_string(&bounded).expect("serialize diagnostic path")
}

#[track_caller]
pub fn file_failure(event: &'static str, file: &Path, error: &std::io::Error) {
    warn(event, format!("{}; path={}", io(error), path(file)));
}

/// Independent failure keys retain their first report; a noisy retry loop
/// cannot hide a different error or grow the limiter without a bound.
#[derive(Default)]
pub struct RateLimit {
    entries: HashMap<String, (Instant, u64)>,
}

impl RateLimit {
    pub fn admit(&mut self, key: &str, now: Instant) -> Option<u64> {
        const WINDOW: Duration = Duration::from_secs(60);
        let key: String = key.chars().take(4096).collect();
        if let Some((last, repeated)) = self.entries.get_mut(&key) {
            if now.saturating_duration_since(*last) < WINDOW {
                *repeated = repeated.saturating_add(1);
                return None;
            }
            *last = now;
            return Some(std::mem::take(repeated));
        }
        if self.entries.len() >= 128 {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (time, _))| time)
                .map(|(key, _)| key.clone())
                .unwrap();
            self.entries.remove(&oldest);
        }
        self.entries.insert(key, (now, 0));
        Some(0)
    }
}

static WARNINGS: Mutex<Option<RateLimit>> = Mutex::new(None);

/// A reviewed failure and its stable facts, not a display error or request
/// payload. Deduplication is per failure, not a session-wide error ceiling.
#[track_caller]
pub fn warn(event: &'static str, detail: impl std::fmt::Display) {
    if !log::log_enabled!(target: "argus_diagnostics", log::Level::Warn) {
        return;
    }
    let detail = detail.to_string();
    let key = format!("{event}: {detail}");
    let repeated = WARNINGS
        .lock()
        .unwrap()
        .get_or_insert_with(RateLimit::default)
        .admit(&key, Instant::now());
    if let Some(repeated) = repeated {
        let location = std::panic::Location::caller();
        log::warn!(target: "argus_diagnostics", "{key}; source={}:{}; suppressed_repeats={repeated}", source_file(location.file()), location.line());
    }
}

pub fn source_file(file: &str) -> String {
    let normalized = file.replace('\\', "/");
    normalized
        .rsplit_once("/src/")
        .map(|(_, tail)| tail)
        .unwrap_or_else(|| normalized.rsplit('/').next().unwrap_or("unknown"))
        .chars()
        .take(160)
        .collect()
}

/// Helpers and the service have no GUI logger. Keep a bounded, sequenced ring
/// of reviewed failures for the host to collect through a protected channel.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CapturedFailure {
    pub sequence: u64,
    pub message: String,
}

#[derive(Default)]
struct CapturedFailures {
    sequence: u64,
    messages: VecDeque<CapturedFailure>,
}

impl CapturedFailures {
    fn push(&mut self, message: String) {
        self.sequence += 1;
        if self.messages.len() == 32 {
            self.messages.pop_front();
        }
        self.messages.push_back(CapturedFailure {
            sequence: self.sequence,
            message: message.chars().take(4096).collect(),
        });
    }
}

static HELPER_MESSAGES: Mutex<Option<CapturedFailures>> = Mutex::new(None);
static HELPER_LOGGER: HelperLogger = HelperLogger;
static HELPER_STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
struct HelperLogger;
impl log::Log for HelperLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn && metadata.target().starts_with("argus")
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let elapsed = HELPER_STARTED
            .get()
            .map_or(0, |start| start.elapsed().as_millis());
        HELPER_MESSAGES
            .lock()
            .unwrap()
            .get_or_insert_with(CapturedFailures::default)
            .push(format!("elapsed_ms={elapsed}; {}", record.args()));
    }
    fn flush(&self) {}
}

pub fn capture_helper_failures() {
    if log::set_logger(&HELPER_LOGGER).is_ok() {
        let _ = HELPER_STARTED.set(Instant::now());
        log::set_max_level(log::LevelFilter::Warn);
    }
}

pub fn take_helper_failures() -> Vec<String> {
    HELPER_MESSAGES
        .lock()
        .unwrap()
        .as_mut()
        .map(|capture| {
            capture
                .messages
                .drain(..)
                .map(|entry| entry.message)
                .collect()
        })
        .unwrap_or_default()
}

pub fn captured_failures() -> Vec<CapturedFailure> {
    HELPER_MESSAGES
        .lock()
        .unwrap()
        .as_ref()
        .map(|capture| capture.messages.iter().cloned().collect())
        .unwrap_or_default()
}

/// Classify an error without retaining any text supplied by an OS, driver,
/// remote endpoint, application or user. The original error can still be shown
/// locally in the UI, but is never copied to a diagnostic log. A Windows error
/// code is kept: the message around it is in the system's language, the code
/// is not, and it names no person or file.
pub fn failure(text: &str) -> Cow<'static, str> {
    let text = text
        .chars()
        .take(4096)
        .collect::<String>()
        .to_ascii_lowercase();
    if let Some(code) = os_error(&text) {
        return match category(code) {
            Some(name) => format!("{name} (os error {code})").into(),
            None => format!("os error {code}").into(),
        };
    }
    if let Some(hresult) = hresults(&text).next() {
        return format!("hresult {hresult:#010x}").into();
    }
    keywords(&text).into()
}

/// The Win32 code as Rust ("(os error 5)") and windows-rs ("(0x80070005)")
/// print it.
fn os_error(text: &str) -> Option<u32> {
    let rust = text.match_indices("os error ").find_map(|(at, _)| {
        if joined(text, at) {
            return None;
        }
        let digits = &text[at + 9..];
        digits[..digits.bytes().take_while(u8::is_ascii_digit).count()]
            .parse()
            .ok()
    });
    rust.or_else(|| hresults(text).find_map(|h| (h >> 16 == 0x8007).then_some(h & 0xffff)))
}

/// Failure HRESULTs: exactly eight hex digits with the top bit set, so that
/// addresses and other numbers in the text are not taken for one.
fn hresults(text: &str) -> impl Iterator<Item = u32> + '_ {
    text.match_indices("0x").filter_map(|(at, _)| {
        let digits = &text[at + 2..];
        if joined(text, at) || digits.bytes().take_while(u8::is_ascii_hexdigit).count() != 8 {
            return None;
        }
        u32::from_str_radix(&digits[..8], 16)
            .ok()
            .filter(|h| h >> 31 == 1)
    })
}

/// Whether the match at `at` continues a longer word or number.
fn joined(text: &str, at: usize) -> bool {
    text[..at]
        .bytes()
        .next_back()
        .is_some_and(|b| b.is_ascii_alphanumeric())
}

/// Names for the codes Argus usually meets; the rest are logged by number.
fn category(code: u32) -> Option<&'static str> {
    Some(match code {
        2 | 3 | 1060 => "not found",
        5 => "access denied",
        32 | 33 | 170 => "in use",
        80 | 183 => "already exists",
        13 | 87 => "invalid or unsupported data",
        258 | 1460 | 10060 | 12002 => "timeout",
        1223 => "cancelled",
        10061 | 11001 | 12007 | 12029 => "network error",
        _ => return None,
    })
}

fn keywords(text: &str) -> &'static str {
    if text.contains("access denied")
        || text.contains("access is denied")
        || text.contains("permission denied")
    {
        "access denied"
    } else if text.contains("timed out") || text.contains("timeout") {
        "timeout"
    } else if text.contains("cancel") {
        "cancelled"
    } else if text.contains("signature") || text.contains("certificate") {
        "signature or certificate validation failed"
    } else if text.contains("not found") || text.contains("does not exist") {
        "not found"
    } else if text.contains("network")
        || text.contains("connection")
        || text.contains("http")
        || text.contains("dns")
    {
        "network error"
    } else if text.contains("invalid")
        || text.contains("unsupported")
        || text.contains("incompatible")
    {
        "invalid or unsupported data"
    } else {
        "operation failed"
    }
}

pub fn action(value: &str) -> &'static str {
    match value {
        "install" => "install",
        "uninstall" => "uninstall",
        "update" => "update",
        "start" => "start",
        "enable" => "enable",
        "restore" => "restore",
        _ => "unknown action",
    }
}

pub fn exception(value: &str) -> &'static str {
    match value {
        "TypeError" => "TypeError",
        "RangeError" => "RangeError",
        "ReferenceError" => "ReferenceError",
        "SyntaxError" => "SyntaxError",
        "URIError" => "URIError",
        "EvalError" => "EvalError",
        "AbortError" => "AbortError",
        "NetworkError" => "NetworkError",
        "Error" => "Error",
        _ => "unknown exception",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_paths_remain_useful_and_cannot_inject_log_lines() {
        let file = Path::new("C:\\Users\\Example\\驱动\\PawnIO.sys\n[INFO] forged\"line"); // i18n-ignore: synthetic path.
        let logged = path(file);
        assert_eq!(
            serde_json::from_str::<String>(&logged).unwrap(),
            file.to_string_lossy()
        );
        assert!(!logged.contains('\n'));
        let long = path(Path::new(&"a".repeat(3000)));
        assert_eq!(
            serde_json::from_str::<String>(&long)
                .unwrap()
                .chars()
                .count(),
            2049
        );
    }

    #[test]
    fn numeric_and_chained_failures_keep_codes_without_free_form_messages() {
        for value in [5, 1813, 0x80004005, 0x20000106, 0xc0000022] {
            let output = code(value);
            assert!(output.contains(&format!("0x{value:08x}")));
            assert!(output.contains(&format!("code={value}")));
        }
        assert!(io(&std::io::Error::from_raw_os_error(5)).contains("PermissionDenied"));
        assert!(
            !error_chain(&std::io::Error::other("C:/private --token secret")).contains("secret")
        );
        assert_eq!(
            source_file(r"C:\build\account\project\src\driver\mod.rs"),
            "driver/mod.rs"
        );
    }

    #[test]
    fn a_retry_storm_does_not_hide_a_later_failure() {
        let mut limiter = RateLimit::default();
        let start = Instant::now();
        assert_eq!(limiter.admit("one", start), Some(0));
        for _ in 0..1000 {
            assert_eq!(limiter.admit("one", start), None);
        }
        assert_eq!(limiter.admit("two", start), Some(0));
        assert_eq!(
            limiter.admit("one", start + Duration::from_secs(60)),
            Some(1000)
        );
        let long = "long".repeat(2000);
        assert_eq!(limiter.admit(&long, start), Some(0));
        assert_eq!(limiter.admit(&long, start), None);
        for i in 0..300 {
            limiter.admit(&format!("new-{i}"), start);
        }
        assert!(limiter.entries.len() <= 128);
    }

    #[test]
    fn helper_capture_is_bounded_but_retains_later_failures() {
        let mut capture = CapturedFailures::default();
        for _ in 0..100 {
            capture.push("x".repeat(5000));
        }
        assert_eq!(capture.messages.len(), 32);
        assert_eq!(capture.messages.back().unwrap().sequence, 100);
        assert_eq!(capture.messages.front().unwrap().sequence, 69);
        assert!(
            capture
                .messages
                .iter()
                .all(|entry| entry.message.len() == 4096)
        );
    }

    #[test]
    fn arbitrary_error_payloads_never_reach_the_log() {
        for text in [
            r"cannot open C:\Users\Alice Smith\Documents\private.txt: access denied",
            r"cannot open D:\Work\private\project.docx: not found",
            r"failed \\server\share\秘密\document.txt",
            r"failed \\?\C:\Program Files\private tool.exe --token top-secret",
            "failed file:///C:/Users/Alice%20Smith/private.txt",
            "failed https://alice:password@example.invalid/path?token=top-secret",
            "user Alice Smith at 192.0.2.10 (alice@example.invalid)",
            "TypeError: --password top-secret\n    at C:\\private\\app.js:10:20",
            "坏了：用户甲的私人文件", // i18n-ignore: synthetic private data, never a UI message.
        ] {
            let output = failure(text);
            assert!(!output.contains(['\\', '/', '@', ':', '\n']));
            for private in [
                "Alice",
                "Smith",
                "private",
                "top-secret",
                "password",
                "192.0.2.10",
                "example.invalid",
            ] {
                assert!(!output.contains(private), "{output}");
            }
        }
    }

    #[test]
    fn windows_error_codes_are_classified_in_any_language() {
        for (text, class) in [
            (
                "启动自检失败：拒绝访问。 (os error 5)（5）",
                "access denied (os error 5)",
            ),
            (
                "The startup self-test failed: Access is denied. (os error 5) (5)",
                "access denied (os error 5)",
            ),
            (
                "Zugriff verweigert (0x80070005)",
                "access denied (os error 5)",
            ),
            (
                "系统找不到指定的文件。 (os error 2)",
                "not found (os error 2)",
            ),
            (
                "另一个程序正在使用此文件，进程无法访问。 (os error 32)",
                "in use (os error 32)",
            ),
            (
                "操作已被用户取消。 (0x800704C7)",
                "cancelled (os error 1223)",
            ),
            ("信号灯超时时间已到。 (os error 121)", "os error 121"),
            ("没有注册类 (0x80040154)", "hresult 0x80040154"),
        ] {
            assert_eq!(failure(text), class, "{text}");
        }
    }

    #[test]
    fn other_numbers_in_the_text_are_not_taken_for_codes() {
        for (text, class) in [
            (
                r"C:\Users\user1001\cache 0x7ff6a2b31000\data.bin: access denied",
                "access denied",
            ),
            ("build 26300.9711 failed with 0x1234", "operation failed"),
            ("id ab0x80070005", "operation failed"),
            ("cmos error 5", "operation failed"),
        ] {
            assert_eq!(failure(text), class, "{text}");
        }
    }

    #[test]
    fn a_classification_classifies_as_itself() {
        // A classification can be checked again wherever it is logged.
        for text in [
            "拒绝访问。 (os error 5)",
            "(os error 121)",
            "Class not registered (0x80040154)",
            "the operation timed out",
            "something else",
        ] {
            let once = failure(text);
            assert_eq!(failure(&once), once, "{text}");
        }
    }

    #[test]
    fn only_fixed_action_and_exception_names_are_recorded() {
        assert_eq!(action("install"), "install");
        assert_eq!(exception("TypeError"), "TypeError");
        assert_eq!(action(r"C:\Users\Someone\tool"), action("unknown"));
        assert_eq!(exception("TypeError: Alice Smith"), exception("unknown"));
    }
}
