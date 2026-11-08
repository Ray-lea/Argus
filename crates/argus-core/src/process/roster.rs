//! Independent process identity snapshots. Counters are attached only after
//! proving they belong to the same process lifetime, including delayed replies.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
use windows::Win32::Foundation::{HANDLE, NTSTATUS};

use super::ProcessRow;
use crate::nt::{
    SYSTEM_BASIC_PROCESS_INFORMATION, SystemBasicProcessInformation, query_system_information,
};
use crate::util::AlignedBuf;

#[derive(Clone)]
pub(super) struct Identity {
    pub pid: u32,
    pub parent: u32,
    pub sequence: u64,
    pub name: Arc<str>,
}

pub(super) struct Frame {
    started: Instant,
    finished: Instant,
    entries: Vec<Identity>,
}

pub(super) fn sequence(handle: HANDLE) -> Option<u64> {
    let mut number = 0u64;
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            PROCESSINFOCLASS(92),
            (&mut number as *mut u64).cast(),
            size_of::<u64>() as u32,
            std::ptr::null_mut(),
        )
    };
    (status.0 >= 0).then_some(number)
}

fn decode(buffer: &AlignedBuf, returned: usize) -> Option<Vec<Identity>> {
    if returned > buffer.len() {
        return None;
    }
    let base = buffer.as_ptr() as usize;
    let mut offset = 0usize;
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    loop {
        let header_end = offset.checked_add(size_of::<SystemBasicProcessInformation>())?;
        if !offset.is_multiple_of(8) || header_end > returned {
            return None;
        }
        let p = unsafe {
            &*buffer
                .as_ptr()
                .add(offset)
                .cast::<SystemBasicProcessInformation>()
        };
        let end = if p.next == 0 {
            returned
        } else {
            offset.checked_add(p.next as usize)?
        };
        if end < header_end || end > returned {
            return None;
        }
        let pid = p.pid.0 as usize as u32;
        if pid != 0 {
            if !seen.insert(pid)
                || !p.name.length.is_multiple_of(2)
                || p.name.length > p.name.maximum_length
            {
                return None;
            }
            let name = if p.name.length == 0 {
                Arc::from("")
            } else {
                let start = (p.name.buffer as usize).checked_sub(base)?;
                if start < header_end
                    || !start.is_multiple_of(2)
                    || start.checked_add(p.name.length as usize)? > end
                {
                    return None;
                }
                Arc::from(unsafe { p.name.to_string_lossy() })
            };
            entries.push(Identity {
                pid,
                parent: p.parent.0 as usize as u32,
                sequence: p.sequence,
                name,
            });
        }
        if p.next == 0 {
            return Some(entries);
        }
        offset = end;
    }
}

fn unsupported(status: NTSTATUS) -> bool {
    matches!(status.0 as u32, 0xc000_0002 | 0xc000_0003 | 0xc000_00bb)
}

struct Reader {
    requests: mpsc::SyncSender<()>,
    replies: mpsc::Receiver<Result<Frame, NTSTATUS>>,
    pending: bool,
    disabled: bool,
}

impl Reader {
    fn new() -> Option<Self> {
        let mut buffer = AlignedBuf::with_capacity(128 * 1024);
        Self::spawn(move || {
            let returned = crate::diagnostics::sample_phase("process-roster", || {
                query_system_information(SYSTEM_BASIC_PROCESS_INFORMATION, &mut buffer)
            })?;
            decode(&buffer, returned).ok_or(NTSTATUS(0xc000_0004u32 as i32))
        })
        .inspect_err(|error| {
            crate::diagnostics::warn("process roster worker", crate::diagnostics::io(error))
        })
        .ok()
    }

