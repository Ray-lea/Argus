use std::path::Path;
use std::time::Duration;

use argus_core::{actions, details, process};

/// Longest wait for the leftovers to finish exiting before starting anyway.
const WAIT: Duration = Duration::from_secs(3);

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
