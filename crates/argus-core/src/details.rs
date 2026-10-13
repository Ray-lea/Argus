//! On-demand details for a single process (too costly to collect for all).

use serde::Serialize;
use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, LookupAccountSidW, PSID,
    SID_NAME_USE, TOKEN_ELEVATION, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TOKEN_USER, TokenElevation, TokenIntegrityLevel, TokenUser,
};
use windows::Win32::System::SystemInformation::{
    IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64,
    IMAGE_FILE_MACHINE_ARMNT, IMAGE_FILE_MACHINE_I386, IMAGE_FILE_MACHINE_UNKNOWN,
};
use windows::Win32::System::Threading::{
    GetPriorityClass, IsWow64Process2, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::core::{PCWSTR, PWSTR};

use crate::meta::{VersionInfo, version_info};
use crate::nt::{
    STATUS_BUFFER_OVERFLOW, STATUS_BUFFER_TOO_SMALL, STATUS_INFO_LENGTH_MISMATCH, UnicodeString,
};
use crate::util::{AlignedBuf, OwnedHandle, from_wide};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessDetails {
    pub pid: u32,
    /// False when the process could not be opened (usually needs admin).
    pub accessible: bool,
    pub command_line: Option<String>,
    pub user: Option<String>,
    /// `untrusted` | `low` | `medium` | `mediumPlus` | `high` | `system` | `protected`
    pub integrity: Option<&'static str>,
    pub elevated: Option<bool>,
    /// `x64` | `x86` | `arm64` | `arm`
    pub architecture: Option<&'static str>,
    /// `idle` | `belowNormal` | `normal` | `aboveNormal` | `high` | `realtime`
    pub priority_class: Option<&'static str>,
    pub version: VersionInfo,
}

pub fn query(pid: u32, path: Option<&str>) -> ProcessDetails {
    let mut d = ProcessDetails {
        pid,
        version: path.map(version_info).unwrap_or_default(),
        ..Default::default()
    };
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return d;
    };
    let handle = OwnedHandle(handle);
    d.accessible = true;
    d.command_line = command_line(handle.0);
    d.architecture = architecture(handle.0);
    d.priority_class = priority_class(handle.0);

    let mut token = HANDLE::default();
    if unsafe { OpenProcessToken(handle.0, TOKEN_QUERY, &mut token) }.is_ok() {
        let token = OwnedHandle(token);
        d.user = token_user(token.0);
        d.integrity = token_integrity(token.0);
        d.elevated = token_elevated(token.0);
    }
    d
}

/// One process's command line, on its own (no other details).
pub fn command_line_of(pid: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let handle = OwnedHandle(handle);
    command_line(handle.0)
}

fn command_line(process: HANDLE) -> Option<String> {
    let mut buf = AlignedBuf::with_capacity(1024);
    for _ in 0..3 {
        let mut needed = 0u32;
        let status = unsafe {
            NtQueryInformationProcess(
                process,
                ProcessCommandLineInformation,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut needed,
            )
        };
        match status.0 {
            STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_TOO_SMALL | STATUS_BUFFER_OVERFLOW => {
                buf.grow_to(needed as usize + 16);
            }
            s if s < 0 => return None,
            _ => {
                // The result is a UNICODE_STRING pointing into the same buffer.
                let us = unsafe { &*(buf.as_ptr() as *const UnicodeString) };
                let s = unsafe { us.to_string_lossy() };
                return (!s.is_empty()).then_some(s);
            }
        }
    }
    None
}

fn architecture(process: HANDLE) -> Option<&'static str> {
    let mut proc_machine = IMAGE_FILE_MACHINE::default();
    let mut native_machine = IMAGE_FILE_MACHINE::default();
    unsafe { IsWow64Process2(process, &mut proc_machine, Some(&mut native_machine)) }.ok()?;
    let machine = if proc_machine == IMAGE_FILE_MACHINE_UNKNOWN {
        native_machine
    } else {
        proc_machine
    };
    Some(match machine {
        IMAGE_FILE_MACHINE_AMD64 => "x64",
        IMAGE_FILE_MACHINE_I386 => "x86",
        IMAGE_FILE_MACHINE_ARM64 => "arm64",
        IMAGE_FILE_MACHINE_ARMNT => "arm",
        _ => return None,
    })
}

fn priority_class(process: HANDLE) -> Option<&'static str> {
    Some(match unsafe { GetPriorityClass(process) } {
        0x40 => "idle",
        0x4000 => "belowNormal",
        0x20 => "normal",
        0x8000 => "aboveNormal",
        0x80 => "high",
        0x100 => "realtime",
        _ => return None,
    })
}

fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Option<AlignedBuf> {
    let mut len = 0u32;
    let _ = unsafe { GetTokenInformation(token, class, None, 0, &mut len) };
    if len == 0 {
        return None;
    }
    let mut buf = AlignedBuf::with_capacity(len as usize);
    unsafe { GetTokenInformation(token, class, Some(buf.as_mut_ptr()), len, &mut len) }.ok()?;
    Some(buf)
}

fn token_user(token: HANDLE) -> Option<String> {
    let buf = token_info(token, TokenUser)?;
    let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
    account_name(user.User.Sid)
}

fn account_name(sid: PSID) -> Option<String> {
    let mut name = [0u16; 256];
    let mut domain = [0u16; 256];
    let (mut name_len, mut domain_len) = (name.len() as u32, domain.len() as u32);
    let mut use_ = SID_NAME_USE::default();
    unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            sid,
            Some(PWSTR(name.as_mut_ptr())),
            &mut name_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut domain_len,
            &mut use_,
        )
    }
    .ok()?;
    let (name, domain) = (from_wide(&name), from_wide(&domain));
    Some(if domain.is_empty() {
        name
    } else {
        format!(r"{domain}\{name}")
    })
}

fn token_integrity(token: HANDLE) -> Option<&'static str> {
    let buf = token_info(token, TokenIntegrityLevel)?;
    let label = unsafe { &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL) };
    let sid = label.Label.Sid;
    let rid = unsafe {
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return None;
        }
        *GetSidSubAuthority(sid, count as u32 - 1)
    };
    Some(match rid {
        r if r < 0x1000 => "untrusted",
        r if r < 0x2000 => "low",
        r if r < 0x2100 => "medium",
        r if r < 0x3000 => "mediumPlus",
        r if r < 0x4000 => "high",
        r if r < 0x5000 => "system",
        _ => "protected",
    })
}

fn token_elevated(token: HANDLE) -> Option<bool> {
    let buf = token_info(token, TokenElevation)?;
    let e = unsafe { &*(buf.as_ptr() as *const TOKEN_ELEVATION) };
    Some(e.TokenIsElevated != 0)
}
