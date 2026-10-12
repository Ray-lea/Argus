use std::ffi::c_void;
use std::path::Path;
use std::sync::OnceLock;

use windows::Win32::Security::{LookupAccountNameW, SID_NAME_USE};

use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, GetSecurityInfo, SE_FILE_OBJECT, SE_KERNEL_OBJECT, SetSecurityInfo,
};
use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce,
    GetSecurityDescriptorDacl, GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, RevertToSelf, SECURITY_ATTRIBUTES,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER, TokenIntegrityLevel, TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
};
use windows::Win32::System::Pipes::{GetNamedPipeClientProcessId, ImpersonateNamedPipeClient};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};
use windows::core::{PCWSTR, PWSTR};

use crate::util::{Handle, Result, UNSAFE_PATH, wide, win};

pub struct Descriptor(pub PSECURITY_DESCRIPTOR);
impl Descriptor {
    pub fn new(sddl: &str) -> Result<Self> {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let s = wide(sddl);
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(s.as_ptr()),
                1,
                &mut sd,
                None,
            )
        }
        .map_err(win)?;
        Ok(Self(sd))
    }

    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: false.into(),
        }
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}

fn sid_string(sid: PSID) -> Result<String> {
    let mut p = PWSTR::null();
    unsafe { ConvertSidToStringSidW(sid, &mut p) }.map_err(win)?;
    let text = unsafe { p.to_string() }.map_err(|_| 87u32);
    unsafe {
        let _ = LocalFree(Some(HLOCAL(p.0.cast())));
    }
    text
}

pub fn validate_sid(sid: &str) -> Result<()> {
    if sid.len() > 184 || sid.contains('\0') {
        return Err(87);
    }
    let mut p = PSID::default();
    unsafe { ConvertStringSidToSidW(PCWSTR(wide(sid).as_ptr()), &mut p) }.map_err(win)?;
    let canonical = sid_string(p);
    unsafe {
        let _ = LocalFree(Some(HLOCAL(p.0)));
    }
    if canonical? != sid {
        return Err(87);
    }
    Ok(())
}

pub(crate) fn token_user(token: HANDLE) -> Result<String> {
    let mut size = 0;
    unsafe {
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
    }
    if size == 0 || size > 4096 {
        return Err(87);
    }
    let mut data = vec![0u64; (size as usize).div_ceil(8)];
    unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(data.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    }
    .map_err(win)?;
    sid_string(unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid })
}

pub fn current_sid() -> Result<String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(win)?;
    token_user(Handle(token).0)
}

/// Identification-only impersonation is enough to read the token. Always
/// revert before executing an operation, including every error path.
pub fn authenticate(pipe: HANDLE, owner: &str) -> Result<u32> {
    unsafe { ImpersonateNamedPipeClient(pipe) }.map_err(win)?;
    let result = (|| {
        let mut token = HANDLE::default();
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token) }
            .map_err(win)?;
        let token = Handle(token);
        if token_user(token.0)? != owner {
            return Err(5);
        }
        let mut data = [0u64; 64];
        let mut size = 0;
        unsafe {
            GetTokenInformation(
                token.0,
                TokenIntegrityLevel,
                Some(data.as_mut_ptr().cast()),
                std::mem::size_of_val(&data) as u32,
                &mut size,
            )
        }
        .map_err(win)?;
        let sid = unsafe { (*data.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
        let count = unsafe { *GetSidSubAuthorityCount(sid) };
        if count == 0 || unsafe { *GetSidSubAuthority(sid, u32::from(count) - 1) } < 0x2000 {
            return Err(5);
        }
        let mut pid = 0;
        unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) }.map_err(win)?;
        Ok(pid)
    })();
    // Continuing under an unknown impersonation token is never safe.
    if unsafe { RevertToSelf() }.is_err() {
        std::process::abort();
    }
    result
}

pub fn pipe_descriptor(owner: &str) -> Result<Descriptor> {
    validate_sid(owner)?;
    // FILE_WRITE_DATA, not GENERIC_WRITE (which grants CREATE_PIPE_INSTANCE).
    Descriptor::new(&format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00100083;;;{owner})S:(ML;;NW;;;ME)"
    ))
}

pub fn directory_descriptor() -> Result<Descriptor> {
    Descriptor::new("O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FRFX;;;BU)")
}

