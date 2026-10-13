//! WebView2 processes an earlier Argus left behind.
//!
//! When Argus ends without closing its WebView (it crashed, was ended from a
//! task manager, or a development run was stopped), its WebView2 processes
//! can outlive it for minutes or hours, holding the user data folder: on a
//! busy machine their threads, at low or idle priority, wait for the CPU
//! while the processes wind down. Creating the next start's WebView then fails
//! ("The requested resource is in use", 0x800700AA) and Argus exits before
//! showing a window. So before the window is created, the browser processes of
//! this build's own folder whose parent is gone are ended, with the processes
//! they started, and waited for. A running Argus is its WebView's parent, so
//! its WebView is left alone; other apps' WebViews use other folders.

use std::path::Path;
use std::time::Duration;

use argus_core::{actions, details, process};

/// Longest wait for the leftovers to finish exiting before starting anyway.
const WAIT: Duration = Duration::from_secs(3);

/// Ends the leftovers of `user_data` (the WebView2 user data folder); returns
/// how many processes were ended.
pub fn end(user_data: &Path) -> usize {
    let ours = user_data.to_string_lossy().to_lowercase();
    let webviews = process::find("msedgewebview2.exe");
    let mut targets = Vec::new();
    for browser in webviews.iter().filter(|p| !p.parent_running) {
        let Some(command) = details::command_line_of(browser.pid) else {
            continue;
        };
        // Renderer, GPU and other helper processes carry a --type.
        let ours_too = user_data_dir(&command).is_some_and(|dir| dir.to_lowercase() == ours);
        if command.contains("--type=") || !ours_too {
            continue;
        }
        let helpers = webviews
            .iter()
            .filter(|p| p.parent_pid == browser.pid && p.create_time >= browser.create_time);
        targets.extend(
            std::iter::once(browser)
                .chain(helpers)
                .map(|p| (p.pid, p.create_time)),
        );
    }
    if targets.is_empty() {
        return 0;
    }
    actions::end_and_wait(&targets, WAIT)
}

/// The `--user-data-dir` a WebView2 browser process was started with.
fn user_data_dir(command: &str) -> Option<&str> {
    const FLAG: &str = "--user-data-dir=";
    let rest = &command[command.find(FLAG)? + FLAG.len()..];
    match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next(),
        None => rest.split_whitespace().next(),
    }
}

#[cfg(test)]
mod tests {
    use super::user_data_dir;

    #[test]
    fn reads_the_user_data_folder() {
        let quoted = r#""C:\WebView\msedgewebview2.exe" --embedded-browser-webview=1 --user-data-dir="C:\Users\A B\AppData\Local\app.id\EBWebView" --noerrdialogs"#;
        assert_eq!(
            user_data_dir(quoted),
            Some(r"C:\Users\A B\AppData\Local\app.id\EBWebView")
        );
        let bare = r"msedgewebview2.exe --user-data-dir=C:\Data\EBWebView --type=renderer";
        assert_eq!(user_data_dir(bare), Some(r"C:\Data\EBWebView"));
        assert_eq!(user_data_dir("msedgewebview2.exe --type=gpu-process"), None);
    }
}
