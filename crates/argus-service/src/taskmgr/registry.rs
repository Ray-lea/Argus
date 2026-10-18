use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, HANDLE, WIN32_ERROR};
use windows::Win32::Security::{
    AccessCheck, DACL_SECURITY_INFORMATION, DuplicateToken, GENERIC_MAPPING,
    GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SecurityIdentification, TOKEN_DUPLICATE, TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{BOOL, PCWSTR, PWSTR};

use super::Cause;
use super::state::{Journal, Observation, Result, Store, Value};
use crate::{security, util};
use argus_core::tr;

pub const IFEO: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";
pub const KEY: &str =
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\taskmgr.exe";
pub const JOURNAL: &str = "taskmgr-recovery.json";
const READ_ACCESS: REG_SAM_FLAGS = REG_SAM_FLAGS(KEY_QUERY_VALUE.0 | KEY_ENUMERATE_SUB_KEYS.0);
const WRITE_ACCESS: REG_SAM_FLAGS = REG_SAM_FLAGS(KEY_QUERY_VALUE.0 | KEY_SET_VALUE.0);
pub struct Key(pub HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// A failed registry call. The Win32 code stays available to explain a
/// refusal (`refusal`); the window gets the text.
#[derive(Debug)]
pub enum Error {
    Os(OsError),
    Text(String),
}

/// Fixed API names and access masks only: safe to include in public logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Call {
    Open(u32),
    Create(u32),
    QueryValue,
    Enumerate,
    QueryInfo,
    SetValue,
    DeleteValue,
    Flush,
    DeleteKey,
    ReadSecurity,
}

impl Call {
    pub fn name(self) -> &'static str {
        match self {
            Self::Open(_) => "RegOpenKeyExW",
            Self::Create(_) => "RegCreateKeyExW",
            Self::QueryValue => "RegQueryValueExW",
            Self::Enumerate => "RegEnumKeyExW",
            Self::QueryInfo => "RegQueryInfoKeyW",
            Self::SetValue => "RegSetValueExW",
            Self::DeleteValue => "RegDeleteValueW",
            Self::Flush => "RegFlushKey",
            Self::DeleteKey => "RegDeleteKeyW",
            Self::ReadSecurity => "RegGetKeySecurity",
        }
    }

    fn access(self) -> u32 {
        match self {
            Self::Open(access) | Self::Create(access) => access,
            Self::QueryValue | Self::QueryInfo | Self::Flush => KEY_QUERY_VALUE.0,
            Self::Enumerate => KEY_ENUMERATE_SUB_KEYS.0,
            Self::SetValue | Self::DeleteValue => KEY_SET_VALUE.0,
            Self::DeleteKey => 0x0001_0000,
            Self::ReadSecurity => 0x0002_0000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OsError {
    pub call: Call,
    pub code: u32,
}

impl OsError {
    pub fn log(self) -> String {
        format!(
            "{} (access 0x{:08x}, os error {})",
            self.call.name(),
            self.call.access(),
            self.code
        )
    }
}

impl Error {
    pub fn denied(&self) -> bool {
        self.os().is_some_and(|error| error.code == 5)
    }

    pub fn os(&self) -> Option<OsError> {
        match self {
            Self::Os(error) => Some(*error),
            Self::Text(_) => None,
        }
    }
}

impl From<Error> for String {
    fn from(error: Error) -> Self {
        match error {
            Error::Os(error) => tr!(
                "taskManager.registryCallFailed",
                api = error.call.name(),
                error = util::message(error.code)
            ),
            Error::Text(text) => text,
        }
    }
}

pub type RegResult<T> = std::result::Result<T, Error>;

fn err(code: WIN32_ERROR, call: Call) -> RegResult<()> {
    if code.0 == 0 {
        Ok(())
    } else {
        Err(Error::Os(OsError { call, code: code.0 }))
    }
}

pub fn open(path: &str, write: bool) -> RegResult<Option<Key>> {
    open_in(
        HKEY_LOCAL_MACHINE,
        path,
        if write { WRITE_ACCESS } else { READ_ACCESS },
    )
}

fn open_in(root: HKEY, path: &str, access: REG_SAM_FLAGS) -> RegResult<Option<Key>> {
    let mut key = HKEY::default();
    let code = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(util::wide(path).as_ptr()),
            None,
            access,
            &mut key,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    err(code, Call::Open(access.0))?;
    Ok(Some(Key(key)))
}

pub fn read_value(key: &Key, name: &str) -> RegResult<Option<Value>> {
    let name = util::wide(name);
    let mut kind = REG_VALUE_TYPE::default();
    let mut size = 0;
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    err(code, Call::QueryValue)?;
    if size > 16 * 1024 {
        return Err(Error::Text(tr!("taskManager.valueTooLong")));
    }
    let mut bytes = vec![0; size as usize];
    err(
        unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut kind),
                Some(bytes.as_mut_ptr()),
                Some(&mut size),
            )
        },
        Call::QueryValue,
    )?;
    bytes.truncate(size as usize);
    Ok(Some(Value {
        kind: kind.0,
        bytes,
    }))
}

