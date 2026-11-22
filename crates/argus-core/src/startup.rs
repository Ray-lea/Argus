//! Programs that start at logon: Run keys, Startup folders and packaged tasks.
//! Each source retains its own identity and Windows enabled/disabled state.

mod packaged;

use std::{io::ErrorKind, os::windows::fs::OpenOptionsExt, path::Path};

use serde::Serialize;
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, FILETIME, MAX_PATH, WIN32_ERROR,
};
use windows::Win32::Storage::FileSystem::{DELETE, FILE_FLAG_OPEN_REPARSE_POINT};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
    STGM_READ,
};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_BINARY,
    REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW,
    RegDeleteValueW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::UI::Shell::{
    FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR, FOF_NOERRORUI, FOF_SILENT,
    IShellLinkW, SHFILEOPSTRUCTW, SHFileOperationW, ShellLink,
};
use windows::core::{Interface, PCWSTR, PWSTR};

use crate::actions::ActionError;
use crate::meta::{MetaStore, version_info};
use crate::util::{from_wide, wide};

const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

#[derive(Debug, Clone, Copy)]
struct Location {
    id: &'static str,
    root: HKEY,
    /// Registry key holding the entries, or None for a Startup folder.
    key: Option<&'static str>,
    /// Startup folder, relative to an environment variable.
    folder: Option<(&'static str, &'static str)>,
    approved: &'static str,
}

// HKEY wraps a raw pointer; these are predefined, process-independent handles.
unsafe impl Sync for Location {}

const LOCATIONS: &[Location] = &[
    Location {
        id: "hkcuRun",
        root: HKEY_CURRENT_USER,
        key: Some(r"Software\Microsoft\Windows\CurrentVersion\Run"),
        folder: None,
        approved: "Run",
    },
    Location {
        id: "hklmRun",
        root: HKEY_LOCAL_MACHINE,
        key: Some(r"Software\Microsoft\Windows\CurrentVersion\Run"),
        folder: None,
        approved: "Run",
    },
    Location {
        id: "hklmRun32",
        root: HKEY_LOCAL_MACHINE,
        key: Some(r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run"),
        folder: None,
        approved: "Run32",
    },
    Location {
        id: "userFolder",
        root: HKEY_CURRENT_USER,
        key: None,
        folder: Some(("APPDATA", r"Microsoft\Windows\Start Menu\Programs\Startup")),
        approved: "StartupFolder",
    },
    Location {
        id: "commonFolder",
        root: HKEY_LOCAL_MACHINE,
        key: None,
        folder: Some((
            "ProgramData",
            r"Microsoft\Windows\Start Menu\Programs\StartUp",
        )),
        approved: "StartupFolder",
    },
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    /// `<location>|<entry>`; pass back to [`set_enabled`].
    pub id: String,
    pub name: String,
    /// Original registry value, shortcut name or packaged task identifier.
    pub entry: String,
    pub command: String,
    /// Executable the command runs, when it could be resolved.
    pub path: Option<String>,
    /// Run key, Startup folder or `package`.
    pub location: &'static str,
    /// Applies to all users (needs administrator rights to change).
    pub machine: bool,
    pub enabled: bool,
    /// `managed` or `unavailable`; None when the switch can be changed.
    pub blocked: Option<&'static str>,
    /// False when the target executable no longer exists.
    pub exists: bool,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub icon: Option<u32>,
}

/// Closes a registry key on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open_key(
    root: HKEY,
    path: &str,
    access: windows::Win32::System::Registry::REG_SAM_FLAGS,
) -> Option<Key> {
    let w = wide(path);
    let mut key = HKEY::default();
    let result = unsafe { RegOpenKeyExW(root, PCWSTR(w.as_ptr()), None, access, &mut key) };
    if result != ERROR_SUCCESS && result.0 != 2 {
        crate::diagnostics::warn(
            "startup RegOpenKeyExW",
            format!(
                "{}; machine={}; key={}",
                crate::diagnostics::code(result.0),
                root == HKEY_LOCAL_MACHINE,
                crate::diagnostics::path(Path::new(path))
            ),
        );
    }
    (result == ERROR_SUCCESS).then_some(Key(key))
}

