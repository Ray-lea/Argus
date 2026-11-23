//! Fixed-size rolling records for process observation.
use super::{ProgramIdentity, WatchError, WatchPoint, WatchState};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

pub const FILE_NAME: &str = "program-watch.v1.bin";
const MAGIC: &[u8; 8] = b"ARGWCH01";
const HEADER: u64 = 1 << 20;
const RECORD: u64 = 144;
pub const CAPACITY: usize = 131072;
pub const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
pub const MAX_BYTES: u64 = HEADER + CAPACITY as u64 * RECORD;
const PRESENT_MASK: u16 = (1 << 14) - 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Metadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<ProgramIdentity>,
    pub name: String,
}

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub seq: u64,
    pub pid: u32,
    pub created: i64,
    pub point: WatchPoint,
}

pub(super) struct Store {
    file: File,
    pub meta: Metadata,
    pub entries: VecDeque<Entry>,
    next: u64,
    pub damaged: bool,
}

// Integrity detection for torn records, not authentication of local files.
fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(2166136261, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(16777619)
    })
}

fn state_byte(state: WatchState) -> u8 {
    match state {
        WatchState::Recording => 1,
        WatchState::Gap => 2,
        WatchState::Exited => 3,
        WatchState::Stopped => 4,
        WatchState::Interrupted => 5,
        WatchState::Limit => 6,
    }
}

impl Entry {
    fn encode(&self) -> [u8; RECORD as usize] {
        let p = &self.point;
        let mut bytes = [0; RECORD as usize];
        bytes[0..8].copy_from_slice(&self.seq.to_le_bytes());
        bytes[8..16].copy_from_slice(&p.ts.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.pid.to_le_bytes());
        bytes[20..28].copy_from_slice(&self.created.to_le_bytes());
        bytes[28..36].copy_from_slice(&p.memory.unwrap_or_default().to_le_bytes());
        bytes[36..44].copy_from_slice(&p.commit.unwrap_or_default().to_le_bytes());
        bytes[44..52].copy_from_slice(&p.working_set.unwrap_or_default().to_le_bytes());
        bytes[52..56].copy_from_slice(&p.cpu.unwrap_or_default().to_le_bytes());
        bytes[56..60].copy_from_slice(&p.gpu.unwrap_or_default().to_le_bytes());
        bytes[60..68].copy_from_slice(&p.gpu_memory.unwrap_or_default().to_le_bytes());
        bytes[68..76].copy_from_slice(&p.disk_read.unwrap_or_default().to_le_bytes());
        bytes[76..84].copy_from_slice(&p.disk_write.unwrap_or_default().to_le_bytes());
        bytes[84..92].copy_from_slice(&p.io_read.unwrap_or_default().to_le_bytes());
        bytes[92..100].copy_from_slice(&p.io_write.unwrap_or_default().to_le_bytes());
        bytes[100..108].copy_from_slice(&p.net_received.unwrap_or_default().to_le_bytes());
        bytes[108..116].copy_from_slice(&p.net_sent.unwrap_or_default().to_le_bytes());
        bytes[116..120].copy_from_slice(&p.threads.unwrap_or_default().to_le_bytes());
        bytes[120..124].copy_from_slice(&p.handles.unwrap_or_default().to_le_bytes());
        bytes[124] = state_byte(p.state);
        let present: u16 = u16::from(p.memory.is_some())
            | (u16::from(p.commit.is_some()) << 1)
            | (u16::from(p.working_set.is_some()) << 2)
            | (u16::from(p.cpu.is_some()) << 3)
            | (u16::from(p.gpu.is_some()) << 4)
            | (u16::from(p.gpu_memory.is_some()) << 5)
            | (u16::from(p.disk_read.is_some()) << 6)
            | (u16::from(p.disk_write.is_some()) << 7)
            | (u16::from(p.io_read.is_some()) << 8)
            | (u16::from(p.io_write.is_some()) << 9)
            | (u16::from(p.net_received.is_some()) << 10)
            | (u16::from(p.net_sent.is_some()) << 11)
            | (u16::from(p.threads.is_some()) << 12)
            | (u16::from(p.handles.is_some()) << 13);
        bytes[126..128].copy_from_slice(&present.to_le_bytes());
        let sum = checksum(&bytes[..128]);
        bytes[128..132].copy_from_slice(&sum.to_le_bytes());
        bytes[136..144].copy_from_slice(&(!self.seq).to_le_bytes());
        bytes
    }

