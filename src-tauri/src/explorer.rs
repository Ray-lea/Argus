//! Restart only the current desktop shell, retaining its account and desktop.

mod process;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WAIT_FAILED, WAIT_OBJECT_0, WPARAM};
use windows::Win32::Security::{
    EqualSid, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER, TokenElevation,
    TokenSessionId, TokenUser,
};
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW,
    WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetShellWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    SMTO_ABORTIFHUNG, SMTO_BLOCK, SendMessageTimeoutW, WM_NULL,
};
use windows::core::{PCWSTR, PWSTR, w};

use crate::tasks::{Handle, token_buffer};

const HELPER: &str = "--argus-restart-explorer";
const EXIT_EXPLORER: u32 = 0x5B4;
static RESTARTING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy)]
enum Stage {
    Validate = 0,
    Token = 1,
    Environment = 2,
    Spawn = 3,
    Shutdown = 8,
    Restart = 9,
    DesktopReady = 11,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub pid: u32,
    pub create_time: i64,
}

struct Shell {
    identity: Identity,
    process: Handle,
    path: String,
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
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
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
        process,
        path,
    })
}

pub fn current() -> Option<Identity> {
    inspect().ok().map(|s| s.identity)
}

/// All UI termination routes share this check before reaching a privileged backend.
pub fn check_termination(pid: u32) -> Result<(), argus_core::actions::ActionError> {
    if pid != 0 && pid == shell_pid() {
        return Err(argus_core::actions::ActionError::Os(argus_core::tr!(
            "host.desktopShellTerminate"
        )));
    }
    Ok(())
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

fn taskbar(pid: u32) -> Option<HWND> {
    let Ok(taskbar) = (unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }) else {
        return None;
    };
    let mut owner = 0;
    unsafe { GetWindowThreadProcessId(taskbar, Some(&mut owner)) };
    (owner == pid && unsafe { IsWindowVisible(taskbar) }.as_bool()).then_some(taskbar)
}

fn responsive(window: HWND) -> bool {
    !window.0.is_null()
        && unsafe {
            SendMessageTimeoutW(
                window,
                WM_NULL,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                1000,
                None,
            )
        }
        .0 != 0
}

fn ready(old: Identity) -> Option<Identity> {
    let shell = current()?;
    (shell != old
        && responsive(unsafe { GetShellWindow() })
        && taskbar(shell.pid).is_some_and(responsive)
        && current() == Some(shell))
    .then_some(shell)
}

fn wait_ready(old: Identity, timeout: Duration) -> bool {
    let until = Instant::now() + timeout;
    let mut candidate: Option<(Identity, Instant)> = None;
    loop {
        match ready(old) {
            Some(shell) => match candidate {
                Some((previous, since)) if previous == shell => {
                    if since.elapsed() >= Duration::from_secs(1) {
                        return true;
                    }
                }
                _ => candidate = Some((shell, Instant::now())),
            },
            None => candidate = None,
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn restart_shell(shell: Shell) -> Result<(), RestartError> {
    let old = shell.identity;
    let tray = taskbar(old.pid).ok_or_else(|| RestartError::new(ErrorKind::Unsupported, 50))?;
    if !responsive(unsafe { GetShellWindow() }) || !responsive(tray) {
        return Err(RestartError::new(ErrorKind::Shutdown, 1460).at(Stage::Shutdown));
    }
    let prepared = process::Prepared::new(&shell)?;
    if current() != Some(old) || taskbar(old.pid) != Some(tray) {
        return Err(RestartError::new(ErrorKind::Stale, 1168));
    }
    log::info!(
        "Explorer restart: replacement prepared; old_pid={}",
        old.pid
    );
    // Explorer's own Exit Explorer request (also used by ExplorerPatcher's
    // ExitExplorer) avoids Restart Manager's session-ending notifications.
    unsafe { PostMessageW(Some(tray), EXIT_EXPLORER, WPARAM(0), LPARAM(0)) }
        .map_err(|e| RestartError::win(ErrorKind::Shutdown, e).at(Stage::Shutdown))?;
    // An exit code or a vanished window is not proof that teardown finished.
    // Do not start another shell while the old process is still shutting down.
    match unsafe { WaitForSingleObject(shell.process.0, 30_000) } {
        WAIT_OBJECT_0 => {}
        WAIT_FAILED => {
            let error = RestartError::win(ErrorKind::Shutdown, windows::core::Error::from_thread());
            return Err(error.at(Stage::Shutdown));
        }
        _ => return Err(RestartError::new(ErrorKind::Shutdown, 1460).at(Stage::Shutdown)),
    }
    drop(shell);
    prepared.start()?;
    if !wait_ready(old, Duration::from_secs(15)) {
        return Err(RestartError::new(ErrorKind::Restore, 1460).at(Stage::DesktopReady));
    }
    log::info!(
        "Explorer restart: desktop and taskbar responsive; new_pid={}",
        shell_pid()
    );
    Ok(())
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
        restart_shell(shell)
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
    fn termination_policy_protects_the_current_desktop_process() {
        let shell = shell_pid();
        if shell != 0 {
            assert!(matches!(
                check_termination(shell),
                Err(argus_core::actions::ActionError::Os(_))
            ));
        }
        assert!(check_termination(u32::MAX).is_ok());
    }

    #[test]
    fn a_window_must_answer_messages_before_it_is_ready() {
        use std::sync::mpsc;
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, MSG, PostThreadMessageW,
            WM_QUIT, WS_POPUP,
        };
        let (created_tx, created_rx) = mpsc::channel();
        let (pump_tx, pump_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || unsafe {
            let window = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            created_tx
                .send((window.0 as usize, GetCurrentThreadId()))
                .unwrap();
            pump_rx.recv().unwrap();
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                DispatchMessageW(&message);
            }
            DestroyWindow(window).unwrap();
        });
        let (window, owner) = created_rx.recv().unwrap();
        let window = HWND(window as *mut _);
        let blocked = responsive(window);
        pump_tx.send(()).unwrap();
        let pumping = responsive(window);
        unsafe { PostThreadMessageW(owner, WM_QUIT, WPARAM(0), LPARAM(0)) }.unwrap();
        thread.join().unwrap();
        assert!(!blocked);
        assert!(pumping);
        assert!(!responsive(HWND::default()));
    }

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
            let original = RestartError::new(kind, 1223).at(Stage::Shutdown);
            assert_eq!((original.exit_code() >> 20) & 0xf, kind as u32);
            assert_eq!((original.exit_code() >> 16) & 0xf, Stage::Shutdown as u32);
            assert_eq!(original.exit_code() & 0xffff, 1223);
        }
        assert_eq!(
            RestartError::new(ErrorKind::Prepare, 0xc0000142).exit_code(),
            0xc0000142
        );
    }
}
