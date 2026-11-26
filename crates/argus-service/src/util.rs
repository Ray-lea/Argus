use argus_core::tr;
use windows::Win32::Foundation::{CloseHandle, HANDLE};

pub type Result<T> = std::result::Result<T, u32>;
pub const UNSAFE_PATH: u32 = 0x2000_0001;
pub const OTHER_OWNER: u32 = 0x2000_0002;
pub const INCOMPATIBLE: u32 = 0x2000_0003;
pub const FOREIGN_SERVICE: u32 = 0x2000_0004;
/// The task manager replacement held up a service update or removal: Argus's
/// own value is not restored yet.
pub const TASK_MANAGER_ACTIVE: u32 = 0x2000_0005;
/// The same, because another program's configuration holds taskmgr.exe's
/// entry and Argus does not overwrite it.
pub const TASK_MANAGER_CONFLICT: u32 = 0x2000_0006;
/// An enabled replacement could not be brought to the new build, so the
/// service update stopped before changing the service.
pub const TASK_MANAGER_FOLLOW: u32 = 0x2000_0007;
pub const ADMIN_OUTDATED: u32 = 0x2000_0008;
/// The service is registered, but its authorization record is gone or
/// unreadable: nothing says whose it is, so an update cannot bring it back.
pub const DAMAGED: u32 = 0x2000_0009;
/// Task Scheduler refused to register the administrator startup task; the
/// log keeps the code it returned.
pub const TASK_REJECTED: u32 = 0x2000_000A;

pub fn message(code: u32) -> String {
    match code {
        UNSAFE_PATH => tr!("service.unsafePath"),
        OTHER_OWNER => tr!("service.otherOwner"),
        INCOMPATIBLE => tr!("service.incompatible"),
        FOREIGN_SERVICE => tr!("service.foreignService"),
        TASK_MANAGER_ACTIVE => tr!("service.taskManagerActive"),
        TASK_MANAGER_CONFLICT => tr!("service.taskManagerConflict"),
        TASK_MANAGER_FOLLOW => tr!("service.taskManagerFollow"),
        ADMIN_OUTDATED => tr!("admin.outdated"),
        DAMAGED => tr!("service.damaged.owner"),
        TASK_REJECTED => tr!("admin.taskRejected"),
        1223 => tr!("service.canceled"),
        1460 => tr!("service.timeout"),
        _ => tr!(
            "service.osError",
            error = system_text(code),
            code = shown_code(code)
        ),
    }
}

/// The system's description of `code`, without the "(os error N)" that the
/// message adds once, readably.
fn system_text(code: u32) -> String {
    let text = std::io::Error::from_raw_os_error(code as i32).to_string();
    match text.strip_suffix(&format!(" (os error {})", code as i32)) {
        Some(text) => text.trim_end().to_owned(),
        None => text,
    }
}

/// Win32 errors as the decimal number Windows documents them by; HRESULTs
/// and NTSTATUS values in hex, as `0x80004005` rather than a signed or
/// ten-digit decimal number.
fn shown_code(code: u32) -> String {
    if code > 0xFFFF {
        format!("0x{code:08X}")
    } else {
        code.to_string()
    }
}

pub fn win(e: windows::core::Error) -> u32 {
    let code = e.code().0 as u32;
    if code >> 16 == 0x8007 {
        code & 0xffff
    } else {
        code
    }
}

#[track_caller]
pub fn stage<T>(name: &'static str, result: Result<T>) -> Result<T> {
    if let Err(code) = &result {
        argus_core::diagnostics::warn(name, argus_core::diagnostics::code(*code));
    }
    result
}

#[track_caller]
pub fn file_stage<T>(name: &'static str, path: &std::path::Path, result: Result<T>) -> Result<T> {
    if let Err(code) = &result {
        argus_core::diagnostics::warn(
            name,
            format!(
                "{}; path={}",
                argus_core::diagnostics::code(*code),
                log_path(path)
            ),
        );
    }
    result
}

/// `diagnostics::path` with every account SID folder replaced by `<SID>`:
/// administrator runtime directories are named after the account's SID, and
/// the log is attached to public bug reports.
pub fn log_path(path: &std::path::Path) -> String {
    use std::path::{Component, PathBuf};
    let sid = |name: &std::ffi::OsStr| {
        name.to_str().is_some_and(|name| {
            name.get(..4)
                .is_some_and(|p| p.eq_ignore_ascii_case("S-1-"))
                && name[4..].bytes().all(|b| b.is_ascii_digit() || b == b'-')
        })
    };
    let redacted: PathBuf = path
        .components()
        .map(|part| match part {
            Component::Normal(name) if sid(name) => std::ffi::OsStr::new("<SID>"),
            part => part.as_os_str(),
        })
        .collect();
    argus_core::diagnostics::path(&redacted)
}

pub fn io(e: std::io::Error) -> u32 {
    e.raw_os_error().unwrap_or(31) as u32
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// A GUID as `CoCreateGuid` names files and folders here, without braces.
pub fn valid_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

pub struct Handle(pub HANDLE);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn non_win32_hresult_is_not_misreported_as_access_denied() {
        for (hresult, expected) in [
            (0x80070005u32, 5),
            (0x80070715, 1813),
            (0x80004005, 0x80004005),
            (0xc0000022, 0xc0000022),
        ] {
            assert_eq!(
                win(windows::core::Error::from_hresult(windows::core::HRESULT(
                    hresult as i32
                ))),
                expected
            );
        }
    }

    #[test]
    fn a_native_failure_shows_its_code_once_and_hresults_in_hex() {
        assert_eq!(shown_code(5), "5");
        assert_eq!(shown_code(1813), "1813");
        assert_eq!(shown_code(0x80004005), "0x80004005");
        assert_eq!(shown_code(0xC0000022), "0xC0000022");
        for code in [5, 2, 1813, 0x80004005, 0x80070005] {
            let text = system_text(code);
            assert!(!text.is_empty());
            assert!(!text.contains("os error"), "{text}");
            assert!(!text.contains(&(code as i32).to_string()), "{text}");
        }
    }
}
