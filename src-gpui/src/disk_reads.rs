//! Disk details reads, and the marker kept around them
//! (`argus_app::read_marker`).
//!
//! A disk's details are read from its storage driver, by queries a faulty
//! driver may answer by corrupting kernel memory or by never answering. The
//! details page is restored at start and reads when it opens, so such a
//! driver would bring the system down with every start. Every read is
//! therefore made with a marker on disk; a marker found at start means the
//! last session ended while a read was pending, and the page then waits to be
//! asked (`held`) instead of reading by itself.
use argus_app::read_marker::{Leftover, ReadMarker};
use std::{
    path::Path,
    sync::{Arc, OnceLock},
};

/// In the application's local data folder: apart for each identifier, and
/// for each account Argus runs as.
const FILE: &str = "disk-details-read.pending";
static MARKER: OnceLock<Arc<ReadMarker>> = OnceLock::new();

/// Looks for the marker of the last session, once, before the interface
/// starts: only the instance that owns `local_data` calls this.
pub fn install(local_data: &Path) {
    let (marker, leftover) = ReadMarker::open(local_data.join(FILE), "disk details");
    if let Some(leftover) = leftover {
        log::warn!("{}", describe(&leftover));
    }
    let _ = MARKER.set(marker);
}

/// Whether the details page is to wait for a request instead of reading when
/// it opens. Reads no file: safe to ask while drawing.
pub fn held() -> bool {
    MARKER.get().is_some_and(|marker| marker.held())
}

/// Makes a read of PhysicalDrive `disk` with the marker on disk first. Runs
/// where the read does, off the UI thread: the marker is flushed before.
pub fn read<T>(disk: u32, call: impl FnOnce() -> T) -> T {
    match MARKER.get() {
        Some(marker) => marker.read(disk, call),
        None => call(),
    }
}

/// A normal exit of the application.
pub fn finish() {
    if let Some(marker) = MARKER.get() {
        marker.finish();
    }
}

/// The log's line about a marker found at start: disk numbers only, which
/// the `disk: N; bus=…; storage driver=…` lines tell apart.
fn describe(leftover: &Leftover) -> String {
    let disks = |disks: &[u32]| {
        if disks.is_empty() {
            "none".to_owned()
        } else {
            disks
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        }
    };
    let pending = match &leftover.pending {
        Some(pending) => format!(
            "disks in flight: {}; returned: {}",
            disks(&pending.reading),
            disks(&pending.returned)
        ),
        None => "marker unreadable".to_owned(),
    };
    format!(
        "disk details: the last session ended with a read pending ({pending}); the page waits for a request instead of reading by itself"
    )
}

#[cfg(test)]
mod tests {
    use super::describe;
    use argus_app::read_marker::{Leftover, Pending};

    #[test]
    fn the_log_names_the_disks_by_number_only() {
        let line = describe(&Leftover {
            pending: Some(Pending {
                reading: vec![1],
                returned: vec![0, 2],
            }),
        });
        assert!(line.contains("disks in flight: 1; returned: 0,2"), "{line}");
        let line = describe(&Leftover {
            pending: Some(Pending {
                reading: vec![],
                returned: vec![3],
            }),
        });
        assert!(
            line.contains("disks in flight: none; returned: 3"),
            "{line}"
        );
        let line = describe(&Leftover { pending: None });
        assert!(line.contains("marker unreadable"), "{line}");
        assert!(!line.contains('\n'));
    }
}
