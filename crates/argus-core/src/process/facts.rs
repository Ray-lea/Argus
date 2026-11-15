//! Per-instance process facts for optional columns, read off the sampling
//! thread: each takes a few microseconds to a few hundred, which adds up over
//! hundreds of processes and bursts of new ones.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender};

use serde::Serialize;
use windows::Win32::Foundation::HANDLE;

/// What stays fixed for a process instance.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub command_line: Option<Arc<str>>,
    /// `x64` | `x86` | `arm64` | `arm`
    pub architecture: Option<&'static str>,
    pub elevated: Option<bool>,
}

/// Reads of an instance's facts, one per sample at most. A process that has
/// only just started may not have its command line in place yet; what is
/// still missing after these stays unknown.
pub(super) const READS: u8 = 3;

/// Requests waiting for the reader; more wait for a later sample.
const QUEUE: usize = 256;

impl Facts {
    pub(super) fn complete(&self) -> bool {
        self.command_line.is_some() && self.architecture.is_some() && self.elevated.is_some()
    }

    /// Reads what is still missing.
    pub(super) fn fill(&mut self, process: HANDLE) {
        self.fill_with(
            || crate::details::command_line(process).map(Into::into),
            || crate::details::architecture(process),
            || crate::details::elevated(process),
        );
    }

    /// Takes what is still missing from `other`.
    pub(super) fn merge(&mut self, other: Facts) {
        self.fill_with(
            || other.command_line,
            || other.architecture,
            || other.elevated,
        );
    }

    fn fill_with(
        &mut self,
        command_line: impl FnOnce() -> Option<Arc<str>>,
        architecture: impl FnOnce() -> Option<&'static str>,
        elevated: impl FnOnce() -> Option<bool>,
    ) {
        if self.command_line.is_none() {
            self.command_line = command_line();
        }
        if self.architecture.is_none() {
            self.architecture = architecture();
        }
        if self.elevated.is_none() {
            self.elevated = elevated();
        }
    }
}

/// An instance by PID and creation time in FILETIME ticks.
pub(super) type Key = (u32, i64);

struct Request {
    key: Key,
    known: Facts,
}

/// A thread that opens each requested instance again, checking its creation
/// time, and reads the facts it is still missing.
pub(super) struct Reader {
    requests: SyncSender<Request>,
    answers: Receiver<(Key, Facts)>,
}

impl Reader {
    pub(super) fn new() -> Option<Self> {
        let (requests, queue) = mpsc::sync_channel::<Request>(QUEUE);
        let (reply, answers) = mpsc::channel();
        std::thread::Builder::new()
            .name("argus-process-facts".into())
            .spawn(move || {
                while let Ok(Request { key, mut known }) = queue.recv() {
                    if let Some(handle) = super::query_handle(key.0, key.1) {
                        known.fill(handle.0);
                    }
                    if reply.send((key, known)).is_err() {
                        return;
                    }
                }
            })
            .inspect_err(|error| {
                crate::diagnostics::warn(
                    "process facts worker creation",
                    crate::diagnostics::io(error),
                )
            })
            .ok()?;
        Some(Self { requests, answers })
    }

    /// Queues a read; false when the queue is full.
    pub(super) fn request(&self, key: Key, known: Facts) -> bool {
        self.requests.try_send(Request { key, known }).is_ok()
    }

    /// The reads finished since the last call.
    pub(super) fn answers(&self) -> impl Iterator<Item = (Key, Facts)> + '_ {
        self.answers.try_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::filetime_ticks;
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    fn expected_architecture() -> &'static str {
        if cfg!(target_arch = "aarch64") {
            "arm64"
        } else if cfg!(target_arch = "x86") {
            "x86"
        } else {
            "x64"
        }
    }

    #[test]
    fn later_reads_fill_only_the_facts_still_missing() {
        let mut facts = Facts::default();
        facts.fill_with(|| None, || Some("x64"), || Some(false));
        assert!(!facts.complete());
        facts.fill_with(
            || Some("app.exe --flag".into()),
            || panic!("architecture is known"),
            || panic!("elevation is known"),
        );
        assert!(facts.complete());
        let mut seeded = Facts {
            command_line: Some("seeded.exe".into()),
            ..Facts::default()
        };
        seeded.merge(facts);
        assert_eq!(seeded.command_line.as_deref(), Some("seeded.exe"));
        assert_eq!(seeded.architecture, Some("x64"));
    }

    #[test]
    fn the_reader_answers_for_a_live_instance_and_serializes_as_columns() {
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        }
        .unwrap();
        let key = (std::process::id(), filetime_ticks(created));
        let reader = Reader::new().unwrap();
        assert!(reader.request(key, Facts::default()));
        // A creation time that doesn't match is another instance: nothing is read.
        assert!(reader.request((key.0, key.1 + 1), Facts::default()));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut answers = Vec::new();
        while answers.len() < 2 && std::time::Instant::now() < deadline {
            answers.extend(reader.answers());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let facts = answers.iter().find(|(k, _)| *k == key).unwrap().1.clone();
        let other = &answers.iter().find(|(k, _)| *k != key).unwrap().1;
        assert_eq!(other, &Facts::default());
        let exe = std::env::current_exe().unwrap();
        let stem = exe.file_stem().unwrap().to_string_lossy().to_lowercase();
        assert!(
            facts
                .command_line
                .as_deref()
                .is_some_and(|line| line.to_lowercase().contains(&stem))
        );
        assert_eq!(facts.architecture, Some(expected_architecture()));
        assert!(facts.elevated.is_some());
        let row = serde_json::to_value(super::super::ProcessRow {
            facts,
            ..Default::default()
        })
        .unwrap();
        assert!(row["commandLine"].is_string());
        assert_eq!(row["architecture"], expected_architecture());
        assert!(row["elevated"].is_boolean());
    }
}
