//! Restart only the current desktop shell. A helper keeps the shell's own identity.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS, FILETIME, HANDLE};
use windows::Win32::Security::{
    DuplicateTokenEx, EqualSid, SecurityImpersonation, TOKEN_ADJUST_DEFAULT,
    TOKEN_ADJUST_SESSIONID, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_QUERY,
    TOKEN_USER, TokenElevation, TokenPrimary, TokenSessionId, TokenUser,
};
use windows::Win32::System::Environment::CreateEnvironmentBlock;
use windows::Win32::System::RestartManager::{
    CCH_RM_SESSION_KEY, RM_PROCESS_INFO, RM_UNIQUE_PROCESS, RmEndSession, RmExplorer, RmGetList,
    RmRegisterResources, RmRestart, RmShutdown, RmShutdownOnlyRegistered, RmStartSession,
};
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_PROCESS_LOGON_FLAGS, CREATE_UNICODE_ENVIRONMENT,
    CreateProcessWithTokenW, GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, INFINITE,
    OpenProcess, OpenProcessToken, PROCESS_INFORMATION, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, STARTUPINFOW,
    WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetShellWindow, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::{PCWSTR, PWSTR, w};

use crate::tasks::{Environment, Handle, quote, token_buffer, wide};

const HELPER: &str = "--argus-restart-explorer";
static RESTARTING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy)]
enum Stage {
    Validate,
    Token,
    Environment,
    HelperLaunch,
    HelperResult,
    SessionStart,
    Register,
    Resources,
    Shutdown,
    Restart,
    Recovery,
    DesktopReady,
    Image,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    Stale = 1,
    Unsupported,
    Permission,
    Prepare,
    Shutdown,
    Restore,
    Busy,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartError {
    kind: ErrorKind,
    code: u32,
    pub shell_missing: bool,
    #[serde(skip)]
    stage: Stage,
}

impl RestartError {
    fn new(kind: ErrorKind, code: u32) -> Self {
        Self {
            kind,
            code,
            shell_missing: shell_pid() == 0,
            stage: Stage::Validate,
        }
    }
    fn at(mut self, stage: Stage) -> Self {
        self.stage = stage;
        self
    }
    fn win(kind: ErrorKind, error: windows::core::Error) -> Self {
        let code = error.code().0 as u32;
        Self::new(
            kind,
            if code & 0xffff0000 == 0x80070000 {
                code & 0xffff
            } else {
                code
            },
        )
    }
    // Private helper exit codes keep the stage and native Win32 failure together.
    fn exit_code(&self) -> u32 {
        if self.code > 0xffff {
            return self.code;
        }
        0x60000000 | ((self.kind as u32) << 20) | ((self.stage as u32) << 16) | self.code
    }
    fn from_exit(code: u32) -> Self {
        let kind = if code & 0xff000000 == 0x60000000 {
            match (code >> 20) & 0xf {
                1 => ErrorKind::Stale,
                2 => ErrorKind::Unsupported,
                3 => ErrorKind::Permission,
                4 => ErrorKind::Prepare,
                5 => ErrorKind::Shutdown,
                6 => ErrorKind::Restore,
                7 => ErrorKind::Busy,
                _ => ErrorKind::Prepare,
            }
        } else {
            ErrorKind::Prepare
        };
        Self::new(
            kind,
            if code & 0xff000000 == 0x60000000 {
                code & 0xffff
            } else {
                code
            },
        )
        .at(if code & 0xff000000 == 0x60000000 {
            match (code >> 16) & 0xf {
                0 => Stage::Validate,
                1 => Stage::Token,
                2 => Stage::Environment,
                3 => Stage::HelperLaunch,
                4 => Stage::HelperResult,
                5 => Stage::SessionStart,
                6 => Stage::Register,
                7 => Stage::Resources,
                8 => Stage::Shutdown,
                9 => Stage::Restart,
                10 => Stage::Recovery,
                11 => Stage::DesktopReady,
                _ => Stage::Image,
            }
        } else {
            Stage::HelperResult
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub pid: u32,
    pub create_time: i64,
}

struct Shell {
    identity: Identity,
    started: FILETIME,
    process: Handle,
    path: String,
}
struct Session(u32);
impl Drop for Session {
    fn drop(&mut self) {
        let _ = unsafe { RmEndSession(self.0) };
    }
}
struct Restarting;
impl Drop for Restarting {
    fn drop(&mut self) {
        RESTARTING.store(false, Ordering::Release);
    }
}

fn shell_pid() -> u32 {
    let window = unsafe { GetShellWindow() };
    let mut pid = 0;
    if !window.0.is_null() {
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    }
    pid
}

fn system_explorer() -> Result<String, RestartError> {
    let mut buffer = [0u16; 32768];
    let len = unsafe { GetWindowsDirectoryW(Some(&mut buffer)) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err(RestartError::new(ErrorKind::Prepare, 3));
    }
    Ok(format!(
        "{}\\explorer.exe",
        String::from_utf16_lossy(&buffer[..len])
    ))
}

fn inspect() -> Result<Shell, RestartError> {
    let pid = shell_pid();
    if pid == 0 {
        return Err(RestartError::new(ErrorKind::Stale, 1168));
    }
    let process = Handle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(|e| RestartError::win(ErrorKind::Permission, e))?,
    );
    let (mut started, mut ended, mut kernel, mut user) = Default::default();
    unsafe { GetProcessTimes(process.0, &mut started, &mut ended, &mut kernel, &mut user) }
        .map_err(|e| RestartError::win(ErrorKind::Prepare, e))?;
    let mut buffer = [0u16; 32768];
    let mut len = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    }
    .map_err(|e| RestartError::win(ErrorKind::Prepare, e))?;
    let path = system_explorer()?;
    if !String::from_utf16_lossy(&buffer[..len as usize]).eq_ignore_ascii_case(&path) {
        return Err(RestartError::new(ErrorKind::Unsupported, 50));
    }
    let ticks = ((started.dwHighDateTime as u64) << 32) | started.dwLowDateTime as u64;
    let identity = Identity {
        pid,
        create_time: (ticks / 10000) as i64 - 11644473600000,
    };
    if shell_pid() != pid {
        return Err(RestartError::new(ErrorKind::Stale, 1168));
    }
    Ok(Shell {
        identity,
        started,
        process,
        path,
    })
}

pub fn current() -> Option<Identity> {
    inspect().ok().map(|s| s.identity)
}

fn token(process: HANDLE, duplicate: bool) -> Result<Handle, RestartError> {
    let mut handle = HANDLE::default();
    unsafe {
        OpenProcessToken(
            process,
            TOKEN_QUERY
                | if duplicate {
                    TOKEN_DUPLICATE
                } else {
                    TOKEN_QUERY
                },
            &mut handle,
        )
    }
    .map_err(|e| RestartError::win(ErrorKind::Permission, e))?;
    Ok(Handle(handle))
}

fn token_data(
    handle: HANDLE,
    class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<Vec<usize>, RestartError> {
    token_buffer(handle, class).map_err(|e| RestartError::new(ErrorKind::Permission, e.code))
}

fn verify_tokens(shell: HANDLE, own: HANDLE, same_user: bool) -> Result<(), RestartError> {
    let elevation = token_data(shell, TokenElevation)?;
    let session = token_data(shell, TokenSessionId)?;
    let own_session = token_data(own, TokenSessionId)?;
    if unsafe { (*elevation.as_ptr().cast::<TOKEN_ELEVATION>()).TokenIsElevated } != 0
        || session[0] != own_session[0]
    {
        return Err(RestartError::new(ErrorKind::Unsupported, 50));
    }
    if same_user {
        let user = token_data(shell, TokenUser)?;
        let own_user = token_data(own, TokenUser)?;
        let equal = unsafe {
            EqualSid(
                (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid,
                (*own_user.as_ptr().cast::<TOKEN_USER>()).User.Sid,
            )
        }
        .is_ok();
        if !equal {
            return Err(RestartError::new(ErrorKind::Permission, 5));
        }
    }
    Ok(())
}

fn ready(old: Identity) -> bool {
    let Some(shell) = current() else { return false };
    if shell == old {
        return false;
    }
    let Ok(taskbar) = (unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }) else {
        return false;
    };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(taskbar, Some(&mut pid)) };
    pid == shell.pid && unsafe { IsWindowVisible(taskbar) }.as_bool()
}

fn wait_ready(old: Identity, timeout: Duration) -> bool {
    let until = Instant::now() + timeout;
    loop {
        if ready(old) {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn restart_standard(shell: Shell) -> Result<(), RestartError> {
    let shell_token = token(shell.process.0, false)?;
    let own_token = token(unsafe { GetCurrentProcess() }, false)?;
    verify_tokens(shell_token.0, own_token.0, true)?;
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
    let mut handle = 0;
    let result = unsafe { RmStartSession(&mut handle, None, PWSTR(key.as_mut_ptr())) };
    if result != ERROR_SUCCESS {
        return Err(RestartError::new(ErrorKind::Prepare, result.0).at(Stage::SessionStart));
    }
    let session = Session(handle);
    let target = RM_UNIQUE_PROCESS {
        dwProcessId: shell.identity.pid,
        ProcessStartTime: shell.started,
    };
    let result = unsafe { RmRegisterResources(session.0, None, Some(&[target]), None) };
    if result != ERROR_SUCCESS {
        return Err(RestartError::new(ErrorKind::Prepare, result.0).at(Stage::Register));
    }
    let mut infos: [RM_PROCESS_INFO; 2] = unsafe { std::mem::zeroed() };
    let (mut needed, mut count, mut reasons) = (0, infos.len() as u32, 0);
    let result = unsafe {
        RmGetList(
            session.0,
            &mut needed,
            &mut count,
            Some(infos.as_mut_ptr()),
            &mut reasons,
        )
    };
    if result != ERROR_SUCCESS && result != ERROR_MORE_DATA {
        return Err(RestartError::new(ErrorKind::Prepare, result.0).at(Stage::Resources));
    }
    if result != ERROR_SUCCESS
        || count != 1
        || reasons != 0
        || infos[0].Process.dwProcessId != shell.identity.pid
        || infos[0].Process.ProcessStartTime != shell.started
        || infos[0].ApplicationType != RmExplorer
        || !infos[0].bRestartable.as_bool()
    {
        return Err(RestartError::new(ErrorKind::Unsupported, 50).at(Stage::Resources));
    }
    if current() != Some(shell.identity) {
        return Err(RestartError::new(ErrorKind::Stale, 1168));
    }
    let shutdown = unsafe { RmShutdown(session.0, RmShutdownOnlyRegistered.0 as u32, None) };
    // Restart is required even when shutdown only partly succeeded.
    let restart = unsafe { RmRestart(session.0, None, None) };
    if wait_ready(shell.identity, Duration::from_secs(10)) {
        return Ok(());
    }
    if shell_pid() != 0 {
        return Err(RestartError::new(
            ErrorKind::Shutdown,
            if shutdown.0 != 0 { shutdown.0 } else { 1460 },
        )
        .at(Stage::Shutdown));
    }
    // The same standard user owns this worker. Never fall back to an elevated shell.
    if let Err(error) = crate::tasks::launch(0, quote(&shell.path), false) {
        return Err(RestartError::new(ErrorKind::Restore, error.code).at(Stage::Recovery));
    }
    if wait_ready(shell.identity, Duration::from_secs(15)) {
        return Ok(());
    }
    Err(RestartError::new(
        ErrorKind::Restore,
        if restart.0 != 0 { restart.0 } else { 1460 },
    )
    .at(Stage::DesktopReady))
}

fn restart_as_shell(shell: Shell) -> Result<(), RestartError> {
    let shell_token = token(shell.process.0, true)?;
    let own_token = token(unsafe { GetCurrentProcess() }, false)?;
    verify_tokens(shell_token.0, own_token.0, false)?;
    let mut primary = HANDLE::default();
    unsafe {
        DuplicateTokenEx(
            shell_token.0,
            // Secondary Logon also requires these two rights on the duplicated token.
            TOKEN_QUERY
                | TOKEN_DUPLICATE
                | TOKEN_ASSIGN_PRIMARY
                | TOKEN_ADJUST_DEFAULT
                | TOKEN_ADJUST_SESSIONID,
            None,
            SecurityImpersonation,
            TokenPrimary,
            &mut primary,
        )
    }
    .map_err(|e| RestartError::win(ErrorKind::Permission, e))?;
    let primary = Handle(primary);
    let mut environment = std::ptr::null_mut();
    unsafe { CreateEnvironmentBlock(&mut environment, Some(primary.0), false) }
        .map_err(|e| RestartError::win(ErrorKind::Prepare, e).at(Stage::Environment))?;
    let environment = Environment(environment);
    let exe = std::env::current_exe()
        .map_err(|e| RestartError::new(ErrorKind::Prepare, e.raw_os_error().unwrap_or(2) as u32))?;
    let exe = exe.to_string_lossy();
    let mut line = wide(&format!(
        "{} {HELPER} {} {}",
        quote(&exe),
        shell.identity.pid,
        shell.identity.create_time
    ));
    let exe = wide(&exe);
    let mut desktop = wide("WinSta0\\Default");
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        lpDesktop: PWSTR(desktop.as_mut_ptr()),
        ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // Start before shutting anything down; inaccessible binaries or tokens leave the desktop intact.
    unsafe {
        CreateProcessWithTokenW(
            primary.0,
            CREATE_PROCESS_LOGON_FLAGS(0),
            PCWSTR(exe.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            Some(environment.0),
            PCWSTR::null(),
            &startup,
            &mut process,
        )
    }
    .map_err(|e| RestartError::win(ErrorKind::Prepare, e).at(Stage::HelperLaunch))?;
    let child = Handle(process.hProcess);
    drop(Handle(process.hThread));
    unsafe { WaitForSingleObject(child.0, INFINITE) };
    let mut code = 0;
    unsafe { GetExitCodeProcess(child.0, &mut code) }
        .map_err(|e| RestartError::win(ErrorKind::Prepare, e))?;
    if code == 0 {
        Ok(())
    } else {
        Err(RestartError::from_exit(code))
    }
}

pub fn restart(pid: u32, create_time: i64) -> Result<(), RestartError> {
    if RESTARTING.swap(true, Ordering::Acquire) {
        return Err(RestartError::new(ErrorKind::Busy, 170));
    }
    let _guard = Restarting;
    let result = (|| {
        let shell = inspect()?;
        if shell.identity != (Identity { pid, create_time }) {
            return Err(RestartError::new(ErrorKind::Stale, 1168));
        }
        if argus_core::privilege::is_elevated() {
            restart_as_shell(shell)
        } else {
            restart_standard(shell)
        }
    })();
    if let Err(error) = &result {
        log::warn!(
            "Explorer restart failed: kind={:?}; stage={:?}; code=0x{:08X}; shell_missing={}",
            error.kind,
            error.stage,
            error.code,
            error.shell_missing
        );
    }
    result
}

pub fn run_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_none_or(|s| s != HELPER) {
        return None;
    }
    let result = if args.len() != 4 || argus_core::privilege::is_elevated() {
        Err(RestartError::new(ErrorKind::Permission, 5))
    } else {
        match (args[2].parse(), args[3].parse()) {
            (Ok(pid), Ok(created)) => restart(pid, created),
            _ => Err(RestartError::new(ErrorKind::Stale, 87)),
        }
    };
    Some(result.err().map_or(0, |e| e.exit_code() as i32))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_errors_preserve_stage_and_native_code() {
        for kind in [
            ErrorKind::Stale,
            ErrorKind::Unsupported,
            ErrorKind::Permission,
            ErrorKind::Prepare,
            ErrorKind::Shutdown,
            ErrorKind::Restore,
            ErrorKind::Busy,
        ] {
            let original = RestartError::new(kind, 1223);
            let restored = RestartError::from_exit(original.exit_code());
            assert_eq!(restored.kind as u32, kind as u32);
            assert_eq!(restored.code, 1223);
        }
        assert_eq!(RestartError::from_exit(0xc0000142).code, 0xc0000142);
    }
}
