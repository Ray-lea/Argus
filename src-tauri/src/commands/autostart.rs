//! The sign-in entry (`Run` value named after the product). The autostart
//! plugin writes the running executable. Run by the task manager replacement,
//! that is an interface copy in the privileged service's folder: it never gets
//! updates, and the next enable or restore of the replacement deletes it. So a
//! copy names the installed argus.exe instead, and the installed Argus points
//! an entry that names a copy back at itself.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use argus_core::tr;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_BINARY, REG_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW, RegSetValueExW,
};
use windows::core::{PCWSTR, w};

const RUN: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const APPROVED: PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");
/// As `lib.rs` configures the autostart plugin.
const ARGS: &str = "--hidden";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn read(root: HKEY, key: PCWSTR, name: &str) -> Option<String> {
    let name = wide(name);
    let mut size = 0u32;
    let query = |data: Option<*mut core::ffi::c_void>, size: &mut u32| unsafe {
        RegGetValueW(
            root,
            key,
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            data,
            Some(size),
        )
    };
    if query(None, &mut size).is_err() || size > 64 * 1024 {
        return None;
    }
    let mut buf = vec![0u16; (size as usize).div_ceil(2)];
    query(Some(buf.as_mut_ptr().cast()), &mut size).ok().ok()?;
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn write(root: HKEY, key: PCWSTR, name: &str, value: &str) -> Result<(), WIN32_ERROR> {
    let name = wide(name);
    let data = wide(value);
    let result = unsafe {
        RegSetKeyValueW(
            root,
            key,
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        )
    };
    if result.is_ok() { Ok(()) } else { Err(result) }
}

/// Marks the entry enabled in the Startup apps list, as the plugin does.
fn approve(name: &str) {
    let mut key = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, APPROVED, None, KEY_SET_VALUE, &mut key) }.is_ok()
    {
        let name = wide(name);
        let mut enabled = [0u8; 12];
        enabled[0] = 2;
        unsafe {
            let _ = RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_BINARY, Some(&enabled));
            let _ = RegCloseKey(key);
        }
    }
}

/// `argus-taskmgr-ui-<GUID>.exe`, as the replacement names its copies.
pub fn is_interface_copy(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        n.to_ascii_lowercase()
            .strip_prefix("argus-taskmgr-ui-")
            .and_then(|s| s.strip_suffix(".exe"))
            .is_some_and(|id| {
                id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
            })
    })
}

/// The program and the arguments of a `Run` command: quoted, or unquoted up
/// to `.exe` as the plugin writes it.
fn split(command: &str) -> Option<(&str, &str)> {
    let command = command.trim_start();
    if let Some(rest) = command.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((&rest[..end], rest[end + 1..].trim_start()));
    }
    let end = command.to_ascii_lowercase().find(".exe")? + 4;
    Some((&command[..end], command[end..].trim_start()))
}

/// The command for `target` in place of an interface copy, with the same
/// arguments; None when `old` does not start a copy.
fn retarget(old: &str, target: &Path) -> Option<String> {
    let (program, args) = split(old)?;
    is_interface_copy(Path::new(program)).then(|| command(target, args))
}

fn command(program: &Path, args: &str) -> String {
    let program = format!("\"{}\"", program.display());
    if args.is_empty() {
        program
    } else {
        format!("{program} {args}")
    }
}

/// The argus.exe the installer put in place: per machine, so recorded under
/// HKLM, which only administrators can change.
fn installed(product: &str) -> Option<PathBuf> {
    let key = wide(&format!(
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{product}"
    ));
    let key = PCWSTR(key.as_ptr());
    let path = install_path(
        &read(HKEY_LOCAL_MACHINE, key, "InstallLocation")?,
        &read(HKEY_LOCAL_MACHINE, key, "MainBinaryName")?,
    )?;
    path.is_file().then_some(path)
}