pub fn list(meta: &MetaStore) -> Vec<StartupItem> {
    let mut out = Vec::new();
    for loc in LOCATIONS {
        let entries = match (loc.key, loc.folder) {
            (Some(key), _) => registry_entries(loc.root, key),
            (None, Some(folder)) => folder_entries(folder),
            _ => Vec::new(),
        };
        let approved = open_key(loc.root, &format!(r"{APPROVED}\{}", loc.approved), KEY_READ);
        for (name, command, path) in entries {
            let enabled = approved.as_ref().is_none_or(|k| is_enabled(k, &name));
            let exists = path.as_deref().is_some_and(|p| Path::new(p).is_file());
            let (publisher, description, icon) = match path.as_deref().filter(|_| exists) {
                Some(p) => {
                    let v = version_info(p);
                    let icon = meta.get_or_request(p).and_then(|m| m.icon);
                    (v.company, v.description, icon)
                }
                None => (None, None, None),
            };
            out.push(StartupItem {
                id: format!("{}|{}", loc.id, name),
                name: display_name(
                    &name,
                    loc.folder.is_some(),
                    description.as_deref(),
                    path.as_deref(),
                ),
                entry: name,
                command,
                path,
                location: loc.id,
                machine: loc.root == HKEY_LOCAL_MACHINE,
                enabled,
                blocked: None,
                exists,
                publisher,
                description,
                icon,
            });
        }
    }
    out.extend(packaged::list(meta));
    out
}

fn display_name(
    entry: &str,
    shortcut: bool,
    description: Option<&str>,
    path: Option<&str>,
) -> String {
    if shortcut {
        return if entry.to_ascii_lowercase().ends_with(".lnk") {
            entry[..entry.len() - 4].to_owned()
        } else {
            entry.to_owned()
        };
    }
    description
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            path.and_then(|p| Path::new(p).file_name())
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| entry.to_owned())
}

fn registry_entries(root: HKEY, path: &str) -> Vec<(String, String, Option<String>)> {
    let Some(key) = open_key(root, path, KEY_READ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut name = vec![0u16; 16384];
    let mut data = vec![0u8; 64 * 1024];
    for index in 0.. {
        let mut name_len = name.len() as u32;
        let mut data_len = data.len() as u32;
        let mut kind = 0u32;
        let r = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            )
        };
        if r != ERROR_SUCCESS {
            break; // ERROR_NO_MORE_ITEMS (or an oversized value we skip)
        }
        if kind != REG_SZ.0 && kind != REG_EXPAND_SZ.0 {
            continue;
        }
        let value = from_wide(unsafe {
            std::slice::from_raw_parts(data.as_ptr() as *const u16, data_len as usize / 2)
        });
        let command = expand(value.trim());
        if command.is_empty() {
            continue;
        }
        let path = exe_from_command(&command);
        out.push((
            String::from_utf16_lossy(&name[..name_len as usize]),
            command,
            path,
        ));
    }
    out
}

fn folder_entries((var, rel): (&str, &str)) -> Vec<(String, String, Option<String>)> {
    let Ok(base) = std::env::var(var) else {
        return Vec::new();
    };
    let dir = Path::new(&base).join(rel);
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if file.eq_ignore_ascii_case("desktop.ini") || !entry.path().is_file() {
            continue;
        }
        let full = entry.path().to_string_lossy().into_owned();
        let (command, path) = if file.to_lowercase().ends_with(".lnk") {
            match resolve_shortcut(&full) {
                Some((target, args)) if args.is_empty() => (format!("\"{target}\""), Some(target)),
                Some((target, args)) => (format!("\"{target}\" {args}"), Some(target)),
                None => (full.clone(), None),
            }
        } else {
            (format!("\"{full}\""), Some(full.clone()))
        };
        out.push((file, command, path));
    }
    out
}

fn expand(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let w = wide(s);
    let mut buf = vec![0u16; 4096];
    let n = unsafe { ExpandEnvironmentStringsW(PCWSTR(w.as_ptr()), Some(&mut buf)) };
    if n == 0 || n as usize > buf.len() {
        s.to_owned()
    } else {
        from_wide(&buf)
    }
}

