//! User-requested launches. Shell activation runs on its own STA, off the UI thread.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use windows::Win32::Foundation::{
    CloseHandle, E_ACCESSDENIED, GENERIC_ALL, HANDLE, HLOCAL, HWND, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSidToSidW, EXPLICIT_ACCESS_W, GRANT_ACCESS, SetEntriesInAclW, TRUSTEE_IS_SID,
    TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows::Win32::Security::{
    CreateRestrictedToken, EqualSid, GetTokenInformation, LUA_TOKEN, PSID, SID_AND_ATTRIBUTES,
    SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DEFAULT_DACL,
    TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TOKEN_USER, TokenDefaultDacl, TokenElevation, TokenIntegrityLevel, TokenUser,
};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Environment::{
    CreateEnvironmentBlock, DestroyEnvironmentBlock, ExpandEnvironmentStringsW,
    FreeEnvironmentStringsW, GetEnvironmentStringsW, SetEnvironmentVariableW,
};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::SystemServices::SE_GROUP_INTEGRITY;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, CreateProcessW,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, GetCurrentProcessId, GetExitCodeProcess,
    INFINITE, OpenProcess, OpenProcessToken, PROCESS_CREATE_PROCESS, PROCESS_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOEXW, STARTUPINFOW, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetShellWindow, GetWindowThreadProcessId, SW_SHOWNORMAL,
};
use windows::core::{PCWSTR, PWSTR, w};

const HELPER_ARG: &str = "--argus-run-task";
const MAX_COMMAND: usize = 8192;
/// Helper exit codes about the helper itself, never a Win32 error from the
/// launch it was asked for. Bit 29 marks application-defined codes; the values
/// stay clear of the service's own (`argus_service::util`).
const HELPER_ELEVATED: u32 = 0x2000_0101;
const HELPER_ENVIRONMENT: u32 = 0x2000_0102;
static RUNNING: AtomicBool = AtomicBool::new(false);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
}

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

