//! Failure details from explicit elevated helpers. Reports are pinned,
//! administrator-owned files, bound to the helper PID and creation time.
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use argus_core::diagnostics as log;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::Storage::FileSystem::CreateDirectoryW;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessId, GetProcessTimes};
use windows::core::PCWSTR;

use crate::{
    security,
    util::{Handle, Result, io, wide, win},
};

const LIMIT: u64 = 256 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceDiagnostics {
    pid: u32,
    created: u64,
    entries: Vec<log::CapturedFailure>,
}

pub(crate) fn snapshot() -> Option<ServiceDiagnostics> {
    let entries = log::captured_failures();
    if entries.is_empty() {
        return None;
    }
    let (pid, created) = identity(unsafe { GetCurrentProcess() }).ok()?;
    Some(ServiceDiagnostics {
        pid,
        created,
        entries,
    })
}

#[derive(Default)]
struct Cursor {
    identity: (u32, u64),
    sequence: u64,
}

impl Cursor {
    fn accept<'a>(
        &mut self,
        report: &'a ServiceDiagnostics,
        expected_pid: u32,
    ) -> Vec<&'a log::CapturedFailure> {
        if report.pid != expected_pid
            || report.created == 0
            || report.entries.len() > 32
            || report
                .entries
                .iter()
                .any(|entry| entry.sequence == 0 || entry.message.chars().count() > 4096)
            || report
                .entries
                .windows(2)
                .any(|pair| pair[0].sequence >= pair[1].sequence)
        {
            return Vec::new();
        }
        if self.identity != (report.pid, report.created) {
            self.identity = (report.pid, report.created);
            self.sequence = 0;
        }
        let fresh = report
            .entries
            .iter()
            .filter(|entry| entry.sequence > self.sequence)
            .collect();
        if let Some(last) = report.entries.last() {
            self.sequence = self.sequence.max(last.sequence);
        }
        fresh
    }
}

static SERVICE_CURSOR: Mutex<Cursor> = Mutex::new(Cursor {
    identity: (0, 0),
    sequence: 0,
});

pub(crate) fn import(report: &ServiceDiagnostics, expected_pid: u32) {
    for entry in SERVICE_CURSOR.lock().unwrap().accept(report, expected_pid) {
        log::warn(
            "privileged service diagnostic",
            format!("service_pid={}; detail={:?}", report.pid, entry.message),
        );
    }
}

/// Only failing background reads request a diagnostic Ping, at most once per
/// minute. Normal sampling never makes an extra round trip.
#[derive(Default)]
pub(crate) struct Probe {
    last: Option<(u32, Instant)>,
}

impl Probe {
    pub fn collect(&mut self, pid: u32) {
        if self.last.is_some_and(|(previous, at)| {
            previous == pid && at.elapsed() < std::time::Duration::from_secs(60)
        }) {
            return;
        }
        self.last = Some((pid, Instant::now()));
        let _ = crate::pipe::request_with_timeout(
            crate::PIPE_NAME,
            pid,
            crate::Operation::Ping,
            std::time::Duration::from_secs(2),
        );
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    format: u8,
    pid: u32,
    created: u64,
    exit_code: u32,
    elapsed_ms: u64,
    messages: Vec<String>,
}

fn identity(process: HANDLE) -> Result<(u32, u64)> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) }
        .map_err(win)?;
    Ok((
        unsafe { GetProcessId(process) },
        (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
    ))
}

fn root() -> Result<PathBuf> {
    Ok(crate::install::directory()?
        .parent()
        .ok_or(87u32)?
        .join("Argus Sensor Setup"))
}

fn filename(pid: u32, created: u64) -> String {
    format!("helper-{pid}-{created}.json")
}

fn report_name(name: &str) -> bool {
    let Some(body) = name
        .strip_prefix("helper-")
        .and_then(|s| s.strip_suffix(".json"))
    else {
        return false;
    };
    let Some((pid, created)) = body.split_once('-') else {
        return false;
    };
    pid.parse::<u32>().is_ok() && created.parse::<u64>().is_ok()
}

fn pin_root(path: &Path, create: bool) -> Result<(Handle, Handle)> {
    let parent = security::protect_directory(path.parent().ok_or(87u32)?)?;
    if create {
        let sd = security::directory_descriptor()?;
        if let Err(error) = unsafe {
            CreateDirectoryW(
                PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
                Some(&sd.attributes()),
            )
        } {
            let code = win(error);
            if code != 183 {
                return Err(code);
            }
        }
    }
    Ok((parent, security::protect_directory(path)?))
}