/// The standard-permission client pins the SCM-reported process while checking
/// the pipe PID. Grant only query/synchronize on this process, never injection
/// or termination rights.
pub fn allow_process_query(owner: &str) -> Result<()> {
    validate_sid(owner)?;
    let sd = Descriptor::new(&format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00101000;;;{owner})"
    ))?;
    let mut present = false.into();
    let mut defaulted = false.into();
    let mut dacl = std::ptr::null_mut();
    unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) }
        .map_err(win)?;
    unsafe {
        SetSecurityInfo(
            GetCurrentProcess(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl),
            None,
        )
    }
    .ok()
    .map_err(win)
}

/// Pin the directory against renaming, reject reparse points, and refuse any
/// owner/allow ACE that could let an unprivileged account replace service code.
pub fn protect_directory(path: &Path) -> Result<Handle> {
    let path = wide(&path.to_string_lossy());
    let h = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            0x20000 | FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(win)?;
    let handle = Handle(h);
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(h, &mut info) }.map_err(win)?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(UNSAFE_PATH);
    }
    let mut owner = PSID::default();
    let mut dacl = std::ptr::null_mut::<ACL>();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe {
        GetSecurityInfo(
            h,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            Some(&mut sd),
        )
    }
    .ok()
    .map_err(win)?;
    let _sd = Descriptor(sd);
    if !trusted(&sid_string(owner)?) || dacl.is_null() {
        return Err(UNSAFE_PATH);
    }
    for index in 0..unsafe { (*dacl).AceCount } {
        let mut ace: *mut c_void = std::ptr::null_mut();
        unsafe { GetAce(dacl, u32::from(index), &mut ace) }.map_err(win)?;
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        // Inherit-only entries do not apply to this object. Service files are
        // created with their own protected DACL, not the parent's template.
        if header.AceFlags & 0x08 != 0 {
            continue;
        }
        if header.AceType == 1 {
            continue;
        } // Deny ACEs cannot grant write access.
        if header.AceType != 0 {
            return Err(UNSAFE_PATH);
        }
        let allow = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = PSID((&allow.SidStart as *const u32).cast_mut().cast());
        // Generic all/write, maximum allowed, delete, write DAC/owner, and
        // file/directory writes (including delete-child).
        if allow.Mask & 0x520d_0156 != 0 && !trusted(&sid_string(sid)?) {
            return Err(UNSAFE_PATH);
        }
    }
    Ok(handle)
}

fn trusted(sid: &str) -> bool {
    if matches!(sid, "S-1-5-18" | "S-1-5-32-544") {
        return true;
    }
    // Program Files itself is normally owned by TrustedInstaller. Resolve the
    // local service SID rather than accepting arbitrary service-account ACEs.
    static INSTALLER: OnceLock<Option<String>> = OnceLock::new();
    INSTALLER
        .get_or_init(|| {
            let mut bytes = [0u64; 16];
            let mut size = std::mem::size_of_val(&bytes) as u32;
            let mut domain = [0u16; 256];
            let mut domain_size = domain.len() as u32;
            let mut kind = SID_NAME_USE::default();
            let sid = PSID(bytes.as_mut_ptr().cast());
            unsafe {
                LookupAccountNameW(
                    None,
                    PCWSTR(wide(r"NT SERVICE\TrustedInstaller").as_ptr()),
                    Some(sid),
                    &mut size,
                    Some(PWSTR(domain.as_mut_ptr())),
                    &mut domain_size,
                    &mut kind,
                )
            }
            .ok()?;
            sid_string(sid).ok()
        })
        .as_deref()
        == Some(sid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptor_uses_current_account_and_rejects_sddl_injection() {
        let sid = current_sid().unwrap();
        assert!(pipe_descriptor(&sid).is_ok());
        assert!(pipe_descriptor("S-1-1-0)(A;;GA;;;WD)").is_err());
    }
    #[test]
    fn refuses_user_writable_service_directory() {
        assert!(protect_directory(&std::env::temp_dir()).is_err());
    }

    #[test]
    fn system_program_files_is_a_trusted_parent() {
        let dir = crate::install::directory().unwrap();
        protect_directory(dir.parent().unwrap()).unwrap();
    }
}