    fn decode(bytes: &[u8; RECORD as usize], slot: usize) -> Option<Self> {
        let seq = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
        let ts = i64::from_le_bytes(bytes[8..16].try_into().ok()?);
        let pid = u32::from_le_bytes(bytes[16..20].try_into().ok()?);
        let created = i64::from_le_bytes(bytes[20..28].try_into().ok()?);
        let present = u16::from_le_bytes(bytes[126..128].try_into().ok()?);
        if seq == 0
            || (seq - 1) % CAPACITY as u64 != slot as u64
            || !seq != u64::from_le_bytes(bytes[136..144].try_into().ok()?)
            || checksum(&bytes[..128]) != u32::from_le_bytes(bytes[128..132].try_into().ok()?)
            || pid == 0
            || created <= 0
            || ts <= 0
            || bytes[125] != 0
            || bytes[132..136] != [0; 4]
            || present & !PRESENT_MASK != 0
        {
            return None;
        }
        let state = match bytes[124] {
            1 => WatchState::Recording,
            2 => WatchState::Gap,
            3 => WatchState::Exited,
            4 => WatchState::Stopped,
            5 => WatchState::Interrupted,
            6 => WatchState::Limit,
            _ => return None,
        };
        if state != WatchState::Recording && present != 0 {
            return None;
        }
        let u64_at = |bit: u16, range: std::ops::Range<usize>| {
            (present & (1 << bit) != 0)
                .then(|| u64::from_le_bytes(bytes[range].try_into().unwrap()))
        };
        let u32_at = |bit: u16, range: std::ops::Range<usize>| {
            (present & (1 << bit) != 0)
                .then(|| u32::from_le_bytes(bytes[range].try_into().unwrap()))
        };
        let f32_at = |bit: u16, range: std::ops::Range<usize>| -> Option<Option<f32>> {
            if present & (1 << bit) == 0 {
                return Some(None);
            }
            let value = f32::from_le_bytes(bytes[range].try_into().unwrap());
            (value.is_finite() && value >= 0.).then_some(Some(value))
        };
        let cpu = f32_at(3, 52..56)?;
        let gpu = f32_at(4, 56..60)?;
        Some(Self {
            seq,
            pid,
            created,
            point: WatchPoint {
                ts,
                state,
                memory: u64_at(0, 28..36),
                commit: u64_at(1, 36..44),
                working_set: u64_at(2, 44..52),
                cpu,
                gpu,
                gpu_memory: u64_at(5, 60..68),
                disk_read: u64_at(6, 68..76),
                disk_write: u64_at(7, 76..84),
                io_read: u64_at(8, 84..92),
                io_write: u64_at(9, 92..100),
                net_received: u64_at(10, 100..108),
                net_sent: u64_at(11, 108..116),
                threads: u32_at(12, 116..120),
                handles: u32_at(13, 120..124),
            },
        })
    }
}

