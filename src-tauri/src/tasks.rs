//! User-requested launches. Shell activation runs on its own STA, off the UI thread.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, HLOCAL, HWND, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSidToSidW, EXPLICIT_ACCESS_W, GRANT_ACCESS, SetEntriesInAclW, TRUSTEE_IS_SID,
    TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows::Win32::Security::{
    CreateRestrictedToken, GetTokenInformation, LUA_TOKEN, PSID, SID_AND_ATTRIBUTES,
    SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DEFAULT_DACL,
    TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TOKEN_USER, TokenDefaultDacl, TokenElevation, TokenIntegrityLevel, TokenUser,
};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Environment::{
    CreateEnvironmentBlock, DestroyEnvironmentBlock, ExpandEnvironmentStringsW,
};
use windows::Win32::System::SystemServices::SE_GROUP_INTEGRITY;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, GetCurrentProcess,
    GetExitCodeProcess, INFINITE, OpenProcessToken, PROCESS_INFORMATION, STARTUPINFOW,
    WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, PWSTR, w};

const HELPER_ARG: &str = "--argus-run-task";
const MAX_COMMAND: usize = 8192;
static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    Invalid,
    Cancelled,
    NotFound,
    AccessDenied,
    StandardUnavailable,
    ElevationRequired,
    NoAssociation,
    Busy,
    Os,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskError {
    pub kind: ErrorKind,
    pub code: u32,
    #[serde(skip)]
    stage: &'static str,
}

impl TaskError {
    fn code(stage: &'static str, code: u32) -> Self {
        let kind = match code {
            2 | 3 => ErrorKind::NotFound,
            5 => ErrorKind::AccessDenied,
            87 => ErrorKind::Invalid,
            740 => ErrorKind::ElevationRequired,
            1155 => ErrorKind::NoAssociation,
            1223 => ErrorKind::Cancelled,
            _ => ErrorKind::Os,
        };
        Self { kind, code, stage }
    }
    fn win(stage: &'static str, error: windows::core::Error) -> Self {
        let code = error.code().0 as u32;
        Self::code(
            stage,
            if code & 0xffff0000 == 0x80070000 {
                code & 0xffff
            } else {
                code
            },
        )
    }
}

pub(crate) struct Handle(pub(crate) HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}
struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}
pub(crate) struct Environment(pub(crate) *mut std::ffi::c_void);
impl Drop for Environment {
    fn drop(&mut self) {
        let _ = unsafe { DestroyEnvironmentBlock(self.0) };
    }
}
struct Running;
impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

pub(crate) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Windows quoting for the private helper's arguments, not a shell command.
pub(crate) fn quote(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        result.push_str(&"\\".repeat(if ch == '"' { slashes * 2 + 1 } else { slashes }));
        slashes = 0;
        result.push(ch);
    }
    result.push_str(&"\\".repeat(slashes * 2));
    result.push('"');
    result
}

fn valid(command: &str) -> bool {
    !command.trim().is_empty()
        && !command.contains(['\0', '\r', '\n'])
        && command.encode_utf16().count() <= MAX_COMMAND
}

#[derive(PartialEq)]
enum Existing {
    File,
    Other,
}

/// Keep the argument tail verbatim. Re-quoting parsed arguments changes what many apps receive.
fn split_command(
    command: &str,
    existing: impl Fn(&str) -> Option<Existing>,
) -> Result<(&str, &str), TaskError> {
    let command = command.trim();
    if !valid(command) {
        return Err(TaskError::code("validate", 87));
    }
    if let Some(rest) = command.strip_prefix('"') {
        let end = rest.find('"').ok_or_else(|| TaskError::code("quote", 87))?;
        if end == 0 {
            return Err(TaskError::code("quote", 87));
        }
        let tail = &rest[end + 1..];
        if !tail.is_empty() && !tail.starts_with(char::is_whitespace) {
            return Err(TaskError::code("quote", 87));
        }
        return Ok((&rest[..end], tail.trim_start()));
    }
    if existing(command).is_some() {
        return Ok((command, ""));
    }
    // A complete path containing spaces takes precedence over shorter prefixes.
    for (end, _) in command
        .char_indices()
        .rev()
        .filter(|(_, c)| c.is_whitespace())
    {
        let candidate = &command[..end];
        if existing(candidate) == Some(Existing::File) {
            return Ok((candidate, command[end..].trim_start()));
        }
    }
    let end = command.find(char::is_whitespace).unwrap_or(command.len());
    let file = &command[..end];
    // An unresolved path with spaces must not become a shorter executable.
    // ShellExecute would otherwise turn C:\Program into C:\Program.exe.
    if end < command.len() && file.contains(['\\', '/']) && !file.contains("://") {
        return Err(TaskError::code("unresolved program path", 2));
    }
    Ok((file, command[end..].trim_start()))
}