    fn spawn(
        mut read: impl FnMut() -> Result<Vec<Identity>, NTSTATUS> + Send + 'static,
    ) -> std::io::Result<Self> {
        let (requests, incoming) = mpsc::sync_channel(1);
        let (output, replies) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("argus-process-roster".into())
            .spawn(move || {
                while incoming.recv().is_ok() {
                    let started = Instant::now();
                    let reply = read().map(|entries| Frame {
                        started,
                        finished: Instant::now(),
                        entries,
                    });
                    let unavailable = reply.as_ref().is_err_and(|status| unsupported(*status));
                    if output.send(reply).is_err() || unavailable {
                        break;
                    }
                }
            })?;
        Ok(Self {
            requests,
            replies,
            pending: false,
            disabled: false,
        })
    }

    fn read(&mut self) -> Option<Frame> {
        if self.disabled {
            return None;
        }
        if !self.pending {
            if self.requests.try_send(()).is_err() {
                return None;
            }
            self.pending = true;
        }
        match self.replies.recv_timeout(Duration::from_millis(5)) {
            Ok(reply) => {
                self.pending = false;
                match reply {
                    Ok(frame) => Some(frame),
                    Err(status) => {
                        self.disabled = unsupported(status);
                        if self.disabled {
                            log::info!(
                                "basic process information unavailable; code=0x{:08x}; using full snapshots",
                                status.0 as u32
                            );
                        } else {
                            crate::diagnostics::warn(
                                "process roster query",
                                crate::diagnostics::code(status.0 as u32),
                            );
                        }
                        None
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.disabled = true;
                None
            }
        }
    }
}

struct Live {
    identity: Identity,
    first_seen: Instant,
    created: Option<i64>,
    key: Option<Arc<str>>,
    pending: Option<ProcessRow>,
    counters: Option<(ProcessRow, Instant)>,
}

pub(super) struct Roster {
    reader: Option<Reader>,
    live: HashMap<u32, Live>,
    frame: Option<(Instant, Instant)>,
}

impl Roster {
    pub fn new() -> Self {
        let mut roster = Self {
            reader: Reader::new(),
            live: HashMap::new(),
            frame: None,
        };
        roster.refresh();
        roster
    }

    pub fn refresh(&mut self) {
        if let Some(frame) = self.reader.as_mut().and_then(Reader::read) {
            self.observe(frame);
        }
    }

    fn observe(&mut self, frame: Frame) {
        let mut previous = std::mem::take(&mut self.live);
        for identity in frame.entries {
            let live = previous
                .remove(&identity.pid)
                .filter(|old| old.identity.sequence == identity.sequence)
                .unwrap_or_else(|| Live {
                    identity: identity.clone(),
                    first_seen: frame.finished,
                    created: None,
                    key: None,
                    pending: None,
                    counters: None,
                });
            self.live.insert(identity.pid, live);
        }
        self.frame = Some((frame.started, frame.finished));
    }

    pub fn merge(
        &mut self,
        rows: Vec<ProcessRow>,
        counted: Option<(Instant, Instant)>,
        mut placeholder: impl FnMut(&Identity) -> ProcessRow,
    ) -> Vec<ProcessRow> {
        let Some((roster_started, roster_finished)) = self.frame else {
            return rows;
        };
        if counted.is_some_and(|(start, _)| start > roster_finished) {
            // A fresh full snapshot can take over if the identity reader is late.
            // Carry proven UI keys across this handoff without trusting PID alone.
            return rows
                .into_iter()
                .map(|mut row| {
                    if let Some(live) = self.live.get(&row.pid)
                        && live.created == Some(row.create_time)
                        && row
                            .sequence_number
                            .is_none_or(|sequence| sequence == live.identity.sequence)
                    {
                        row.instance_key.clone_from(&live.key);
                    }
                    row
                })
                .collect();
        }
        let mut counters: HashMap<_, _> = rows.into_iter().map(|row| (row.pid, row)).collect();
        let mut out = Vec::with_capacity(self.live.len());
        for live in self.live.values_mut() {
            let candidate = counters.remove(&live.identity.pid).filter(|row| {
                if let Some(sequence) = row.sequence_number {
                    return sequence == live.identity.sequence;
                }
                live.created == Some(row.create_time)
                    || counted.is_some_and(|(start, end)| {
                        live.first_seen <= start && roster_started >= end
                    })
            });
            let mut row = if let Some(row) = candidate {
                live.pending = None;
                live.created = Some(row.create_time);
                live.counters = Some((row.clone(), counted.map_or(roster_finished, |(_, at)| at)));
                row
            } else if let Some((row, at)) = &live.counters {
                let mut row = row.clone();
                row.sample_reused = true;
                row.stale = super::cache_expired(Some(*at), Instant::now());
                row
            } else {
                live.pending
                    .get_or_insert_with(|| placeholder(&live.identity))
                    .clone()
            };
            let key = live.key.get_or_insert_with(|| {
                format!("{}:seq:{}", row.pid, live.identity.sequence).into()
            });
            row.instance_key = Some(key.clone());
            row.listed_by_roster = true;
            row.sequence_number = Some(live.identity.sequence);
            out.push(row);
        }
        out.sort_unstable_by_key(|row| row.pid);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn identity(pid: u32, sequence: u64) -> Identity {
        Identity {
            pid,
            parent: 4,
            sequence,
            name: "probe.exe".into(),
        }
    }
    fn frame(at: Instant, entries: Vec<Identity>) -> Frame {
        Frame {
            started: at,
            finished: at + Duration::from_micros(1),
            entries,
        }
    }
    fn empty() -> Roster {
        Roster {
            reader: None,
            live: HashMap::new(),
            frame: None,
        }
    }
    fn pending(id: &Identity) -> ProcessRow {
        ProcessRow {
            pid: id.pid,
            ppid: id.parent,
            name: id.name.clone(),
            create_time: -1,
            counters_pending: true,
            sample_reused: true,
            ..Default::default()
        }
    }
    fn counted(pid: u32, sequence: Option<u64>, created: i64) -> ProcessRow {
        ProcessRow {
            pid,
            name: "probe.exe".into(),
            create_time: created,
            sequence_number: sequence,
            cpu: 42.,
            private_ws: 4096,
            ..Default::default()
        }
    }

    #[test]
    fn fresh_roster_updates_lifetimes_while_old_counters_wait() {
        let start = Instant::now();
        let mut roster = empty();
        roster.observe(frame(start, vec![identity(10, 1)]));
        let span = Some((start, start + Duration::from_millis(10)));
        let row = counted(10, Some(1), 100);
        let first = roster.merge(vec![row.clone()], span, pending);
        let original_key = first[0].instance_key.clone();
        roster.observe(frame(
            start + Duration::from_secs(1),
            vec![identity(10, 1), identity(20, 2)],
        ));
        let mut reused = row.clone();
        reused.sample_reused = true;
        let second = roster.merge(vec![reused], span, pending);
        assert_eq!(second.len(), 2);
        assert_eq!(second[0].instance_key, original_key);
        assert_eq!(second[0].cpu, 42.);
        assert!(second[1].counters_pending);
        let new_key = second[1].instance_key.clone();
        roster.observe(frame(start + Duration::from_secs(2), vec![identity(20, 2)]));
        // A late full reply cannot bring the exited PID back.
        let third = roster.merge(vec![row], span, pending);
        assert_eq!(third.len(), 1);
        assert_eq!(third[0].pid, 20);
        // Receiving counters preserves selection/history identity.
        let fourth = roster.merge(vec![counted(20, Some(2), 200)], span, pending);
        assert!(!fourth[0].counters_pending);
        assert_eq!(fourth[0].instance_key, new_key);
    }

    #[test]
    fn reused_pid_never_inherits_old_counts_or_selection() {
        let at = Instant::now();
        let mut roster = empty();
        let span = Some((at, at));
        roster.observe(frame(at, vec![identity(10, 1)]));
        let old = roster.merge(vec![counted(10, Some(1), 100)], span, pending);
        roster.observe(frame(at + Duration::from_secs(1), vec![identity(10, 2)]));
        let replacement = roster.merge(vec![counted(10, Some(1), 100)], span, pending);
        assert!(replacement[0].counters_pending);
        assert_ne!(replacement[0].instance_key, old[0].instance_key);
        assert_eq!(replacement[0].create_time, -1);
        assert_eq!(replacement[0].cpu, 0.);
    }

    #[test]
    fn protected_process_needs_a_bracket_before_attaching_unsequenced_counters() {
        let at = Instant::now();
        let mut roster = empty();
        roster.observe(frame(at, vec![identity(10, 5)]));
        let old = roster.merge(
            vec![counted(10, None, 100)],
            Some((at - Duration::from_secs(2), at - Duration::from_secs(1))),
            pending,
        );
        assert!(old[0].counters_pending);
        roster.observe(frame(at + Duration::from_secs(3), vec![identity(10, 5)]));
        let ready = roster.merge(
            vec![counted(10, None, 500)],
            Some((at + Duration::from_secs(1), at + Duration::from_secs(2))),
            pending,
        );
        assert!(!ready[0].counters_pending);
        assert_eq!(ready[0].instance_key, old[0].instance_key);
        // The same PID with a later sequence cannot reuse the established binding.
        roster.observe(frame(at + Duration::from_secs(4), vec![identity(10, 6)]));
        let reused = roster.merge(
            vec![counted(10, None, 500)],
            Some((at + Duration::from_secs(1), at + Duration::from_secs(2))),
            pending,
        );
        assert!(reused[0].counters_pending);
    }

    #[test]
    fn unavailable_basic_api_is_probed_once_and_full_rows_survive() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let mut reader = Reader::spawn(move || {
            seen.fetch_add(1, Ordering::Relaxed);
            Err(NTSTATUS(0xc000_0003u32 as i32))
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !reader.disabled {
            assert!(Instant::now() < deadline);
            reader.read();
        }
        for _ in 0..4 {
            assert!(reader.read().is_none());
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let mut roster = empty();
        let rows = roster.merge(vec![counted(10, None, 100)], None, pending);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cpu, 42.);
    }

    #[test]
    fn blocked_counter_query_leaves_identity_collection_available() {
        let (send, receive) = mpsc::channel();
        let mut full = super::super::query::ProcessQuery::spawn(move |_| {
            let _ = receive.recv_timeout(Duration::from_secs(2));
            Ok(0)
        })
        .unwrap();
        assert!(full.read(1).is_none());
        let mut reader = Reader::spawn(|| Ok(vec![identity(10, 1)])).unwrap();
        let end = Instant::now() + Duration::from_secs(1);
        let frame = loop {
            if let Some(frame) = reader.read() {
                break frame;
            }
            assert!(Instant::now() < end);
        };
        assert_eq!(frame.entries[0].pid, 10);
        assert!(full.read(1).is_none());
        send.send(()).unwrap();
    }

    #[test]
    fn blocked_roster_does_not_block_the_counter_worker() {
        let (send, receive) = mpsc::channel();
        let mut reader = Reader::spawn(move || {
            let _ = receive.recv_timeout(Duration::from_secs(2));
            Ok(vec![])
        })
        .unwrap();
        assert!(reader.read().is_none());
        let mut full = super::super::query::ProcessQuery::spawn(|_| Ok(0)).unwrap();
        assert!(full.read(1).is_some());
        send.send(()).unwrap();
    }

    #[test]
    fn native_names_must_stay_inside_their_record() {
        let mut buffer = AlignedBuf::with_capacity(56);
        let ptr = buffer.as_mut_ptr().cast::<SystemBasicProcessInformation>();
        unsafe {
            (*ptr).pid = HANDLE(123usize as *mut _);
            (*ptr).name.length = 2;
            (*ptr).name.maximum_length = 2;
            (*ptr).name.buffer = buffer.as_mut_ptr().cast::<u8>().add(48).cast();
            *(*ptr).name.buffer = b'a' as u16;
        }
        assert_eq!(&*decode(&buffer, 56).unwrap()[0].name, "a");
        unsafe {
            (*ptr).name.buffer = buffer.as_mut_ptr().cast::<u8>().add(56).cast();
        }
        assert!(decode(&buffer, 56).is_none());
        unsafe {
            (*ptr).next = 8;
        }
        assert!(decode(&buffer, 56).is_none());
    }
}
