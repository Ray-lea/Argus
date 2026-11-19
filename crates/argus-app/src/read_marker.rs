//! A marker file kept around on-demand device reads, so that a read which
//! brings the system down is not made again by itself at the next start.
//!
//! Some reads reach third-party drivers that cannot be told from sound ones
//! beforehand. A storage miniport that corrupts kernel memory when asked for
//! a disk's health log bug-checks seconds after the query has returned (issue
//! #133); another never completes the request and takes the system disk with
//! it (#48). A page that makes such a read whenever it opens, and is restored
//! at every start, repeats the crash with every start.
//!
//! The marker is written and flushed to the disk before a read is sent, kept
//! while the read is in flight and for [`HOLD`] after it returned, and removed
//! after that or when the application exits normally. A marker found at start
//! therefore means that the last session ended inside that window: by a bug
//! check, a power loss, a frozen system or a killed process. From then on
//! [`ReadMarker::held`] says so and the marker stays, through normal exits
//! too, until a read asked for since has outlived the hold: reads that would
//! run by themselves wait to be asked for.
//!
//! Only the file's presence decides. Its contents name what was read, for the
//! log. Nothing here fails a read: a marker that cannot be written, flushed or
//! removed is reported and the read goes ahead.
use argus_core::diagnostics;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, Once, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// How long the marker outlives a read that returned. The bug checks of issue
/// #133 came 8 to 35 seconds after the query that corrupted the pool had
/// returned, whenever the damaged block was next touched: the marker stays
/// for more than three times the longest delay seen.
pub const HOLD: Duration = Duration::from_secs(120);
/// How long a normal exit waits for a read in flight to return. Reads take
/// milliseconds: an exit just after one was asked for is no reason to hold
/// the next session, and one still going after this may never return.
const EXIT_WAIT: Duration = Duration::from_millis(300);
/// The layout of the file's contents.
const VERSION: u32 = 1;
/// More than a marker of any number of disks takes.
const LONGEST: u64 = 4096;

/// What a marker names: the subjects (a disk's number) whose read was in
/// flight, and those whose read had returned within the hold.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    #[serde(default)]
    pub reading: Vec<u32>,
    #[serde(default)]
    pub returned: Vec<u32>,
}
impl Pending {
    /// Nothing pending: what a marker that could not be removed is left saying.
    fn is_empty(&self) -> bool {
        self.reading.is_empty() && self.returned.is_empty()
    }
}

#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    #[serde(flatten)]
    pending: Pending,
}

/// The marker the last session left behind.
#[derive(Debug, PartialEq, Eq)]
pub struct Leftover {
    /// What it names; `None` where its contents could not be read.
    pub pending: Option<Pending>,
}

#[derive(Default)]
struct State {
    /// The last session left the marker, and no read since has outlived the
    /// hold: the marker stays whatever happens.
    inherited: bool,
    /// The reads in flight, by subject.
    reading: Vec<u32>,
    /// The subjects read since the marker was last removed.
    returned: Vec<u32>,
    /// Since when no read has been in flight, after one returned.
    quiet_since: Option<Instant>,
    /// What the file was last written to say; `None` while this session has
    /// not written it.
    written: Option<Pending>,
}
impl State {
    fn pending(&self) -> Pending {
        let mut reading = self.reading.clone();
        reading.sort_unstable();
        reading.dedup();
        let mut returned = self
            .returned
            .iter()
            .copied()
            .filter(|subject| !reading.contains(subject))
            .collect::<Vec<_>>();
        returned.sort_unstable();
        returned.dedup();
        Pending { reading, returned }
    }
}

pub struct ReadMarker {
    path: PathBuf,
    /// What is read, in the log's words.
    name: &'static str,
    held: AtomicBool,
    state: Mutex<State>,
    wake: Condvar,
    upkeep: Once,
}

impl ReadMarker {
    /// Looks for the marker of the last session at `path`. `name` is what the
    /// reads are of, as the log calls it.
    pub fn open(path: PathBuf, name: &'static str) -> (Arc<Self>, Option<Leftover>) {
        let leftover = leftover(&path);
        let marker = Arc::new(Self {
            path,
            name,
            held: AtomicBool::new(leftover.is_some()),
            state: Mutex::new(State {
                inherited: leftover.is_some(),
                ..State::default()
            }),
            wake: Condvar::new(),
            upkeep: Once::new(),
        });
        (marker, leftover)
    }

