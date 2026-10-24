use super::*;
use windows::Win32::{
    Foundation::{HWND, PROPERTYKEY},
    System::Com::{
        CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile, STGM_READ,
        STGM_READWRITE, StructuredStorage::PROPVARIANT,
    },
    UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow},
};
use windows::core::{GUID, Interface};

const APP_ID: PROPERTYKEY = property(5);
const RELAUNCH: PROPERTYKEY = property(2);
const RELAUNCH_ICON: PROPERTYKEY = property(3);
const RELAUNCH_NAME: PROPERTYKEY = property(4);
const fn property(pid: u32) -> PROPERTYKEY {
    PROPERTYKEY {
        fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid,
    }
}

fn registry_string(
    root: windows::Win32::System::Registry::HKEY,
    key: &str,
    name: &str,
) -> Option<String> {
    use windows::Win32::System::Registry::*;
    let (key, name) = (wide(key), wide(name));
    let mut data = [0u16; 32768];
    let mut bytes = size_of_val(&data) as u32;
    unsafe {
        RegGetValueW(
            root,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    }
    .ok()
    .ok()?;
    Some(String::from_utf16_lossy(
        &data[..(bytes as usize / 2).saturating_sub(1)],
    ))
}
pub(super) fn original(id: &str, current: &Path) -> PathBuf {
    use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    if id != "dev.argus.taskmanager" || !crate::taskmgr::is_interface_copy(current) {
        return current.into();
    }
    let key = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Argus";
    let found = (|| {
        let folder = registry_string(HKEY_LOCAL_MACHINE, key, "InstallLocation")?;
        let binary = registry_string(HKEY_LOCAL_MACHINE, key, "MainBinaryName")?;
        if binary.contains(['\\', '/']) || binary.is_empty() {
            return None;
        }
        let path = Path::new(folder.trim_matches('"')).join(binary);
        path.is_file().then_some(path)
    })();
    found.unwrap_or_else(|| current.into())
}
fn without_run_as_admin(value: &str) -> Option<String> {
    let tokens: Vec<_> = value.split_whitespace().collect();
    tokens
        .iter()
        .any(|t| t.eq_ignore_ascii_case("RUNASADMIN"))
        .then(|| {
            tokens
                .into_iter()
                .filter(|t| !t.eq_ignore_ascii_case("RUNASADMIN"))
                .collect::<Vec<_>>()
                .join(" ")
        })
}
fn clear_run_as_admin(source: &Path) -> Result<()> {
    use windows::Win32::System::Registry::*;
    let key = r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers";
    let name = source.to_string_lossy();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let Some(value) = registry_string(root, key, &name).and_then(|s| without_run_as_admin(&s))
        else {
            continue;
        };
        let (key, name, value) = (wide(key), wide(&name), wide(&value));
        unsafe {
            RegSetKeyValueW(
                root,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                REG_SZ.0,
                Some(value.as_ptr().cast()),
                (value.len() * 2) as u32,
            )
        }
        .ok()
        .map_err(win)?;
    }
    Ok(())
}