fn save(report: &Report) -> Result<()> {
    if !argus_core::privilege::is_elevated() {
        return Err(5);
    }
    let root = root()?;
    let _pins = pin_root(&root, true)?;
    // Only our bounded report files are pruned; installer files and lock files
    // belong to other operations and are never touched here.
    let mut old: Vec<_> = fs::read_dir(&root)
        .map_err(io)?
        .take(512)
        .filter_map(|entry| {
            let entry = entry.ok()?;
            report_name(&entry.file_name().to_string_lossy()).then(|| {
                (
                    entry.metadata().ok().and_then(|m| m.modified().ok()),
                    entry.path(),
                )
            })
        })
        .collect();
    old.sort_by_key(|(modified, _)| *modified);
    for (_, path) in old.iter().take(old.len().saturating_sub(15)) {
        if let Ok(pin) = security::protect_directory(path) {
            drop(pin);
            let _ = fs::remove_file(path);
        }
    }
    let bytes = serde_json::to_vec(report).map_err(|_| 13u32)?;
    if bytes.len() as u64 > LIMIT {
        return Err(13);
    }
    let path = root.join(filename(report.pid, report.created));
    let mut file = crate::install::new_protected_file(&path)?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(io)
}

/// Called only in a dedicated helper process, before the Tauri logger exists.
pub(crate) fn helper(action: &'static str, run: impl FnOnce() -> Result<()>) -> i32 {
    log::capture_helper_failures();
    let started = Instant::now();
    let result = run();
    let code = result.err().unwrap_or(0);
    if code != 0 {
        log::warn(action, log::code(code));
    }
    let messages = log::take_helper_failures();
    if !messages.is_empty()
        && let Ok((pid, created)) = identity(unsafe { GetCurrentProcess() })
    {
        let _ = save(&Report {
            format: 1,
            pid,
            created,
            exit_code: code,
            elapsed_ms: started.elapsed().as_millis() as u64,
            messages,
        });
    }
    code as i32
}

