//! Packaged startup tasks are per-user AppModel registrations, not Run values.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use windows::ApplicationModel::Package;
use windows::Data::Xml::Dom::{IXmlNode, XmlDocument, XmlLoadSettings};
use windows::Management::Deployment::PackageManager;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_INVALID_DATA};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE,
    REG_VALUE_TYPE, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::Win32::UI::Shell::SHLoadIndirectString;
use windows::core::{HSTRING, PCWSTR};

use super::{Key, StartupItem};
use crate::actions::ActionError;
use crate::meta::MetaStore;
use crate::util::{from_wide, wide};

const SYSTEM_APP_DATA: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData";

struct CachedPackage {
    manifest: PathBuf,
    executable_root: String,
    modified: SystemTime,
    items: Vec<PackagedItem>,
}
static CACHE: LazyLock<Mutex<HashMap<String, CachedPackage>>> = LazyLock::new(Default::default);

#[derive(Debug, Clone, Copy)]
struct TaskKind {
    enabled_by_default: bool,
    user_configurable: bool,
    immediate_registration: bool,
}

#[derive(Clone)]
struct PackagedItem {
    item: StartupItem,
    kind: TaskKind,
}

struct Apartment(bool);
impl Apartment {
    fn new() -> Self {
        Self(unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok())
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { RoUninitialize() };
        }
    }
}

#[derive(Debug)]
struct Task {
    id: String,
    name: String,
    executable: String,
    arguments: String,
    kind: TaskKind,
}

fn attribute(node: &IXmlNode, name: &str) -> String {
    node.Attributes()
        .and_then(|attrs| attrs.GetNamedItem(&HSTRING::from(name)))
        .and_then(|value| value.InnerText())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn tasks(xml: &str) -> windows::core::Result<Vec<Task>> {
    let doc = XmlDocument::new()?;
    let settings = XmlLoadSettings::new()?;
    settings.SetProhibitDtd(true)?;
    settings.SetResolveExternals(false)?;
    doc.LoadXmlWithSettings(
        &HSTRING::from(xml.trim_start_matches('\u{feff}')),
        &settings,
    )?;
    let managed_capability = doc.SelectSingleNode(&HSTRING::from(
        "/*[local-name()='Package']/*[local-name()='Capabilities']/*[local-name()='CustomCapability' and @Name='Microsoft.nonUserConfigurableStartupTasks_8wekyb3d8bbwe']",
    )).is_ok();
    let nodes = doc.SelectNodes(&HSTRING::from(
        "/*[local-name()='Package']/*[local-name()='Applications']/*[local-name()='Application']/*[local-name()='Extensions']/*[local-name()='Extension' and @Category='windows.startupTask']/*[local-name()='StartupTask']",
    ))?;
    let mut out = Vec::new();
    for node in nodes {
        let id = attribute(&node, "TaskId");
        if !valid_component(&id) {
            continue;
        }
        let extension = node.ParentNode()?;
        let application = extension.ParentNode()?.ParentNode()?;
        let mut executable = attribute(&extension, "Executable");
        if executable.is_empty() {
            executable = attribute(&application, "Executable");
        }
        let arguments = extension
            .SelectSingleNode(&HSTRING::from("@*[local-name()='Parameters']"))
            .and_then(|value| value.InnerText())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let kind = TaskKind {
            enabled_by_default: matches!(attribute(&node, "Enabled").as_str(), "true" | "1"),
            // Windows 10 ignores UserConfigurable without the custom capability;
            // newer Windows refuses such a package at registration.
            user_configurable: !managed_capability
                || !node
                    .SelectSingleNode(&HSTRING::from("@*[local-name()='UserConfigurable']"))
                    .and_then(|value| value.InnerText())
                    .is_ok_and(|value| matches!(value.to_string().as_str(), "false" | "0")),
            immediate_registration: managed_capability
                && node
                    .SelectSingleNode(&HSTRING::from("@*[local-name()='ImmediateRegistration']"))
                    .and_then(|value| value.InnerText())
                    .is_ok_and(|value| matches!(value.to_string().as_str(), "true" | "1")),
        };
        out.push(Task {
            id,
            name: attribute(&node, "DisplayName"),
            executable,
            arguments,
            kind,
        });
    }
    Ok(out)
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['\\', '/'])
        && !value.chars().any(char::is_control)
}