pub fn observe(path: &str) -> RegResult<Observation> {
    let Some(key) = open(path, false)? else {
        return Ok(Observation::default());
    };
    let mut name = [0u16; 256];
    let mut len = name.len() as u32;
    let code = unsafe {
        RegEnumKeyExW(
            key.0,
            0,
            Some(PWSTR(name.as_mut_ptr())),
            &mut len,
            None,
            None,
            None,
            None,
        )
    };
    if code != ERROR_NO_MORE_ITEMS {
        err(code, Call::Enumerate)?;
    }
    Ok(Observation {
        debugger: read_value(&key, "Debugger")?,
        filtered: read_value(&key, "UseFilter")?.is_some() || code != ERROR_NO_MORE_ITEMS,
    })
}

/// What an entry holds, to tell the user what keeps Argus from enabling.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub debugger: Option<Value>,
    pub use_filter: bool,
    /// The first few subkey names.
    pub subkeys: Vec<String>,
    pub subkey_count: u32,
}

pub fn describe(path: &str) -> RegResult<Found> {
    describe_in(HKEY_LOCAL_MACHINE, path)
}

fn describe_in(root: HKEY, path: &str) -> RegResult<Found> {
    let Some(key) = open_in(root, path, READ_ACCESS)? else {
        return Ok(Found::default());
    };
    let mut count = 0;
    err(
        unsafe {
            RegQueryInfoKeyW(
                key.0,
                None,
                None,
                None,
                Some(&mut count),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
        },
        Call::QueryInfo,
    )?;
    let mut subkeys = Vec::new();
    for index in 0..count.min(4) {
        let mut name = [0u16; 256];
        let mut len = name.len() as u32;
        let code = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if code == ERROR_NO_MORE_ITEMS {
            break;
        }
        err(code, Call::Enumerate)?;
        subkeys.push(String::from_utf16_lossy(&name[..len as usize]));
    }
    Ok(Found {
        debugger: read_value(&key, "Debugger")?,
        use_filter: read_value(&key, "UseFilter")?.is_some(),
        subkeys,
        subkey_count: count,
    })
}

pub fn write(path: &str, value: Option<&Value>) -> RegResult<()> {
    write_in(HKEY_LOCAL_MACHINE, path, value)
}

fn write_in(root: HKEY, path: &str, value: Option<&Value>) -> RegResult<()> {
    let key = match open_in(root, path, WRITE_ACCESS)? {
        Some(key) => key,
        None if value.is_none() => return Ok(()),
        None => create_in(root, path)?,
    };
    set_debugger(&key, value)
}

/// Creates a key, or opens the one that appeared meanwhile.
pub fn create(path: &str) -> RegResult<Key> {
    create_in(HKEY_LOCAL_MACHINE, path)
}

fn create_in(root: HKEY, path: &str) -> RegResult<Key> {
    let mut raw = HKEY::default();
    err(
        unsafe {
            RegCreateKeyExW(
                root,
                PCWSTR(util::wide(path).as_ptr()),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                WRITE_ACCESS,
                None,
                &mut raw,
                None,
            )
        },
        Call::Create(WRITE_ACCESS.0),
    )?;
    Ok(Key(raw))
}

/// Sets or deletes the Debugger value, then flushes the key.
fn set_debugger(key: &Key, value: Option<&Value>) -> RegResult<()> {
    let name = util::wide("Debugger");
    let code = match value {
        Some(value) => unsafe {
            RegSetValueExW(
                key.0,
                PCWSTR(name.as_ptr()),
                None,
                REG_VALUE_TYPE(value.kind),
                Some(&value.bytes),
            )
        },
        None => unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) },
    };
    if code != ERROR_FILE_NOT_FOUND || value.is_some() {
        err(
            code,
            if value.is_some() {
                Call::SetValue
            } else {
                Call::DeleteValue
            },
        )?;
    }
    err(unsafe { RegFlushKey(key.0) }, Call::Flush)
}