pub struct Handle(pub HANDLE);
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
pub struct Environment(pub *mut std::ffi::c_void);
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

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Windows quoting for the private helper's arguments, not a shell command.
pub fn quote(value: &str) -> String {
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
    // Win32 path normalization drops trailing spaces and dots, so "code ."
    // would otherwise match a folder named "code" and lose its argument.
    let existing = |path: &str| {
        let last = path.rsplit(['\\', '/']).next().unwrap_or(path);
        let normalized = matches!(last, "." | "..") || !last.ends_with([' ', '.']);
        normalized.then(|| existing(path)).flatten()
    };
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

pub fn token_buffer(
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

/// Whose desktop shell may start the helper.
#[derive(Clone, Copy, PartialEq)]
enum Shell {
    /// Only a shell running as this process's own account: a task keeps the
    /// account of the Argus that started it.
    OwnAccount,
    /// The account that owns the desktop. UAC may have approved the installer
    /// with another administrator's credentials; the installed app belongs
    /// to the person at the desktop.
    DesktopAccount,
}

enum ShellLaunch {
    /// The helper ran under the shell and reported the launch result.
    Done(Result<(), TaskError>),
    /// The shell is elevated, and so is everything it starts.
    Elevated,
    /// No helper ran under the shell, or it reported stopping before the
    /// launch; the stage and code say why.
    Unavailable(&'static str, u32),
}

fn helper_line(owner: usize, command: &str) -> Result<(Vec<u16>, Vec<u16>), TaskError> {
    let exe = std::env::current_exe()
        .map_err(|e| TaskError::code("current executable", e.raw_os_error().unwrap_or(2) as u32))?;
    let exe = exe.to_string_lossy();
    let line = wide(&format!(
        "{} {HELPER_ARG} {} {owner}",
        quote(&exe),
        quote(command)
    ));
    if line.len() > 32767 {
        return Err(TaskError::code("helper command size", 87));
    }
    Ok((wide(&exe), line))
}

fn same_account(process: HANDLE) -> bool {
    let user = |process: HANDLE| {
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
        let token = Handle(token);
        token_buffer(token.0, TokenUser).ok()
    };
    let (Some(theirs), Some(ours)) = (user(process), user(unsafe { GetCurrentProcess() })) else {
        return false;
    };
    unsafe {
        EqualSid(
            (*theirs.as_ptr().cast::<TOKEN_USER>()).User.Sid,
            (*ours.as_ptr().cast::<TOKEN_USER>()).User.Sid,
        )
    }
    .is_ok()
}

/// The process behind the desktop's shell window, opened to create a child
/// under it.
fn shell_process(scope: Shell) -> Result<Handle, (&'static str, u32)> {
    let shell_pid = || {
        let window = unsafe { GetShellWindow() };
        let mut pid = 0;
        if !window.0.is_null() {
            unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        }
        pid
    };
    let pid = shell_pid();
    if pid == 0 {
        return Err(("shell window", 1168));
    }
    let (mut theirs, mut ours) = (0, 0);
    if unsafe { ProcessIdToSessionId(pid, &mut theirs) } == 0
        || unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut ours) } == 0
        || theirs != ours
    {
        return Err(("shell session", 50));
    }
    let open = || unsafe {
        OpenProcess(
            PROCESS_CREATE_PROCESS | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
    };
    let process = match open() {
        // Another account's processes grant an administrator nothing by DACL.
        Err(e) if e.code() == E_ACCESSDENIED && argus_core::privilege::enable_debug_privilege() => {
            open()
        }
        result => result,
    }
    .map(Handle)
    .map_err(|e| ("shell process", TaskError::win("shell process", e).code))?;
    // The shell may have exited, and its process ID been reused, in between.
    if shell_pid() != pid {
        return Err(("shell process", 1168));
    }
    if scope == Shell::OwnAccount && !same_account(process.0) {
        return Err(("shell account", 5));
    }
    Ok(process)
}

/// Starts the helper as a child of `parent`: it runs with that process's
/// token, in its logon session. It still inherits this process's environment,
/// which the helper replaces itself (`adopt_account_environment`).
fn spawn_under(
    parent: HANDLE,
    exe: &[u16],
    line: &mut [u16],
) -> Result<PROCESS_INFORMATION, TaskError> {
    let mut directory = [0u16; 1024];
    let length = unsafe { GetSystemDirectoryW(Some(&mut directory)) } as usize;
    if length == 0 || length >= directory.len() {
        return Err(TaskError::code("system directory", 3));
    }
    let mut attribute = crate::explorer::ParentAttribute::new(parent)
        .map_err(|e| TaskError::win("parent attribute", e))?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attribute.pointer();
    let mut process = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(exe.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
            None,
            PCWSTR(directory.as_ptr()),
            &startup.StartupInfo,
            &mut process,
        )
    }
    .map_err(|e| TaskError::win("CreateProcessW", e))?;
    Ok(process)
}

fn helper_result(process: PROCESS_INFORMATION) -> Result<u32, TaskError> {
    let handle = Handle(process.hProcess);
    drop(Handle(process.hThread));
    unsafe { WaitForSingleObject(handle.0, INFINITE) };
    let mut code = 0;
    unsafe { GetExitCodeProcess(handle.0, &mut code) }
        .map_err(|e| TaskError::win("launch helper result", e))?;
    Ok(code)
}

/// Runs the helper as a child of the desktop shell, with the shell's own
/// token: the account's real standard token in the interactive logon session,
/// with its drive mappings and device namespace.
fn shell_helper(owner: usize, command: &str, scope: Shell) -> ShellLaunch {
    let parent = match shell_process(scope) {
        Ok(parent) => parent,
        Err((stage, code)) => return ShellLaunch::Unavailable(stage, code),
    };
    let (exe, mut line) = match helper_line(owner, command) {
        Ok(line) => line,
        Err(error) => return ShellLaunch::Done(Err(error)),
    };
    match spawn_under(parent.0, &exe, &mut line) {
        Ok(process) => shell_outcome(helper_result(process)),
        // No helper process exists, so nothing was started.
        Err(error) => ShellLaunch::Unavailable(error.stage, error.code),
    }
}

/// Only the helper's own reports that it stopped before the launch allow
/// another attempt. Any other end, a crash (an NTSTATUS exit code) or an
/// unknown exit code included, may come after the command already started:
/// it is the result, and the command is never started a second time.
fn shell_outcome(result: Result<u32, TaskError>) -> ShellLaunch {
    match result {
        Ok(0) => ShellLaunch::Done(Ok(())),
        Ok(HELPER_ELEVATED) => ShellLaunch::Elevated,
        Ok(HELPER_ENVIRONMENT) => ShellLaunch::Unavailable("shell helper", HELPER_ENVIRONMENT),
        Ok(code) => ShellLaunch::Done(Err(TaskError::code("launch helper", code))),
        Err(error) => ShellLaunch::Done(Err(error)),
    }
}

/// Filter this process's token; obtaining the linked token yields identification-level access.
/// The result stays in the elevated logon session, whose device namespace and
/// drive mappings differ from the desktop's, so this is the fallback for when
/// no desktop shell can start the helper.
fn filtered_helper(owner: usize, command: &str) -> Result<(), TaskError> {
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
    let (exe, mut line) = helper_line(owner, command)?;
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
    match helper_result(process)? {
        0 => Ok(()),
        HELPER_ELEVATED => Err(TaskError {
            kind: ErrorKind::StandardUnavailable,
            code: 5,
            stage: "launch helper",
        }),
        code => Err(TaskError::code("launch helper", code)),
    }
}

/// Starts `command` with the account's standard rights from this elevated
/// process: under the desktop shell when it runs as this account with standard
/// rights, otherwise with a token filtered from this process's own.
fn standard(owner: usize, command: &str) -> Result<(), TaskError> {
    match shell_helper(owner, command, Shell::OwnAccount) {
        ShellLaunch::Done(result) => return result,
        ShellLaunch::Elevated => {
            log::info!("standard launch: the desktop shell is elevated; using a filtered token");
        }
        ShellLaunch::Unavailable(stage, code) => log::info!(
            "standard launch: no desktop shell at {stage}; code=0x{code:08X}; using a filtered token"
        ),
    }
    filtered_helper(owner, command)
}

fn run_worker(
    work: impl FnOnce() -> Result<(), TaskError> + Send + 'static,
) -> Result<(), TaskError> {
    if RUNNING.swap(true, Ordering::Acquire) {
        return Err(TaskError {
            kind: ErrorKind::Busy,
            code: 170,
            stage: "already running",
        });
    }
    let _running = Running;
    std::thread::Builder::new()
        .name("argus-launch".into())
        .spawn(work)
        .map_err(|e| TaskError::code("launch worker", e.raw_os_error().unwrap_or(8) as u32))?
        .join()
        .unwrap_or_else(|_| Err(TaskError::code("launch worker", 31)))
}

/// Starts the installed app for the installer's finish page and the relaunch
/// after an update, the way the desktop shell would start it. An elevated
/// installer hands the launch to the shell, so the first run gets the same
/// token, logon session and account as every later one, even when UAC took
/// another administrator's credentials.
pub fn launch_installed(command: String) -> Result<(), TaskError> {
    if !valid(&command) {
        return Err(TaskError::code("validate", 87));
    }
    run_worker(move || {
        if !argus_core::privilege::is_elevated() {
            return activate(0, &command, false);
        }
        match shell_helper(0, &command, Shell::DesktopAccount) {
            ShellLaunch::Done(Ok(())) => Ok(()),
            // Unlike a new task, Argus may be started twice: a second start
            // only hands over to the first (single instance). So a helper
            // that failed, or crashed with the launch unknown, is retried.
            ShellLaunch::Done(Err(error)) => {
                log::info!(
                    "installed launch: shell helper failed at {}; code=0x{:08X}; using a filtered token",
                    error.stage,
                    error.code
                );
                filtered_helper(0, &command)
            }
            // An elevated shell (UAC off, the built-in Administrator) starts
            // everything elevated. There are no standard rights to return to,
            // and a filtered token would only differ from every later start.
            ShellLaunch::Elevated => activate(0, &command, false),
            ShellLaunch::Unavailable(..) => filtered_helper(0, &command),
        }
    })
}

pub fn launch(owner: usize, command: String, elevated: bool) -> Result<(), TaskError> {
    if !valid(&command) {
        return Err(TaskError::code("validate", 87));
    }
    let result = run_worker(move || {
        if !elevated && argus_core::privilege::is_elevated() {
            standard(owner, &command)
        } else {
            activate(owner, &command, elevated)
        }
    });
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

/// The characters of a double-NUL-terminated environment block, up to its
/// final entry's terminator.
///
/// # Safety
/// `block` must point to a block that `CreateEnvironmentBlock` or
/// `GetEnvironmentStringsW` returned and that outlives the slice.
unsafe fn environment_block<'a>(block: *const u16) -> &'a [u16] {
    const LIMIT: usize = 4 * 1024 * 1024;
    let mut len = 0;
    while len < LIMIT && unsafe { *block.add(len) != 0 || *block.add(len + 1) != 0 } {
        len += 1;
    }
    unsafe { std::slice::from_raw_parts(block, len) }
}

/// `NAME=value` entries of an environment block. A name may itself start with
/// `=` (the per-drive current directories), so the separator is searched from
/// the second character.
fn environment_entries(block: &[u16]) -> Vec<(&[u16], &[u16])> {
    block
        .split(|&c| c == 0)
        .filter_map(|entry| {
            let split = entry.iter().skip(1).position(|&c| c == u16::from(b'='))? + 1;
            Some((&entry[..split], &entry[split + 1..]))
        })
        .collect()
}

/// A child created under another process takes that process's token but its
/// creator's environment, and the creator may be an administrator signed in as
/// another account. Replace it with the environment Windows builds for the
/// account this process runs as, before anything reads `USERPROFILE`.
fn adopt_account_environment() -> Result<(), TaskError> {
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut token,
        )
    }
    .map_err(|e| TaskError::win("OpenProcessToken", e))?;
    let token = Handle(token);
    let mut block = std::ptr::null_mut();
    unsafe { CreateEnvironmentBlock(&mut block, Some(token.0), false) }
        .map_err(|e| TaskError::win("CreateEnvironmentBlock", e))?;
    let block = Environment(block);
    let wanted = environment_entries(unsafe { environment_block(block.0.cast()) });
    if wanted.is_empty() {
        return Err(TaskError::code("account environment", 203));
    }
    let set = |name: &[u16], value: Option<&[u16]>| {
        let name: Vec<u16> = name.iter().copied().chain(Some(0)).collect();
        let value: Option<Vec<u16>> = value.map(|v| v.iter().copied().chain(Some(0)).collect());
        unsafe {
            SetEnvironmentVariableW(
                PCWSTR(name.as_ptr()),
                value
                    .as_ref()
                    .map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr())),
            )
        }
    };
    let current = unsafe { GetEnvironmentStringsW() };
    if !current.is_null() {
        let stale: Vec<Vec<u16>> = environment_entries(unsafe { environment_block(current.0) })
            .into_iter()
            .map(|(name, _)| name.to_vec())
            .filter(|name| name.first() != Some(&u16::from(b'=')))
            .collect();
        let _ = unsafe { FreeEnvironmentStringsW(PCWSTR(current.0)) };
        for name in stale {
            let _ = set(&name, None);
        }
    }
    for (name, value) in wanted {
        set(name, Some(value)).map_err(|e| TaskError::win("SetEnvironmentVariableW", e))?;
    }
    Ok(())
}

