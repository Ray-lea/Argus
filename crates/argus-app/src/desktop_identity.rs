//! Desktop account verification for safe installer restart and de-elevation.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    EqualSid, GetSidSubAuthority, GetSidSubAuthorityCount, PSID, TOKEN_DUPLICATE, TOKEN_GROUPS,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenElevation, TokenGroups, TokenIntegrityLevel,
    TokenIsAppContainer, TokenSessionId,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

#[derive(Debug)]
pub struct ProfileError {
    pub stage: &'static str,
    pub code: u32,
    pub path: Option<std::path::PathBuf>,
}
use crate::tasks::{Handle, token_buffer};

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
pub fn desktop_token() -> Result<Handle, ProfileError> {
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
