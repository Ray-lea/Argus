//! Privilege state of the Argus process itself.

use windows::Win32::Foundation::{HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
    SE_DEBUG_NAME, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION,
    TOKEN_PRIVILEGES, TOKEN_QUERY, TokenElevation,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, HIGH_PRIORITY_CLASS, OpenProcessToken, SetPriorityClass,
};
use windows::core::PCWSTR;

use crate::util::OwnedHandle;

/// Whether this process runs with an elevated (administrator) token.
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let token = OwnedHandle(token);
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok()
            && elevation.TokenIsElevated != 0
    }
}

/// Enables SeDebugPrivilege when the token holds it (i.e. when elevated), which
/// lets us open nearly every process. Returns whether it is now enabled.
pub fn enable_debug_privilege() -> bool {
    enable_privilege(SE_DEBUG_NAME)
}

fn enable_privilege(name: PCWSTR) -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .is_err()
        {
            return false;
        }
        let token = OwnedHandle(token);
        let mut luid = LUID::default();
        if LookupPrivilegeValueW(PCWSTR::null(), name, &mut luid).is_err() {
            return false;
        }
        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        // AdjustTokenPrivileges "succeeds" with ERROR_NOT_ALL_ASSIGNED when the
        // privilege is absent, so check the thread's last error as well.
        AdjustTokenPrivileges(token.0, false, Some(&tp), 0, None, None).is_ok()
            && windows::Win32::Foundation::GetLastError().is_ok()
    }
}

/// Runs Argus at high priority so it stays responsive when the machine is
/// saturated — exactly when people reach for a task manager.
pub fn raise_self_priority() {
    unsafe {
        let _ = SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS);
    }
}