fn command(id: &str, launcher: &Path) -> String {
    format!("\"{}\" --argus-launch {id}", launcher.display())
}
fn properties(store: &IPropertyStore, id: &str, launcher: &Path, icon: &Path) -> Result<()> {
    for (key, value) in [
        (&APP_ID, id.to_owned()),
        (&RELAUNCH, command(id, launcher)),
        (&RELAUNCH_ICON, format!("{},0", icon.display())),
        (&RELAUNCH_NAME, "Argus".into()),
    ] {
        unsafe { store.SetValue(key, &PROPVARIANT::from(value.as_str())) }.map_err(win)?;
    }
    unsafe { store.Commit() }.map_err(win)
}
pub fn window_relaunch(id: &str, hwnd: HWND) {
    let Some(path) =
        launcher(id).filter(|p| p.file_name().is_some_and(|f| f == "argus-launcher.exe"))
    else {
        return;
    };
    let result = (|| {
        let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(hwnd) }.map_err(win)?;
        properties(&store, id, &path, &path.with_file_name("argus.ico"))
    })();
    let _ = util::stage("administrator window relaunch properties", result);
}
fn known(id: &GUID) -> Result<PathBuf> {
    let raw = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.map_err(win)?;
    let text = unsafe { raw.to_string() }.map_err(|_| 13u32);
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    Ok(PathBuf::from(text?))
}
pub(super) fn retarget(id: &str, source: &Path, launcher: &Path) -> Result<()> {
    if !launcher.is_file() {
        return Ok(());
    }
    clear_run_as_admin(source)?;
    let _com = task::Scheduler::new()?;
    let mut roots = vec![known(&FOLDERID_Desktop)?, known(&FOLDERID_Programs)?];
    if let Ok(roaming) = known(&FOLDERID_RoamingAppData) {
        roots.push(roaming.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"));
    }
    for root in roots {
        walk(&root, 0, id, source, launcher);
    }
    Ok(())
}
fn walk(root: &Path, depth: usize, id: &str, source: &Path, launcher: &Path) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // Do not follow junctions/symlinks out of the shell's shortcut folders.
        use std::os::windows::fs::MetadataExt;
        if entry
            .metadata()
            .is_ok_and(|m| m.file_attributes() & 0x400 != 0)
        {
            continue;
        }
        if kind.is_dir() {
            walk(&entry.path(), depth + 1, id, source, launcher);
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        {
            let result = rewrite(&entry.path(), id, source, launcher);
            let _ = util::file_stage("administrator shortcut migration", &entry.path(), result);
        }
    }
}
fn rewrite(path: &Path, id: &str, source: &Path, launcher: &Path) -> Result<()> {
    unsafe {
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(win)?;
        let file: IPersistFile = link.cast().map_err(win)?;
        if file
            .Load(PCWSTR(wide(&path.to_string_lossy()).as_ptr()), STGM_READ)
            .is_err()
        {
            return Ok(());
        }
        let mut target = [0u16; 32768];
        if link.GetPath(&mut target, std::ptr::null_mut(), 0).is_err() {
            return Ok(());
        }
        let end = target.iter().position(|c| *c == 0).unwrap_or(target.len());
        let target = PathBuf::from(String::from_utf16_lossy(&target[..end]));
        if !target
            .to_string_lossy()
            .eq_ignore_ascii_case(&source.to_string_lossy())
            && !is_copy(&target, id)
            && !(id == "dev.argus.taskmanager" && crate::taskmgr::is_interface_copy(&target))
        {
            return Ok(());
        }
        // Only plain application shortcuts are migrated. Preserve custom
        // commands and their arguments rather than silently changing intent.
        let mut args = [0u16; 2048];
        link.GetArguments(&mut args).map_err(win)?;
        if args[0] != 0 {
            return Ok(());
        }
        // Only recognized Argus links are opened for writing. Check again
        // after reopening so a changed link is not overwritten from stale data.
        drop(file);
        drop(link);
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(win)?;
        let file: IPersistFile = link.cast().map_err(win)?;
        file.Load(
            PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
            STGM_READWRITE,
        )
        .map_err(win)?;
        let mut current = [0u16; 32768];
        link.GetPath(&mut current, std::ptr::null_mut(), 0)
            .map_err(win)?;
        let end = current
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(current.len());
        link.GetArguments(&mut args).map_err(win)?;
        if !String::from_utf16_lossy(&current[..end])
            .eq_ignore_ascii_case(&target.to_string_lossy())
            || args[0] != 0
        {
            return Ok(());
        }
        link.SetPath(PCWSTR(wide(&launcher.to_string_lossy()).as_ptr()))
            .map_err(win)?;
        link.SetArguments(PCWSTR(wide(&format!("--argus-launch {id}")).as_ptr()))
            .map_err(win)?;
        link.SetIconLocation(
            PCWSTR(wide(&launcher.with_file_name("argus.ico").to_string_lossy()).as_ptr()),
            0,
        )
        .map_err(win)?;
        let flags: IShellLinkDataList = link.cast().map_err(win)?;
        flags
            .SetFlags(flags.GetFlags().map_err(win)? & !SLDF_RUNAS_USER.0 as u32)
            .map_err(win)?;
        let store: IPropertyStore = link.cast().map_err(win)?;
        properties(&store, id, launcher, &launcher.with_file_name("argus.ico"))?;
        file.Save(PCWSTR(wide(&path.to_string_lossy()).as_ptr()), true)
            .map_err(win)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatibility_migration_keeps_other_user_choices() {
        assert_eq!(
            without_run_as_admin("~ RUNASADMIN HIGHDPIAWARE WIN8RTM"),
            Some("~ HIGHDPIAWARE WIN8RTM".into())
        );
        assert_eq!(without_run_as_admin("~ HIGHDPIAWARE"), None);
        assert_eq!(without_run_as_admin("RUNASADMINISTRATOR"), None);
    }
}
