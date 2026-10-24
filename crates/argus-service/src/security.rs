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
    IsTokenRestricted, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, RevertToSelf,
    SECURITY_ATTRIBUTES, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER, TokenIntegrityLevel,
    TokenIsAppContainer, TokenUser,
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
    // Only a person's account may be authorized: local and domain accounts
    // (S-1-5-21-…) and Microsoft Entra accounts (S-1-12-1-…). Never a group
    // or well-known SID such as Everyone, Anonymous or SYSTEM.
    if !(sid.starts_with("S-1-5-21-") || sid.starts_with("S-1-12-1-")) {
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

/// Task Scheduler may normalize a principal SID to its local account name.
pub(crate) fn account_sid(name: &str) -> Result<String> {
    if validate_sid(name).is_ok() {
        return Ok(name.into());
    }
    let mut bytes = [0u64; 32];
    let mut size = size_of_val(&bytes) as u32;
    let mut domain = [0u16; 256];
    let mut domain_size = domain.len() as u32;
    let mut kind = SID_NAME_USE::default();
    let sid = PSID(bytes.as_mut_ptr().cast());
    unsafe {
        LookupAccountNameW(
            None,
            PCWSTR(wide(name).as_ptr()),
            Some(sid),
            &mut size,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut domain_size,
            &mut kind,
        )
    }
    .map_err(win)?;
    sid_string(sid)
}

/// A sandboxed process of the owner (restricted or AppContainer token) runs
/// with less than the owner's rights and must not borrow them from the service.
pub(crate) fn sandboxed(token: HANDLE) -> Result<bool> {
    // The bindings map the BOOL to a Result: Ok means restricted.
    if unsafe { IsTokenRestricted(token) }.is_ok() {
        return Ok(true);
    }
    let mut app_container = 0u32;
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token,
            TokenIsAppContainer,
            Some((&raw mut app_container).cast()),
            size_of::<u32>() as u32,
            &mut size,
        )
    }
    .map_err(win)?;
    Ok(app_container != 0)
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
        if token_user(token.0)? != owner || sandboxed(token.0)? {
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
    let mut trace = PathInspection::default();
    inspect_path_with(path, &mut trace).map_err(|error| {
        argus_core::diagnostics::warn(
            "protected path rejected",
            format!(
                "{}; path={}; check={trace:?}",
                argus_core::diagnostics::code(error.code()),
                argus_core::diagnostics::path(path)
            ),
        );
        error.code()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathError {
    Os(u32),
    ReparsePoint,
    Owner,
    Permissions,
    Writable,
}

/// Facts from the handle/ACE that actually failed; never account names or
/// arbitrary security-descriptor text. Callers supply the affected path.
#[derive(Debug, Default)]
pub(crate) struct PathInspection {
    pub stage: &'static str,
    pub reason: Option<PathError>,
    pub owner: &'static str,
    pub attributes: Option<u32>,
    pub ace_index: Option<u16>,
    pub ace_type: Option<u8>,
    pub ace_flags: Option<u8>,
    pub access_mask: Option<u32>,
    pub trustee: &'static str,
}

fn principal_name(sid: &str) -> &'static str {
    match sid {
        "S-1-5-18" => "SYSTEM",
        "S-1-5-32-544" => "Administrators",
        "S-1-5-32-545" => "Users",
        "S-1-1-0" => "Everyone",
        "S-1-5-11" => "Authenticated Users",
        "S-1-5-13" => "Terminal Server User",
        "S-1-3-0" => "Creator Owner",
        "S-1-15-2-1" => "All Application Packages",
        "S-1-15-2-2" => "All Restricted Application Packages",
        value if trusted(value) => "TrustedInstaller",
        _ => "other account or group",
    }
}

impl PathError {
    pub(crate) fn code(self) -> u32 {
        match self {
            Self::Os(code) => code,
            _ => UNSAFE_PATH,
        }
    }
}

impl From<u32> for PathError {
    fn from(code: u32) -> Self {
        Self::Os(code)
    }
}

pub(crate) fn inspect_path_with(
    path: &Path,
    trace: &mut PathInspection,
) -> std::result::Result<Handle, PathError> {
    let result = inspect_path_inner(path, trace);
    trace.reason = result.as_ref().err().copied();
    result
}

fn inspect_path_inner(
    path: &Path,
    trace: &mut PathInspection,
) -> std::result::Result<Handle, PathError> {
    let path = wide(&path.to_string_lossy());
    trace.stage = "CreateFileW (READ_CONTROL | FILE_READ_ATTRIBUTES, open reparse point)";
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
    trace.stage = "GetFileInformationByHandle";
    unsafe { GetFileInformationByHandle(h, &mut info) }.map_err(win)?;
    trace.attributes = Some(info.dwFileAttributes);
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(PathError::ReparsePoint);
    }
    let mut owner = PSID::default();
    let mut dacl = std::ptr::null_mut::<ACL>();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    trace.stage = "GetSecurityInfo (owner and DACL)";
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
    check_acl_with(owner, dacl, 0x520d_0156, trace)?;
    Ok(handle)
}

pub(crate) fn inspect_registry_key(
    key: windows::Win32::System::Registry::HKEY,
    trace: &mut PathInspection,
) -> std::result::Result<(), PathError> {
    let mut owner = PSID::default();
    let mut dacl = std::ptr::null_mut::<ACL>();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    trace.stage = "GetSecurityInfo (registry owner and DACL)";
    unsafe {
        GetSecurityInfo(
            HANDLE(key.0),
            windows::Win32::Security::Authorization::SE_REGISTRY_KEY,
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
    // Registry writes are SET_VALUE/CREATE_SUB_KEY, not file write bits.
    let result = check_acl_with(owner, dacl, 0x520d_0006, trace);
    trace.reason = result.as_ref().err().copied();
    result
}

fn check_acl_with(
    owner: PSID,
    dacl: *mut ACL,
    write_mask: u32,
    trace: &mut PathInspection,
) -> std::result::Result<(), PathError> {
    trace.stage = "owner SID validation";
    let owner = sid_string(owner)?;
    trace.owner = principal_name(&owner);
    if !trusted(&owner) {
        return Err(PathError::Owner);
    }
    trace.stage = "DACL presence";
    if dacl.is_null() {
        return Err(PathError::Permissions);
    }
    for index in 0..unsafe { (*dacl).AceCount } {
        trace.stage = "GetAce";
        trace.ace_index = Some(index);
        let mut ace: *mut c_void = std::ptr::null_mut();
        unsafe { GetAce(dacl, u32::from(index), &mut ace) }.map_err(win)?;
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        trace.ace_type = Some(header.AceType);
        trace.ace_flags = Some(header.AceFlags);
        trace.access_mask = None;
        trace.trustee = "";
        // Inherit-only entries do not apply to this object. Service files are
        // created with their own protected DACL, not the parent's template.
        if header.AceFlags & 0x08 != 0 {
            continue;
        }
        if header.AceType == 1 {
            continue;
        } // Deny ACEs cannot grant write access.
        if header.AceType != 0 {
            trace.stage = "unsupported ACE type";
            return Err(PathError::Permissions);
        }
        let allow = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = PSID((&allow.SidStart as *const u32).cast_mut().cast());
        trace.stage = "allow ACE write permissions";
        trace.access_mask = Some(allow.Mask);
        if allow.Mask & write_mask != 0 {
            let sid = sid_string(sid)?;
            trace.trustee = principal_name(&sid);
            if !trusted(&sid) {
                return Err(PathError::Writable);
            }
        }
    }
    Ok(())
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

    fn inspect_acl(sddl: &str) -> std::result::Result<(), PathError> {
        inspect_acl_mask(sddl, 0x520d_0156)
    }

    fn inspect_acl_mask(sddl: &str, mask: u32) -> std::result::Result<(), PathError> {
        inspect_acl_details(sddl, mask, &mut PathInspection::default())
    }

    fn inspect_acl_details(
        sddl: &str,
        mask: u32,
        trace: &mut PathInspection,
    ) -> std::result::Result<(), PathError> {
        let descriptor = Descriptor::new(sddl).unwrap();
        let mut owner = PSID::default();
        let mut defaulted = false.into();
        let mut present = false.into();
        let mut dacl = std::ptr::null_mut();
        unsafe {
            windows::Win32::Security::GetSecurityDescriptorOwner(
                descriptor.0,
                &mut owner,
                &mut defaulted,
            )
        }
        .unwrap();
        unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) }
            .unwrap();
        check_acl_with(owner, dacl, mask, trace)
    }

    #[test]
    fn unsafe_acl_diagnostics_identify_the_exact_owner_or_grant() {
        let mut trace = PathInspection::default();
        assert_eq!(
            inspect_acl_details(
                "O:BAG:BAD:(A;;FA;;;SY)(A;OICIID;FA;;;WD)",
                0x520d_0156,
                &mut trace
            ),
            Err(PathError::Writable)
        );
        assert_eq!(trace.owner, "Administrators");
        assert_eq!(trace.trustee, "Everyone");
        assert_eq!(trace.ace_index, Some(1));
        assert_eq!(trace.access_mask, Some(0x1f01ff));
        assert_eq!(trace.ace_flags, Some(0x13));
        let mut trace = PathInspection::default();
        assert_eq!(
            inspect_acl_details("O:BUG:BUD:(A;;FR;;;BU)", 0x520d_0156, &mut trace),
            Err(PathError::Owner)
        );
        assert_eq!(trace.owner, "Users");
        assert_eq!(trace.stage, "owner SID validation");
    }

    #[test]
    fn missing_path_preserves_the_api_and_native_failure() {
        let path = std::env::temp_dir().join(format!(
            "argus-missing-{:?}",
            unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap()
        ));
        let mut trace = PathInspection::default();
        let error = inspect_path_with(&path, &mut trace).err().unwrap();
        assert!(matches!(error, PathError::Os(2 | 3)));
        assert_eq!(trace.reason, Some(error));
        assert!(trace.stage.starts_with("CreateFileW"));
    }

    #[test]
    fn uninstall_registry_metadata_must_not_be_user_writable() {
        assert_eq!(
            inspect_acl_mask(
                "O:BAG:BAD:(A;;KA;;;SY)(A;;KA;;;BA)(A;;KR;;;BU)",
                0x520d_0006
            ),
            Ok(())
        );
        for mask in [0x2, 0x4, 0x10000, 0x40000, 0x80000, 0x10000000, 0x40000000] {
            let sddl = format!("O:BAG:BAD:(A;;KA;;;SY)(A;;0x{mask:x};;;BU)");
            assert_eq!(
                inspect_acl_mask(&sddl, 0x520d_0006),
                Err(PathError::Writable)
            );
        }
    }

    #[test]
    fn driver_store_full_control_is_rejected_but_inherited_read_access_is_safe() {
        // The failing machine grants Everyone full control on FileRepository,
        // even though its driver package and the driver itself are read-only.
        assert_eq!(
            inspect_acl(
                "O:BAG:BAD:(A;OICIID;FA;;;SY)(A;OICIID;FA;;;WD)(A;OICIID;0x1200a9;;;AC)(A;OICIID;0x1200a9;;;S-1-15-2-2)"
            ),
            Err(PathError::Writable)
        );
        assert_eq!(
            inspect_acl(
                "O:BAG:BAD:AI(A;OICIID;FA;;;SY)(A;OICIID;0x1200a9;;;WD)(A;OICIID;0x1200a9;;;AC)(A;OICIID;0x1200a9;;;S-1-15-2-2)"
            ),
            Ok(())
        );
        assert_eq!(
            inspect_acl(
                "O:SYG:SYD:AI(A;ID;FA;;;SY)(A;ID;0x1200a9;;;WD)(A;ID;0x1200a9;;;AC)(A;ID;0x1200a9;;;S-1-15-2-2)"
            ),
            Ok(())
        );
    }

    #[test]
    fn path_failures_preserve_the_existing_service_error_codes() {
        assert_eq!(PathError::Os(5).code(), 5);
        for error in [
            PathError::Owner,
            PathError::Permissions,
            PathError::Writable,
            PathError::ReparsePoint,
        ] {
            assert_eq!(error.code(), UNSAFE_PATH);
        }
    }

    #[test]
    fn descriptor_uses_current_account_and_rejects_sddl_injection() {
        let sid = current_sid().unwrap();
        assert!(pipe_descriptor(&sid).is_ok());
        assert!(pipe_descriptor("S-1-1-0)(A;;GA;;;WD)").is_err());
    }

    #[test]
    fn only_user_accounts_can_be_authorized() {
        assert!(validate_sid("S-1-5-21-1004336348-1177238915-682003330-1001").is_ok());
        assert!(validate_sid("S-1-12-1-1234567890-1234567890-1234567890-1234567890").is_ok());
        // Everyone, Anonymous, SYSTEM, Administrators, Authenticated Users.
        for sid in ["S-1-1-0", "S-1-5-7", "S-1-5-18", "S-1-5-32-544", "S-1-5-11"] {
            assert!(validate_sid(sid).is_err(), "{sid}");
        }
    }

    #[test]
    fn restricted_tokens_count_as_sandboxed() {
        use windows::Win32::Security::{
            CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, SID_AND_ATTRIBUTES, TOKEN_ASSIGN_PRIMARY,
            TOKEN_DUPLICATE,
        };
        let mut own = HANDLE::default();
        unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
                &mut own,
            )
        }
        .unwrap();
        let own = Handle(own);
        assert!(!sandboxed(own.0).unwrap());

        // Restricted to the user's own SID, as a sandboxing broker would do.
        let mut size = 0;
        unsafe {
            let _ = GetTokenInformation(own.0, TokenUser, None, 0, &mut size);
        }
        let mut data = vec![0u64; (size as usize).div_ceil(8)];
        unsafe {
            GetTokenInformation(
                own.0,
                TokenUser,
                Some(data.as_mut_ptr().cast()),
                size,
                &mut size,
            )
        }
        .unwrap();
        let user = unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User };
        let restrict = [SID_AND_ATTRIBUTES {
            Sid: user.Sid,
            Attributes: 0,
        }];
        let mut restricted = HANDLE::default();
        unsafe {
            CreateRestrictedToken(
                own.0,
                DISABLE_MAX_PRIVILEGE,
                None,
                None,
                Some(&restrict),
                &mut restricted,
            )
        }
        .unwrap();
        let restricted = Handle(restricted);
        assert!(sandboxed(restricted.0).unwrap());
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