/// The executable a command line runs, resolved the way CreateProcess does
/// for unquoted paths containing spaces.
fn exe_from_command(command: &str) -> Option<String> {
    let cmd = command.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        return rest
            .split('"')
            .next()
            .map(str::to_owned)
            .filter(|s| !s.is_empty());
    }
    let mut candidates: Vec<&str> = cmd.match_indices(' ').map(|(i, _)| &cmd[..i]).collect();
    candidates.push(cmd);
    for c in &candidates {
        if Path::new(c).is_file() {
            return Some((*c).to_owned());
        }
        let with_exe = format!("{c}.exe");
        if !c.to_lowercase().ends_with(".exe") && Path::new(&with_exe).is_file() {
            return Some(with_exe);
        }
    }
    // A bare program name such as `ctfmon.exe`: look in System32.
    let first = candidates.first().copied().unwrap_or(cmd);
    if !first.contains('\\') {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let in_system = Path::new(&root).join("System32").join(first);
        if in_system.is_file() {
            return Some(in_system.to_string_lossy().into_owned());
        }
    }
    Some(first.to_owned())
}

/// Target and arguments of a `.lnk` shortcut.
fn resolve_shortcut(path: &str) -> Option<(String, String)> {
    unsafe {
        // Fine on any thread: MTA if nobody initialized COM here yet.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        let w = wide(path);
        file.Load(PCWSTR(w.as_ptr()), STGM_READ).ok()?;
        let mut target = vec![0u16; MAX_PATH as usize * 4];
        link.GetPath(&mut target, std::ptr::null_mut(), 0).ok()?;
        let mut args = vec![0u16; 4096];
        let _ = link.GetArguments(&mut args);
        let target = from_wide(&target);
        (!target.is_empty()).then(|| (target, from_wide(&args)))
    }
}

/// `StartupApproved` values are 12 bytes; an odd first byte means disabled.
fn is_enabled(approved: &Key, name: &str) -> bool {
    let w = wide(name);
    let mut data = [0u8; 12];
    let mut len = data.len() as u32;
    let mut kind = REG_VALUE_TYPE::default();
    let r = unsafe {
        RegQueryValueExW(
            approved.0,
            PCWSTR(w.as_ptr()),
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut len),
        )
    };
    r != ERROR_SUCCESS || len == 0 || data[0] & 1 == 0
}