impl Store {
    pub fn create(path: &Path, meta: Metadata) -> Result<Self, WatchError> {
        let encoded = serde_json::to_vec(&meta).map_err(|_| WatchError::Invalid)?;
        if meta
            .program
            .as_ref()
            .is_some_and(|program| !program.valid())
            || meta.name.is_empty()
            || meta.name.len() > 1024
            || encoded.len() as u64 > HEADER - 16
        {
            return Err(WatchError::Invalid);
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| WatchError::Storage)?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(MAGIC)?;
            file.write_all(&(encoded.len() as u32).to_le_bytes())?;
            file.write_all(&checksum(&encoded).to_le_bytes())?;
            file.write_all(&encoded)?;
            file.set_len(HEADER)?;
            file.flush()
        })();
        if result.is_err() {
            drop(file);
            let _ = std::fs::remove_file(path);
            return Err(WatchError::Storage);
        }
        Ok(Self {
            file,
            meta,
            entries: VecDeque::new(),
            next: 1,
            damaged: false,
        })
    }

    pub fn open(path: &Path, now: i64) -> Result<Self, WatchError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| WatchError::Storage)?;
        let size = file.metadata().map_err(|_| WatchError::Storage)?.len();
        if !(HEADER..=MAX_BYTES).contains(&size) {
            return Err(WatchError::Invalid);
        }
        let mut header = [0u8; 16];
        file.read_exact(&mut header)
            .map_err(|_| WatchError::Storage)?;
        let len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
        if &header[..8] != MAGIC || len as u64 > HEADER - 16 {
            return Err(WatchError::Invalid);
        }
        let mut data = vec![0; len];
        file.read_exact(&mut data)
            .map_err(|_| WatchError::Storage)?;
        if checksum(&data) != u32::from_le_bytes(header[12..16].try_into().unwrap()) {
            return Err(WatchError::Invalid);
        }
        let meta: Metadata = serde_json::from_slice(&data).map_err(|_| WatchError::Invalid)?;
        if meta
            .program
            .as_ref()
            .is_some_and(|program| !program.valid())
            || meta.name.is_empty()
            || meta.name.len() > 1024
        {
            return Err(WatchError::Invalid);
        }
        file.seek(SeekFrom::Start(HEADER))
            .map_err(|_| WatchError::Storage)?;
        let mut raw = Vec::new();
        file.read_to_end(&mut raw)
            .map_err(|_| WatchError::Storage)?;
        let mut entries = Vec::new();
        let mut damaged = raw.len() % RECORD as usize != 0;
        let mut invalid_slots = Vec::new();
        for (slot, bytes) in raw.as_chunks::<{ RECORD as usize }>().0.iter().enumerate() {
            if bytes.iter().all(|b| *b == 0) {
                continue;
            }
            match Entry::decode(bytes, slot) {
                Some(entry) => entries.push(entry),
                None => {
                    damaged = true;
                    invalid_slots.push(slot);
                }
            }
        }
        entries.sort_unstable_by_key(|entry| entry.seq);
        // Only the newest ring generation can survive a partial overwrite.
        let next = entries
            .last()
            .map_or(Some(1), |entry| entry.seq.checked_add(1))
            .ok_or(WatchError::Invalid)?;
        entries.retain(|entry| {
            let keep = next - entry.seq <= CAPACITY as u64;
            if !keep {
                invalid_slots.push(((entry.seq - 1) % CAPACITY as u64) as usize);
            }
            keep
        });
        if entries.windows(2).any(|p| p[0].point.ts > p[1].point.ts) {
            return Err(WatchError::Invalid);
        }
        // Remove unreadable values as well: expiry must not leave damaged payloads indefinitely.
        for slot in invalid_slots {
            file.seek(SeekFrom::Start(HEADER + slot as u64 * RECORD))
                .and_then(|_| file.write_all(&[0; RECORD as usize]))
                .map_err(|_| WatchError::Storage)?;
        }
        if raw.len() % RECORD as usize != 0 {
            file.set_len(HEADER + (raw.len() as u64 / RECORD) * RECORD)
                .map_err(|_| WatchError::Storage)?;
        }
        let mut store = Self {
            file,
            meta,
            entries: entries.into(),
            next,
            damaged,
        };
        store.prune(now)?;
        Ok(store)
    }

    fn offset(seq: u64) -> u64 {
        HEADER + ((seq - 1) % CAPACITY as u64) * RECORD
    }

    pub fn prune(&mut self, now: i64) -> Result<(), WatchError> {
        let cutoff = now.saturating_sub(RETENTION_MS);
        while self
            .entries
            .front()
            .is_some_and(|entry| entry.point.ts < cutoff)
        {
            let entry = self.entries.front().unwrap();
            self.file
                .seek(SeekFrom::Start(Self::offset(entry.seq)))
                .and_then(|_| self.file.write_all(&[0; RECORD as usize]))
                .map_err(|_| WatchError::Storage)?;
            self.entries.pop_front();
        }
        self.file.flush().map_err(|_| WatchError::Storage)
    }

    pub fn push(&mut self, pid: u32, created: i64, point: WatchPoint) -> Result<(), WatchError> {
        if pid == 0
            || created <= 0
            || point.ts <= 0
            || self
                .entries
                .back()
                .is_some_and(|last| point.ts < last.point.ts)
            || (point.state != WatchState::Recording && point.has_any_metric())
            || point.cpu.is_some_and(|v| !v.is_finite() || v < 0.)
            || point.gpu.is_some_and(|v| !v.is_finite() || v < 0.)
        {
            return Err(WatchError::Invalid);
        }
        let next = self.next.checked_add(1).ok_or(WatchError::Invalid)?;
        self.prune(point.ts)?;
        let entry = Entry {
            seq: self.next,
            pid,
            created,
            point,
        };
        self.file
            .seek(SeekFrom::Start(Self::offset(entry.seq)))
            .and_then(|_| self.file.write_all(&entry.encode()))
            .map_err(|_| WatchError::Storage)?;
        self.next = next;
        while self
            .entries
            .front()
            .is_some_and(|old| next - old.seq > CAPACITY as u64)
        {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), WatchError> {
        self.file.flush().map_err(|_| WatchError::Storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn meta() -> Metadata {
        Metadata {
            name: "same.exe".into(),
            program: Some(ProgramIdentity {
                native_path: "\\Device\\disk\\one\\same.exe".into(),
                display_path: "C:\\one\\same.exe".into(),
            }),
        }
    }
    fn point(ts: i64, value: u64) -> WatchPoint {
        WatchPoint {
            memory: Some(value),
            commit: Some(value + 1),
            working_set: Some(value + 2),
            cpu: Some(12.5),
            gpu: Some(4.25),
            gpu_memory: Some(value + 3),
            disk_read: Some(value + 4),
            disk_write: Some(value + 5),
            io_read: Some(value + 6),
            io_write: Some(value + 7),
            net_received: Some(value + 8),
            net_sent: Some(value + 9),
            threads: Some(16),
            handles: Some(256),
            ..WatchPoint::marker(ts, WatchState::Recording)
        }
    }
    fn path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "argus-watch-store-{}-{tag}.bin",
            std::process::id()
        ))
    }

    #[test]
    fn rolling_quota_and_runs_survive_reopen_without_splicing() {
        let path = path("quota");
        let start = super::super::now_ms();
        let began = std::time::Instant::now();
        let mut store = Store::create(&path, meta()).unwrap();
        for i in 0..CAPACITY + 3 {
            store
                .push(
                    if i % 2 == 0 { 17 } else { 18 },
                    100 + (i % 2) as i64,
                    point(start + i as i64, i as u64),
                )
                .unwrap();
        }
        store.flush().unwrap();
        assert_eq!(store.entries.len(), CAPACITY);
        assert_eq!(store.file.metadata().unwrap().len(), MAX_BYTES);
        let writer_us = began.elapsed().as_micros();
        drop(store);
        let began = std::time::Instant::now();
        let store = Store::open(&path, start + CAPACITY as i64).unwrap();
        println!(
            "ring_records={} bytes={} writer_us={writer_us} reopen_us={}",
            CAPACITY,
            MAX_BYTES,
            began.elapsed().as_micros()
        );
        assert_eq!(store.entries.len(), CAPACITY);
        assert_eq!(store.entries.front().unwrap().point.memory, Some(3));
        let last = &store.entries.back().unwrap().point;
        let expected = (CAPACITY + 2) as u64;
        assert_eq!(last.memory, Some(expected));
        assert_eq!(last.commit, Some(expected + 1));
        assert_eq!(last.working_set, Some(expected + 2));
        assert_eq!(last.cpu, Some(12.5));
        assert_eq!(last.gpu, Some(4.25));
        assert_eq!(last.gpu_memory, Some(expected + 3));
        assert_eq!(last.disk_read, Some(expected + 4));
        assert_eq!(last.disk_write, Some(expected + 5));
        assert_eq!(last.io_read, Some(expected + 6));
        assert_eq!(last.io_write, Some(expected + 7));
        assert_eq!(last.net_received, Some(expected + 8));
        assert_eq!(last.net_sent, Some(expected + 9));
        assert_eq!(last.threads, Some(16));
        assert_eq!(last.handles, Some(256));
        assert_eq!(store.entries.front().unwrap().pid, 18);
        assert!(!store.damaged);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn expiry_erases_values_and_torn_record_does_not_become_a_measurement() {
        let path = path("expiry");
        let start = super::super::now_ms();
        let mut store = Store::create(&path, meta()).unwrap();
        store.push(17, 100, point(start, 12345)).unwrap();
        store
            .push(17, 100, point(start + RETENTION_MS + 1, 23456))
            .unwrap();
        assert_eq!(store.entries.len(), 1);
        let mut raw = std::fs::read(&path).unwrap();
        assert!(
            raw[HEADER as usize..HEADER as usize + RECORD as usize]
                .iter()
                .all(|b| *b == 0)
        );
        drop(store);
        raw[HEADER as usize + RECORD as usize + 30] ^= 1;
        std::fs::write(&path, raw).unwrap();
        let store = Store::open(&path, start + RETENTION_MS + 1).unwrap();
        assert!(store.entries.is_empty());
        assert!(store.damaged);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn unknown_header_and_oversize_are_rejected_without_truncation() {
        let path = path("invalid");
        let store = Store::create(&path, meta()).unwrap();
        drop(store);
        let mut raw = std::fs::read(&path).unwrap();
        raw[0] ^= 1;
        std::fs::write(&path, &raw).unwrap();
        assert!(matches!(
            Store::open(&path, super::super::now_ms()),
            Err(WatchError::Invalid)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), raw);
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        drop(file);
        assert!(matches!(
            Store::open(&path, super::super::now_ms()),
            Err(WatchError::Invalid)
        ));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), MAX_BYTES + 1);
        std::fs::remove_file(path).unwrap();
    }
}
