use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

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
    Os(u32),
    Text(String),
}

impl Error {
    pub fn denied(&self) -> bool {
        matches!(self, Self::Os(5))
    }
}

impl From<Error> for String {
    fn from(error: Error) -> Self {
        match error {
            Error::Os(code) => util::message(code),
            Error::Text(text) => text,
        }
    }
}

pub type RegResult<T> = std::result::Result<T, Error>;

fn err(code: WIN32_ERROR) -> RegResult<()> {
    if code.0 == 0 {
        Ok(())
    } else {
        Err(Error::Os(code.0))
    }
}

pub fn open(path: &str, write: bool) -> RegResult<Option<Key>> {
    open_in(
        HKEY_LOCAL_MACHINE,
        path,
        KEY_READ
            | if write {
                KEY_SET_VALUE
            } else {
                Default::default()
            },
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
    err(code)?;
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
    err(code)?;
    if size > 16 * 1024 {
        return Err(Error::Text(tr!("taskManager.valueTooLong")));
    }
    let mut bytes = vec![0; size as usize];
    err(unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut size),
        )
    })?;
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
        err(code)?;
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
    let Some(key) = open_in(root, path, KEY_READ)? else {
        return Ok(Found::default());
    };
    let mut count = 0;
    err(unsafe {
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
    })?;
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
        err(code)?;
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
    let key = match open(path, true)? {
        Some(key) => key,
        None if value.is_none() => return Ok(()),
        None => create(path)?,
    };
    set_debugger(&key, value)
}

/// Creates a key, or opens the one that appeared meanwhile.
pub fn create(path: &str) -> RegResult<Key> {
    create_in(HKEY_LOCAL_MACHINE, path)
}

fn create_in(root: HKEY, path: &str) -> RegResult<Key> {
    let mut raw = HKEY::default();
    err(unsafe {
        RegCreateKeyExW(
            root,
            PCWSTR(util::wide(path).as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_SET_VALUE,
            None,
            &mut raw,
            None,
        )
    })?;
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
    if code != ERROR_FILE_NOT_FOUND {
        err(code)?;
    }
    err(unsafe { RegFlushKey(key.0) })
}

/// Deletes a key without subkeys; one that is gone already is fine.
pub fn delete(path: &str) -> RegResult<()> {
    let code = unsafe { RegDeleteKeyW(HKEY_LOCAL_MACHINE, PCWSTR(util::wide(path).as_ptr())) };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    err(code)
}

/// Why an administrator could not change an entry under IFEO, when the error
/// alone does not say: its permissions forbid it, or something outside them
/// (security software that guards the registry, a policy) refused.
pub fn refusal(error: &Error, path: &str) -> Option<Cause> {
    if !error.denied() || !argus_core::privilege::is_elevated() {
        return None;
    }
    Some(if write_allowed(path)? {
        Cause::Blocked
    } else {
        Cause::Permissions
    })
}

/// Whether the DACL of `path` lets this process create subkeys and set
/// values; while `path` does not exist, IFEO's, which its entries inherit.
/// Read-only: it never tries the write. None when it cannot tell.
pub fn write_allowed(path: &str) -> Option<bool> {
    write_allowed_in(HKEY_LOCAL_MACHINE, path, IFEO)
}

fn write_allowed_in(root: HKEY, path: &str, parent: &str) -> Option<bool> {
    // READ_CONTROL alone: the security descriptor is all this reads.
    let read = REG_SAM_FLAGS(0x0002_0000);
    let key = match open_in(root, path, read).ok()? {
        Some(key) => key,
        None => open_in(root, parent, read).ok()??,
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
    err(unsafe { RegGetKeySecurity(key.0, parts, Some(descriptor), &mut size) }).ok()?;
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
    let wanted = (KEY_CREATE_SUB_KEY | KEY_SET_VALUE).0;
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
    /// Why the last change to taskmgr.exe's entry was refused, when it was.
    pub refused: Option<Cause>,
}
impl Machine {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, refused: None }
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
            self.refused = refusal(&error, KEY);
            super::explain(
                tr!("taskManager.entryChangeFailed", error = String::from(error)),
                self.refused,
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
            err(unsafe { RegSetKeySecurity(key.0, DACL_SECURITY_INFORMATION, descriptor.0) })
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
    fn write_permission_comes_from_the_dacl_of_the_key_or_its_parent() {
        let sid = security::current_sid().unwrap();
        let scratch = Scratch::new();
        let missing = format!(r"{}\ArgusTaskmgrProbe-missing.exe", scratch.0);
        let allowed = || write_allowed_in(HKEY_CURRENT_USER, &missing, &scratch.0);
        assert_eq!(allowed(), Some(true));
        scratch.permit(&format!("D:P(A;;KR;;;{sid})"));
        assert_eq!(allowed(), Some(false));
        assert_eq!(
            write_allowed_in(HKEY_CURRENT_USER, &scratch.0, &scratch.0),
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
        err(unsafe {
            RegSetValueExW(
                key.0,
                PCWSTR(util::wide("UseFilter").as_ptr()),
                None,
                REG_DWORD,
                Some(&0u32.to_le_bytes()),
            )
        })
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
    fn os_errors_keep_their_code_and_text() {
        let error = Error::Os(5);
        assert!(error.denied());
        assert_eq!(String::from(error), util::message(5));
        assert!(!Error::Text("text".into()).denied());
        // A refusal is explained only for administrators.
        if !argus_core::privilege::is_elevated() {
            assert_eq!(refusal(&Error::Os(5), KEY), None);
        }
    }
}