/// Enables or disables an entry the way Task Manager does, without touching the
/// entry itself.
pub fn set_enabled(id: &str, enabled: bool) -> Result<(), ActionError> {
    let (loc_id, name) = id.split_once('|').ok_or(ActionError::NotFound)?;
    if loc_id == "package" {
        return packaged::set_enabled(name, enabled);
    }
    let loc = LOCATIONS
        .iter()
        .find(|l| l.id == loc_id)
        .ok_or(ActionError::NotFound)?;

    let mut data = [0u8; 12];
    if enabled {
        data[0] = 0x02;
    } else {
        data[0] = 0x03;
        let ft: FILETIME = unsafe { GetSystemTimeAsFileTime() };
        data[4..8].copy_from_slice(&ft.dwLowDateTime.to_le_bytes());
        data[8..12].copy_from_slice(&ft.dwHighDateTime.to_le_bytes());
    }

    let path = wide(&format!(r"{APPROVED}\{}", loc.approved));
    let mut key = HKEY::default();
    let r = unsafe {
        RegCreateKeyExW(
            loc.root,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    r.ok().inspect_err(|error| {
        crate::diagnostics::windows_failure("startup RegCreateKeyExW", error)
    })?;
    let key = Key(key);
    let value = wide(name);
    unsafe { RegSetValueExW(key.0, PCWSTR(value.as_ptr()), None, REG_BINARY, Some(&data)) }
        .ok()
        .inspect_err(|error| {
            crate::diagnostics::windows_failure("startup RegSetValueExW", error)
        })?;
    Ok(())
}

/// Deletes an entry: its Run key value, or its file in a Startup folder (to
/// the Recycle Bin), and with it the switch that enabled or disabled it.
/// Packaged tasks belong to their app and are not deleted.
pub fn delete(id: &str) -> Result<(), ActionError> {
    let (loc_id, name) = id.split_once('|').ok_or(ActionError::NotFound)?;
    if loc_id == "package" {
        return Err(ActionError::Refused);
    }
    let loc = LOCATIONS
        .iter()
        .find(|l| l.id == loc_id)
        .ok_or(ActionError::NotFound)?;
    match (loc.key, loc.folder) {
        (Some(key), _) => delete_value(loc.root, key, name)?,
        (None, Some((var, rel))) => {
            let base = std::env::var(var).map_err(|_| ActionError::NotFound)?;
            remove_file(&Path::new(&base).join(rel), name, true)?;
        }
        _ => return Err(ActionError::NotFound),
    }
    forget_switch(loc.root, loc.approved, name);
    Ok(())
}

/// A failed registry call as the action's error: a key or value not there
/// is gone already.
fn registry_error(event: &'static str, result: WIN32_ERROR) -> ActionError {
    if result == ERROR_FILE_NOT_FOUND {
        return ActionError::NotFound;
    }
    let error = windows::core::Error::from(result.to_hresult());
    crate::diagnostics::windows_failure(event, &error);
    error.into()
}

fn delete_value(root: HKEY, path: &str, name: &str) -> Result<(), ActionError> {
    let path = wide(path);
    let mut key = HKEY::default();
    let opened =
        unsafe { RegOpenKeyExW(root, PCWSTR(path.as_ptr()), None, KEY_SET_VALUE, &mut key) };
    if opened != ERROR_SUCCESS {
        return Err(registry_error("startup delete RegOpenKeyExW", opened));
    }
    let key = Key(key);
    let value = wide(name);
    let deleted = unsafe { RegDeleteValueW(key.0, PCWSTR(value.as_ptr())) };
    if deleted != ERROR_SUCCESS {
        return Err(registry_error("startup RegDeleteValueW", deleted));
    }
    Ok(())
}

/// Takes away the switch Windows keeps for an entry deleted, so that one
/// added later under the name starts enabled. Failing to is only logged:
/// the entry itself is gone.
fn forget_switch(root: HKEY, approved: &str, name: &str) {
    let path = wide(&format!(r"{APPROVED}\{approved}"));
    let mut key = HKEY::default();
    if unsafe { RegOpenKeyExW(root, PCWSTR(path.as_ptr()), None, KEY_SET_VALUE, &mut key) }
        != ERROR_SUCCESS
    {
        return;
    }
    let key = Key(key);
    let value = wide(name);
    let result = unsafe { RegDeleteValueW(key.0, PCWSTR(value.as_ptr())) };
    if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
        crate::diagnostics::warn(
            "startup forget switch RegDeleteValueW",
            crate::diagnostics::code(result.0),
        );
    }
}

/// Deletes the file `name` in `folder`, to the Recycle Bin with `recycle`,
/// asking and showing nothing. Only a plain name in the folder itself goes.
fn remove_file(folder: &Path, name: &str, recycle: bool) -> Result<(), ActionError> {
    if matches!(name, "" | "." | "..") || name.contains(['\\', '/', ':', '\0']) {
        return Err(ActionError::Refused);
    }
    let path = folder.join(name);
    if !path.is_file() {
        return Err(ActionError::NotFound);
    }
    // Check the caller's delete access on the directory entry itself before
    // handing it to the Shell. Release the handle before the recycle operation.
    if let Err(error) = std::fs::File::options()
        .access_mode(DELETE.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(&path)
    {
        crate::diagnostics::file_failure("startup open for deletion", &path, &error);
        return Err(match error.kind() {
            ErrorKind::PermissionDenied => ActionError::AccessDenied,
            ErrorKind::NotFound => ActionError::NotFound,
            _ => ActionError::Os(error.to_string()),
        });
    }
    // The paths, each ended by a NUL, and the list by another.
    let mut from = wide(&path.to_string_lossy());
    from.push(0);
    let silent = FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_NOCONFIRMMKDIR;
    let flags = if recycle {
        silent | FOF_ALLOWUNDO
    } else {
        silent
    };
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: flags.0 as u16,
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut operation) };
    if code == 0 && !operation.fAnyOperationsAborted.as_bool() {
        return Ok(());
    }
    crate::diagnostics::warn(
        "startup SHFileOperationW",
        format!(
            "{}; aborted={}; recycle={recycle}",
            crate::diagnostics::code(code as u32),
            operation.fAnyOperationsAborted.as_bool()
        ),
    );
    Err(shell_error(code as u32))
}