fn executable_path(root: &Path, executable: &str) -> Option<String> {
    let relative = Path::new(executable);
    if !root.is_absolute()
        || executable.is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    Some(root.join(relative).to_string_lossy().into_owned())
}

fn resource_name(full_name: &str, identity: &str, value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let Some(resource) = value.strip_prefix("ms-resource:") else {
        return Some(value.to_owned());
    };
    let uris = if resource.starts_with("//") {
        vec![value.to_owned()]
    } else if resource.starts_with('/') {
        vec![format!("ms-resource://{identity}{resource}")]
    } else {
        vec![
            format!("ms-resource://{identity}/resources/{resource}"),
            format!("ms-resource://{identity}/{resource}"),
        ]
    };
    for uri in uris {
        let source = wide(&format!("@{{{full_name}? {uri}}}"));
        let mut output = vec![0u16; 4096];
        if unsafe { SHLoadIndirectString(PCWSTR(source.as_ptr()), &mut output, None) }.is_ok() {
            let name = from_wide(&output);
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn package_tasks(package: &Package) -> windows::core::Result<Vec<PackagedItem>> {
    if package.IsFramework()? || package.IsResourcePackage()? {
        return Ok(Vec::new());
    }
    let identity = package.Id()?;
    let family = identity.FamilyName()?.to_string();
    let full_name = identity.FullName()?.to_string();
    let name = identity.Name()?.to_string();
    if !valid_component(&family) {
        return Ok(Vec::new());
    }
    let directory = package
        .InstalledPath()
        .or_else(|_| package.InstalledLocation().and_then(|folder| folder.Path()))?
        .to_string();
    let executable_root = package
        .EffectivePath()
        .or_else(|_| package.EffectiveLocation().and_then(|folder| folder.Path()))
        .map(|value| value.to_string())
        .unwrap_or_else(|_| directory.clone());
    let manifest = Path::new(&directory).join("AppxManifest.xml");
    {
        let cache = CACHE.lock().unwrap();
        if let Some(cached) = cache.get(&full_name)
            && cached.manifest == manifest
            && cached.executable_root == executable_root
            && std::fs::metadata(&cached.manifest)
                .and_then(|m| m.modified())
                .ok()
                == Some(cached.modified)
        {
            return Ok(cached.items.clone());
        }
    }
    let root = Path::new(&executable_root);
    let Ok(xml) = std::fs::read_to_string(&manifest).inspect_err(|error| {
        crate::diagnostics::file_failure("startup package manifest read", &manifest, error)
    }) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for task in tasks(&xml).inspect_err(|error| {
        crate::diagnostics::warn(
            "startup package manifest parse",
            format!(
                "{}; path={}",
                crate::diagnostics::code(error.code().0 as u32),
                crate::diagnostics::path(&manifest)
            ),
        )
    })? {
        let path = executable_path(root, &task.executable);
        let exists = path.as_deref().is_some_and(|p| Path::new(p).is_file());
        let command = path
            .as_ref()
            .map(|p| {
                if task.arguments.is_empty() {
                    format!("\"{p}\"")
                } else {
                    format!("\"{p}\" {}", task.arguments)
                }
            })
            .unwrap_or_default();
        out.push(PackagedItem {
            kind: task.kind,
            item: StartupItem {
                id: format!("package|{family}|{}", task.id),
                name: resource_name(&full_name, &name, &task.name)
                    .or_else(|| {
                        package
                            .DisplayName()
                            .ok()
                            .map(|s| s.to_string())
                            .filter(|s| !s.is_empty())
                    })
                    .unwrap_or_else(|| name.clone()),
                entry: task.id,
                command,
                path: path.clone(),
                location: "package",
                machine: false,
                enabled: false,
                blocked: None,
                exists,
                publisher: package
                    .PublisherDisplayName()
                    .ok()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty()),
                description: None,
                icon: None,
            },
        });
    }
    if let Ok(modified) = std::fs::metadata(&manifest).and_then(|m| m.modified()) {
        CACHE.lock().unwrap().insert(
            full_name,
            CachedPackage {
                manifest,
                executable_root,
                modified,
                items: out.clone(),
            },
        );
    }
    Ok(out)
}

pub(super) fn list(meta: &MetaStore) -> Vec<StartupItem> {
    let _apartment = Apartment::new();
    let Ok(packages) = PackageManager::new()
        .and_then(|m| m.FindPackagesByUserSecurityId(&HSTRING::new()))
        .inspect_err(|error| {
            crate::diagnostics::windows_failure("startup package enumeration", error)
        })
    else {
        return Vec::new();
    };
    let mut present = HashSet::new();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for package in packages {
        if let Ok(name) = package.Id().and_then(|id| id.FullName()) {
            present.insert(name.to_string());
        }
        for PackagedItem { mut item, kind } in package_tasks(&package)
            .inspect_err(|error| {
                crate::diagnostics::windows_failure("startup package metadata", error)
            })
            .unwrap_or_default()
        {
            if !seen.insert(item.id.clone()) {
                continue;
            }
            let identity = item.id.strip_prefix("package|").unwrap();
            let (family, task) = identity.split_once('|').unwrap();
            (item.enabled, item.blocked) = state(family, task, kind);
            item.exists = item.path.as_deref().is_some_and(|p| Path::new(p).is_file());
            item.icon = item
                .path
                .as_deref()
                .filter(|_| item.exists)
                .and_then(|p| meta.get_or_request(p))
                .and_then(|m| m.icon);
            out.push(item);
        }
    }
    CACHE
        .lock()
        .unwrap()
        .retain(|name, _| present.contains(name));
    out
}

fn read_dword(key: &Key, name: &str) -> Result<Option<u32>, windows::core::Error> {
    let mut data = [0u8; 4];
    let mut size = 4;
    let mut kind = REG_VALUE_TYPE::default();
    let name = wide(name);
    let result = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    result.ok()?;
    if kind != REG_DWORD || size != 4 {
        return Err(ERROR_INVALID_DATA.into());
    }
    Ok(Some(u32::from_le_bytes(data)))
}

fn decode_state(
    value: Option<u32>,
    user_enabled: Option<u32>,
    activated: bool,
    kind: TaskKind,
) -> bool {
    // A manifest default starts taking effect after the package's first
    // activation. Explicitly enabling the task in Task Manager arms it earlier.
    // Stored registration values are not the StartupTaskState API enum.
    // Task Manager also treats stored 3 and 4 as disabled and permits enabling.
    value.unwrap_or(if kind.enabled_by_default { 2 } else { 0 }) == 2
        && (activated || kind.immediate_registration || user_enabled.is_some_and(|v| v != 0))
}

fn read_key(path: &str) -> windows::core::Result<Option<Key>> {
    let path = wide(path);
    let mut raw = Default::default();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            KEY_QUERY_VALUE,
            &mut raw,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    result.ok()?;
    Ok(Some(Key(raw)))
}

fn state(family: &str, task: &str, kind: TaskKind) -> (bool, Option<&'static str>) {
    let read = || -> windows::core::Result<_> {
        let parent = format!(r"{SYSTEM_APP_DATA}\{family}");
        let activated = read_key(&parent)?
            .map(|key| read_dword(&key, "WasEverActivated"))
            .transpose()?
            .flatten()
            .is_some_and(|value| value != 0);
        let Some(key) = read_key(&format!(r"{parent}\{task}"))? else {
            return Ok(decode_state(None, None, activated, kind));
        };
        Ok(decode_state(
            read_dword(&key, "State")?,
            read_dword(&key, "UserEnabledStartupOnce")?,
            activated,
            kind,
        ))
    };
    match read() {
        Ok(enabled) => (enabled, (!kind.user_configurable).then_some("managed")),
        Err(error) => {
            crate::diagnostics::windows_failure("startup package state read", &error);
            (false, Some("unavailable"))
        }
    }
}

pub(super) fn set_enabled(identity: &str, enabled: bool) -> Result<(), ActionError> {
    let (family, task) = identity.split_once('|').ok_or(ActionError::NotFound)?;
    if !valid_component(family) || !valid_component(task) {
        return Err(ActionError::NotFound);
    }
    // Revalidate against the current user's installed manifest before any write.
    let _apartment = Apartment::new();
    let manager = PackageManager::new()?;
    let packages = manager
        .FindPackagesByUserSecurityIdPackageFamilyName(&HSTRING::new(), &HSTRING::from(family))?;
    let registered = packages.into_iter().find_map(|p| {
        p.InstalledLocation()
            .and_then(|d| d.Path())
            .ok()
            .and_then(|d| {
                std::fs::read_to_string(Path::new(&d.to_string()).join("AppxManifest.xml")).ok()
            })
            .and_then(|xml| tasks(&xml).ok())
            .and_then(|items| items.into_iter().find(|t| t.id == task))
    });
    let Some(registered) = registered else {
        return Err(ActionError::NotFound);
    };
    let kind = registered.kind;
    let (_, blocked) = state(family, task, kind);
    if let Some(reason) = blocked {
        return Err(blocked_error(reason));
    }
    // Persist the choice even if it matches the effective state: a task can
    // look disabled only because its package has not been activated yet.
    let path = wide(&format!(r"{SYSTEM_APP_DATA}\{family}\{task}"));
    let mut raw = Default::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            None,
            &mut raw,
            None,
        )
    }
    .ok()?;
    let key = Key(raw);
    if let Some(reason) = state(family, task, kind).1 {
        return Err(blocked_error(reason));
    }
    if enabled {
        unsafe {
            RegSetValueExW(
                key.0,
                windows::core::w!("UserEnabledStartupOnce"),
                None,
                REG_DWORD,
                Some(&1u32.to_le_bytes()),
            )
        }
        .ok()?;
    } else {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        unsafe {
            RegSetValueExW(
                key.0,
                windows::core::w!("LastDisabledTime"),
                None,
                REG_DWORD,
                Some(&now.to_le_bytes()),
            )
        }
        .ok()?;
    }
    let value = if enabled { 2u32 } else { 1u32 };
    if read_dword(&key, "State")? != Some(value) {
        unsafe {
            RegSetValueExW(
                key.0,
                windows::core::w!("State"),
                None,
                REG_DWORD,
                Some(&value.to_le_bytes()),
            )
        }
        .ok()?;
    }
    if read_dword(&key, "State")? != Some(value) {
        crate::diagnostics::warn(
            "startup package state verification",
            "stored state differs after write",
        );
        return Err(ActionError::Os(crate::tr!("startup.changed")));
    }
    Ok(())
}