    /// Whether reads that would run by themselves are to wait for a request:
    /// the last session left its marker, and no read since has outlived the
    /// hold. Never waits for the disk.
    pub fn held(&self) -> bool {
        self.held.load(Ordering::Acquire)
    }

    /// Makes a read of `subject` with the marker on disk before it starts.
    /// Call it where the read may block: the marker is flushed first.
    pub fn read<T>(self: &Arc<Self>, subject: u32, call: impl FnOnce() -> T) -> T {
        struct Returned<'a>(&'a Arc<ReadMarker>, u32);
        impl Drop for Returned<'_> {
            fn drop(&mut self) {
                self.0.end(self.1, Instant::now());
                self.0.keep();
            }
        }
        self.begin(subject);
        let _returned = Returned(self, subject);
        call()
    }

    /// A normal exit: nothing went wrong while the application ran, so the
    /// marker goes. A read in flight is given a moment to return; one that
    /// has not by then keeps the marker (an exit does not end the request).
    /// So does a marker of the last session, as long as nothing has shown
    /// since that reading is safe.
    pub fn finish(&self) {
        self.finish_within(EXIT_WAIT);
    }

    fn finish_within(&self, wait: Duration) {
        let mut state = self.lock();
        let until = Instant::now() + wait;
        while !state.inherited && !state.reading.is_empty() {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = self
                .wake
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        if state.inherited || !state.reading.is_empty() {
            // What stays names what this session read.
            self.sync(&mut state);
            return;
        }
        state.returned.clear();
        state.quiet_since = None;
        if state.written.is_some() {
            self.clear(&mut state);
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin(&self, subject: u32) {
        let mut state = self.lock();
        state.reading.push(subject);
        state.quiet_since = None;
        self.sync(&mut state);
    }

    fn end(&self, subject: u32, now: Instant) {
        let mut state = self.lock();
        if let Some(at) = state.reading.iter().position(|read| *read == subject) {
            state.reading.swap_remove(at);
        }
        state.returned.push(subject);
        if state.reading.is_empty() {
            state.quiet_since = Some(now);
        }
        drop(state);
        self.wake.notify_all();
    }

    #[cfg(test)]
    fn settle(&self, now: Instant) -> Option<Instant> {
        self.settle_locked(&mut self.lock(), now)
    }

    /// Removes the marker once the reads have outlived the hold, and until
    /// then keeps the file saying what the state does. Returns when to look
    /// again, if nothing changes before.
    fn settle_locked(&self, state: &mut State, now: Instant) -> Option<Instant> {
        let due = state
            .quiet_since
            .filter(|_| state.reading.is_empty())
            .map(|since| since + HOLD);
        if due.is_some_and(|due| now >= due) {
            state.returned.clear();
            state.quiet_since = None;
            if std::mem::take(&mut state.inherited) {
                self.held.store(false, Ordering::Release);
                log::info!(
                    "{}: a read outlived the hold after the last session's marker; reads run by themselves again",
                    self.name
                );
            }
            self.clear(state);
            return None;
        }
        // A read returned since the file was written.
        self.sync(state);
        due
    }

    /// Writes the file where it does not say what the state does. Before a
    /// read this is what puts the marker on disk.
    fn sync(&self, state: &mut State) {
        let pending = state.pending();
        if pending.is_empty() || state.written.as_ref() == Some(&pending) {
            return;
        }
        match write(&self.path, &pending) {
            Ok(()) => state.written = Some(pending),
            Err(error) => diagnostics::file_failure("read marker write", &self.path, &error),
        }
    }

    fn clear(&self, state: &mut State) {
        let error = match fs::remove_file(&self.path) {
            Ok(()) => None,
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => Some(error),
        };
        let Some(error) = error else {
            state.written = None;
            return;
        };
        diagnostics::file_failure("read marker removal", &self.path, &error);
        // Left as it is, it would hold back the next session's reads: it
        // stays to say that nothing is pending, which a start reads as none.
        match write(&self.path, &Pending::default()) {
            Ok(()) => state.written = Some(Pending::default()),
            Err(error) => diagnostics::file_failure("read marker write", &self.path, &error),
        }
    }

    /// Starts the thread that removes the marker after the hold, with the
    /// first read: most sessions make none.
    fn keep(self: &Arc<Self>) {
        self.upkeep.call_once(|| {
            let marker = self.clone();
            let spawned = std::thread::Builder::new()
                .name("argus-read-marker".into())
                .spawn(move || marker.run());
            if let Err(error) = spawned {
                diagnostics::warn("read marker thread", diagnostics::io(&error));
            }
        });
    }

    fn run(&self) {
        let mut state = self.lock();
        loop {
            // The state is settled and waited on under one lock: a read that
            // returns meanwhile changes it, and wakes this, only after.
            state = match self.settle_locked(&mut state, Instant::now()) {
                Some(due) => {
                    let wait = due.saturating_duration_since(Instant::now());
                    self.wake
                        .wait_timeout(state, wait)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => self
                    .wake
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
    }
}

/// The marker a session left at `path`, if any. A file that cannot be read
/// still says what its presence says.
fn leftover(path: &Path) -> Option<Leftover> {
    let read = fs::File::open(path).and_then(|file| {
        let mut bytes = Vec::new();
        file.take(LONGEST).read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    match read {
        Ok(bytes) => {
            let pending = serde_json::from_slice::<Stored>(&bytes)
                .ok()
                .filter(|stored| stored.version == VERSION)
                .map(|stored| stored.pending);
            if pending.as_ref().is_some_and(Pending::is_empty) {
                // Set aside by a session that could not remove it.
                let _ = fs::remove_file(path);
                return None;
            }
            Some(Leftover { pending })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            diagnostics::file_failure("read marker read", path, &error);
            fs::symlink_metadata(path)
                .is_ok()
                .then_some(Leftover { pending: None })
        }
    }
}

/// Replaces the file and has it on the disk before returning: a read sent
/// next may be the last thing the system does.
fn write(path: &Path, pending: &Pending) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&Stored {
        version: VERSION,
        pending: pending.clone(),
    })?;
    bytes.push(b'\n');
    let mut file = fs::File::create(path)?;
    file.write_all(&bytes)?;
    if let Err(error) = file.sync_all() {
        // Written, though perhaps not past a power loss.
        diagnostics::file_failure("read marker flush", path, &error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "argus-read-marker-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn file(&self) -> PathBuf {
            self.0.join("read.pending")
        }
        /// A session starting on this folder.
        fn session(&self) -> (Arc<ReadMarker>, Option<Leftover>) {
            ReadMarker::open(self.file(), "test")
        }
        fn stored(&self) -> Option<Pending> {
            let bytes = fs::read(self.file()).ok()?;
            Some(serde_json::from_slice::<Stored>(&bytes).unwrap().pending)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn pending(reading: &[u32], returned: &[u32]) -> Pending {
        Pending {
            reading: reading.to_vec(),
            returned: returned.to_vec(),
        }
    }

    const SHORT: Duration = Duration::from_millis(1);

    #[test]
    fn the_hold_covers_the_delays_seen() {
        // Issue #133: bug checks 8 to 35 seconds after the query returned.
        assert!(HOLD >= 3 * Duration::from_secs(35));
    }

    #[test]
    fn a_read_is_marked_before_it_starts_and_through_the_hold_after_it() {
        let scratch = Scratch::new("read");
        let (marker, leftover) = scratch.session();
        assert_eq!(leftover, None);
        assert!(!marker.held());
        assert!(!scratch.file().exists());

        marker.begin(3);
        assert_eq!(scratch.stored(), Some(pending(&[3], &[])));
        // Nothing is due while the read is in flight, however long it takes.
        let start = Instant::now();
        assert_eq!(marker.settle(start + 10 * HOLD), None);
        assert!(scratch.file().exists());

        marker.end(3, start);
        assert_eq!(marker.settle(start), Some(start + HOLD));
        assert_eq!(scratch.stored(), Some(pending(&[], &[3])));
        assert_eq!(marker.settle(start + HOLD - SHORT), Some(start + HOLD));
        assert!(scratch.file().exists());
        assert_eq!(marker.settle(start + HOLD), None);
        assert!(!scratch.file().exists());
        // Nothing is left to do or to remove.
        assert_eq!(marker.settle(start + 2 * HOLD), None);
        assert!(!scratch.session().0.held());
    }

    #[test]
    fn the_hold_runs_from_the_last_read_to_return() {
        let scratch = Scratch::new("overlap");
        let (marker, _) = scratch.session();
        let start = Instant::now();
        marker.begin(0);
        marker.begin(1);
        assert_eq!(scratch.stored(), Some(pending(&[0, 1], &[])));
        marker.end(0, start);
        // One is still in flight.
        assert_eq!(marker.settle(start + HOLD), None);
        assert_eq!(scratch.stored(), Some(pending(&[1], &[0])));
        let second = start + Duration::from_secs(50);
        marker.end(1, second);
        assert_eq!(marker.settle(start + HOLD), Some(second + HOLD));
        assert!(scratch.file().exists());

        // Another read within the hold starts it again.
        let third = second + Duration::from_secs(100);
        marker.begin(0);
        assert_eq!(scratch.stored(), Some(pending(&[0], &[1])));
        assert_eq!(marker.settle(second + HOLD), None);
        marker.end(0, third);
        assert_eq!(marker.settle(second + HOLD), Some(third + HOLD));
        assert_eq!(scratch.stored(), Some(pending(&[], &[0, 1])));
        assert_eq!(marker.settle(third + HOLD), None);
        assert!(!scratch.file().exists());
    }

    #[test]
    fn a_normal_exit_removes_the_marker_unless_a_read_is_in_flight() {
        let scratch = Scratch::new("exit");
        let (marker, _) = scratch.session();
        // A session that read nothing leaves nothing.
        marker.finish();
        assert!(!scratch.file().exists());

        // Within the hold: the application ended in order.
        marker.begin(0);
        marker.end(0, Instant::now());
        marker.finish();
        assert!(!scratch.file().exists());
        assert_eq!(scratch.session().1, None);

        // A read that has not returned is not ended by the exit.
        let (marker, _) = scratch.session();
        marker.begin(0);
        marker.begin(2);
        marker.end(2, Instant::now());
        marker.finish_within(Duration::ZERO);
        let (next, leftover) = scratch.session();
        assert_eq!(
            leftover,
            Some(Leftover {
                pending: Some(pending(&[0], &[2]))
            })
        );
        assert!(next.held());
    }

    #[test]
    fn a_normal_exit_waits_for_a_read_that_is_about_to_return() {
        let scratch = Scratch::new("exit-wait");
        let (marker, _) = scratch.session();
        marker.begin(0);
        std::thread::scope(|scope| {
            scope.spawn(|| marker.end(0, Instant::now()));
            // Far longer than the read takes: the wait ends when it returns.
            marker.finish_within(Duration::from_secs(600));
        });
        assert!(!scratch.file().exists());
        assert_eq!(scratch.session().1, None);
    }

    #[test]
    fn a_marker_left_behind_holds_reads_until_one_outlives_the_hold() {
        let scratch = Scratch::new("leftover");
        let (crashed, _) = scratch.session();
        crashed.begin(1);
        crashed.end(1, Instant::now());
        // The session ends here without `finish`: a bug check, a kill.
        drop(crashed);

        let (marker, leftover) = scratch.session();
        assert_eq!(
            leftover,
            Some(Leftover {
                pending: Some(pending(&[1], &[]))
            })
        );
        assert!(marker.held());
        // Time alone changes nothing.
        let start = Instant::now();
        assert_eq!(marker.settle(start + 10 * HOLD), None);
        assert!(marker.held());

        // A read asked for: held until the system has outlived it.
        marker.begin(1);
        marker.end(1, start);
        assert!(marker.held());
        assert_eq!(marker.settle(start + HOLD - SHORT), Some(start + HOLD));
        assert!(marker.held());
        assert_eq!(marker.settle(start + HOLD), None);
        assert!(!marker.held());
        assert!(!scratch.file().exists());

        // From here on it is a session like any other.
        marker.begin(1);
        marker.end(1, start + 2 * HOLD);
        marker.finish();
        assert!(!scratch.file().exists());
        let (next, leftover) = scratch.session();
        assert_eq!(leftover, None);
        assert!(!next.held());
    }

    #[test]
    fn a_leftover_marker_stays_through_normal_exits() {
        let scratch = Scratch::new("sticky");
        let (crashed, _) = scratch.session();
        crashed.begin(0);
        drop(crashed);

        // Started and closed without a read: the next start is held too.
        let (idle, _) = scratch.session();
        assert!(idle.held());
        idle.finish();
        assert_eq!(scratch.stored(), Some(pending(&[0], &[])));

        // Read, then closed before the hold was over: still not shown safe.
        let (hasty, _) = scratch.session();
        assert!(hasty.held());
        hasty.begin(4);
        hasty.end(4, Instant::now());
        hasty.finish();
        assert!(scratch.file().exists());

        let (marker, leftover) = scratch.session();
        assert!(leftover.is_some());
        assert!(marker.held());
    }

    #[test]
    fn a_crash_after_a_requested_read_holds_the_next_session_again() {
        let scratch = Scratch::new("again");
        let (crashed, _) = scratch.session();
        crashed.begin(0);
        drop(crashed);
        let (retry, _) = scratch.session();
        retry.begin(2);
        // Down again while reading another disk.
        drop(retry);
        let (marker, leftover) = scratch.session();
        assert_eq!(
            leftover,
            Some(Leftover {
                pending: Some(pending(&[2], &[]))
            })
        );
        assert!(marker.held());
    }

    #[test]
    fn the_presence_of_a_leftover_decides_whatever_it_holds() {
        for contents in [
            &b""[..],
            b"{",
            b"\0\0\0\0",
            b"not a marker",
            br#"{"version":2,"reading":[],"returned":[]}"#,
            br#"{"reading":[0]}"#,
            br#"{"version":1,"reading":"0"}"#,
        ] {
            let scratch = Scratch::new("contents");
            fs::write(scratch.file(), contents).unwrap();
            let (marker, leftover) = scratch.session();
            assert_eq!(leftover, Some(Leftover { pending: None }), "{contents:?}");
            assert!(marker.held());
            // A read asked for goes ahead and resolves it as any other.
            let start = Instant::now();
            assert_eq!(marker.read(7, || 42), 42);
            assert_eq!(marker.settle(start + 2 * HOLD), None);
            assert!(!marker.held());
            assert!(!scratch.file().exists());
        }
        // Fields a later version adds are left alone.
        let scratch = Scratch::new("fields");
        fs::write(
            scratch.file(),
            br#"{"version":1,"reading":[5],"returned":[6],"since":12}"#,
        )
        .unwrap();
        assert_eq!(
            scratch.session().1,
            Some(Leftover {
                pending: Some(pending(&[5], &[6]))
            })
        );
        // Something that is no file at all.
        let scratch = Scratch::new("folder");
        fs::create_dir(scratch.file()).unwrap();
        let (marker, leftover) = scratch.session();
        assert_eq!(leftover, Some(Leftover { pending: None }));
        assert!(marker.held());
        assert_eq!(marker.read(0, || "read"), "read");
        marker.finish();
    }

    #[test]
    fn a_long_leftover_is_not_read_whole() {
        let scratch = Scratch::new("long");
        fs::write(scratch.file(), vec![b' '; 4 * LONGEST as usize]).unwrap();
        assert_eq!(scratch.session().1, Some(Leftover { pending: None }));
    }

    #[test]
    fn a_marker_that_cannot_be_written_does_not_stop_the_read() {
        let scratch = Scratch::new("unwritable");
        // Its folder is gone.
        let (marker, leftover) = ReadMarker::open(scratch.0.join("missing").join("x"), "test");
        assert_eq!(leftover, None);
        assert_eq!(marker.read(0, || 7), 7);
        let start = Instant::now();
        assert_eq!(marker.settle(start + 2 * HOLD), None);
        marker.finish();
        assert!(!marker.held());
        assert!(!scratch.0.join("missing").exists());
    }

    #[test]
    fn a_marker_that_cannot_be_removed_is_left_saying_nothing_is_pending() {
        let scratch = Scratch::new("stuck");
        let (marker, _) = scratch.session();
        marker.begin(0);
        let start = Instant::now();
        marker.end(0, start);
        // Another program holds the file open without sharing its removal.
        const SHARE_READ_WRITE: u32 = 0x1 | 0x2;
        let other = fs::OpenOptions::new()
            .read(true)
            .share_mode(SHARE_READ_WRITE)
            .open(scratch.file())
            .unwrap();
        assert_eq!(marker.settle(start + HOLD), None);
        assert_eq!(scratch.stored(), Some(Pending::default()));
        // The next read marks it again, and the exit tries again.
        marker.begin(1);
        assert_eq!(scratch.stored(), Some(pending(&[1], &[])));
        marker.end(1, start + HOLD);
        marker.finish();
        assert_eq!(scratch.stored(), Some(Pending::default()));

        // A start takes it for no marker and removes it once it can.
        let (next, leftover) = scratch.session();
        assert_eq!(leftover, None);
        assert!(!next.held());
        assert!(scratch.file().exists());
        drop(other);
        assert_eq!(scratch.session().1, None);
        assert!(!scratch.file().exists());
    }

    #[test]
    fn the_marker_is_there_while_the_read_runs() {
        let scratch = Scratch::new("during");
        let (marker, _) = scratch.session();
        let seen = marker.read(9, || scratch.stored());
        assert_eq!(seen, Some(pending(&[9], &[])));
        // And after it, until the hold is over or the session ends.
        assert!(scratch.file().exists());
        marker.finish();
        assert!(!scratch.file().exists());
    }
}