fn read_report(path: &Path, pid: u32, created: u64, code: u32) -> Result<Report> {
    let _pin = security::protect_directory(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(io)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > LIMIT {
        return Err(13);
    }
    let report: Report = serde_json::from_slice(&bytes).map_err(|_| 13u32)?;
    validate(&report, pid, created, code)?;
    Ok(report)
}

fn validate(report: &Report, pid: u32, created: u64, code: u32) -> Result<()> {
    if report.format != 1
        || (report.pid, report.created, report.exit_code) != (pid, created, code)
        || report.messages.len() > 32
        || report.messages.iter().any(|m| m.chars().count() > 4096)
    {
        return Err(13);
    }
    Ok(())
}

/// The process handle returned by ShellExecuteExW supplies the identity; no
/// PID or report path is accepted from the webview or command-line arguments.
pub(crate) fn collect(action: &'static str, process: HANDLE, code: u32) {
    let read = (|| {
        let (pid, created) = identity(process)?;
        let root = root()?;
        let path = root.join(filename(pid, created));
        if code == 0 && !path.try_exists().map_err(io)? {
            return Ok(None);
        }
        let _pins = pin_root(&root, false)?;
        read_report(&path, pid, created, code).map(Some)
    })();
    match read {
        Ok(Some(report)) => {
            for message in report.messages {
                // A protected file is still quoted: it cannot forge log lines.
                log::warn(
                    action,
                    format!(
                        "helper_pid={}; elapsed_ms={}; detail={message:?}",
                        report.pid, report.elapsed_ms
                    ),
                );
            }
        }
        Ok(None) => {}
        Err(error) => log::warn(
            action,
            format!(
                "helper {}; diagnostic report unavailable: {}",
                log::code(code),
                log::code(error)
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_reports_deduplicate_sequences_and_reject_malformed_batches() {
        let mut report = ServiceDiagnostics {
            pid: 42,
            created: 7,
            entries: vec![log::CapturedFailure {
                sequence: 1,
                message: "native=5".into(),
            }],
        };
        let mut cursor = Cursor::default();
        assert!(cursor.accept(&report, 43).is_empty());
        assert_eq!(cursor.accept(&report, 42).len(), 1);
        assert!(cursor.accept(&report, 42).is_empty());
        report.entries.push(log::CapturedFailure {
            sequence: 2,
            message: "native=1813".into(),
        });
        assert_eq!(cursor.accept(&report, 42).len(), 1);
        report.created += 1;
        assert_eq!(cursor.accept(&report, 42).len(), 2);
        report.entries.swap(0, 1);
        assert!(cursor.accept(&report, 42).is_empty());
        report.entries[0].message = "a".repeat(4097);
        assert!(cursor.accept(&report, 42).is_empty());
    }

    #[test]
    fn ping_diagnostics_are_compatible_in_both_directions() {
        #[derive(Deserialize)]
        #[serde(tag = "kind", content = "value", rename_all = "camelCase")]
        enum OldReply {
            Pong { version: String, protocol: u32 },
        }
        let old = r#"{"kind":"pong","value":{"version":"0.2.0","protocol":9}}"#;
        assert!(matches!(
            serde_json::from_str::<crate::Reply>(old).unwrap(),
            crate::Reply::Pong {
                diagnostics: None,
                ..
            }
        ));
        let new = crate::Reply::Pong {
            version: "0.2.0".into(),
            protocol: 9,
            diagnostics: Some(ServiceDiagnostics {
                pid: 42,
                created: 7,
                entries: vec![log::CapturedFailure {
                    sequence: 1,
                    message: "error 5".into(),
                }],
            }),
        };
        let OldReply::Pong { version, protocol } =
            serde_json::from_slice(&serde_json::to_vec(&new).unwrap()).unwrap();
        assert_eq!(version, "0.2.0");
        assert_eq!(protocol, 9);
    }

    #[test]
    fn user_owned_diagnostic_files_are_not_imported() {
        let path = std::env::temp_dir().join(format!(
            "argus-report-test-{:?}.json",
            unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap()
        ));
        use std::os::windows::io::FromRawHandle;
        use windows::Win32::Storage::FileSystem::{
            CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE, FILE_SHARE_READ,
        };
        let sd = security::Descriptor::new("D:P(A;;FA;;;WD)").unwrap();
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
                FILE_GENERIC_WRITE.0,
                FILE_SHARE_READ,
                Some(&sd.attributes()),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .unwrap();
        let mut file = unsafe { fs::File::from_raw_handle(handle.0) };
        file.write_all(b"{}").unwrap();
        drop(file);
        assert_eq!(
            read_report(&path, 1, 1, 5).err(),
            Some(crate::util::UNSAFE_PATH)
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    #[ignore = "Subprocess fixture for the isolated helper report test"]
    fn helper_report_fixture() {
        assert_eq!(
            std::env::var("ARGUS_DIAGNOSTIC_FIXTURE").as_deref(),
            Ok("emit")
        );
        let code = helper("diagnostic fixture", || {
            let missing = root()?.join("missing-diagnostic-fixture.sys");
            crate::util::file_stage(
                "fixture driver open",
                &missing,
                fs::File::open(&missing).map_err(io),
            )?;
            Ok(())
        });
        std::process::exit(code);
    }

    #[test]
    #[ignore = "Isolated Windows VM: creates a protected helper report"]
    fn protected_helper_report_round_trip() {
        use std::os::windows::io::AsRawHandle;
        assert!(argus_core::privilege::is_elevated());
        log::capture_helper_failures();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "diagnostics::tests::helper_report_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("ARGUS_DIAGNOSTIC_FIXTURE", "emit")
            .spawn()
            .unwrap();
        let code = child.wait().unwrap().code().unwrap() as u32;
        assert!(matches!(code, 2 | 3));
        let handle = HANDLE(child.as_raw_handle());
        let (pid, created) = identity(handle).unwrap();
        collect("fixture report collected", handle, code);
        assert!(
            log::captured_failures()
                .iter()
                .any(|entry| entry.message.contains("fixture driver open")
                    && entry.message.contains("missing-diagnostic-fixture.sys"))
        );
        let report = read_report(
            &root().unwrap().join(filename(pid, created)),
            pid,
            created,
            code,
        )
        .unwrap();
        assert_eq!(report.exit_code, code);
        if let Some(path) = std::env::var_os("ARGUS_DIAGNOSTIC_TEST_IDENTITY") {
            fs::write(path, serde_json::to_vec(&(pid, created, code)).unwrap()).unwrap();
        }
        println!("protected helper round trip passed; pid={pid}; created={created}; exit={code}");
    }

    #[test]
    #[ignore = "Isolated Windows VM: standard account reads the elevated fixture report"]
    fn standard_user_reads_protected_helper_report() {
        assert!(!argus_core::privilege::is_elevated());
        let path = std::env::var_os("ARGUS_DIAGNOSTIC_TEST_IDENTITY").unwrap();
        let (pid, created, code): (u32, u64, u32) =
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let root = root().unwrap();
        let _pins = pin_root(&root, false).unwrap();
        let report = read_report(&root.join(filename(pid, created)), pid, created, code).unwrap();
        assert!(
            report
                .messages
                .iter()
                .any(|message| message.contains("fixture driver open"))
        );
        println!("standard account read the protected report without elevation");
    }
    #[test]
    fn report_identity_and_bounds_are_checked_before_import() {
        let mut report = Report {
            format: 1,
            pid: 42,
            created: 7,
            exit_code: 5,
            elapsed_ms: 12,
            messages: vec!["stage=copy".into()],
        };
        assert!(validate(&report, 42, 7, 5).is_ok());
        assert!(validate(&report, 42, 8, 5).is_err());
        assert!(validate(&report, 43, 7, 5).is_err());
        assert!(validate(&report, 42, 7, 0).is_err());
        report.messages = vec!["x".repeat(4097)];
        assert!(validate(&report, 42, 7, 5).is_err());
        for name in ["helper-12-34.json", "helper-0-0.json"] {
            assert!(report_name(name));
        }
        for name in [
            "install.lock",
            "helper-../secret.json",
            "helper-1-2.json.exe",
        ] {
            assert!(!report_name(name));
        }
    }
}
