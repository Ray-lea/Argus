//! An elevated WebView2 needs the desktop shell for its normal de-elevation path.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    DuplicateTokenEx, EqualSid, GetSidSubAuthority, GetSidSubAuthorityCount, PSID,
    SecurityImpersonation, TOKEN_ADJUST_DEFAULT, TOKEN_ADJUST_SESSIONID, TOKEN_ASSIGN_PRIMARY,
    TOKEN_DUPLICATE, TOKEN_GROUPS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER, TokenElevation,
    TokenGroups, TokenIntegrityLevel, TokenIsAppContainer, TokenPrimary, TokenSessionId, TokenUser,
};
use windows::Win32::System::Environment::CreateEnvironmentBlock;
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_PROCESS_LOGON_FLAGS, CREATE_UNICODE_ENVIRONMENT,
    CreateProcessWithTokenW, GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetShellWindow, IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_YESNO, MessageBoxW,
};
use windows::core::{PCWSTR, PWSTR, w};

use crate::tasks::{Environment, Handle, quote, token_buffer, wide};
use crate::webview_profile::ProfileError;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetProcessWindowStation() -> *mut std::ffi::c_void;
    fn GetThreadDesktop(thread: u32) -> *mut std::ffi::c_void;
    fn GetUserObjectInformationW(
        object: *mut std::ffi::c_void,
        index: i32,
        info: *mut std::ffi::c_void,
        length: u32,
        needed: *mut u32,
    ) -> i32;
}

fn failure(stage: &'static str, error: windows::core::Error) -> ProfileError {
    let code = error.code().0 as u32;
    ProfileError {
        path: None,
        stage,
        code: if code & 0xffff0000 == 0x80070000 {
            code & 0xffff
        } else {
            code
        },
    }
}

fn data(
    token: HANDLE,
    class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<Vec<usize>, ProfileError> {
    token_buffer(token, class).map_err(|e| ProfileError {
        path: None,
        stage: "desktop recovery token",
        code: e.code,
    })
}

fn token(process: HANDLE) -> Result<Handle, ProfileError> {
    let mut handle = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY | TOKEN_DUPLICATE, &mut handle) }
        .map_err(|e| failure("desktop recovery token", e))?;
    Ok(Handle(handle))
}

fn object_name(object: *mut std::ffi::c_void) -> Result<String, ProfileError> {
    let mut buffer = [0u16; 256];
    let mut needed = 0;
    if unsafe {
        GetUserObjectInformationW(
            object,
            2,
            buffer.as_mut_ptr().cast(),
            size_of_val(&buffer) as u32,
            &mut needed,
        )
    } == 0
    {
        return Err(failure(
            "desktop recovery object name",
            windows::core::Error::from_thread(),
        ));
    }
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..len]))
}

/// Match the window station's logon SID, not a process name or the foreground window.
fn desktop_token() -> Result<Handle, ProfileError> {
    let station = unsafe { GetProcessWindowStation() };
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    if !object_name(station)?.eq_ignore_ascii_case("WinSta0")
        || !object_name(desktop)?.eq_ignore_ascii_case("Default")
    {
        return Err(ProfileError {
            path: None,
            stage: "desktop recovery requires the interactive desktop",
            code: 50,
        });
    }
    let mut sid = [0usize; 16];
    let mut needed = 0;
    if unsafe {
        GetUserObjectInformationW(
            station,
            4,
            sid.as_mut_ptr().cast(),
            size_of_val(&sid) as u32,
            &mut needed,
        )
    } == 0
    {
        return Err(failure(
            "desktop recovery logon SID",
            windows::core::Error::from_thread(),
        ));
    }
    if needed == 0 {
        return Err(ProfileError {
            path: None,
            stage: "desktop recovery logon SID absent",
            code: 1168,
        });
    }
    let logon = PSID(sid.as_mut_ptr().cast());
    let own = token(unsafe { GetCurrentProcess() })?;
    let session = data(own.0, TokenSessionId)?[0];
    for candidate in argus_core::process::find_by(|_| true) {
        let Ok(process) =
            (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, candidate.pid) })
        else {
            continue;
        };
        let process = Handle(process);
        let Ok(token) = token(process.0) else {
            continue;
        };
        let eligible = (|| -> Result<bool, ProfileError> {
            if data(token.0, TokenSessionId)?[0] != session
                || data(token.0, TokenElevation)?[0] != 0
                || data(token.0, TokenIsAppContainer)?[0] != 0
            {
                return Ok(false);
            }
            let level = data(token.0, TokenIntegrityLevel)?;
            let level = unsafe { (*level.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
            if unsafe { *GetSidSubAuthority(level, (*GetSidSubAuthorityCount(level) - 1) as u32) }
                != 8192
            {
                return Ok(false);
            }
            let groups = data(token.0, TokenGroups)?;
            let groups = unsafe { &*groups.as_ptr().cast::<TOKEN_GROUPS>() };
            Ok(unsafe {
                std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
            }
            .iter()
            .any(|g| unsafe { EqualSid(g.Sid, logon) }.is_ok()))
        })();
        if matches!(eligible, Ok(true)) {
            return Ok(token);
        }
    }
    Err(ProfileError {
        path: None,
        stage: "desktop recovery account unavailable",
        code: 1168,
    })
}

