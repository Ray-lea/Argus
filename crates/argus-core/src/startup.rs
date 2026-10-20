//! Programs that start at logon: Run keys, Startup folders and packaged tasks.
//! Each source retains its own identity and Windows enabled/disabled state.

mod packaged;

use std::path::Path;

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_SUCCESS, FILETIME, MAX_PATH};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
    STGM_READ,
};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_BINARY,
    REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW,
    RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
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
    (unsafe { RegOpenKeyExW(root, PCWSTR(w.as_ptr()), None, access, &mut key) } == ERROR_SUCCESS)
        .then_some(Key(key))
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
    r.ok()?;
    let key = Key(key);
    let value = wide(name);
    unsafe { RegSetValueExW(key.0, PCWSTR(value.as_ptr()), None, REG_BINARY, Some(&data)) }.ok()?;
    Ok(())
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
}