/// An ordinary-rights helper exits before native window, single-instance or settings setup.
pub fn run_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_none_or(|s| s != HELPER_ARG) {
        return None;
    }
    if args.len() != 4 {
        return Some(5);
    }
    // Under an elevated shell this helper is elevated too; its starter decides
    // what a standard launch means then.
    if argus_core::privilege::is_elevated() {
        return Some(HELPER_ELEVATED as i32);
    }
    let Ok(owner) = args[3].parse::<usize>() else {
        return Some(87);
    };
    if adopt_account_environment().is_err() {
        return Some(HELPER_ENVIRONMENT as i32);
    }
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
    fn only_the_helper_s_own_report_allows_a_second_launch() {
        assert!(matches!(shell_outcome(Ok(0)), ShellLaunch::Done(Ok(()))));
        assert!(matches!(
            shell_outcome(Ok(HELPER_ELEVATED)),
            ShellLaunch::Elevated
        ));
        assert!(matches!(
            shell_outcome(Ok(HELPER_ENVIRONMENT)),
            ShellLaunch::Unavailable(_, HELPER_ENVIRONMENT)
        ));
        // Launch errors, a crash during or after ShellExecuteExW, a failed
        // start and unknown codes alike: the command may have started.
        for code in [2, 1155, 0xC000_0005, 0xC000_0142, 0xC000_0409, 259] {
            assert!(matches!(
                shell_outcome(Ok(code)),
                ShellLaunch::Done(Err(TaskError { code: reported, .. })) if reported == code
            ));
        }
        assert!(matches!(
            shell_outcome(Err(TaskError::code("launch helper result", 6))),
            ShellLaunch::Done(Err(_))
        ));
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
    fn trailing_dots_and_spaces_stay_arguments() {
        // The probe sees what Win32 would open after stripping the tail.
        let probe = |p: &str| match p.trim_end_matches([' ', '.']) {
            "code" => Some(Existing::Other),
            r"C:\Tools\app.exe" => Some(Existing::File),
            _ => None,
        };
        assert_eq!(split_command("code .", probe).unwrap(), ("code", "."));
        assert_eq!(
            split_command(r"C:\Tools\app.exe .", probe).unwrap(),
            (r"C:\Tools\app.exe", ".")
        );
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

    #[test]
    fn environment_blocks_split_at_the_first_separator_after_a_leading_equals() {
        let block: Vec<u16> = "=C:=C:\\dir\0PATH=a;b\0EMPTY=\0X=1=2\0\0"
            .encode_utf16()
            .collect();
        let text = |value: &[u16]| String::from_utf16_lossy(value);
        let entries: Vec<_> = environment_entries(unsafe { environment_block(block.as_ptr()) })
            .into_iter()
            .map(|(name, value)| (text(name), text(value)))
            .collect();
        assert_eq!(
            entries,
            [
                ("=C:", r"C:\dir"),
                ("PATH", "a;b"),
                ("EMPTY", ""),
                ("X", "1=2")
            ]
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
        );
        let empty = [0u16, 0];
        assert!(environment_entries(unsafe { environment_block(empty.as_ptr()) }).is_empty());
        let unset: Vec<u16> = "NOVALUE".encode_utf16().collect();
        assert!(environment_entries(&unset).is_empty());
    }

    /// The harness name of a test in this module, without the crate prefix.
    fn fixture(name: &str) -> String {
        format!("{}::{name}", module_path!().split_once("::").unwrap().1)
    }

    #[test]
    #[ignore = "child fixture used by the_helper_takes_over_its_account_s_environment"]
    fn adopt_environment_fixture() {
        use windows::Win32::UI::Shell::GetUserProfileDirectoryW;
        if std::env::var_os("ARGUS_TASKS_INHERITED").is_none() {
            return;
        }
        adopt_account_environment().unwrap();
        assert!(std::env::var_os("ARGUS_TASKS_INHERITED").is_none());
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.unwrap();
        let token = Handle(token);
        let mut profile = [0u16; 1024];
        let mut length = profile.len() as u32;
        unsafe {
            GetUserProfileDirectoryW(token.0, Some(PWSTR(profile.as_mut_ptr())), &mut length)
        }
        .unwrap();
        let profile = String::from_utf16_lossy(&profile[..length as usize - 1]);
        assert!(
            std::env::var("USERPROFILE")
                .unwrap()
                .eq_ignore_ascii_case(&profile)
        );
        assert!(std::env::var_os("SystemRoot").is_some() && std::env::var_os("PATH").is_some());
    }

    #[test]
    fn the_helper_takes_over_its_account_s_environment() {
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &fixture("adopt_environment_fixture"),
                "--ignored",
            ])
            .env("ARGUS_TASKS_INHERITED", "1")
            .env("USERPROFILE", r"X:\argus-inherited-profile")
            .creation_flags(CREATE_NO_WINDOW.0)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success());
        // The harness also succeeds when the name matches no test.
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }

    #[test]
    #[ignore = "child fixture that stays alive for the process tests"]
    fn waiting_fixture() {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }

    struct Waiting(std::process::Child);
    impl Waiting {
        fn spawn() -> Self {
            use std::os::windows::process::CommandExt;
            Self(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", &fixture("waiting_fixture"), "--ignored"])
                    .creation_flags(CREATE_NO_WINDOW.0)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        }
    }
    impl Drop for Waiting {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Starts the waiting fixture under `parent` and returns the parent
    /// Windows recorded for it.
    fn parent_of_a_child_under(parent: HANDLE) -> Option<u32> {
        use windows::Win32::System::Threading::TerminateProcess;
        let exe = std::env::current_exe().unwrap();
        let exe = exe.to_string_lossy();
        let mut line = wide(&format!(
            "{} --exact {} --ignored",
            quote(&exe),
            fixture("waiting_fixture")
        ));
        let child = spawn_under(parent, &wide(&exe), &mut line).unwrap();
        let process = Handle(child.hProcess);
        drop(Handle(child.hThread));
        let recorded = argus_core::process::find_by(|_| true)
            .into_iter()
            .find(|found| found.pid == child.dwProcessId)
            .map(|found| found.parent_pid);
        unsafe {
            let _ = TerminateProcess(process.0, 0);
            WaitForSingleObject(process.0, 5000);
        }
        recorded
    }

    #[test]
    fn the_helper_is_created_as_a_child_of_the_given_process() {
        let parent = Waiting::spawn();
        let handle = Handle(
            unsafe {
                OpenProcess(
                    PROCESS_CREATE_PROCESS | PROCESS_QUERY_LIMITED_INFORMATION,
                    false,
                    parent.0.id(),
                )
            }
            .unwrap(),
        );
        assert_eq!(parent_of_a_child_under(handle.0), Some(parent.0.id()));
    }

    fn shell() -> (Handle, u32) {
        let shell = shell_process(Shell::OwnAccount)
            .unwrap_or_else(|(stage, code)| panic!("{stage}: code {code}"));
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(GetShellWindow(), Some(&mut pid)) };
        (shell, pid)
    }

    #[test]
    #[ignore = "starts a hidden child under the desktop shell with current rights; shows and elevates nothing"]
    fn live_shell_child() {
        let (shell, pid) = shell();
        assert_eq!(parent_of_a_child_under(shell.0), Some(pid));
    }

    #[test]
    #[ignore = "needs ARGUS_TASKS_HOST=<built argus.exe>: its helper starts dllhost.exe under the desktop shell, which exits at once without a window"]
    fn live_shell_helper() {
        let host = std::env::var("ARGUS_TASKS_HOST").expect("ARGUS_TASKS_HOST");
        let (shell, _) = shell();
        let mut directory = [0u16; 1024];
        let length = unsafe { GetSystemDirectoryW(Some(&mut directory)) } as usize;
        let system = String::from_utf16_lossy(&directory[..length]);
        for (target, expected) in [("dllhost.exe", 0), ("argus-missing-helper-target.exe", 2)] {
            let mut line = wide(&format!(
                "{} {HELPER_ARG} {} 0",
                quote(&host),
                quote(&format!(r"{system}\{target}"))
            ));
            let code = spawn_under(shell.0, &wide(&host), &mut line)
                .and_then(helper_result)
                .unwrap();
            assert_eq!(code, expected, "{target}");
        }
    }
}
