//! Packaged (MSIX) apps go by their app's display name, as Start and Task
//! Manager show them: Windows Terminal is "Terminal" (终端 in Chinese), not its
//! executable's file description "Windows Terminal Host". The name comes from
//! the shell in the Windows display language, like Task Manager's.

use std::path::Path;
use std::sync::Arc;

use windows::Data::Xml::Dom::{IXmlNode, XmlDocument, XmlLoadSettings};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use crate::util::{from_wide, wide};

// Declared here instead of enabling a `windows` feature, which would rebuild the
// `windows` crate and everything sharing it. kernel32 has them since Windows 8.
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetPackageFullName(process: HANDLE, length: *mut u32, name: PWSTR) -> u32;
    fn GetApplicationUserModelId(process: HANDLE, length: *mut u32, id: PWSTR) -> u32;
    fn GetPackagePathByFullName(name: PCWSTR, length: *mut u32, path: PWSTR) -> u32;
}

/// The package and app a process runs as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    /// `Family!AppId`, e.g. `Microsoft.WindowsTerminal_8wekyb3d8bbwe!App`.
    pub aumid: Box<str>,
    pub full_name: Box<str>,
}

/// A process's package identity, None for an unpackaged process (a few µs with a
/// `PROCESS_QUERY_LIMITED_INFORMATION` handle).
pub(crate) fn identity(process: HANDLE) -> Option<Identity> {
    // Room for PACKAGE_FULL_NAME_MAX_LENGTH and APPLICATION_USER_MODEL_ID_MAX_LENGTH.
    let mut full_name = [0u16; 256];
    let mut length = full_name.len() as u32;
    if unsafe { GetPackageFullName(process, &mut length, PWSTR(full_name.as_mut_ptr())) } != 0 {
        return None;
    }
    let mut aumid = [0u16; 256];
    let mut length = aumid.len() as u32;
    // Fails for a packaged process outside any app, e.g. a package's background task.
    if unsafe { GetApplicationUserModelId(process, &mut length, PWSTR(aumid.as_mut_ptr())) } != 0 {
        return None;
    }
    Some(Identity {
        aumid: from_wide(&aumid).into(),
        full_name: from_wide(&full_name).into(),
    })
}

/// What the shell calls a packaged app, and which of its processes go by it.
#[derive(Debug, Default)]
pub struct AppMeta {
    /// The display name, in the Windows display language.
    pub name: Option<Arc<str>>,
    /// The manifest's `Executable`, lowercase with backslashes, relative to the package.
    executable: Option<Box<str>>,
}

impl AppMeta {
    /// The name for this app's process running `path`. Only the app's own
    /// executable goes by it; helpers that inherit the identity (a terminal's
    /// console host, a background task host) keep their descriptions.
    pub fn name_for(&self, path: Option<&str>) -> Option<Arc<str>> {
        let (name, executable) = (self.name.as_ref()?, self.executable.as_deref()?);
        let path = path?.replace('/', "\\").to_lowercase();
        let own = path == executable
            || path
                .strip_suffix(executable)
                .is_some_and(|rest| rest.ends_with('\\'));
        own.then(|| name.clone())
    }
}

/// Resolves an app's name and executable; needs COM on the calling thread.
pub(crate) fn resolve(identity: &Identity) -> AppMeta {
    let Some(name) = display_name(&identity.aumid) else {
        // Not an app the shell knows: e.g. the per-package IDs of Runtime Broker.
        return AppMeta::default();
    };
    let app = identity.aumid.split_once('!').map_or("", |(_, app)| app);
    AppMeta {
        executable: executable(&identity.full_name, app),
        name: Some(name.into()),
    }
}

/// The app's name in the shell's Applications folder, where Start lists apps.
/// It also knows apps hidden from Start (Search, the lock screen).
fn display_name(aumid: &str) -> Option<String> {
    let path = wide(&format!(r"shell:AppsFolder\{aumid}"));
    let item: IShellItem =
        unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }.ok()?;
    let raw = unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) }.ok()?;
    let name = unsafe { raw.to_string() };
    unsafe { CoTaskMemFree(Some(raw.0.cast_const().cast())) };
    let name = name.ok()?.trim().to_owned();
    (!name.is_empty() && name != aumid).then_some(name)
}

fn attribute(node: &IXmlNode, name: &str) -> String {
    node.Attributes()
        .and_then(|attributes| attributes.GetNamedItem(&HSTRING::from(name)))
        .and_then(|value| value.InnerText())
        .map(|value| value.to_string())
        .unwrap_or_default()
}

/// `Executable` of the app `app` in the package's manifest.
fn executable(full_name: &str, app: &str) -> Option<Box<str>> {
    let name = wide(full_name);
    let mut path = vec![0u16; 512];
    let mut length = path.len() as u32;
    let mut status = unsafe {
        GetPackagePathByFullName(PCWSTR(name.as_ptr()), &mut length, PWSTR(path.as_mut_ptr()))
    };
    if status == 122 && (length as usize) > path.len() && length < 32_768 {
        // ERROR_INSUFFICIENT_BUFFER: `length` is the size needed.
        path.resize(length as usize, 0);
        status = unsafe {
            GetPackagePathByFullName(PCWSTR(name.as_ptr()), &mut length, PWSTR(path.as_mut_ptr()))
        };
    }
    if status != 0 {
        return None;
    }
    let xml =
        std::fs::read_to_string(Path::new(&from_wide(&path)).join("AppxManifest.xml")).ok()?;
    let document = XmlDocument::new().ok()?;
    let settings = XmlLoadSettings::new().ok()?;
    settings.SetProhibitDtd(true).ok()?;
    settings.SetResolveExternals(false).ok()?;
    document
        .LoadXmlWithSettings(
            &HSTRING::from(xml.trim_start_matches('\u{feff}')),
            &settings,
        )
        .ok()?;
    let apps = document
        .SelectNodes(&HSTRING::from(
            "/*[local-name()='Package']/*[local-name()='Applications']/*[local-name()='Application']",
        ))
        .ok()?;
    let executable = apps
        .into_iter()
        .find(|node| attribute(node, "Id") == app)
        .map(|node| attribute(&node, "Executable"))?;
    let executable = executable.trim().replace('/', "\\").to_lowercase();
    (!executable.is_empty()).then(|| executable.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_apps_own_executable_goes_by_its_name() {
        let app = AppMeta {
            name: Some("App".into()),
            executable: Some(r"app\main.exe".into()),
        };
        let root = r"C:\Program Files\WindowsApps\Example_1.0.0.0_x64__abc";
        let named = |path: &str| app.name_for(Some(path)).is_some();
        assert!(named(&format!(r"{root}\app\main.exe")));
        assert!(named(&format!(r"{root}/APP/Main.exe")));
        assert!(!named(&format!(r"{root}\app\helper.exe")));
        assert!(!named(&format!(r"{root}\app\notmain.exe")));
        assert!(!named(&format!(r"{root}\main.exe")));
        assert!(app.name_for(None).is_none());
        let unnamed = AppMeta {
            name: None,
            executable: Some("main.exe".into()),
        };
        assert!(unnamed.name_for(Some(r"C:\main.exe")).is_none());
    }
}