pub fn needed(elevated: bool) -> bool {
    elevated && unsafe { GetShellWindow() }.0.is_null()
}

/// No is a normal cancellation. Never silently change the shell or elevate its browser.
pub fn restore() -> Result<bool, ProfileError> {
    let desktop = desktop_token()?;
    let text = wide(&argus_core::tr!("host.restoreDesktopForWebView"));
    let title = wide(&crate::shell::app_title());
    if unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
        )
    } != IDYES
    {
        return Ok(false);
    }
    if !unsafe { GetShellWindow() }.0.is_null() {
        return Ok(true);
    }
    let mut primary = HANDLE::default();
    unsafe {
        DuplicateTokenEx(
            desktop.0,
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
    .map_err(|e| failure("desktop recovery duplicate token", e))?;
    let primary = Handle(primary);
    let mut environment = std::ptr::null_mut();
    unsafe { CreateEnvironmentBlock(&mut environment, Some(primary.0), false) }
        .map_err(|e| failure("desktop recovery environment", e))?;
    let environment = Environment(environment);
    let mut directory = [0u16; 32768];
    let len = unsafe { GetWindowsDirectoryW(Some(&mut directory)) } as usize;
    if len == 0 || len >= directory.len() {
        return Err(ProfileError {
            path: None,
            stage: "desktop recovery Windows directory",
            code: 3,
        });
    }
    let file = format!(
        "{}\\explorer.exe",
        String::from_utf16_lossy(&directory[..len])
    );
    let mut command = wide(&quote(&file));
    let file = wide(&file);
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        lpDesktop: PWSTR(w!("WinSta0\\Default").as_ptr() as *mut _),
        ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessWithTokenW(
            primary.0,
            CREATE_PROCESS_LOGON_FLAGS(0),
            PCWSTR(file.as_ptr()),
            Some(PWSTR(command.as_mut_ptr())),
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            Some(environment.0),
            PCWSTR(directory.as_ptr()),
            &startup,
            &mut process,
        )
    }
    .map_err(|e| failure("desktop recovery launch", e))?;
    drop(Handle(process.hThread));
    let _process = Handle(process.hProcess);
    let original_data = data(desktop.0, TokenUser)?;
    let original = unsafe { (*original_data.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        if let Some(shell) = crate::explorer::current() {
            let process = Handle(
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, shell.pid) }
                    .map_err(|e| failure("desktop recovery verification", e))?,
            );
            let restored = token(process.0)?;
            let user = data(restored.0, TokenUser)?;
            if unsafe { EqualSid(original, (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid) }.is_ok()
                && data(restored.0, TokenElevation)?[0] == 0
            {
                return Ok(true);
            }
            return Err(ProfileError {
                path: None,
                stage: "desktop recovery identity mismatch",
                code: 5,
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(ProfileError {
        path: None,
        stage: "desktop recovery timed out",
        code: 1460,
    })
}