/// Deletes a key without subkeys; one that is gone already is fine.
pub fn delete(path: &str) -> RegResult<()> {
    let code = unsafe { RegDeleteKeyW(HKEY_LOCAL_MACHINE, PCWSTR(util::wide(path).as_ptr())) };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    err(code, Call::DeleteKey)
}

/// Whether the DACL permits the access needed by the failed call. A positive
/// result does not identify why the API failed (handle rights, token restrictions,
/// mandatory integrity, policy and registry filters can still differ).
pub fn refusal(error: &Error, path: &str) -> Option<Cause> {
    if !error.denied() || !argus_core::privilege::is_elevated() {
        return None;
    }
    Some(
        if access_allowed_in(HKEY_LOCAL_MACHINE, path, IFEO, error.os()?.call)? {
            Cause::Blocked
        } else {
            Cause::Permissions
        },
    )
}

fn access_allowed_in(root: HKEY, path: &str, parent: &str, call: Call) -> Option<bool> {
    // READ_CONTROL alone: the security descriptor is all this reads.
    let read = REG_SAM_FLAGS(0x0002_0000);
    let (key, wanted) = match open_in(root, path, read).ok()? {
        Some(key) => (key, call.access()),
        None if matches!(call, Call::Create(_)) => {
            (open_in(root, parent, read).ok()??, KEY_CREATE_SUB_KEY.0)
        }
        None => return None,
    };
    // AccessCheck needs the owner and group too.
    let parts = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut size = 0;
    let _ = unsafe { RegGetKeySecurity(key.0, parts, None, &mut size) };
    if size == 0 || size > 64 * 1024 {
        return None;
    }
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    let descriptor = PSECURITY_DESCRIPTOR(buffer.as_mut_ptr().cast());
    err(
        unsafe { RegGetKeySecurity(key.0, parts, Some(descriptor), &mut size) },
        Call::ReadSecurity,
    )
    .ok()?;
    let mut process = HANDLE::default();
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_DUPLICATE | TOKEN_QUERY,
            &mut process,
        )
    }
    .ok()?;
    let process = util::Handle(process);
    let mut token = HANDLE::default();
    unsafe { DuplicateToken(process.0, SecurityIdentification, &mut token) }.ok()?;
    let token = util::Handle(token);
    let mapping = GENERIC_MAPPING {
        GenericRead: KEY_READ.0,
        GenericWrite: KEY_WRITE.0,
        GenericExecute: KEY_EXECUTE.0,
        GenericAll: KEY_ALL_ACCESS.0,
    };
    let mut privileges = [0u64; 32];
    let mut length = size_of_val(&privileges) as u32;
    let mut granted = 0;
    let mut allowed = BOOL::default();
    unsafe {
        AccessCheck(
            descriptor,
            token.0,
            wanted,
            &mapping,
            Some(privileges.as_mut_ptr().cast()),
            &mut length,
            &mut granted,
            &mut allowed,
        )
    }
    .ok()?;
    Some(allowed.as_bool() && granted & wanted == wanted)
}

