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
/// What `rewrite` points at `launcher`.
struct Targets<'a> {
    id: &'a str,
    /// The installed argus.exe, which plain shortcuts start.
    source: Option<&'a Path>,
    launcher: &'a Path,
    /// The launcher where Argus 0.5 and earlier kept the account's folder.
    moved: Option<&'a Path>,
}

/// Points the account's shortcuts at `launcher`: those that start `source`
/// or an Argus copy, and those that start `moved` (the launcher of Argus 0.5
/// or earlier), as does the sign-in entry then. False when anything could
/// not be changed: `moved` must then stay to forward.
pub(super) fn retarget(id: &str, source: &Path, launcher: &Path, moved: Option<&Path>) -> bool {
    if !launcher.is_file() {
        return false;
    }
    let cleared = util::stage(
        "administrator compatibility flag removal",
        clear_run_as_admin(source),
    )
    .is_ok();
    let Ok(_com) = task::Scheduler::new() else {
        return false;
    };
    let moved_all = retarget_with(&Targets {
        id,
        source: Some(source),
        launcher,
        moved,
    });
    cleared && moved_all
}

/// `retarget` of only what starts the launcher of Argus 0.5 or earlier, for
/// an account whose folder an installer running as another account moved.
/// Needs no elevation, and leaves the process's COM security alone. False
/// when anything could not be changed.
pub(super) fn follow(id: &str, moved: &Path, launcher: &Path) -> bool {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    if !launcher.is_file() {
        return false;
    }
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized.is_err() && initialized.0 as u32 != 0x80010106 {
        argus_core::diagnostics::warn(
            "administrator shortcut move COM initialization",
            argus_core::diagnostics::code(initialized.0 as u32),
        );
        return false;
    }
    let moved_all = retarget_with(&Targets {
        id,
        source: None,
        launcher,
        moved: Some(moved),
    });
    if initialized.is_ok() {
        unsafe { CoUninitialize() };
    }
    moved_all
}

