use std::path::{Path, PathBuf};
use std::time::Duration;

use argus_core::{actions, details, diagnostics, process};

/// Longest wait for the leftovers to finish exiting before starting anyway.
const WAIT: Duration = Duration::from_secs(3);

/// WebView2 user data folders of Argus 0.1.0–0.3.4, which drew its interface
/// in WebView2, inside the app's local data folder: Tauri's default
/// `EBWebView`, and the `WebView2\elevated-*` profiles 0.3.x kept in the
/// desktop account's folder while it ran as another administrator account.
const WEBVIEW_DATA: [&str; 2] = ["EBWebView", "WebView2"];

/// The installer and the uninstaller run Argus with this; see `close_copies`.
pub const CLOSE_COPIES_ARG: &str = "--argus-close-copies";

pub fn is_close_arg(arg: &str) -> bool {
    // Keep accepting the earlier uninstaller's maintenance command.
    matches!(arg, CLOSE_COPIES_ARG | "--argus-uninstall-close")
}

/// Closes verified task manager interface copies and administrator copies
/// during installation or removal.
pub fn close_copies(identifier: &str) {
    let copies: Vec<(u32, i64)> = process::find_by(|name| {
        name.get(..17)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("argus-taskmgr-ui-"))
            || name.to_ascii_lowercase().starts_with("argus-admin-")
    })
    .into_iter()
    .filter(|p| {
        details::image_path_of(p.pid).is_some_and(|path| {
            argus_service::taskmgr::is_interface_copy(Path::new(&path))
                || argus_service::admin::is_copy(Path::new(&path), identifier)
        })
    })
    .map(|p| (p.pid, p.create_time))
    .collect();
    actions::end_and_wait(&copies, WAIT);
}

/// Removes the WebView2 data an earlier Argus left in `local`, the app's local
/// data folder. Hundreds of cache files: kept off the startup path.
pub fn remove_webview_data(local: PathBuf) {
    if !WEBVIEW_DATA
        .iter()
        .any(|name| local.join(name).symlink_metadata().is_ok())
    {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("argus-webview-cleanup".into())
        .spawn(move || remove_webview_data_in(&local));
    if let Err(error) = spawned {
        diagnostics::warn(
            "earlier WebView2 data cleanup thread",
            diagnostics::io(&error),
        );
    }
}

/// Whether no known folder remains. A folder a still-exiting WebView2 process
/// holds files in stays, partly removed, for a later start to try again. Links
/// are removed, never followed.
fn remove_webview_data_in(local: &Path) -> bool {
    let mut gone = true;
    for name in WEBVIEW_DATA {
        let folder = local.join(name);
        match std::fs::remove_dir_all(&folder) {
            Ok(()) => log::info!("removed {name}, the WebView2 data of Argus 0.3 or earlier"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                diagnostics::file_failure("earlier WebView2 data removal", &folder, &error);
                gone = false;
            }
        }
    }
    gone
}

#[cfg(test)]
mod tests {
    use super::{WEBVIEW_DATA, remove_webview_data_in};
    use std::fs::{self, OpenOptions};
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::{Path, PathBuf};

    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(label: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "argus-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        Scratch(root)
    }

    fn write(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    /// Junctions need no privilege, unlike directory symbolic links.
    fn junction(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn removes_only_the_webview_data_of_earlier_versions() {
        let root = scratch("webview-data");
        let local = root.0.join("dev.argus.taskmanager");
        for file in [
            r"EBWebView\Local State",
            r"EBWebView\Default\Cache\Cache_Data\data_0",
            r"WebView2\elevated-0123456789abcdef0123456789abcdef\EBWebView\Local State",
            r"history\2026-11-05.bin",
            r"logs\diagnostics\argus.log",
            r"updates\latest.json",
        ] {
            write(&local.join(file));
        }
        // A link inside the data leads outside it.
        let outside = root.0.join("outside");
        write(&outside.join("keep.txt"));
        junction(&local.join(r"EBWebView\Default\linked"), &outside);

        assert!(remove_webview_data_in(&local));
        for name in WEBVIEW_DATA {
            assert!(local.join(name).symlink_metadata().is_err(), "{name}");
        }
        for kept in [
            r"history\2026-11-05.bin",
            r"logs\diagnostics\argus.log",
            r"updates\latest.json",
        ] {
            assert!(local.join(kept).is_file(), "{kept}");
        }
        assert!(outside.join("keep.txt").is_file());
        // Later starts find nothing left to do.
        assert!(remove_webview_data_in(&local));
    }

    #[test]
    fn a_linked_data_folder_loses_the_link_not_its_target() {
        let root = scratch("webview-link");
        let local = root.0.join("app");
        let outside = root.0.join("outside");
        fs::create_dir(&local).unwrap();
        write(&outside.join("keep.txt"));
        junction(&local.join("EBWebView"), &outside);

        assert!(remove_webview_data_in(&local));
        assert!(local.join("EBWebView").symlink_metadata().is_err());
        assert!(outside.join("keep.txt").is_file());
    }

    #[test]
    fn files_a_webview_still_holds_wait_for_a_later_start() {
        let root = scratch("webview-held");
        let local = root.0.join("app");
        let held = local.join(r"EBWebView\Default\Cookies");
        write(&held);
        write(&local.join(r"EBWebView\Local State"));
        let lock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&held)
            .unwrap();

        assert!(!remove_webview_data_in(&local));
        assert!(local.join("EBWebView").is_dir());
        drop(lock);
        assert!(remove_webview_data_in(&local));
        assert!(local.join("EBWebView").symlink_metadata().is_err());
    }
}