pub struct Machine {
    pub dir: PathBuf,
    /// The first failed change, retained if a recovery write also fails.
    pub refused: Option<Cause>,
    pub failure: Option<OsError>,
}
impl Machine {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            refused: None,
            failure: None,
        }
    }

    pub fn load(&self) -> Result<Option<Journal>> {
        let path = self.dir.join(JOURNAL);
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
            Ok(meta) if meta.len() > 64 * 1024 => return Err(tr!("taskManager.recordTooLong")),
            _ => {}
        }
        let _file = security::protect_directory(&path).map_err(util::message)?;
        let journal: Journal = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|_| tr!("taskManager.recordCorrupt"))?;
        if journal.version != 1
            || journal.before != Observation::default()
            || journal.replacement != super::replacement(&self.dir)
            || !super::valid_image(&journal.image)
        {
            return Err(tr!("taskManager.recordMismatch"));
        }
        security::validate_sid(&journal.owner).map_err(util::message)?;
        Ok(Some(journal))
    }
}
impl Store for Machine {
    fn observe(&mut self) -> Result<Observation> {
        Ok(observe(KEY)?)
    }
    fn write_debugger(&mut self, value: Option<&Value>) -> Result<()> {
        write(KEY, value).map_err(|error| {
            let cause = refusal(&error, KEY);
            // A recovery write may also fail; retain the initiating call.
            if self.failure.is_none() {
                self.refused = cause;
                self.failure = error.os();
            }
            super::explain(
                tr!("taskManager.entryChangeFailed", error = String::from(error)),
                cause,
            )
        })
    }
    fn save(&mut self, journal: &Journal) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(journal).map_err(|e| e.to_string())?;
        atomic_file(&self.dir, JOURNAL, &bytes)
    }
}

