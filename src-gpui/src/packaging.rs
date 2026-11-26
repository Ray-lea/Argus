//! Installer-only commands, handled before settings, collection or window creation.
pub fn run_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--argus-build-info")
            if args.len() == 1
                || matches!(args.as_slice(), [_, hidden, test, flag, id]
            if hidden == "--hidden" && test == "--test" && flag == "--identifier"
                && id.starts_with("dev.argus.taskmanager.test.") && argus_app::config::valid_identifier(id)) =>
        {
            println!(
                "{}",
                serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "build": env!("ARGUS_VERSION"),
                    "buildKind": argus_app::config::build_kind(env!("ARGUS_VERSION"), cfg!(debug_assertions)),
                    "fileVersion": format!("{}.{}", env!("CARGO_PKG_VERSION").split(['-', '+']).next().unwrap(), env!("ARGUS_BUILD_NUMBER")),
                })
            );
            Some(0)
        }
        Some("--argus-build-info") => Some(87),
        Some("--argus-compare-version") => Some(compare_version(
            env!("CARGO_PKG_VERSION"),
            args.get(1).map(String::as_str),
        )),
        Some("--argus-refresh-icons") => Some(if args.len() == 1 {
            refresh_icons();
            0
        } else {
            87
        }),
        Some("--argus-launch-installed") => {
            if args
                .iter()
                .skip(1)
                .any(|s| !matches!(s.as_str(), "--updated" | "--hidden"))
            {
                return Some(87);
            }
            let Ok(exe) = std::env::current_exe() else {
                return Some(2);
            };
            let mut command = argus_app::tasks::quote(&exe.to_string_lossy());
            for arg in &args[1..] {
                command.push(' ');
                command.push_str(arg);
            }
            Some(
                argus_app::tasks::launch_installed(command)
                    .err()
                    .map_or(0, |error| error.code as i32),
            )
        }
        _ => None,
    }
}

/// Makes the shell show the icons of files an install replaced under the same
/// names (argus.exe, and argus.ico for administrator startup). Explorer caches
/// icons by path and index, so shortcuts on the desktop and pinned to the
/// taskbar would keep the previous version's icon until it restarts.
/// SHCNE_ASSOCCHANGED drops that cache only when it comes from a process of
/// Explorer's own bitness: from the 32-bit installer it changes nothing, so
/// the installer runs this. The taskbar draws pinned items, and the running
/// button merged into one, from a copy of its own that it rereads only for an
/// item reported as changed: after the flush, every shortcut this account
/// pinned is reported.
fn refresh_icons() {
    use windows::Win32::UI::Shell::{
        SHCNE_ASSOCCHANGED, SHCNE_UPDATEITEM, SHCNF_FLUSHNOWAIT, SHCNF_IDLIST, SHCNF_PATHW,
        SHChangeNotify,
    };
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED,
            SHCNF_IDLIST | SHCNF_FLUSHNOWAIT,
            None,
            None,
        )
    };
    let Some(roaming) = known_folder(&windows::Win32::UI::Shell::FOLDERID_RoamingAppData) else {
        return;
    };
    let pinned = roaming.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar");
    for link in links_in(&pinned) {
        let path = windows::core::HSTRING::from(link.as_os_str());
        unsafe {
            SHChangeNotify(
                SHCNE_UPDATEITEM,
                SHCNF_PATHW | SHCNF_FLUSHNOWAIT,
                Some(path.as_ptr().cast()),
                None,
            )
        };
    }
}

fn known_folder(id: &windows::core::GUID) -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    let raw = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    let path = unsafe { raw.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    path.map(Into::into)
}

/// The shortcuts directly in `folder`.
fn links_in(folder: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
        })
        .collect()
}

fn compare_version(current: &str, installed: Option<&str>) -> i32 {
    match installed.and_then(|value| semver::Version::parse(value).ok()) {
        Some(installed) => match semver::Version::parse(current)
            .unwrap()
            .cmp_precedence(&installed)
        {
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Less => 2,
        },
        None => 1,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn installer_compares_release_precedence_without_build_metadata() {
        use super::compare_version;
        assert_eq!(compare_version("0.4.0", Some("0.4.0+672.g1234567")), 0);
        assert_eq!(
            compare_version("0.4.0+673.g7654321", Some("0.4.0+672.g1234567")),
            0
        );
        assert_eq!(compare_version("0.4.1", Some("0.4.0+672.g1234567")), 1);
        assert_eq!(compare_version("0.4.1-beta.1", Some("0.4.1")), 2);
        assert_eq!(compare_version("0.4.1", Some("0.4.1-beta.1")), 1);
    }

    #[test]
    fn only_the_shortcuts_directly_in_the_pinned_folder_are_reported() {
        use super::links_in;
        let folder = std::env::temp_dir().join(format!("argus-pinned-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("nested.lnk")).unwrap();
        for name in ["Argus.lnk", "Other.LNK", "notes.txt", "lnk"] {
            std::fs::write(folder.join(name), b"x").unwrap();
        }
        std::fs::write(folder.join("nested.lnk").join("Inner.lnk"), b"x").unwrap();
        let mut found: Vec<_> = links_in(&folder)
            .into_iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(found, ["Argus.lnk", "Other.LNK"]);
        assert!(links_in(&folder.join("missing")).is_empty());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