/// False when a shortcut or sign-in entry to change could not be changed,
/// or a folder that may hold them could not be looked through.
fn retarget_with(targets: &Targets) -> bool {
    let mut all = true;
    if let Some(moved) = targets.moved
        && let Err(code) = follow_run(moved, targets.launcher)
    {
        argus_core::diagnostics::warn(
            "administrator sign-in entry move",
            argus_core::diagnostics::code(code),
        );
        all = false;
    }
    let mut roots = Vec::new();
    for (folder, stage) in [
        (&FOLDERID_Desktop, "administrator desktop folder lookup"),
        (&FOLDERID_Programs, "administrator programs folder lookup"),
    ] {
        match util::stage(stage, known(folder)) {
            Ok(path) => roots.push(path),
            Err(_) => all = false,
        }
    }
    match known(&FOLDERID_RoamingAppData) {
        Ok(roaming) => roots
            .push(roaming.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar")),
        Err(_) => all = false,
    }
    for root in roots {
        walk(&root, 0, targets, &mut all);
    }
    all
}
fn walk(root: &Path, depth: usize, targets: &Targets, all: &mut bool) {
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
            walk(&entry.path(), depth + 1, targets, all);
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        {
            let result = rewrite(&entry.path(), targets);
            if util::file_stage("administrator shortcut migration", &entry.path(), result).is_err()
            {
                *all = false;
            }
        }
    }
}
fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}
/// Whether a shortcut that starts `target` with `args` is one `rewrite`
/// points at the launcher.
fn wanted(targets: &Targets, target: &Path, args: &str) -> bool {
    if targets.moved.is_some_and(|moved| same_path(target, moved)) {
        // Only what Argus wrote: the launcher with its own arguments.
        return args == format!("--argus-launch {}", targets.id);
    }
    // Only plain application shortcuts are migrated. Preserve custom
    // commands and their arguments rather than silently changing intent.
    let Some(source) = targets.source else {
        return false;
    };
    args.is_empty()
        && (same_path(target, source)
            || is_copy(target, targets.id)
            || (targets.id == "dev.argus.taskmanager" && crate::taskmgr::is_interface_copy(target)))
}
/// The path and arguments of a loaded shortcut.
unsafe fn read_link(link: &IShellLinkW) -> Result<(PathBuf, String)> {
    let mut target = [0u16; 32768];
    unsafe { link.GetPath(&mut target, std::ptr::null_mut(), 0) }.map_err(win)?;
    let end = target.iter().position(|c| *c == 0).unwrap_or(target.len());
    let mut args = [0u16; 2048];
    unsafe { link.GetArguments(&mut args) }.map_err(win)?;
    let args_end = args.iter().position(|c| *c == 0).unwrap_or(args.len());
    Ok((
        PathBuf::from(String::from_utf16_lossy(&target[..end])),
        String::from_utf16_lossy(&args[..args_end]),
    ))
}
fn rewrite(path: &Path, targets: &Targets) -> Result<()> {
    let id = targets.id;
    let launcher = targets.launcher;
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
        let Ok(seen) = read_link(&link) else {
            return Ok(());
        };
        if !wanted(targets, &seen.0, &seen.1) {
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
        let current = read_link(&link)?;
        if !same_path(&current.0, &seen.0) || current.1 != seen.1 {
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

/// The sign-in command with `launcher` in place of `moved`, the quoted
/// program administrator startup wrote; None for any other command.
fn moved_command(command: &str, moved: &Path, launcher: &Path) -> Option<String> {
    let rest = command.strip_prefix('"')?;
    let end = rest.find('"')?;
    if !same_path(Path::new(&rest[..end]), moved) {
        return None;
    }
    Some(format!("\"{}\"{}", launcher.display(), &rest[end + 1..]))
}

/// Points the account's sign-in entries that start `moved` at `launcher`,
/// keeping their names, arguments and Startup apps state.
fn follow_run(moved: &Path, launcher: &Path) -> Result<()> {
    use windows::Win32::System::Registry::*;
    let mut key = HKEY::default();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
            None,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut key,
        )
    };
    match opened.0 {
        0 => {}
        2 => return Ok(()),
        code => return Err(code),
    }
    let mut moved_entries = Vec::new();
    for index in 0.. {
        let mut name = [0u16; 16384];
        let mut name_len = name.len() as u32;
        let mut kind = 0u32;
        let mut data = vec![0u16; 16384];
        let mut data_len = (data.len() * 2) as u32;
        let code = unsafe {
            RegEnumValueW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&raw mut kind),
                Some(data.as_mut_ptr().cast()),
                Some(&raw mut data_len),
            )
        };
        match code.0 {
            0 => {}
            // ERROR_NO_MORE_ITEMS
            259 => break,
            // ERROR_MORE_DATA: no command Argus writes is that long.
            234 => continue,
            other => {
                unsafe {
                    let _ = RegCloseKey(key);
                }
                return Err(other);
            }
        }
        if kind != REG_SZ.0 {
            continue;
        }
        let units = (data_len as usize / 2).min(data.len());
        let text = String::from_utf16_lossy(&data[..units]);
        if let Some(command) = moved_command(text.trim_end_matches('\0'), moved, launcher) {
            moved_entries.push((name[..name_len as usize].to_vec(), command));
        }
    }
    let mut result = Ok(());
    for (mut name, command) in moved_entries {
        name.push(0);
        let value = wide(&command);
        let code = unsafe {
            RegSetValueExW(
                key,
                PCWSTR(name.as_ptr()),
                None,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    value.as_ptr().cast(),
                    value.len() * 2,
                )),
            )
        };
        if code.0 != 0 {
            result = Err(code.0);
        }
    }
    unsafe {
        let _ = RegCloseKey(key);
    }
    result
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

    #[test]
    fn only_what_starts_the_moved_launcher_follows_it() {
        let id = "dev.argus.taskmanager";
        let account = r"dev.argus.taskmanager\S-1-5-21-1-2-3-1001\argus-launcher.exe";
        let moved = PathBuf::from(r"C:\Program Files\Argus Runtime").join(account);
        let launcher = PathBuf::from(r"C:\Program Files\Argus Privileged\runtime").join(account);
        let entry =
            |program: &Path| format!("\"{}\" --argus-launch {id} --hidden", program.display());
        assert_eq!(
            moved_command(&entry(&moved), &moved, &launcher),
            Some(entry(&launcher))
        );
        let upper = PathBuf::from(moved.to_string_lossy().to_uppercase());
        assert_eq!(
            moved_command(&entry(&upper), &moved, &launcher),
            Some(entry(&launcher))
        );
        for other in [
            entry(&launcher),
            entry(Path::new(r"C:\Program Files\Argus\argus.exe")),
            format!("{} --argus-launch {id}", moved.display()),
            String::new(),
        ] {
            assert_eq!(moved_command(&other, &moved, &launcher), None, "{other}");
        }

        let installed = Path::new(r"C:\Program Files\Argus\argus.exe");
        let follow = Targets {
            id,
            source: None,
            launcher: &launcher,
            moved: Some(&moved),
        };
        assert!(wanted(&follow, &moved, &format!("--argus-launch {id}")));
        assert!(wanted(&follow, &upper, &format!("--argus-launch {id}")));
        for args in [
            "",
            "--hidden",
            "--argus-launch other",
            "--argus-launch dev.argus.taskmanager --x",
        ] {
            assert!(!wanted(&follow, &moved, args), "{args}");
        }
        // Following alone leaves the plain shortcuts to the next grant.
        assert!(!wanted(&follow, installed, ""));
        let grant = Targets {
            source: Some(installed),
            ..follow
        };
        assert!(wanted(&grant, installed, ""));
        assert!(!wanted(&grant, installed, "--hidden"));
        assert!(wanted(&grant, &moved, &format!("--argus-launch {id}")));
    }
}