pub fn atomic_file(dir: &Path, filename: &str, bytes: &[u8]) -> Result<()> {
    let destination = dir.join(filename);
    if destination.exists() {
        drop(security::protect_directory(&destination).map_err(util::message)?);
    }
    let temporary = dir.join(format!("taskmgr-stage-{}.tmp", super::id()?));
    let result = (|| {
        let mut file = crate::install::new_protected_file(&temporary).map_err(util::message)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        unsafe {
            MoveFileExW(
                PCWSTR(util::wide(&temporary.to_string_lossy()).as_ptr()),
                PCWSTR(util::wide(&destination.to_string_lossy()).as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch key under HKCU, removed with everything below it.
    struct Scratch(String);
    impl Scratch {
        fn new() -> Self {
            let path = format!(r"Software\ArgusTest-{}", super::super::id().unwrap());
            create_in(HKEY_CURRENT_USER, &path).unwrap();
            Self(path)
        }
        fn permit(&self, sddl: &str) {
            let descriptor = security::Descriptor::new(sddl).unwrap();
            // The owner may always change the DACL.
            let key = open_in(HKEY_CURRENT_USER, &self.0, REG_SAM_FLAGS(0x0004_0000))
                .unwrap()
                .unwrap();
            unsafe { RegSetKeySecurity(key.0, DACL_SECURITY_INFORMATION, descriptor.0) }
                .ok()
                .unwrap();
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let sid = security::current_sid().unwrap();
            self.permit(&format!("D:P(A;OICI;KA;;;{sid})"));
            let path = util::wide(&self.0);
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()));
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()));
            }
        }
    }

    #[test]
    fn access_checks_match_the_operation_and_use_the_parent_only_for_creation() {
        let sid = security::current_sid().unwrap();
        let scratch = Scratch::new();
        let missing = format!(r"{}\ArgusTaskmgrProbe-missing.exe", scratch.0);
        let allowed = || {
            access_allowed_in(
                HKEY_CURRENT_USER,
                &missing,
                &scratch.0,
                Call::Create(WRITE_ACCESS.0),
            )
        };
        assert_eq!(allowed(), Some(true));
        // Creating a subkey does not require setting a value on its parent.
        scratch.permit(&format!("D:P(A;;0x4;;;{sid})"));
        assert_eq!(allowed(), Some(true));
        assert_eq!(
            access_allowed_in(HKEY_CURRENT_USER, &scratch.0, &scratch.0, Call::SetValue),
            Some(false)
        );
        assert_eq!(
            access_allowed_in(HKEY_CURRENT_USER, &missing, &scratch.0, Call::SetValue),
            None
        );
        scratch.permit(&format!("D:P(A;;KR;;;{sid})"));
        assert_eq!(allowed(), Some(false));
        assert_eq!(
            access_allowed_in(HKEY_CURRENT_USER, &scratch.0, &scratch.0, Call::SetValue),
            Some(false)
        );
    }

    #[test]
    fn an_entry_is_described_by_what_it_holds() {
        let scratch = Scratch::new();
        let path = format!(r"{}\taskmgr.exe", scratch.0);
        let found = || describe_in(HKEY_CURRENT_USER, &path).unwrap();
        assert_eq!(found(), Found::default());
        let key = create_in(HKEY_CURRENT_USER, &path).unwrap();
        assert_eq!(found(), Found::default());
        create_in(HKEY_CURRENT_USER, &format!(r"{path}\PerfOptions")).unwrap();
        err(
            unsafe {
                RegSetValueExW(
                    key.0,
                    PCWSTR(util::wide("UseFilter").as_ptr()),
                    None,
                    REG_DWORD,
                    Some(&0u32.to_le_bytes()),
                )
            },
            Call::SetValue,
        )
        .unwrap();
        let empty = Value::string("");
        set_debugger(&key, Some(&empty)).unwrap();
        assert_eq!(
            found(),
            Found {
                debugger: Some(empty),
                use_filter: true,
                subkeys: vec!["PerfOptions".into()],
                subkey_count: 1,
            }
        );
    }

    #[test]
    fn updating_values_does_not_require_registry_notifications() {
        let scratch = Scratch::new();
        let sid = security::current_sid().unwrap();
        // Permit querying, setting, enumerating, creating and deleting, but
        // not notifications. The owner retains security-descriptor access.
        scratch.permit(&format!("D:P(A;CI;0x0001000f;;;{sid})"));
        let path = format!(r"{}\probe.exe", scratch.0);
        create_in(HKEY_CURRENT_USER, &path).unwrap();
        let old_access = KEY_READ | KEY_SET_VALUE;
        let old_error = open_in(HKEY_CURRENT_USER, &path, old_access).err().unwrap();
        assert_eq!(
            old_error.os(),
            Some(OsError {
                call: Call::Open(old_access.0),
                code: 5
            })
        );
        assert_eq!(
            access_allowed_in(
                HKEY_CURRENT_USER,
                &path,
                &scratch.0,
                Call::Open(old_access.0)
            ),
            Some(false)
        );
        assert_eq!(
            access_allowed_in(HKEY_CURRENT_USER, &path, &scratch.0, Call::SetValue),
            Some(true)
        );
        let value = Value::string("launcher.exe --probe-redirect");
        write_in(HKEY_CURRENT_USER, &path, Some(&value)).unwrap();
        assert_eq!(
            describe_in(HKEY_CURRENT_USER, &path).unwrap().debugger,
            Some(value.clone())
        );
        let key = open_in(HKEY_CURRENT_USER, &path, KEY_QUERY_VALUE)
            .unwrap()
            .unwrap();
        assert_eq!(read_value(&key, "Debugger").unwrap(), Some(value));
        write_in(HKEY_CURRENT_USER, &path, None).unwrap();
        assert_eq!(read_value(&key, "Debugger").unwrap(), None);
    }

    #[test]
    fn a_writable_dacl_does_not_explain_a_refused_write_on_a_read_only_handle() {
        let scratch = Scratch::new();
        let key = open_in(HKEY_CURRENT_USER, &scratch.0, KEY_QUERY_VALUE)
            .unwrap()
            .unwrap();
        let error = set_debugger(&key, Some(&Value::string("launcher.exe"))).unwrap_err();
        assert_eq!(
            error.os(),
            Some(OsError {
                call: Call::SetValue,
                code: 5
            })
        );
        assert_eq!(
            access_allowed_in(HKEY_CURRENT_USER, &scratch.0, &scratch.0, Call::SetValue),
            Some(true)
        );
        assert_eq!(read_value(&key, "Debugger").unwrap(), None);
    }

    #[test]
    fn os_errors_keep_their_call_and_code() {
        let os = OsError {
            call: Call::Open(WRITE_ACCESS.0),
            code: 5,
        };
        let error = Error::Os(os);
        assert!(error.denied());
        assert_eq!(error.os(), Some(os));
        assert!(String::from(error).contains(os.call.name()));
        assert_eq!(
            serde_json::from_str::<OsError>(&serde_json::to_string(&os).unwrap()).unwrap(),
            os
        );
        assert!(!Error::Text("text".into()).denied());
        // A refusal is explained only for administrators.
        if !argus_core::privilege::is_elevated() {
            assert_eq!(refusal(&Error::Os(os), KEY), None);
        }
    }
}