fn activate(owner: usize, command: &str, elevated: bool) -> Result<(), TaskError> {
    if !valid(command) {
        return Err(TaskError::code("validate", 87));
    }
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
        .ok()
        .map_err(|e| TaskError::win("COM initialization", e))?;
    let _com = Com;
    let input = wide(command);
    let needed = unsafe { ExpandEnvironmentStringsW(PCWSTR(input.as_ptr()), None) } as usize;
    if needed == 0 || needed > 32767 {
        return Err(TaskError::code("environment expansion", 87));
    }
    let mut expanded = vec![0; needed];
    let written =
        unsafe { ExpandEnvironmentStringsW(PCWSTR(input.as_ptr()), Some(&mut expanded)) } as usize;
    if written == 0 || written > expanded.len() {
        return Err(TaskError::code("environment expansion", 87));
    }
    let command = String::from_utf16_lossy(&expanded[..written - 1]);
    let directory = std::env::var("USERPROFILE")
        .ok()
        .filter(|p| Path::new(p).is_dir());
    let (file, arguments) = split_command(&command, |path| {
        let path = Path::new(path);
        let path = match directory.as_ref() {
            Some(base) if !path.is_absolute() => Path::new(base).join(path),
            _ => path.to_path_buf(),
        };
        std::fs::metadata(path).ok().map(|m| {
            if m.is_file() {
                Existing::File
            } else {
                Existing::Other
            }
        })
    })?;
    let file = wide(file);
    let arguments = wide(arguments);
    let directory = directory.map(|p| wide(&p));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI,
        hwnd: HWND(owner as *mut _),
        lpVerb: if elevated { w!("runas") } else { w!("open") },
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(arguments.as_ptr()),
        lpDirectory: directory
            .as_ref()
            .map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| TaskError::win("ShellExecuteExW", e))?;
    if !info.hProcess.is_invalid() {
        drop(Handle(info.hProcess));
    }
    Ok(())
}

pub(crate) fn token_buffer(
    token: HANDLE,
    class: TOKEN_INFORMATION_CLASS,
) -> Result<Vec<usize>, TaskError> {
    let mut needed = 0;
    let _ = unsafe { GetTokenInformation(token, class, None, 0, &mut needed) };
    if needed == 0 || needed > 65536 {
        return Err(TaskError::code("token information size", 87));
    }
    let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
    }
    .map_err(|e| TaskError::win("GetTokenInformation", e))?;
    Ok(buffer)
}

/// The elevated token may grant new process/thread objects only to Administrators.
/// Keep the account's own access after making that group deny-only.
fn user_default_dacl(token: HANDLE) -> Result<(), TaskError> {
    let user_data = token_buffer(token, TokenUser)?;
    let dacl_data = token_buffer(token, TokenDefaultDacl)?;
    let user = unsafe { &*user_data.as_ptr().cast::<TOKEN_USER>() };
    let dacl = unsafe { &*dacl_data.as_ptr().cast::<TOKEN_DEFAULT_DACL>() };
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL.0,
        grfAccessMode: GRANT_ACCESS,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: PWSTR(user.User.Sid.0.cast()),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut acl = std::ptr::null_mut();
    unsafe { SetEntriesInAclW(Some(&[access]), Some(dacl.DefaultDacl), &mut acl) }
        .ok()
        .map_err(|e| TaskError::win("SetEntriesInAclW", e))?;
    let next = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
    let result = unsafe {
        SetTokenInformation(
            token,
            TokenDefaultDacl,
            (&raw const next).cast(),
            size_of::<TOKEN_DEFAULT_DACL>() as u32,
        )
    };
    let _ = unsafe { LocalFree(Some(HLOCAL(acl.cast()))) };
    result.map_err(|e| TaskError::win("standard token default DACL", e))
}

/// Filter this process's token; obtaining the linked token yields identification-level access.
/// No shell process or optional Argus service is needed for this path.
fn standard_helper(owner: usize, command: &str) -> Result<(), TaskError> {
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
            &mut token,
        )
    }
    .map_err(|e| TaskError::win("OpenProcessToken", e))?;
    let token = Handle(token);
    let mut filtered = HANDLE::default();
    unsafe { CreateRestrictedToken(token.0, LUA_TOKEN, None, None, None, &mut filtered) }
        .map_err(|e| TaskError::win("CreateRestrictedToken", e))?;
    let filtered = Handle(filtered);
    user_default_dacl(filtered.0)?;
    let mut sid = PSID::default();
    unsafe { ConvertStringSidToSidW(w!("S-1-16-8192"), &mut sid) }
        .map_err(|e| TaskError::win("medium integrity SID", e))?;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid,
            Attributes: SE_GROUP_INTEGRITY as u32,
        },
    };
    let set = unsafe {
        SetTokenInformation(
            filtered.0,
            TokenIntegrityLevel,
            (&raw const label).cast(),
            (size_of::<TOKEN_MANDATORY_LABEL>() + 12) as u32,
        )
    };
    let _ = unsafe {
        windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(sid.0)))
    };
    set.map_err(|e| TaskError::win("SetTokenInformation", e))?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            filtered.0,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
    }
    .map_err(|e| TaskError::win("verify standard token", e))?;
    if elevation.TokenIsElevated != 0 {
        return Err(TaskError {
            kind: ErrorKind::StandardUnavailable,
            code: 5,
            stage: "verify standard token",
        });
    }
    let mut environment = std::ptr::null_mut();
    unsafe { CreateEnvironmentBlock(&mut environment, Some(filtered.0), false) }
        .map_err(|e| TaskError::win("CreateEnvironmentBlock", e))?;
    let environment = Environment(environment);
    let exe = std::env::current_exe()
        .map_err(|e| TaskError::code("current executable", e.raw_os_error().unwrap_or(2) as u32))?;
    let exe = exe.to_string_lossy();
    let mut line = wide(&format!(
        "{} {HELPER_ARG} {} {owner}",
        quote(&exe),
        quote(command)
    ));
    if line.len() > 32767 {
        return Err(TaskError::code("helper command size", 87));
    }
    let exe = wide(&exe);
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessAsUserW(
            Some(filtered.0),
            PCWSTR(exe.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            Some(environment.0),
            PCWSTR::null(),
            &startup,
            &mut process,
        )
    }
    .map_err(|e| TaskError::win("CreateProcessAsUserW", e))?;
    let process_handle = Handle(process.hProcess);
    drop(Handle(process.hThread));
    unsafe { WaitForSingleObject(process_handle.0, INFINITE) };
    let mut code = 0;
    unsafe { GetExitCodeProcess(process_handle.0, &mut code) }
        .map_err(|e| TaskError::win("launch helper result", e))?;
    if code == 0 {
        Ok(())
    } else {
        Err(TaskError::code("launch helper", code))
    }
}

