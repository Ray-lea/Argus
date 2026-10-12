use super::state::Result;
use crate::util::{Handle, wide};
use std::path::Path;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::Security::{
    DuplicateTokenEx, GetTokenInformation, SecurityImpersonation, TOKEN_ASSIGN_PRIMARY,
    TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_LINKED_TOKEN, TOKEN_QUERY, TokenElevation,
    TokenLinkedToken, TokenPrimary,
};
use windows::Win32::System::Diagnostics::Debug::{
    DebugActiveProcessStop, DebugSetProcessKillOnExit,
};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::*;
use windows::core::{PCWSTR, PWSTR};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
}

pub fn quote(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        result.push_str(&"\\".repeat(if c == '"' { slashes * 2 + 1 } else { slashes }));
        slashes = 0;
        result.push(c);
    }
    result.push_str(&"\\".repeat(slashes * 2));
    result.push('"');
    result
}

pub fn system_directory() -> Result<std::path::PathBuf> {
    let mut buffer = [0u16; 1024];
    let n = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    if n == 0 || n >= buffer.len() {
        return Err("无法定位 Windows 系统目录".into());
    }
    Ok(String::from_utf16_lossy(&buffer[..n]).into())
}

/// The caller supplies a fixed executable, never an IFEO-appended command.
/// The debug connection is detached before resuming, on the same OS thread.
pub fn spawn(exe: &Path, args: &[String], bypass: bool) -> Result<Handle> {
    spawn_impl(exe, args, bypass, false, None)
}

pub fn standard_spawn(exe: &Path, args: &[String]) -> Result<Handle> {
    spawn_impl(exe, args, false, true, None)
}

pub fn probe_as_caller(
    exe: &Path,
    args: &[String],
    pid: u32,
    created: u64,
    owner: &str,
) -> Result<Handle> {
    let process = Handle(
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .map_err(|e| e.to_string())?,
    );
    if identity(process.0)? != created
        || unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0
    {
        return Err("发起操作的界面已退出".into());
    }
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process.0, TOKEN_QUERY | TOKEN_DUPLICATE, &mut token) }
        .map_err(|e| e.to_string())?;
    let token = Handle(token);
    if crate::security::token_user(token.0).map_err(crate::util::message)? != owner {
        return Err("发起操作的账户不匹配".into());
    }
    let mut caller_session = 0;
    let mut session = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut caller_session) } == 0
        || unsafe { ProcessIdToSessionId(std::process::id(), &mut session) } == 0
    {
        return Err(windows::core::Error::from_thread().to_string());
    }
    if caller_session != session {
        return Err("发起操作的会话不匹配".into());
    }
    let limited = limited_token(token)?;
    spawn_impl(exe, args, false, false, Some(limited.0))
}

fn is_high(token: HANDLE) -> Result<bool> {
    let mut elevation = TOKEN_ELEVATION::default();
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of_val(&elevation) as u32,
            &mut size,
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(elevation.TokenIsElevated != 0)
}

fn limited_token(token: Handle) -> Result<Handle> {
    if !is_high(token.0)? {
        return primary(token.0);
    }
    let mut linked = TOKEN_LINKED_TOKEN::default();
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenLinkedToken,
            Some((&mut linked as *mut TOKEN_LINKED_TOKEN).cast()),
            std::mem::size_of_val(&linked) as u32,
            &mut size,
        )
    }
    .map_err(|e| e.to_string())?;
    let linked = Handle(linked.LinkedToken);
    if is_high(linked.0)? {
        return Err("无法获得当前账户的标准权限令牌".into());
    }
    primary(linked.0)
}