fn blocked_error(reason: &str) -> ActionError {
    ActionError::Os(match reason {
        "managed" => crate::tr!("startup.managed"),
        _ => crate::tr!("startup.unavailable"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_ENABLED: TaskKind = TaskKind {
        enabled_by_default: true,
        user_configurable: true,
        immediate_registration: false,
    };
    const DEFAULT_DISABLED: TaskKind = TaskKind {
        enabled_by_default: false,
        user_configurable: true,
        immediate_registration: false,
    };

    #[test]
    fn packaged_tasks_keep_distinct_ids_and_inherit_the_application_executable() {
        let _apartment = Apartment::new();
        let xml = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>
        <Package xmlns=\"http://schemas.microsoft.com/appx/manifest/foundation/windows10\"
          xmlns:desktop=\"http://schemas.microsoft.com/appx/manifest/desktop/windows10\"
          xmlns:uap5=\"http://schemas.microsoft.com/appx/manifest/uap/windows10/5\"
          xmlns:uap10=\"http://schemas.microsoft.com/appx/manifest/uap/windows10/10\">
          <Applications>
            <Application Id=\"Desktop\" Executable=\"app\\Desktop.exe\" EntryPoint=\"Windows.FullTrustApplication\">
              <Extensions>
                <desktop:Extension Category=\"windows.startupTask\" Executable=\"helper.exe\" uap10:Parameters=\"--hidden &amp; --tray\">
                  <desktop:StartupTask TaskId=\"Helper\" DisplayName=\"Helper &amp; tray\" Enabled=\"true\" />
                </desktop:Extension>
                <uap5:Extension Category=\"windows.startupTask\">
                  <uap5:StartupTask TaskId=\"Main\" DisplayName=\"ms-resource:AppName\" Enabled=\"false\" />
                </uap5:Extension>
              </Extensions>
            </Application>
            <Application Id=\"Uwp\" Executable=\"Uwp.exe\" EntryPoint=\"Example.App\">
              <Extensions><uap5:Extension Category=\"windows.startupTask\">
                <uap5:StartupTask TaskId=\"Uwp\" DisplayName=\"Example\" Enabled=\"true\" />
              </uap5:Extension></Extensions>
            </Application>
          </Applications>
        </Package>";
        let items = tasks(xml).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].id, "Helper");
        assert_eq!(items[0].executable, "helper.exe");
        assert_eq!(items[0].arguments, "--hidden & --tray");
        assert_eq!(items[0].name, "Helper & tray");
        assert!(items[0].kind.enabled_by_default);
        assert_eq!(items[1].id, "Main");
        assert_eq!(items[1].executable, r"app\Desktop.exe");
        assert!(!items[1].kind.enabled_by_default);
        assert!(!decode_state(Some(2), Some(0), false, items[2].kind));
    }

    #[test]
    fn enabled_tasks_wait_for_first_activation_or_explicit_user_enable() {
        assert!(!decode_state(Some(2), Some(0), false, DEFAULT_ENABLED));
        assert!(!decode_state(Some(2), None, false, DEFAULT_ENABLED));
        assert!(decode_state(Some(2), Some(1), false, DEFAULT_ENABLED));
        assert!(decode_state(Some(2), Some(0), true, DEFAULT_ENABLED));
        assert!(!decode_state(None, None, false, DEFAULT_ENABLED));
        assert!(decode_state(None, None, true, DEFAULT_ENABLED));
        assert!(!decode_state(None, None, true, DEFAULT_DISABLED));
        for kind in [DEFAULT_DISABLED, DEFAULT_ENABLED] {
            assert!(!decode_state(Some(1), Some(1), true, kind));
            assert!(!decode_state(Some(0), Some(1), true, kind));
            assert!(!decode_state(Some(3), Some(1), true, kind));
            assert!(!decode_state(Some(4), Some(0), true, kind));
            assert!(!decode_state(Some(5), Some(1), true, kind));
        }
    }

    #[test]
    fn package_identifiers_and_executables_cannot_escape_their_location() {
        for id in ["", ".", "..", "../Run", "task\\other", "a\0b"] {
            assert!(!valid_component(id), "{id:?}");
        }
        assert!(valid_component("YourPhone.Start"));
        assert!(valid_component("Startup|Secondary"));
        let root = Path::new(r"C:\Packages\Example");
        for executable in [
            "",
            r"..\other.exe",
            r"C:\other.exe",
            r"\other.exe",
            r"\\server\app.exe",
        ] {
            assert!(executable_path(root, executable).is_none());
        }
        assert_eq!(
            executable_path(root, r"app\Main.exe").as_deref(),
            Some(r"C:\Packages\Example\app\Main.exe")
        );
    }

    #[test]
    fn manifests_cannot_load_external_entities() {
        let _apartment = Apartment::new();
        assert!(tasks(r#"<!DOCTYPE Package [<!ENTITY xxe SYSTEM "file:///C:/Windows/win.ini">]><Package>&xxe;</Package>"#).is_err());
    }

    #[test]
    fn managed_task_attributes_require_the_system_capability() {
        let _apartment = Apartment::new();
        let xml = r#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
          xmlns:uap5="http://schemas.microsoft.com/appx/manifest/uap/windows10/5"
          xmlns:uap11="http://schemas.microsoft.com/appx/manifest/uap/windows10/11"
          xmlns:rescap4="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities/4"
          xmlns:rescap5="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities/5">
          <Applications><Application Id="App" Executable="app.exe"><Extensions>
            <uap5:Extension Category="windows.startupTask">
              <uap5:StartupTask TaskId="Managed" Enabled="true" uap11:UserConfigurable="false" rescap5:ImmediateRegistration="true"/>
            </uap5:Extension>
          </Extensions></Application></Applications><Capabilities/>
        </Package>"#;
        let ordinary = tasks(xml).unwrap();
        assert!(ordinary[0].kind.user_configurable);
        assert!(!ordinary[0].kind.immediate_registration);
        let xml = xml.replace("<Capabilities/>", r#"<Capabilities><rescap4:CustomCapability Name="Microsoft.nonUserConfigurableStartupTasks_8wekyb3d8bbwe"/></Capabilities>"#);
        let managed = tasks(&xml).unwrap();
        assert!(!managed[0].kind.user_configurable);
        assert!(managed[0].kind.immediate_registration);
        assert!(decode_state(Some(2), None, false, managed[0].kind));
        assert!(!decode_state(Some(1), None, false, managed[0].kind));
    }
}