pub fn launch(owner: usize, command: String, elevated: bool) -> Result<(), TaskError> {
    if !valid(&command) {
        return Err(TaskError::code("validate", 87));
    }
    if RUNNING.swap(true, Ordering::Acquire) {
        return Err(TaskError {
            kind: ErrorKind::Busy,
            code: 170,
            stage: "already running",
        });
    }
    let _running = Running;
    let result = std::thread::Builder::new()
        .name("argus-launch".into())
        .spawn(move || {
            if !elevated && argus_core::privilege::is_elevated() {
                standard_helper(owner, &command)
            } else {
                activate(owner, &command, elevated)
            }
        })
        .map_err(|e| TaskError::code("launch worker", e.raw_os_error().unwrap_or(8) as u32))?
        .join()
        .unwrap_or_else(|_| Err(TaskError::code("launch worker", 31)));
    if let Err(error) = &result
        && !matches!(error.kind, ErrorKind::Cancelled)
    {
        log::warn!(
            "new task failed at {}; administrator_requested={elevated}; error=0x{:08X}",
            error.stage,
            error.code
        );
    }
    result
}

/// An ordinary-rights helper exits before Tauri, single-instance or settings setup.
pub fn run_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_none_or(|s| s != HELPER_ARG) {
        return None;
    }
    if args.len() != 4 || argus_core::privilege::is_elevated() {
        return Some(5);
    }
    let Ok(owner) = args[3].parse::<usize>() else {
        return Some(87);
    };
    Some(
        activate(owner, &args[2], false)
            .err()
            .map_or(0, |e| e.code as i32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_keeps_quoted_arguments_and_rejects_broken_program_quotes() {
        assert_eq!(
            split_command(r#""C:\Apps\Example App.exe" --flag "a b""#, |_| None).unwrap(),
            (r"C:\Apps\Example App.exe", r#"--flag "a b""#)
        );
        assert_eq!(
            split_command("cmd.exe /c echo one & two", |_| None).unwrap(),
            ("cmd.exe", "/c echo one & two")
        );
        for bad in [
            "",
            " \t ",
            "\"broken",
            "\"\" arg",
            "\"app.exe\"arg",
            "app\0.exe",
            "app.exe\nother",
        ] {
            assert!(split_command(bad, |_| None).is_err());
        }
    }
    #[test]
    fn complete_paths_with_spaces_are_preserved() {
        let command = r"C:\My Folder\document.txt";
        assert_eq!(
            split_command(command, |p| (p == command).then_some(Existing::File)).unwrap(),
            (command, "")
        );
        assert_eq!(
            quote(r#"C:\path with spaces\"#),
            "\"C:\\path with spaces\\\\\""
        );
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
    }
    #[test]
    fn missing_paths_never_fall_back_to_a_shorter_program() {
        for command in [
            r"C:\Program Files\Missing.exe --flag",
            r"C:\Users\Public\Run Task\missing.exe",
            r".\Missing Folder\app.exe --flag",
        ] {
            let result = split_command(command, |p| {
                (p == r"C:\Program.exe").then_some(Existing::File)
            });
            assert!(matches!(result.unwrap_err().kind, ErrorKind::NotFound));
        }
        assert_eq!(
            split_command(r#"C:\Program Files\App.exe --flag "a b""#, |p| (p
                == r"C:\Program Files\App.exe")
                .then_some(Existing::File))
            .unwrap(),
            (r"C:\Program Files\App.exe", r#"--flag "a b""#)
        );
        assert_eq!(
            split_command(r"C:\A Folder", |p| (p == r"C:\A Folder")
                .then_some(Existing::Other))
            .unwrap(),
            (r"C:\A Folder", "")
        );
    }
}