/// What `SHFileOperationW` returned, as the action's error: a Win32 code,
/// or for the common causes one of the shell's own (`DE_*`, 0x71 and up).
fn shell_error(code: u32) -> ActionError {
    match code {
        // DE_ACCESSDENIEDSRC
        5 | 0x78 => ActionError::AccessDenied,
        // DE_INVALIDFILES
        2 | 3 | 0x7C => ActionError::NotFound,
        1..0x71 => windows::core::Error::from(WIN32_ERROR(code).to_hresult()).into(),
        _ => ActionError::Os(format!("SHFileOperation 0x{code:X}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_names_prefer_program_metadata_without_losing_shortcut_names() {
        assert_eq!(
            display_name(
                "MicrosoftEdgeAutoLaunch_random",
                false,
                Some("Microsoft Edge"),
                Some(r"C:\Apps\msedge.exe")
            ),
            "Microsoft Edge"
        );
        assert_eq!(
            display_name("internal-id", false, None, Some(r"C:\Apps\Example.exe")),
            "Example.exe"
        );
        assert_eq!(
            display_name(
                "Launch hidden.LNK",
                true,
                Some("Example"),
                Some(r"C:\Apps\Example.exe")
            ),
            "Launch hidden"
        );
        assert_eq!(display_name("Value.lnk", false, None, None), "Value.lnk");
        assert_eq!(
            display_name("Missing app", false, Some(" "), None),
            "Missing app"
        );
    }

    /// A name no entry has, and none of another test run.
    fn unique(prefix: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.subsec_nanos());
        format!("{prefix}{}x{nanos}", std::process::id())
    }

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    /// Deletes a value of the test's own, whatever became of it.
    struct Cleanup(String, String);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = delete_value(HKEY_CURRENT_USER, &self.0, &self.1);
        }
    }

    fn value_exists(path: &str, name: &str) -> bool {
        let Some(key) = open_key(HKEY_CURRENT_USER, path, KEY_READ) else {
            return false;
        };
        let name = wide(name);
        let result =
            unsafe { RegQueryValueExW(key.0, PCWSTR(name.as_ptr()), None, None, None, None) };
        result == ERROR_SUCCESS
    }

    /// A Run value of the test's own, disabled so that it has a switch too,
    /// goes with its switch; deleting it again says it is gone.
    #[test]
    fn a_run_entry_is_deleted_with_its_switch() {
        let name = unique("Argus test entry ");
        let switch = format!(r"{APPROVED}\Run");
        let _value = Cleanup(RUN.into(), name.clone());
        let _switch = Cleanup(switch.clone(), name.clone());
        let command = wide(r#""C:\Windows\System32\cmd.exe" /c exit"#);
        let key = open_key(HKEY_CURRENT_USER, RUN, KEY_SET_VALUE).expect("Run key");
        unsafe {
            RegSetValueExW(
                key.0,
                PCWSTR(wide(&name).as_ptr()),
                None,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    command.as_ptr().cast(),
                    command.len() * 2,
                )),
            )
        }
        .ok()
        .expect("test value");
        drop(key);
        let id = format!("hkcuRun|{name}");
        set_enabled(&id, false).expect("switch");
        assert!(value_exists(RUN, &name) && value_exists(&switch, &name));

        delete(&id).expect("deleted");
        assert!(!value_exists(RUN, &name), "the entry is gone");
        assert!(!value_exists(&switch, &name), "and its switch");
        assert!(matches!(delete(&id), Err(ActionError::NotFound)));
    }

    #[test]
    fn entries_for_all_users_need_administrator_rights_and_packages_stay() {
        // Never there, so nothing is deleted either way: without the rights
        // the key does not open, with them the value is not found.
        let result = delete(&format!("hklmRun|{}", unique("Argus missing ")));
        if crate::privilege::is_elevated() {
            assert!(matches!(result, Err(ActionError::NotFound)));
        } else {
            assert!(matches!(result, Err(ActionError::AccessDenied)));
        }
        assert!(matches!(
            delete("package|Some.App!Task"),
            Err(ActionError::Refused)
        ));
        assert!(matches!(
            delete("elsewhere|name"),
            Err(ActionError::NotFound)
        ));
        assert!(matches!(delete("no separator"), Err(ActionError::NotFound)));
    }

    /// A folder of the test's own with a file in it, taken apart afterwards
    /// whatever its permissions became.
    struct Folder(std::path::PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::process::Command::new("icacls")
                .arg(&self.0)
                .args(["/reset", "/t", "/q"])
                .output();
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn folder_with(file: &str) -> Folder {
        let folder = std::env::temp_dir().join(unique("argus-startup-"));
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join(file), "test").unwrap();
        Folder(folder)
    }

    #[test]
    fn a_startup_file_goes_and_only_by_its_own_name() {
        let folder = folder_with("Test app.lnk");
        for name in ["", ".", "..", r"..\Test app.lnk", "a/b", "C:x", "x\0y"] {
            assert!(matches!(
                remove_file(&folder.0, name, false),
                Err(ActionError::Refused)
            ));
        }
        assert!(matches!(
            remove_file(&folder.0, "Other.lnk", false),
            Err(ActionError::NotFound)
        ));
        remove_file(&folder.0, "Test app.lnk", false).expect("deleted");
        assert!(!folder.0.join("Test app.lnk").exists());
    }

    /// A file nobody may delete is refused as access denied, with no window
    /// shown to ask.
    #[test]
    fn a_startup_file_without_the_rights_is_access_denied() {
        let folder = folder_with("Locked.lnk");
        // Everyone (S-1-1-0) is denied deleting the file and the folder's
        // children.
        for (path, right) in [
            (folder.0.join("Locked.lnk"), "*S-1-1-0:(D)"),
            (folder.0.clone(), "*S-1-1-0:(DC)"),
        ] {
            let status = std::process::Command::new("icacls")
                .arg(&path)
                .args(["/deny", right, "/q"])
                .status()
                .expect("icacls");
            assert!(status.success());
        }
        let result = remove_file(&folder.0, "Locked.lnk", true);
        assert!(
            matches!(result, Err(ActionError::AccessDenied)),
            "{result:?}"
        );
        assert!(folder.0.join("Locked.lnk").exists());
    }

    /// A file deleted goes to the Recycle Bin. On demand only, as it passes
    /// through the account's Recycle Bin: the entry it leaves there, found
    /// by the folder's unique name, is removed afterwards.
    #[test]
    #[ignore = "uses the account's Recycle Bin"]
    fn a_startup_file_goes_to_the_recycle_bin() {
        let folder = folder_with("Recycled.lnk");
        remove_file(&folder.0, "Recycled.lnk", true).expect("recycled");
        assert!(!folder.0.join("Recycled.lnk").exists());

        // `$I…` records the original path (UTF-16) of the `$R…` beside it.
        let mark = folder.0.file_name().unwrap().to_string_lossy().into_owned();
        let mark = mark
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let drive = folder.0.components().next().unwrap().as_os_str().to_owned();
        let bin = std::path::PathBuf::from(format!(r"{}\$Recycle.Bin", drive.to_string_lossy()));
        let mut found = 0;
        for account in std::fs::read_dir(&bin).unwrap().flatten() {
            let Ok(entries) = std::fs::read_dir(account.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(rest) = name.strip_prefix("$I") else {
                    continue;
                };
                let record = std::fs::read(entry.path()).unwrap_or_default();
                if record.windows(mark.len()).any(|window| window == mark) {
                    std::fs::remove_file(entry.path()).unwrap();
                    std::fs::remove_file(account.path().join(format!("$R{rest}"))).unwrap();
                    found += 1;
                }
            }
        }
        assert_eq!(found, 1, "the file is in the Recycle Bin, once");
    }
}