/// The executable an uninstall entry's InstallLocation (the installer writes
/// it quoted) and MainBinaryName name.
fn install_path(location: &str, binary: &str) -> Option<PathBuf> {
    let dir = location.trim().trim_matches('"');
    if dir.is_empty() || binary.is_empty() || binary.contains(['\\', '/']) {
        return None;
    }
    let path = Path::new(dir).join(binary);
    (!is_interface_copy(&path)).then_some(path)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Whether `exe` may change the sign-in entry: an interface copy, or the
/// installed argus.exe itself. A build run from anywhere else (a test or
/// development build, an unpacked portable copy) leaves the entry alone.
fn may_repair(exe: &Path, installed: &Path) -> bool {
    is_interface_copy(exe) || same_file(exe, installed)
}

pub fn running_as_copy() -> bool {
    std::env::current_exe().is_ok_and(|exe| is_interface_copy(&exe))
}

/// Turns the entry on from an interface copy: for the installed argus.exe,
/// or not at all.
pub fn enable_installed(product: &str) -> Result<(), String> {
    let exe = installed(product).ok_or_else(|| tr!("host.autostartNotInstalled"))?;
    write(HKEY_CURRENT_USER, RUN, product, &command(&exe, ARGS))
        .map_err(|e| windows::core::Error::from(e.to_hresult()).message())?;
    approve(product);
    Ok(())
}

static CHECKED: AtomicBool = AtomicBool::new(false);

/// Points an entry that starts an interface copy (one a copy of an earlier
/// version wrote) at the installed argus.exe, keeping its arguments and its
/// state in the Startup apps list. Only the installed Argus or a copy does
/// this, never a build run from elsewhere. Once per process: when the
/// settings are first read, or before a start that handed over to a running
/// instance exits (that instance may be the copy the entry started).
pub fn repair_once(product: &str) {
    if CHECKED.swap(true, Ordering::Relaxed) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(target) = installed(product) else {
        return;
    };
    if !may_repair(&exe, &target) {
        return;
    }
    for (root, label) in [
        (HKEY_CURRENT_USER, "current user"),
        (HKEY_LOCAL_MACHINE, "machine"),
    ] {
        let Some(new) = read(root, RUN, product).and_then(|old| retarget(&old, &target)) else {
            continue;
        };
        match write(root, RUN, product, &new) {
            Ok(()) => log::info!(
                "autostart: the {label} entry started a task manager interface copy; it now starts the installed Argus"
            ),
            Err(e) => log::warn!(
                "autostart: the {label} entry starts a task manager interface copy and could not be changed: {}",
                argus_core::diagnostics::failure(&format!("(os error {})", e.0))
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::RegDeleteTreeW;

    #[test]
    fn interface_copies_are_told_by_name() {
        let copy = r"C:\Program Files\Argus Privileged\argus-taskmgr-ui-d0a67540-30bd-4012-a790-1e6626c98fd5.exe";
        assert!(is_interface_copy(Path::new(copy)));
        assert!(is_interface_copy(Path::new(&copy.to_ascii_uppercase())));
        assert!(!is_interface_copy(Path::new(
            r"C:\Program Files\Argus\argus.exe"
        )));
        assert!(!is_interface_copy(Path::new(
            r"C:\Program Files\Argus Privileged\argus-taskmgr-ui-x.exe"
        )));
        assert!(!is_interface_copy(Path::new(
            r"C:\Program Files\Argus Privileged\argus-taskmgr-launcher.exe"
        )));
    }

    #[test]
    fn only_entries_that_start_a_copy_are_retargeted() {
        let target = Path::new(r"C:\Program Files\Argus\argus.exe");
        assert_eq!(
            retarget(
                r"C:\Program Files\Argus Privileged\argus-taskmgr-ui-d0a67540-30bd-4012-a790-1e6626c98fd5.exe --hidden",
                target
            )
            .as_deref(),
            Some(r#""C:\Program Files\Argus\argus.exe" --hidden"#)
        );
        assert_eq!(
            retarget(r"C:\Program Files\Argus\argus.exe --hidden", target),
            None
        );
        assert_eq!(retarget(r"D:\Argus\argus.exe", target), None);
    }

    #[test]
    fn the_installed_path_comes_from_the_uninstall_entry() {
        let installed = Some(PathBuf::from(r"C:\Program Files\Argus\argus.exe"));
        // The installer writes the folder quoted.
        assert_eq!(
            install_path(r#""C:\Program Files\Argus""#, "argus.exe"),
            installed
        );
        assert_eq!(
            install_path(r#" "C:\Program Files\Argus\" "#, "argus.exe"),
            installed
        );
        assert_eq!(
            install_path(r"C:\Program Files\Argus", "argus.exe"),
            installed
        );
        for (location, binary) in [
            ("", "argus.exe"),
            (r#""""#, "argus.exe"),
            (r"C:\Program Files\Argus", ""),
            (r"C:\Program Files\Argus", r"..\other.exe"),
            (r"C:\Program Files\Argus", "bin/argus.exe"),
            (
                r"C:\Program Files\Argus Privileged",
                "argus-taskmgr-ui-d0a67540-30bd-4012-a790-1e6626c98fd5.exe",
            ),
        ] {
            assert_eq!(install_path(location, binary), None, "{location} {binary}");
        }
    }

    #[test]
    fn only_the_installed_argus_or_a_copy_may_repair_the_entry() {
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = Scratch(std::env::temp_dir().join(format!(
            "argus-autostart-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        let installed = root.0.join(r"Argus\argus.exe");
        let release = root.0.join(r"target\release\argus.exe");
        let debug = root.0.join(r"target\debug\argus.exe");
        for path in [&installed, &release, &debug] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"").unwrap();
        }
        assert!(may_repair(&installed, &installed));
        // The same file however it is spelled.
        assert!(may_repair(&root.0.join(r"ARGUS\ARGUS.EXE"), &installed));
        assert!(may_repair(
            &root.0.join(r"target\..\Argus\argus.exe"),
            &installed
        ));
        // Test and development builds never touch the real entry.
        assert!(!may_repair(&release, &installed));
        assert!(!may_repair(&debug, &installed));
        assert!(!may_repair(&release, &root.0.join(r"Missing\argus.exe")));
        // A copy of the task manager replacement, wherever it runs from.
        assert!(may_repair(
            Path::new(
                r"C:\Program Files\Argus Privileged\argus-taskmgr-ui-d0a67540-30bd-4012-a790-1e6626c98fd5.exe"
            ),
            &installed
        ));
    }

    #[test]
    fn strings_round_trip_through_the_registry() {
        // A scratch key of this test, never the Run key.
        struct Scratch(Vec<u16>);
        impl Drop for Scratch {
            fn drop(&mut self) {
                unsafe {
                    let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(self.0.as_ptr()));
                }
            }
        }
        let scratch = Scratch(wide(&format!(r"Software\ArgusTest-{}", std::process::id())));
        let key = PCWSTR(scratch.0.as_ptr());
        let long = "x".repeat(300);
        for value in [
            r#""C:\Program Files\Argus\argus.exe" --hidden"#,
            "",
            long.as_str(),
        ] {
            write(HKEY_CURRENT_USER, key, "Argus", value).unwrap();
            assert_eq!(
                read(HKEY_CURRENT_USER, key, "Argus").as_deref(),
                Some(value)
            );
        }
        assert_eq!(read(HKEY_CURRENT_USER, key, "Missing"), None);
        drop(scratch);
        let key = wide(&format!(r"Software\ArgusTest-{}", std::process::id()));
        assert_eq!(read(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), "Argus"), None);
    }

    #[test]
    fn run_commands_split_into_program_and_arguments() {
        // As the plugin writes it: unquoted, with spaces in the path.
        assert_eq!(
            split(r"C:\Program Files\Argus Privileged\argus-taskmgr-ui-1.exe --hidden"),
            Some((
                r"C:\Program Files\Argus Privileged\argus-taskmgr-ui-1.exe",
                "--hidden"
            ))
        );
        assert_eq!(
            split(r#""C:\Program Files\Argus\argus.exe" --hidden"#),
            Some((r"C:\Program Files\Argus\argus.exe", "--hidden"))
        );
        assert_eq!(
            split(r"C:\Argus\ARGUS.EXE"),
            Some((r"C:\Argus\ARGUS.EXE", ""))
        );
        assert_eq!(split(r#""C:\unterminated --hidden"#), None);
        assert_eq!(split("rundll32"), None);
        assert_eq!(
            command(Path::new(r"C:\Program Files\Argus\argus.exe"), "--hidden"),
            r#""C:\Program Files\Argus\argus.exe" --hidden"#
        );
        assert_eq!(
            command(Path::new(r"C:\Argus\argus.exe"), ""),
            r#""C:\Argus\argus.exe""#
        );
    }
}