fn primary(token: HANDLE) -> Result<Handle> {
    let mut result = HANDLE::default();
    unsafe {
        DuplicateTokenEx(
            token,
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
            None,
            SecurityImpersonation,
            TokenPrimary,
            &mut result,
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(Handle(result))
}

fn spawn_impl(
    exe: &Path,
    args: &[String],
    bypass: bool,
    standard: bool,
    as_token: Option<HANDLE>,
) -> Result<Handle> {
    let file = wide(&exe.to_string_lossy());
    let mut command = wide(
        &std::iter::once(exe.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(|a| quote(&a))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let directory = wide(&system_directory()?.to_string_lossy());
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    let flags = CREATE_NO_WINDOW
        | if bypass {
            DEBUG_ONLY_THIS_PROCESS | CREATE_SUSPENDED
        } else {
            Default::default()
        };
    let limited = if standard && argus_core::privilege::is_elevated() {
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .map_err(|e| e.to_string())?;
        Some(limited_token(Handle(token))?)
    } else {
        None
    };
    if let Some(token) = as_token.or(limited.as_ref().map(|t| t.0)) {
        let mut environment = std::ptr::null_mut();
        unsafe { CreateEnvironmentBlock(&mut environment, Some(token), false) }
            .map_err(|e| e.to_string())?;
        let result = unsafe {
            CreateProcessWithTokenW(
                token,
                LOGON_WITH_PROFILE,
                PCWSTR(file.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                flags | CREATE_UNICODE_ENVIRONMENT,
                Some(environment),
                PCWSTR(directory.as_ptr()),
                &startup,
                &mut info,
            )
        };
        unsafe {
            let _ = DestroyEnvironmentBlock(environment);
        }
        result.map_err(|e| e.to_string())?;
    } else {
        unsafe {
            CreateProcessW(
                PCWSTR(file.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                false,
                flags,
                None,
                PCWSTR(directory.as_ptr()),
                &startup,
                &mut info,
            )
        }
        .map_err(|e| e.to_string())?;
    }
    let process = Handle(info.hProcess);
    let thread = Handle(info.hThread);
    if bypass {
        let result = (|| {
            unsafe {
                DebugSetProcessKillOnExit(false)?;
                DebugActiveProcessStop(info.dwProcessId)?;
            }
            if unsafe { ResumeThread(thread.0) } == u32::MAX {
                return Err(windows::core::Error::from_thread());
            }
            Ok(())
        })();
        if let Err(error) = result {
            unsafe {
                let _ = TerminateProcess(process.0, 1);
                WaitForSingleObject(process.0, 1000);
            }
            return Err(format!("原生启动脱离调试失败：{error}"));
        }
    }
    Ok(process)
}

pub fn exit_code(process: &Handle, timeout: Duration) -> Result<u32> {
    if unsafe { WaitForSingleObject(process.0, timeout.as_millis().min(u32::MAX as u128) as u32) }
        != WAIT_OBJECT_0
    {
        unsafe {
            let _ = TerminateProcess(process.0, 1);
            WaitForSingleObject(process.0, 1000);
        }
        return Err("启动自检超时".into());
    }
    let mut code = 0;
    unsafe { GetExitCodeProcess(process.0, &mut code) }.map_err(|e| e.to_string())?;
    Ok(code)
}

pub fn native() -> Result<()> {
    spawn(&system_directory()?.join("taskmgr.exe"), &[], true).map(|_| ())
}

pub fn identity(process: HANDLE) -> Result<u64> {
    let (mut c, mut e, mut k, mut u) = Default::default();
    unsafe { GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u) }
        .map_err(|e| e.to_string())?;
    Ok((u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
}

pub fn wait(event: HANDLE, parent: HANDLE, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    loop {
        if unsafe { WaitForSingleObject(parent, 0) } == WAIT_OBJECT_0 {
            return false;
        }
        if unsafe { WaitForSingleObject(event, 20) } == WAIT_OBJECT_0 {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "child fixture used by detached_debug_start_runs_to_completion"]
    fn native_child() {
        assert_eq!(2 + 2, 4);
    }
    #[test]
    fn detached_debug_start_runs_to_completion() {
        let process = spawn(
            &std::env::current_exe().unwrap(),
            &[
                "--exact".into(),
                "taskmgr::launch::tests::native_child".into(),
                "--ignored".into(),
            ],
            true,
        )
        .unwrap();
        assert_eq!(exit_code(&process, Duration::from_secs(5)).unwrap(), 0);
    }
    #[test]
    fn windows_arguments_are_quoted_without_shell_interpretation() {
        assert_eq!(quote(r"C:\with space\"), "\"C:\\with space\\\\\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
    }
}
