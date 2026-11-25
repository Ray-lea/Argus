//! Append-only system and process history, partitioned by UTC day. The live
//! sampler publishes ten minutes of unrounded system samples independently of
//! the five-second frames. Times and spans are milliseconds. System readings
//! average finite samples independently; CPU peak is their maximum. Process
//! working set, dedicated GPU memory, threads and handles are frame maxima.
//!
//! Sealed days use ARGH v4. The minute packets keep independent system and
//! process blocks, the observed intervals, and exact gap evidence. See the
//! minute module for the reader and atomic writer.
//!
//! # Version 3
//!
//! `YYYY-MM-DD.hist` begins with `ARGH\x03`. Each record is a u8 tag, u32
//! payload length and payload. Fixed-width numbers are little endian.
//!
//! * 3, channel definition: UTF-8 JSON pair [local index, HistoryChannel].
//!   A new index appends a channel; a repeated index changes its display label.
//! * 4, process identity: UTF-8 JSON pair [local index, {name,pid,created}].
//!   Creation time is Unix ms; nonpositive denotes unavailable.
//! * 5, session: kind u8 (1 start, 2 disabled, 3 exit), wall time, estimated
//!   boot time and unbiased interrupt time, each i64 ms. Day rollover repeats
//!   the session evidence. Boot estimates within two seconds are the same boot.
//! * 6, frame: timestamp and unbiased clock i64, span u32, process offset u64,
//!   process length u32, then channel values in index order. Byte quantities use
//!   f64, other units f32. NaN means a known channel with no reading. Negative
//!   infinity means the device was absent. Later catalog entries have no slot in
//!   earlier frames. The count and widths follow from the record and catalog.
//! * 7, quarter-hour system acceleration record: first, last and CPU peak frame
//!   i64, frame count u32 and highest peak f64, followed per channel by count
//!   u32 and sum, minimum and maximum f64. Partial groups fall back to frames.
//! * 8, channel presence: a bitmap in catalog order, low bit first, effective
//!   at the next frame. A new record is written on device arrival or removal.
//!
//! `YYYY-MM-DD.proc` contains byte ranges committed by tag 6. Each entry has a
//! ULEB128 identity index (bit 16 omits network, bit 17 omits threads and handles), flags u8, nonzero CPU/GPU f32 (bits 0/1), working
//! set, I/O rate, network rate, threads and handles, then optional disk read,
//! disk write, GPU memory and power (bits 2..5). Bit 6 stores nonzero power as
//! f32. A present zero power has no payload. Bit 7 adds a four-rate mask and
//! the divisors of fractional I/O, network, disk-read and disk-write means.
//! Their integer payload holds the numerator. Integer fields use ULEB128.
//! Working set and GPU memory encode page count plus one for aligned values,
//! or zero followed by the exact byte count. The writer flushes process bytes
//! before committing the main frame. Reopening truncates incomplete records
//! and unreferenced process tails to the last matching committed boundary.
//!
//! `YYYY-MM-DD.roll` accelerates process tables. Tag 1 with a u32 length
//! contains first timestamp, exclusive end and covered milliseconds (i64),
//! then pairs of identity index and mergeable Totals. These retain weighted
//! sums, maxima and observed durations for nullable fields and each trend.
//! Writers build summaries during regular frame appends. Queries combine whole
//! groups only inside one trend bin and decode detail at the edges. A missing
//! summary file or torn final summary falls back to the underlying frames. Process
//! summaries span up to five minutes.
//!
//! `.idx` contains optional append-only checkpoints of committed main-file byte
//! ranges. Each tag-1 packet begins with the start and end u32 offsets, followed
//! by copies of metadata and tag-9 runs. A run stores first/last clock stamps
//! (wall i64, unbiased i64, span u32), process end u64, and pairs of u32 UTC-day
//! milliseconds and main-file offsets. Gaps split runs. Queries load these compact
//! addresses and system summaries, then index the uncheckpointed main-file tail.
//! Missing or incomplete checkpoints fall back to the primary day file.
//!
//! NICs use the Windows interface GUID. Disks use the PnP interface path
//! or descriptor serial; GPUs use the PnP instance or PCI address. The last fallback
//! is an unambiguous model (and capacity for disks). Without a distinguishable
//! identity only the system total is recorded. Disk/GPU identity strings use
//! case-folded FNV-1a 64. Board fans use the controller channel, batteries are
//! aggregated across packs. All device data comes from the collected snapshot.
//!
//! Day files of other versions (the v1/v2 of releases before 0.6) are not
//! read, and a day begun in one starts over in v3. Legacy queries project v3
//! back into the original JSON structures.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ffi::OsStr;
use std::fs;
#[cfg(not(test))]
use std::fs::{File, metadata};
#[cfg(test)]
mod test_files;
use std::fs::OpenOptions;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(test)]
use test_files::{File, metadata};

use arc_swap::ArcSwap;
use serde::Serialize;

use crate::collector::Snapshot;

/// Length of one frame.
pub const FRAME_MS: i64 = 5_000;
mod events;
pub use events::{HistoryEvent, HistoryEvents, HistoryTop, ResourceKind, events};
mod options;
pub use options::*;
mod directory;
pub(crate) use directory::owned_directory;
pub use directory::{prepare_directory, select_directory, validate_directory};
mod maintenance;
pub use maintenance::{maintain, maintain_at, maintain_until};
mod migration;
pub use migration::move_records;
mod minute;
use minute::{MINUTE_MAGIC, MINUTE_MS, MINUTE_VERSION, Minute};

/// The day file format written and read: v3 (frames, with process rows in
/// a `.proc` sidecar). Sealed days may be v4 (`minute::MINUTE_MAGIC`).
/// Files of other versions, such as the v1/v2 days of releases before 0.6,
/// are not read: as if nothing had been recorded that day.
const MAGIC: &[u8; 5] = b"ARGH\x03";
/// The version byte of `MAGIC`, which also tells a raw day (`Index::version`)
/// from a sealed one (`MINUTE_VERSION`).
const VERSION: u8 = MAGIC[4];
const DAY_MS: i64 = 86_400_000;
// A normal day holds 17,280 frames. Reject oversized files before allocating or repairing.
const MAX_FILE_BYTES: usize = 128 << 20;
const MAX_METADATA_BYTES: usize = 16 << 10;
const MAX_CHANNELS: usize = 512;
const MAX_INSTANCES: usize = 65_536;
const MAX_FRAME_ROWS: usize = MAX_INSTANCES;
// Ten minutes at the sampler's shortest supported cadence (250 ms), inclusive.
pub(crate) const MAX_RECENT: usize = 2401;
pub const RECENT_MS: i64 = 10 * 60_000;
const MAX_FRAMES: usize = 100_000;
const MAX_POINTS: usize = 4096;
const MAX_BUCKETS: usize = 100_000;
const MAX_QUERY_CELLS: usize = 2_000_000;

/// System totals for one frame (or one chart bucket).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    /// Unix ms: the frame's start, or a bucket's average.
    pub ts: i64,
    /// An actual frame, chosen as the first frame of a bucket. Unlike `ts`,
    /// this timestamp can always address its process table.
    pub frame_ts: i64,
    /// Actual frame containing the highest CPU sample; earliest frame on ties.
    pub cpu_peak_ts: i64,
    /// Average CPU, percent.
    pub cpu: f32,
    /// Highest CPU sample in the frame (or bucket).
    pub cpu_peak: f32,
    /// Busiest GPU engine, percent.
    pub gpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Bytes per second from available disk counters.
    pub disk: u64,
    /// Bytes per second, all adapters.
    pub net: u64,
    /// Battery charge, percent, averaged over the frame; None on machines
    /// without a battery (or in frames recorded while it was absent).
    pub battery: Option<f32>,
    /// Battery power flow, mW (positive while charging), averaged over the
    /// frame; None when no battery reports a rate.
    pub battery_rate: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryProcess {
    pub name: String,
    pub pid: u32,
    /// None in legacy history or when the instance could not be identified.
    pub create_time: Option<i64>,
    /// CPU, I/O, GPU and network are averages over the frame.
    pub cpu: f32,
    /// Maximum private working set in the frame, bytes.
    pub mem: u64,
    pub io: u64,
    pub gpu: f32,
    pub net: u64,
}

/// What the recorded history takes on disk.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryUsage {
    /// Size of all day files.
    pub bytes: u64,
    /// Unix ms of the oldest recorded frame.
    pub first: Option<i64>,
    pub days: usize,
    pub breakdown: Breakdown,
    pub limit: HistoryLimit,
    pub estimate: HistoryEstimate,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEstimate {
    pub bytes_per_day: u64,
}
#[derive(Clone, Copy, Default, Serialize, serde::Deserialize)]
struct Measurement {
    recording: Recording,
    frames: u64,
    bytes: u64,
}
impl Measurement {
    fn save(&self, dir: &Path, day: i64) {
        if self.frames > 0 {
            let path = day_path(dir, day).with_extension("measure");
            if let Err(e) = serde_json::to_vec(self)
                .map_err(io::Error::other)
                .and_then(|b| fs::write(&path, b))
            {
                crate::diagnostics::file_failure("history size measurement", &path, &e);
            }
        }
    }
    fn add(&mut self, recording: Recording, bytes: u64) {
        self.recording = recording;
        self.frames = self.frames.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
    }
}

/// One frame: the system and the processes that stood out at that moment.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMoment {
    pub system: HistoryPoint,
    pub processes: Vec<HistoryProcess>,
}

// ---- Recording -------------------------------------------------------------------

#[derive(Default)]
struct ProcAcc {
    disk_read: Mean,
    disk_write: Mean,
    power: Mean,
    gpu_mem: Option<u64>,
    threads: u32,
    handles: u32,
    measured: bool,
    missing: u32,
    name: Arc<str>,
    pid: u32,
    create_time: i64,
    cpu: f64,
    mem: u64,
    io: f64,
    gpu: f64,
    net: f64,
}

impl ProcAcc {
    fn divisor(&self, samples: u32) -> f64 {
        samples.saturating_sub(self.missing).max(1) as f64
    }
}

#[derive(Default)]
struct Window {
    /// How many processes the programs that run the most had in the last
    /// sample, and how many programs it had (`programs_of`).
    programs: Vec<(Arc<str>, u32)>,
    program_count: u32,
    values: Vec<Mean>,
    present: Vec<bool>,
    awake: i64,
    boot: i64,
    last: i64,
    interval: i64,
    start: i64,
    samples: u32,
    cpu_peak: f32,
    procs: HashMap<Arc<str>, ProcAcc>,
}

struct DayFile {
    recorded_until: Option<i64>,
    day: i64,
    out: BufWriter<File>,
    writer: DayWriter,
    _owner: fs::File,
}

pub struct HistoryRecorder {
    dir: PathBuf,
    file: Option<DayFile>,
    window: Window,
    catalog: Catalog,
    live: Live,
    shared: SharedLive,
    boot: Option<i64>,
    clock_is_live: bool,
    stopped: bool,
    recording: Recording,
    pending_recording: Option<Recording>,
    measurement: Measurement,
}

impl HistoryRecorder {
    pub fn active_day(&self) -> Option<i64> {
        self.file.as_ref().map(|f| f.day)
    }
    pub fn configure(&mut self, recording: Recording) {
        self.pending_recording = (self.recording != recording).then_some(recording);
    }
    pub fn directory(&self) -> &Path {
        &self.dir
    }
    pub fn new(dir: PathBuf) -> Self {
        let shared = SharedLive::default();
        ACTIVE.rcu(|active| {
            let mut entries = (**active).clone();
            entries.insert(dir.clone(), shared.clone());
            entries
        });
        Self {
            shared,
            live: Live::default(),
            catalog: Catalog::default(),
            boot: None,
            clock_is_live: false,
            stopped: false,
            recording: Recording::default(),
            pending_recording: None,
            measurement: Measurement::default(),
            dir,
            file: None,
            window: Window::default(),
        }
    }

    /// Deletes everything recorded so far, the frame in progress included;
    /// recording carries on into a new file.
    pub fn clear(&mut self) -> io::Result<()> {
        self.measurement = Measurement::default();
        // Close the day file before deleting it.
        self.file = None;
        self.window = Window::default();
        self.boot = None;
        self.live = Live::default();
        self.catalog = Catalog::default();
        self.shared.store(Arc::default());
        clear(&self.dir)
    }

    /// Adds a sample; writes the previous frame once a sample of the next one arrives.
    pub fn record(&mut self, s: &Snapshot) {
        // The on-disk name uses exactly four Gregorian year digits.
        if self.stopped || !(-719_528..2_932_897).contains(&s.timestamp.div_euclid(DAY_MS)) {
            return;
        }
        if self
            .live
            .samples
            .back()
            .is_some_and(|prev| s.timestamp < prev.ts)
        {
            return;
        }
        let changed = self.pending_recording.take();
        if let Some(recording) = changed {
            self.window.interval = s
                .timestamp
                .saturating_sub(self.window.last)
                .clamp(1, FRAME_MS);
            self.flush_window();
            self.recording = recording;
            self.measurement = Measurement::default();
        }
        let slot = align(s.timestamp, 0, FRAME_MS);
        if self.window.samples > 0 && slot != align(self.window.start, 0, FRAME_MS) {
            if slot == align(self.window.start, 0, FRAME_MS).saturating_add(FRAME_MS) {
                self.window.interval = slot - self.window.last;
            }
            self.flush_window();
        }
        let first_sample = changed.is_some() || self.live.samples.is_empty();
        let boot = *self
            .boot
            .get_or_insert_with(|| s.timestamp.saturating_sub(s.system.uptime_ms as i64));
        if self
            .file
            .as_ref()
            .is_none_or(|f| f.day != slot.div_euclid(DAY_MS))
            && let Err(error) = self.open(s.timestamp, boot, unbiased_ms())
        {
            crate::diagnostics::file_failure(
                "history open",
                &day_path(&self.dir, slot.div_euclid(DAY_MS)),
                &error,
            );
        }
        if self
            .file
            .as_ref()
            .and_then(|file| file.recorded_until)
            .is_some_and(|end| s.timestamp < end)
        {
            return;
        }
        if self
            .live
            .samples
            .back()
            .is_some_and(|p| p.ts.div_euclid(DAY_MS) != slot.div_euclid(DAY_MS))
        {
            self.catalog = Catalog::default();
        }
        let values = readings(&mut self.catalog, s, self.recording.groups);
        let interval = self
            .live
            .samples
            .back()
            .map(|p| s.timestamp.saturating_sub(p.ts))
            .filter(|&d| d > 0 && d <= 60_000)
            .unwrap_or(1000);
        let w = &mut self.window;
        if w.samples == 0 {
            w.start = if first_sample { s.timestamp } else { slot };
            w.awake = unbiased_ms()
                .checked_sub(s.timestamp - w.start)
                .unwrap_or(-1);
            w.boot = boot;
            self.live.first.get_or_insert(Stamp {
                ts: w.start,
                span: FRAME_MS,
                boot,
                awake: w.awake,
                stop: 0,
            });
            if let Some(prev) = self.live.previous
                && let Some(g) = gap(
                    prev,
                    Stamp {
                        ts: w.start,
                        span: FRAME_MS,
                        boot,
                        awake: w.awake,
                        stop: 0,
                    },
                )
            {
                self.live.gaps.push_back(g);
            }
        }
        w.interval = interval;
        w.last = s.timestamp;
        w.samples = w.samples.saturating_add(1);
        w.values.resize(values.len(), Mean::default());
        w.present.resize(values.len(), false);
        for (i, &v) in values.iter().enumerate() {
            w.values[i].add(v);
            w.present[i] |= v != f64::NEG_INFINITY;
        }
        w.cpu_peak = w.cpu_peak.max(s.system.cpu);
        (w.programs, w.program_count) = programs_of(&s.processes);
        if self.live.catalog.as_ref() != &self.catalog.rows {
            self.live.catalog = Arc::new(self.catalog.rows.clone());
        }
        self.live.samples.push_back(Arc::new(Sample {
            ts: s.timestamp,
            span: interval,
            slot: w.start,
            catalog: self.live.catalog.clone(),
            values,
        }));
        while self.live.samples.len() > MAX_RECENT
            || self
                .live
                .samples
                .front()
                .is_some_and(|p| p.ts < s.timestamp.saturating_sub(RECENT_MS))
        {
            self.live.evicted = true;
            self.live.samples.pop_front();
        }
        while self.live.frames.len() > MAX_RECENT
            || self
                .live
                .frames
                .front()
                .is_some_and(|&ts| ts < s.timestamp.saturating_sub(RECENT_MS + FRAME_MS))
        {
            self.live.frames.pop_front();
        }
        while self.live.gaps.len() > MAX_RECENT
            || self
                .live
                .gaps
                .front()
                .is_some_and(|g| g.to < s.timestamp.saturating_sub(RECENT_MS))
        {
            self.live.gaps.pop_front();
        }
        self.shared.store(Arc::new(self.live.clone()));
        for p in &s.processes {
            let key = p
                .instance_key
                .clone()
                .unwrap_or_else(|| format!("{}:{}", p.pid, p.create_time).into());
            if w.procs.len() >= MAX_INSTANCES && !w.procs.contains_key(&key) {
                continue;
            }
            let acc = w.procs.entry(key).or_insert_with(|| ProcAcc {
                name: p.name.clone(),
                pid: p.pid,
                ..Default::default()
            });
            if p.create_time > 0 {
                acc.create_time = p.create_time;
            }
            if p.stale || p.sample_reused || p.counters_pending {
                acc.missing = acc.missing.saturating_add(1);
                continue;
            }
            acc.measured = true;
            if let Some(v) = p.disk_read.filter(|_| self.recording.groups.process_disk) {
                acc.disk_read.add(v as f64);
            }
            if let Some(v) = p.disk_write.filter(|_| self.recording.groups.process_disk) {
                acc.disk_write.add(v as f64);
            }
            if let Some(v) = p.power {
                acc.power.add(v as f64);
            }
            if self.recording.groups.process_gpu {
                acc.gpu_mem = acc.gpu_mem.max(p.gpu_dedicated);
                acc.threads = acc.threads.max(p.threads);
                acc.handles = acc.handles.max(p.handles);
            }
            acc.cpu += p.cpu as f64;
            acc.mem = acc.mem.max(p.private_ws);
            acc.io += p.io_read as f64 + p.io_write as f64 + p.io_other as f64;
            acc.gpu += p.gpu as f64;
            if self.recording.groups.process_network {
                acc.net += p.net_sent as f64 + p.net_received as f64;
            }
        }
    }

    fn open(&mut self, ts: i64, boot: i64, awake: i64) -> io::Result<()> {
        let day = ts.div_euclid(DAY_MS);
        if self.file.as_ref().is_some_and(|f| f.day == day) {
            return Ok(());
        }
        if let Some(old) = self.file.take() {
            self.measurement.save(&self.dir, old.day);
            self.measurement = Measurement::default();
        }
        let mut file = open_day(&self.dir, day)?;
        forget(&self.dir);
        file.marker(1, ts, boot, awake)?;
        self.file = Some(file);
        Ok(())
    }
    /// Called by the sampler on enable, including before its first snapshot.
    pub fn start(dir: PathBuf) -> Self {
        let mut rec = Self::new(dir);
        let ts = unix_ms();
        let boot =
            ts - unsafe { windows::Win32::System::SystemInformation::GetTickCount64() } as i64;
        rec.boot = Some(boot);
        rec.clock_is_live = true;
        if let Err(e) = rec.open(ts, boot, unbiased_ms()) {
            crate::diagnostics::file_failure(
                "history start marker",
                &day_path(&rec.dir, ts.div_euclid(DAY_MS)),
                &e,
            );
        }
        rec
    }
    fn flush_window(&mut self) {
        if self.window.samples == 0 {
            return;
        }
        let w = std::mem::take(&mut self.window);
        match self.write(&w) {
            Ok(false) => (),
            Ok(true) => {
                self.live.frames.push_back(w.start);
                self.live.previous = Some(Stamp {
                    ts: w.start,
                    span: w.span(),
                    boot: w.boot,
                    awake: w.awake,
                    stop: 0,
                });
            }
            Err(error) => {
                self.file = None;
                crate::diagnostics::file_failure(
                    "history frame write",
                    &day_path(&self.dir, w.start.div_euclid(DAY_MS)),
                    &error,
                );
            }
        }
    }
    pub fn stop(&mut self, disabled: bool) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let ts = if self.clock_is_live {
            unix_ms()
        } else {
            self.window.last
        };
        if self.clock_is_live && self.window.samples > 0 {
            self.window.interval = self
                .window
                .interval
                .min(ts.saturating_sub(self.window.last))
                .max(1);
        }
        self.flush_window();
        if let Some(file) = self.file.as_mut() {
            let reason = if disabled { 2 } else { 3 };
            if let Err(e) = file.marker(reason, ts, self.boot.unwrap_or(0), unbiased_ms()) {
                crate::diagnostics::file_failure(
                    "history stop marker",
                    &day_path(&self.dir, file.day),
                    &e,
                );
            }
        }
        ACTIVE.rcu(|active| {
            let mut entries = (**active).clone();
            if entries
                .get(&self.dir)
                .is_some_and(|v| Arc::ptr_eq(v, &self.shared))
            {
                entries.remove(&self.dir);
            }
            entries
        });
        if let Some(file) = &self.file {
            self.measurement.save(&self.dir, file.day);
        }
        self.shared = SharedLive::default();
        self.live = Live::default();
        self.file = None;
    }
    fn write(&mut self, w: &Window) -> io::Result<bool> {
        self.open(w.start, w.boot, w.awake)?;
        let file = self.file.as_mut().ok_or_else(invalid)?;
        if file.recorded_until.is_some_and(|end| w.start < end) {
            return Ok(false);
        }
        let picked = select(
            w.procs.values().filter(|p| p.measured).collect(),
            self.recording.processes,
            |p, metric| {
                let n = p.divisor(w.samples);
                match metric {
                    0 => p.cpu / n,
                    1 => p.mem as f64,
                    2 => p.io / n,
                    3 => p.gpu / n,
                    4 => p.net / n,
                    _ => {
                        p.disk_read.get().unwrap_or_default()
                            + p.disk_write.get().unwrap_or_default()
                    }
                }
            },
            |p| (p.pid, p.create_time),
        );
        let size = file.writer.frame(
            &mut file.out,
            &self.catalog,
            w,
            &picked,
            self.recording.groups,
        )?;
        file.recorded_until = Some(w.start + w.span());
        self.measurement.add(self.recording, size);
        self.live.measurement = self.measurement;
        Ok(true)
    }
}

impl Drop for HistoryRecorder {
    fn drop(&mut self) {
        self.stop(false);
    }
}
/// How many programs a frame keeps the process count of (`ProgramCount`).
const PROGRAMS_KEPT: usize = 8;

/// How many processes each program runs, a program being an executable name
/// with case set aside (spelled as first seen): the most first, then by
/// name, as many as `PROGRAMS_KEPT`; and how many programs there are.
fn programs_of(processes: &[crate::process::ProcessRow]) -> (Vec<(Arc<str>, u32)>, u32) {
    let mut by_name: HashMap<String, (Arc<str>, u32)> = HashMap::with_capacity(processes.len());
    for p in processes {
        by_name
            .entry(p.name.to_lowercase())
            .or_insert_with(|| (p.name.clone(), 0))
            .1 += 1;
    }
    let total = by_name.len() as u32;
    let mut list: Vec<(String, Arc<str>, u32)> = by_name
        .into_iter()
        .map(|(key, (name, count))| (key, name, count))
        .collect();
    list.sort_unstable_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    list.truncate(PROGRAMS_KEPT);
    (
        list.into_iter()
            .map(|(_, name, count)| (name, count))
            .collect(),
        total,
    )
}

/// Each resource contributes its busiest distinct instances. An unknown/pending
/// observation is removed by the caller before ranking.
fn select<T: Copy>(
    mut rows: Vec<T>,
    cap: u32,
    score: impl Fn(T, usize) -> f64,
    identity: impl Fn(T) -> (u32, i64),
) -> Vec<T> {
    if cap == 0 {
        return rows;
    }
    let mut picked = Vec::new();
    let scale = if cap == 40 { 2 } else { 1 };
    for (metric, (quota, floor)) in [(5, 0.5), (5, 0.), (3, 0.), (2, 0.5), (2, 0.), (3, 0.)]
        .into_iter()
        .enumerate()
    {
        let order = |a: &T, b: &T| {
            score(*b, metric)
                .total_cmp(&score(*a, metric))
                .then_with(|| identity(*a).cmp(&identity(*b)))
        };
        let n = rows.len().min(quota * scale);
        if n < rows.len() {
            rows.select_nth_unstable_by(n, order);
        }
        rows[..n].sort_unstable_by(order);
        for &p in &rows[..n] {
            if score(p, metric) > floor && !picked.iter().any(|&q| identity(q) == identity(p)) {
                picked.push(p);
            }
        }
    }
    picked
}

fn day_path(dir: &Path, day: i64) -> PathBuf {
    dir.join(format!("{}.hist", day_name(day)))
}

/// `YYYY-MM-DD` of a day number (days since the Unix epoch), proleptic Gregorian.
fn day_name(day: i64) -> String {
    // Howard Hinnant's civil_from_days.
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Day number of a `YYYY-MM-DD` name (the inverse of `day_name`).
/// The day a file in the history folder holds, if it is a day file.
fn day_of(file_name: &OsStr) -> Option<i64> {
    day_number(file_name.to_str()?.strip_suffix(".hist")?)
}

fn owned_day(name: &OsStr) -> Option<i64> {
    let text = name.to_str()?;
    day_of(name).or_else(|| {
        day_number(
            text.strip_suffix(".proc")
                .or_else(|| text.strip_suffix(".roll"))
                .or_else(|| text.strip_suffix(".idx"))
                .or_else(|| text.strip_suffix(".measure"))
                .or_else(|| text.strip_suffix(".merge-tmp"))?,
        )
    })
}

fn day_number(name: &str) -> Option<i64> {
    if name.len() != 10
        || name.as_bytes()[4] != b'-'
        || name.as_bytes()[7] != b'-'
        || !name
            .bytes()
            .enumerate()
            .all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit())
    {
        return None;
    }
    let mut it = name.split('-');
    let (y, m, d): (i64, i64, i64) = (
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    );
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let day = era * 146_097 + doe - 719_468;
    (day_name(day) == name).then_some(day)
}

/// Deletes every day file in the folder. Nothing may be recording into it
/// (`HistoryRecorder::clear` closes its file first).
pub fn clear(dir: &Path) -> io::Result<()> {
    forget(dir);
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let _lease = maintenance::directory_lease(dir)?;
    for entry in entries {
        let entry = entry?;
        if owned_day(&entry.file_name()).is_some() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

// ---- Reading -----------------------------------------------------------------------

fn field<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..)?.first_chunk().copied()
}

/// Day files in the folder, oldest first.
fn days(dir: &Path) -> Vec<i64> {
    #[cfg(test)]
    if let Some(day) = test_files::day(dir) {
        return vec![day];
    }
    let mut days: Vec<i64> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| day_of(&e.file_name()))
        .collect();
    days.sort_unstable();
    days
}

/// System totals between `from` and `to` (Unix ms), averaged into at most
/// `buckets` points; empty buckets are left out, so gaps (Argus not running)
/// stay gaps. The CPU peak is the highest of the bucket.
pub fn legacy_series(dir: &Path, from: i64, to: i64, buckets: usize) -> Vec<HistoryPoint> {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    if to <= from || !(1..=MAX_POINTS).contains(&buckets) {
        return Vec::new();
    }
    #[derive(Default, Clone)]
    struct Acc {
        n: u64,
        ts: f64,
        frame_ts: i64,
        cpu_peak_ts: i64,
        cpu: f64,
        cpu_peak: f32,
        gpu: f64,
        mem_used: f64,
        mem_total: u64,
        disk: f64,
        net: f64,
        battery: f64,
        battery_n: u64,
        battery_rate: f64,
        battery_rate_n: u64,
    }
    let mut acc = vec![Acc::default(); buckets];
    let span = to.abs_diff(from) as f64;
    visit_days(dir, align(from, 0, MINUTE_MS), to, false, |d| {
        for i in d.range.clone() {
            let Some(row) = d.row(i) else { continue };
            let p = legacy_point(row.ts(), |k| finite(row.get(*d.idx.catalog.by_key.get(k)?)));
            if (p.ts >= from || (row.span() == MINUTE_MS && p.ts + MINUTE_MS > from)) && p.ts <= to
            {
                let i = ((p.ts.max(from).abs_diff(from) as f64 / span) * buckets as f64) as usize;
                let a = &mut acc[i.min(buckets - 1)];
                if a.n == 0 {
                    a.frame_ts = p.ts;
                }
                if a.n == 0
                    || p.cpu_peak > a.cpu_peak
                    || (p.cpu_peak == a.cpu_peak && p.ts < a.cpu_peak_ts)
                {
                    a.cpu_peak_ts = p.ts;
                }
                a.n += 1;
                a.ts += p.ts as f64;
                a.cpu += p.cpu as f64;
                a.cpu_peak = a.cpu_peak.max(p.cpu_peak);
                a.gpu += p.gpu as f64;
                a.mem_used += p.mem_used as f64;
                a.mem_total = p.mem_total;
                a.disk += p.disk as f64;
                a.net += p.net as f64;
                if let Some(v) = p.battery {
                    a.battery += v as f64;
                    a.battery_n += 1;
                }
                if let Some(v) = p.battery_rate {
                    a.battery_rate += v as f64;
                    a.battery_rate_n += 1;
                }
            }
        }
    });
    acc.into_iter()
        .filter(|a| a.n > 0)
        .map(|a| {
            let n = a.n as f64;
            HistoryPoint {
                ts: (a.ts / n) as i64,
                frame_ts: a.frame_ts,
                cpu_peak_ts: a.cpu_peak_ts,
                cpu: (a.cpu / n) as f32,
                cpu_peak: a.cpu_peak,
                gpu: (a.gpu / n) as f32,
                mem_used: (a.mem_used / n) as u64,
                mem_total: a.mem_total,
                disk: (a.disk / n) as u64,
                net: (a.net / n) as u64,
                battery: (a.battery_n > 0).then_some((a.battery / a.battery_n as f64) as f32),
                battery_rate: (a.battery_rate_n > 0)
                    .then_some((a.battery_rate / a.battery_rate_n as f64) as f32),
            }
        })
        .collect()
}

/// Index in a file or live buffer is local to its catalog; the string key is permanent.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryChannel {
    pub key: String,
    pub unit: String,
    pub group: String,
    pub label: String,
    #[serde(rename = "deviceId")]
    pub device: Option<String>,
    pub service: bool,
    pub first: i64,
    pub last: i64,
}

#[derive(Default, Clone)]
struct Catalog {
    rows: Vec<HistoryChannel>,
    by_key: HashMap<String, usize>,
    offsets: Vec<usize>,
    bytes: usize,
}

impl Catalog {
    fn register(
        &mut self,
        key: &str,
        unit: &str,
        group: &str,
        label: &str,
        device: Option<&str>,
        ts: i64,
    ) -> Option<usize> {
        if let Some(&i) = self.by_key.get(key) {
            if self.rows[i].label != label {
                self.rows[i].label = label.into();
            }
            return Some(i);
        }
        let i = self.rows.len();
        if i >= MAX_CHANNELS {
            return None;
        }
        self.offsets.push(self.bytes);
        self.bytes += if unit == "bytes" { 8 } else { 4 };
        self.by_key.insert(key.into(), i);
        self.rows.push(HistoryChannel {
            key: key.into(),
            unit: unit.into(),
            group: group.into(),
            label: label.into(),
            service: key == "cpuVoltage" || key.starts_with("fan."),
            device: device.map(str::to_owned),
            first: ts,
            last: ts,
        });
        Some(i)
    }
}

// FNV-1a is specified here (unlike DefaultHasher) so the same hardware identity
// produces the same compact key across builds. Never persist raw serials or PnP paths.
fn hardware_id(identity: &str) -> String {
    let hash = identity.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b.to_ascii_lowercase())).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}
fn readings(catalog: &mut Catalog, s: &Snapshot, groups: RecordGroups) -> Vec<f64> {
    let sys = &s.system;
    let hw = &s.hardware;
    let sensors = hw.cpu_sensors.as_ref();
    let mut values = vec![f64::NEG_INFINITY; catalog.rows.len()];
    let mut set = |key: &str,
                   unit: &str,
                   group: &str,
                   label: &str,
                   device: Option<&str>,
                   value: Option<f64>| {
        if !groups.includes(key) {
            return;
        }
        // Leave room for the system totals registered after disk, NIC and GPU readings.
        if device.is_some()
            && catalog.rows.len() >= MAX_CHANNELS - 32
            && !catalog.by_key.contains_key(key)
        {
            return;
        }
        let Some(i) = catalog.register(key, unit, group, label, device, s.timestamp) else {
            return;
        };
        if i >= values.len() {
            values.resize(i + 1, f64::NEG_INFINITY);
        }
        values[i] = value.filter(|v| v.is_finite()).unwrap_or(f64::NAN);
    };
    for (key, unit, value) in [
        ("cpu", "percent", Some(sys.cpu as f64)),
        ("cpuPeak", "percent", Some(sys.cpu as f64)),
        ("cpuKernel", "percent", Some(sys.cpu_kernel as f64)),
        ("cpuClock", "megahertz", hw.cpu_mhz.map(f64::from)),
        (
            "cpuPower",
            "watts",
            hw.power.package.or(hw.power.cores).map(f64::from),
        ),
        (
            "cpuTemp",
            "celsius",
            sensors
                .and_then(|s| s.package_celsius)
                .or_else(|| {
                    sensors.and_then(|s| {
                        s.core_temperatures
                            .iter()
                            .map(|c| c.celsius)
                            .reduce(f32::max)
                    })
                })
                .or_else(|| hw.thermal.iter().map(|z| z.celsius).reduce(f32::max))
                .map(f64::from),
        ),
        (
            "cpuVoltage",
            "volts",
            sensors.and_then(|s| s.core_voltage).map(f64::from),
        ),
    ] {
        set(key, unit, "cpu", "", None, value);
    }
    if let Some(sensor) = sensors {
        for f in &sensor.board_fans {
            let id = f.channel.to_string();
            let label = match &sensor.board_sensor {
                Some(chip) => format!("{chip} #{}", u16::from(f.channel) + 1),
                None => format!("#{}", u16::from(f.channel) + 1),
            };
            set(
                &format!("fan.{id}"),
                "rpm",
                "cpu",
                &label,
                Some(&id),
                Some(f.rpm.into()),
            );
        }
    }
    for (key, value) in [
        ("memUsed", Some(sys.mem_used)),
        ("memTotal", Some(sys.mem_total)),
        ("commitUsed", Some(sys.commit_used)),
        ("commitLimit", Some(sys.commit_limit)),
        ("memCached", Some(sys.cached)),
        ("memCompressed", sys.memory_compression.map(|c| c.resident)),
        ("pagedPool", Some(sys.kernel_paged)),
        ("nonpagedPool", Some(sys.kernel_nonpaged)),
    ] {
        set(key, "bytes", "memory", "", None, value.map(|v| v as f64));
    }
    for (key, v) in [
        ("processes", sys.process_count),
        ("threads", sys.thread_count),
        ("handles", sys.handle_count),
    ] {
        set(key, "count", "system", "", None, Some(v.into()));
    }
    let mut dr = (!hw.disks.is_empty()).then_some(0.);
    let mut dw = dr;
    for d in &hw.disks {
        dr = dr.zip(d.read).map(|(a, b)| a + b as f64);
        dw = dw.zip(d.write).map(|(a, b)| a + b as f64);
        let id = if !d.info.history_id.is_empty() {
            hardware_id(&d.info.history_id)
        } else if !d.info.model.is_empty()
            && hw
                .disks
                .iter()
                .filter(|other| other.info.model == d.info.model && other.info.size == d.info.size)
                .count()
                == 1
        {
            hardware_id(&format!("{}-{}", d.info.model, d.info.size))
        } else {
            continue;
        };
        for (metric, unit, v) in [
            ("read", "bytesPerSecond", d.read.map(|v| v as f64)),
            ("write", "bytesPerSecond", d.write.map(|v| v as f64)),
            ("active", "percent", d.active.map(f64::from)),
            ("response", "milliseconds", d.response_ms.map(f64::from)),
            ("temp", "celsius", d.temperature.map(f64::from)),
        ] {
            set(
                &format!("disk.{id}.{metric}"),
                unit,
                "disk",
                &d.info.model,
                Some(&id),
                v,
            );
        }
    }
    set("diskRead", "bytesPerSecond", "disk", "", None, dr);
    set("diskWrite", "bytesPerSecond", "disk", "", None, dw);
    let (mut rx, mut tx) = (0., 0.);
    for n in &hw.nics {
        rx += n.rx as f64;
        tx += n.tx as f64;
        let id = &n.info.history_guid;
        for (metric, v) in [("receive", n.rx), ("send", n.tx)] {
            set(
                &format!("nic.{id}.{metric}"),
                "bytesPerSecond",
                "network",
                &n.info.name,
                Some(id),
                Some(v as f64),
            );
        }
    }
    set(
        "netReceive",
        "bytesPerSecond",
        "network",
        "",
        None,
        Some(rx),
    );
    set("netSend", "bytesPerSecond", "network", "", None, Some(tx));
    set(
        "gpu",
        "percent",
        "gpu",
        "",
        None,
        Some(hw.gpus.iter().map(|g| g.util).fold(0., f32::max) as f64),
    );
    for g in &hw.gpus {
        let id = if let Some(identity) = &g.info.history_id {
            hardware_id(identity)
        } else if !g.info.name.is_empty()
            && hw
                .gpus
                .iter()
                .filter(|other| other.info.name == g.info.name)
                .count()
                == 1
        {
            hardware_id(&g.info.name)
        } else {
            continue;
        };
        for (metric, unit, v) in [
            ("util", "percent", Some(g.util as f64)),
            ("dedicated", "bytes", Some(g.dedicated_used as f64)),
            ("shared", "bytes", Some(g.shared_used as f64)),
            (
                "dedicatedTotal",
                "bytes",
                Some(g.info.dedicated_total as f64),
            ),
            ("sharedTotal", "bytes", Some(g.info.shared_total as f64)),
            ("temp", "celsius", g.temperature.map(f64::from)),
            ("power", "watts", g.board_power.map(f64::from)),
            ("clock", "megahertz", g.clock_mhz.map(f64::from)),
            ("fan", "rpm", g.fan_rpm.map(f64::from)),
        ] {
            set(
                &format!("gpu.{id}.{metric}"),
                unit,
                "gpu",
                &g.info.name,
                Some(&id),
                v,
            );
        }
    }
    let mean = |values: &[f64]| {
        (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
    };
    let bs = &hw.batteries;
    for (key, unit, v) in [
        (
            "battery",
            "percent",
            mean(&bs.iter().map(|b| b.percent as f64).collect::<Vec<_>>()),
        ),
        (
            "batteryRate",
            "milliwatts",
            bs.iter()
                .filter_map(|b| b.rate.map(f64::from))
                .reduce(|a, b| a + b),
        ),
        (
            "batteryVoltage",
            "volts",
            mean(
                &bs.iter()
                    .filter_map(|b| b.voltage.map(|mv| f64::from(mv) / 1000.))
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            "batteryTemp",
            "celsius",
            mean(
                &bs.iter()
                    .filter_map(|b| b.temperature.map(f64::from))
                    .collect::<Vec<_>>(),
            ),
        ),
    ] {
        set(key, unit, "battery", "", None, v);
    }
    values
}

#[derive(Default, Clone, Copy)]
struct Mean {
    sum: f64,
    n: u32,
}
impl Mean {
    fn add(&mut self, v: f64) {
        if v.is_finite() {
            self.sum += v;
            self.n = self.n.saturating_add(1);
        }
    }
    fn get(self) -> Option<f64> {
        (self.n > 0).then(|| self.sum / f64::from(self.n))
    }
}

#[derive(Clone)]
struct Sample {
    ts: i64,
    span: i64,
    slot: i64,
    catalog: Arc<Vec<HistoryChannel>>,
    values: Vec<f64>,
}
#[derive(Clone, Default)]
struct Live {
    measurement: Measurement,
    evicted: bool,
    previous: Option<Stamp>,
    first: Option<Stamp>,
    gaps: VecDeque<Gap>,
    catalog: Arc<Vec<HistoryChannel>>,
    samples: VecDeque<Arc<Sample>>,
    frames: VecDeque<i64>,
}
type SharedLive = Arc<ArcSwap<Live>>;
static ACTIVE: LazyLock<ArcSwap<HashMap<PathBuf, SharedLive>>> = LazyLock::new(Default::default);
fn live(dir: &Path) -> Arc<Live> {
    ACTIVE
        .load()
        .get(dir)
        .map(|s| s.load_full())
        .unwrap_or_default()
}
fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
fn unbiased_ms() -> i64 {
    use windows::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime;
    let mut ticks = 0;
    if unsafe { QueryUnbiasedInterruptTime(&mut ticks) }.as_bool() {
        (ticks / 10_000) as i64
    } else {
        -1
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Breakdown {
    pub system: u64,
    pub processes: u64,
    pub watch: u64,
}
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameProcess {
    pub name: String,
    pub pid: u32,
    pub create_time: Option<i64>,
    pub cpu: f32,
    pub gpu: f32,
    pub mem: u64,
    pub io: f64,
    pub net: f64,
    pub disk_read: Option<f64>,
    pub disk_write: Option<f64>,
    pub gpu_mem: Option<u64>,
    pub power: Option<f64>,
    pub threads: Option<u32>,
    pub handles: Option<u32>,
}
/// How many processes a program ran in a frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProgramCount {
    pub name: String,
    pub count: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct HistoryFrame {
    pub ts: i64,
    pub span: i64,
    pub system: BTreeMap<String, Option<f64>>,
    pub processes: Vec<FrameProcess>,
    /// The programs running the most processes, of all the frame's last
    /// sample had (not only the processes it kept): most first, then by
    /// name. Empty when unknown (frames written before 0.6).
    pub programs: Vec<ProgramCount>,
    /// How many programs that sample had; 0 when unknown.
    #[serde(rename = "programCount")]
    pub program_count: u32,
}

const CHANNEL: u8 = 3;
const INSTANCE: u8 = 4;
const MARKER: u8 = 5;
const FRAME: u8 = 6;
/// A program's entry in the day's dictionary: `(id, name)`, before the first
/// frame that counts it.
const PROGRAM: u8 = 14;
/// The program counts of the frame (or minute) just before, written after
/// every frame since 0.6: how many programs (u16), then `PROGRAMS_KEPT`
/// slots of dictionary id and count (u16 each; a count of 0 ends the list).
/// At a fixed size, frames keep fixed strides in checkpointed runs
/// (`FRAME_RUN_COUNTED`); it is not copied into the checkpoints.
const PROGRAMS: u8 = 15;
const PROGRAMS_LEN: usize = 2 + PROGRAMS_KEPT * 4;
fn packet(out: &mut impl Write, tag: u8, data: &[u8]) -> io::Result<()> {
    out.write_all(&[tag])?;
    out.write_all(
        &u32::try_from(data.len())
            .map_err(|_| invalid())?
            .to_le_bytes(),
    )?;
    out.write_all(data)
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid history record")
}
fn next_packet(bytes: &[u8]) -> Option<(u8, &[u8])> {
    let (header, rest) = bytes.split_first_chunk::<5>()?;
    let [tag, len @ ..] = *header;
    Some((tag, rest.get(..u32::from_le_bytes(len) as usize)?))
}

fn packets(bytes: &[u8], mut at: usize, mut visit: impl FnMut(usize, u8, &[u8]) -> bool) -> usize {
    while let Some((tag, payload)) = bytes.get(at..).and_then(next_packet) {
        if !visit(at, tag, payload) {
            break;
        }
        at += 5 + payload.len();
    }
    at
}

fn write_marker(out: &mut impl Write, kind: u8, ts: i64, boot: i64, awake: i64) -> io::Result<()> {
    let mut p = vec![kind];
    for v in [ts, boot, awake] {
        p.extend(v.to_le_bytes());
    }
    packet(out, MARKER, &p)
}
fn var(out: &mut Vec<u8>, mut v: u64) {
    while v >= 128 {
        out.push(v as u8 | 128);
        v >>= 7;
    }
    out.push(v as u8);
}
// Working sets and video allocations normally contain whole pages. Zero escapes exact unaligned byte counts.
fn put_memory(out: &mut Vec<u8>, v: u64) {
    if v.is_multiple_of(4096) {
        var(out, 1 + v / 4096);
    } else {
        var(out, 0);
        var(out, v);
    }
}
fn take_memory(raw: &mut &[u8]) -> Option<u64> {
    match take_var(raw)? {
        0 => take_var(raw),
        n => (n - 1).checked_mul(4096),
    }
}
fn take_var(bytes: &mut &[u8]) -> Option<u64> {
    let mut v = 0;
    for shift in (0..64).step_by(7) {
        let (&b, rest) = bytes.split_first()?;
        *bytes = rest;
        if shift == 63 && b > 1 {
            return None;
        }
        v |= u64::from(b & 127) << shift;
        if b < 128 {
            return Some(v);
        }
    }
    None
}
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, serde::Deserialize)]
struct Identity {
    name: Arc<str>,
    pid: u32,
    created: i64,
}
struct DayWriter {
    checkpoints: Option<Checkpoints>,
    presence: Vec<u8>,
    system: SystemRoll,
    roll: Roll,
    roll_network: bool,
    roll_file: BufWriter<File>,
    proc: BufWriter<File>,
    pos: u64,
    catalog: Catalog,
    ids: HashMap<Identity, u32>,
    next_id: u32,
    /// Program ids by lower-case name (`PROGRAM`).
    program_ids: HashMap<String, u32>,
}
const FRAME_PREFIX: usize = 8 + 8 + 4 + 8 + 4;
struct Stored<'a> {
    ts: i64,
    awake: i64,
    span: i64,
    offset: u64,
    len: usize,
    values: &'a [u8],
}
impl<'a> Stored<'a> {
    fn parse(b: &'a [u8]) -> Option<Self> {
        let values = b.get(FRAME_PREFIX..)?;
        let span = i64::from(u32::from_le_bytes(field(b, 16)?));
        let offset = u64::from_le_bytes(field(b, 20)?);
        let len = u32::from_le_bytes(field(b, 28)?) as usize;
        offset.checked_add(len as u64)?;
        if !values.len().is_multiple_of(4)
            || values.len() > MAX_CHANNELS * 8
            || !(1..=FRAME_MS).contains(&span)
            || len > MAX_FRAME_ROWS * 160
        {
            return None;
        }
        Some(Self {
            ts: i64::from_le_bytes(field(b, 0)?),
            awake: i64::from_le_bytes(field(b, 8)?),
            span,
            offset,
            len,
            values,
        })
    }
    fn value(&self, i: usize, catalog: &Catalog) -> f64 {
        let Some(&at) = catalog.offsets.get(i) else {
            return f64::NEG_INFINITY;
        };
        if catalog.rows[i].unit == "bytes" {
            field(self.values, at).map(f64::from_le_bytes)
        } else {
            field(self.values, at).map(|v| f32::from_le_bytes(v) as f64)
        }
        .unwrap_or(f64::NEG_INFINITY)
    }
}
impl DayWriter {
    fn frame(
        &mut self,
        out: &mut impl Write,
        catalog: &Catalog,
        w: &Window,
        picked: &[&ProcAcc],
        groups: RecordGroups,
    ) -> io::Result<u64> {
        let mut metadata = Vec::new();
        let mut summary_bytes = 0;
        let index_before = self.checkpoints.as_ref().map_or(0, |c| c.written);
        if self.system.count > 0
            && self.system.start.div_euclid(ROLL_MS) != w.start.div_euclid(ROLL_MS)
        {
            self.system.finish(&mut metadata)?;
        }
        let mut values = vec![f64::NEG_INFINITY; self.catalog.rows.len()];
        for (i, c) in catalog
            .rows
            .iter()
            .enumerate()
            .filter(|(i, _)| w.present.get(*i) == Some(&true))
        {
            let known = self.catalog.by_key.get(&c.key).copied();
            let changed = known.is_none_or(|j| self.catalog.rows[j].label != c.label);
            let Some(j) = self.catalog.register(
                &c.key,
                &c.unit,
                &c.group,
                &c.label,
                c.device.as_deref(),
                w.start,
            ) else {
                continue;
            };
            if changed {
                let mut row = self.catalog.rows[j].clone();
                row.first = w.start;
                row.last = w.start;
                packet(&mut metadata, CHANNEL, &serde_json::to_vec(&(j, row))?)?;
            }
            values.resize(self.catalog.rows.len(), f64::NEG_INFINITY);
            values[j] = if c.key == "cpuPeak" {
                w.cpu_peak as f64
            } else {
                w.values[i].get().unwrap_or(f64::NAN)
            };
        }
        if self.roll.recorded > 0
            && (self.roll_network != groups.process_network
                || self.roll.first.div_euclid(PROCESS_ROLL_MS)
                    != w.start.div_euclid(PROCESS_ROLL_MS))
        {
            summary_bytes = self.roll.write(&mut self.roll_file)?;
        }
        if self.roll.recorded == 0 {
            self.roll.first = w.start;
            self.roll_network = groups.process_network;
        }
        let mut rows = Vec::new();
        for p in picked {
            let ident = Identity {
                name: p.name.clone(),
                pid: p.pid,
                created: p.create_time,
            };
            let next = self.next_id;
            let id = match self.ids.entry(ident) {
                std::collections::hash_map::Entry::Occupied(e) => *e.get(),
                std::collections::hash_map::Entry::Vacant(_) if next as usize >= MAX_INSTANCES => {
                    continue;
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    packet(
                        &mut metadata,
                        INSTANCE,
                        &serde_json::to_vec(&(next, e.key()))?,
                    )?;
                    self.next_id += 1;
                    *e.insert(next)
                }
            };
            let n = p.divisor(w.samples);
            let encoded_id = id as u64
                + MAX_INSTANCES as u64
                    * (u64::from(!groups.process_network) | (u64::from(!groups.process_gpu) << 1));
            var(&mut rows, encoded_id);
            let vals = [(p.cpu / n) as f32, (p.gpu / n) as f32];
            let mut rates = [
                (p.io, n),
                (p.net, n),
                (p.disk_read.sum, f64::from(p.disk_read.n)),
                (p.disk_write.sum, f64::from(p.disk_write.n)),
            ];
            let mut fractions = 0u8;
            for (i, (rate, count)) in rates.iter_mut().enumerate() {
                if *count > 0.0 {
                    if *rate % *count == 0. {
                        *rate /= *count;
                    } else {
                        fractions |= 1 << i;
                    }
                }
            }
            let flags = u8::from(vals[0] != 0.)
                | u8::from(vals[1] != 0.) << 1
                | u8::from(p.disk_read.n > 0) << 2
                | u8::from(p.disk_write.n > 0) << 3
                | u8::from(p.gpu_mem.is_some()) << 4
                | u8::from(p.power.n > 0) << 5
                | u8::from(p.power.get().is_some_and(|v| v != 0.)) << 6
                | u8::from(fractions != 0) << 7;
            rows.push(flags);
            for v in vals {
                if v != 0. {
                    rows.extend(v.to_le_bytes());
                }
            }
            put_memory(&mut rows, p.mem);
            var(&mut rows, rates[0].0 as u64);
            if groups.process_network {
                var(&mut rows, rates[1].0 as u64);
            }
            if groups.process_gpu {
                var(&mut rows, u64::from(p.threads));
                var(&mut rows, u64::from(p.handles));
            }
            for &(value, count) in &rates[2..] {
                if count > 0. {
                    var(&mut rows, value as u64);
                }
            }
            if let Some(v) = p.gpu_mem {
                put_memory(&mut rows, v);
            }
            if let Some(v) = p.power.get().filter(|&v| v != 0.) {
                rows.extend((v as f32).to_le_bytes());
            }
            if fractions != 0 {
                rows.push(fractions);
                for (i, (_, count)) in rates.iter().enumerate() {
                    if fractions & (1 << i) != 0 {
                        var(&mut rows, *count as u64);
                    }
                }
            }
        }
        // The counts' dictionary entries go before the frame, the counts
        // right after it (`DayRead::programs`).
        let mut counted = Vec::new();
        for (name, count) in &w.programs {
            let key = name.to_lowercase();
            let id = match self.program_ids.get(&key) {
                Some(&id) => id,
                None if self.program_ids.len() >= usize::from(u16::MAX) => continue,
                None => {
                    let id = self.program_ids.len() as u32;
                    packet(&mut metadata, PROGRAM, &serde_json::to_vec(&(id, &**name))?)?;
                    self.program_ids.insert(key, id);
                    id
                }
            };
            counted.push((id, *count));
        }
        let mut frame = Vec::with_capacity(FRAME_PREFIX + values.len() * 4);
        frame.extend(w.start.to_le_bytes());
        frame.extend(w.awake.to_le_bytes());
        frame.extend((w.span() as u32).to_le_bytes());
        frame.extend(self.pos.to_le_bytes());
        frame.extend((rows.len() as u32).to_le_bytes());
        // Match the precision of the individual frame values in the summary.
        for (v, c) in values.iter_mut().zip(&self.catalog.rows) {
            if c.unit != "bytes" {
                *v = (*v as f32) as f64;
            }
        }
        let mut presence = vec![0; values.len().div_ceil(8)];
        for (i, &v) in values.iter().enumerate() {
            if v != f64::NEG_INFINITY {
                presence[i / 8] |= 1 << (i % 8);
            }
        }
        if presence != self.presence {
            packet(&mut metadata, PRESENCE, &presence)?;
            self.presence = presence;
        }
        self.system.add(w.start, &values, w.cpu_peak);
        for (v, c) in values.into_iter().zip(&self.catalog.rows) {
            if c.unit == "bytes" {
                frame.extend(v.to_le_bytes());
            } else {
                frame.extend((v as f32).to_le_bytes());
            }
        }
        packet(&mut metadata, FRAME, &frame)?;
        let mut programs = [0u8; PROGRAMS_LEN];
        let small = |v: u32| u16::try_from(v).unwrap_or(u16::MAX).to_le_bytes();
        programs[..2].copy_from_slice(&small(w.program_count));
        for (slot, (id, count)) in counted.into_iter().enumerate() {
            programs[2 + slot * 4..4 + slot * 4].copy_from_slice(&small(id));
            programs[4 + slot * 4..6 + slot * 4].copy_from_slice(&small(count));
        }
        packet(&mut metadata, PROGRAMS, &programs)?;
        if (w.start + w.span()).rem_euclid(ROLL_MS) == 0 {
            self.system.finish(&mut metadata)?;
        }
        self.proc.write_all(&rows)?;
        self.proc.flush()?;
        out.write_all(&metadata)?;
        out.flush()?;
        self.index_append(&metadata);
        self.pos += rows.len() as u64;
        self.roll.recorded += w.span();
        self.roll.end = w.start + w.span();
        decode_rows(&rows, |id, m| {
            self.roll
                .rows
                .entry(id)
                .or_default()
                .merge(Totals::observation(m, w.start, w.span()))
        });
        let index_bytes = self
            .checkpoints
            .as_ref()
            .map_or(0, |c| c.written.saturating_sub(index_before));
        Ok((metadata.len() + rows.len()) as u64 + index_bytes + summary_bytes)
    }
}
impl Drop for DayWriter {
    fn drop(&mut self) {
        if let Some(cp) = &mut self.checkpoints
            && let Err(e) = cp.commit()
        {
            crate::diagnostics::file_failure("history index close", &cp.path, &e);
        }
        if let Err(e) = self.roll.write(&mut self.roll_file) {
            crate::diagnostics::warn(
                "history process summary",
                format!("win32={:?}", e.raw_os_error()),
            );
        }
    }
}
impl Window {
    fn span(&self) -> i64 {
        (self.last - self.start + self.interval)
            .clamp(1, FRAME_MS - self.start.rem_euclid(FRAME_MS))
    }
}
fn decode_rows(mut raw: &[u8], mut visit: impl FnMut(usize, Metrics)) -> Option<()> {
    let mut count = 0;
    while !raw.is_empty() {
        count += 1;
        if count > MAX_FRAME_ROWS {
            return None;
        }
        let encoded = usize::try_from(take_var(&mut raw)?).ok()?;
        let omitted = encoded / MAX_INSTANCES;
        if omitted > 3 {
            return None;
        }
        let id = encoded % MAX_INSTANCES;
        let (&flags, tail) = raw.split_first()?;
        raw = tail;
        if flags & 64 != 0 && flags & 32 == 0 {
            return None;
        }
        let float = |b: &mut &[u8]| -> Option<f64> {
            let (v, r) = b.split_at_checked(4)?;
            *b = r;
            finite(f32::from_le_bytes(v.try_into().ok()?) as f64)
        };
        let cpu = if flags & 1 != 0 { float(&mut raw)? } else { 0. };
        let gpu = if flags & 2 != 0 { float(&mut raw)? } else { 0. };
        let mem = take_memory(&mut raw)?;
        let io = take_var(&mut raw)?;
        let net = if omitted & 1 == 0 {
            take_var(&mut raw)? as f64
        } else {
            f64::NAN
        };
        let mut count = || {
            if omitted & 2 != 0 {
                Some(None)
            } else {
                Some(Some(u32::try_from(take_var(&mut raw)?).ok()?))
            }
        };
        let threads = count()?;
        let handles = count()?;
        let mut opt = |flag| -> Option<Option<u64>> {
            if flags & flag != 0 {
                Some(Some(take_var(&mut raw)?))
            } else {
                Some(None)
            }
        };
        let disk_read = opt(4u8)?;
        let disk_write = opt(8u8)?;
        let gpu_mem = if flags & 16 != 0 {
            Some(take_memory(&mut raw)?)
        } else {
            None
        };
        let power = if flags & 64 != 0 {
            Some(float(&mut raw)?)
        } else {
            (flags & 32 != 0).then_some(0.)
        };
        let mut rates = [
            Some(io as f64),
            Some(net),
            disk_read.map(|v| v as f64),
            disk_write.map(|v| v as f64),
        ];
        if flags & 128 != 0 {
            let (&mask, tail) = raw.split_first()?;
            raw = tail;
            if mask == 0 || mask & !15 != 0 {
                return None;
            }
            for (i, rate) in rates.iter_mut().enumerate() {
                if mask & (1 << i) != 0 {
                    let n = take_var(&mut raw)?;
                    if n == 0 {
                        return None;
                    }
                    *rate.as_mut()? /= n as f64;
                }
            }
        }
        visit(
            id,
            Metrics {
                cpu,
                gpu,
                mem,
                io: rates[0]?,
                net: rates[1]?,
                threads,
                handles,
                disk_read: rates[2],
                disk_write: rates[3],
                gpu_mem,
                power,
            },
        );
    }
    Some(())
}
#[derive(Clone, Copy, Default)]
struct Metrics {
    cpu: f64,
    gpu: f64,
    mem: u64,
    io: f64,
    net: f64,
    threads: Option<u32>,
    handles: Option<u32>,
    disk_read: Option<f64>,
    disk_write: Option<f64>,
    gpu_mem: Option<u64>,
    power: Option<f64>,
}
impl Metrics {
    fn process(self, id: &Identity) -> FrameProcess {
        FrameProcess {
            name: id.name.to_string(),
            pid: id.pid,
            create_time: (id.created > 0).then_some(id.created),
            cpu: self.cpu as f32,
            gpu: self.gpu as f32,
            mem: self.mem,
            io: self.io,
            net: self.net,
            threads: self.threads,
            handles: self.handles,
            disk_read: self.disk_read,
            disk_write: self.disk_write,
            gpu_mem: self.gpu_mem,
            power: self.power,
        }
    }
}
impl From<FrameProcess> for HistoryProcess {
    fn from(p: FrameProcess) -> Self {
        Self {
            name: p.name,
            pid: p.pid,
            create_time: p.create_time,
            cpu: p.cpu,
            gpu: p.gpu,
            mem: p.mem,
            io: p.io as u64,
            net: p.net as u64,
        }
    }
}
fn legacy_point(ts: i64, get: impl Fn(&str) -> Option<f64>) -> HistoryPoint {
    HistoryPoint {
        ts,
        frame_ts: ts,
        cpu_peak_ts: ts,
        cpu: get("cpu").unwrap_or_default() as f32,
        cpu_peak: get("cpuPeak").unwrap_or_default() as f32,
        gpu: get("gpu").unwrap_or_default() as f32,
        mem_used: get("memUsed").unwrap_or_default() as u64,
        mem_total: get("memTotal").unwrap_or_default() as u64,
        disk: get("disk")
            .or_else(|| Some(get("diskRead")? + get("diskWrite")?))
            .unwrap_or_default() as u64,
        net: get("network")
            .or_else(|| Some(get("netReceive")? + get("netSend")?))
            .unwrap_or_default() as u64,
        battery: get("battery").map(|v| v as f32),
        battery_rate: get("batteryRate").map(|v| v as f32),
    }
}

#[derive(Clone, Copy, Default, Serialize, serde::Deserialize)]
struct Stamp {
    ts: i64,
    span: i64,
    boot: i64,
    awake: i64,
    stop: u8,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Gap {
    pub from: i64,
    pub to: i64,
    pub reason: &'static str,
}
fn gap(a: Stamp, b: Stamp) -> Option<Gap> {
    let from = a.ts.checked_add(a.span)?;
    if b.ts <= from {
        return None;
    }
    let reason = if a.stop == 2 {
        "off"
    } else if a.boot == 0 || b.boot == 0 {
        "unknown"
    } else if a.boot.abs_diff(b.boot) > 2000 {
        "shutdown"
    } else if a.awake >= 0
        && b.awake >= a.awake
        && i128::from(b.ts) - i128::from(a.ts) - i128::from(b.awake - a.awake) > 2000
    {
        "sleep"
    } else {
        "notRunning"
    };
    Some(Gap {
        from,
        to: b.ts,
        reason,
    })
}
#[derive(Clone, Default)]
struct Index {
    day: i64,
    version: u8,
    valid: usize,
    limited: bool,
    disordered: bool,
    catalog: Catalog,
    ids: Vec<Identity>,
    /// Program names by id (`PROGRAM`).
    programs: Vec<String>,
    frames: Vec<(u32, u32)>,
    summaries: Vec<RollRef>,
    masks: Vec<(usize, Vec<u8>)>,
    labels: Vec<(i64, usize, String)>,
    gaps: Vec<Gap>,
    first: Option<Stamp>,
    last: Option<Stamp>,
    boot: i64,
    proc_end: u64,
    process_bytes: u64,
    edges: Option<[Stamp; 2]>,
}
impl Index {
    fn gap_start(&self) -> Option<Stamp> {
        self.edges.map(|e| e[0]).or(self.first)
    }
    fn gap_end(&self) -> Option<Stamp> {
        self.edges.map(|e| e[1]).or(self.last)
    }
    fn stamp(&mut self, s: Stamp, at: usize) {
        if let Some(prev) = self.last
            && let Some(g) = gap(prev, s)
        {
            self.gaps.push(g);
        }
        self.first.get_or_insert(s);
        self.last = Some(s);
        self.frames
            .push((s.ts.rem_euclid(DAY_MS) as u32, at as u32));
    }
    // In a checkpoint, expand runs back to main-file byte positions. Every
    // address and the block end must agree with the catalog's frame width.
    fn scan(&mut self, bytes: &[u8], proc_limit: u64, checkpoint_end: Option<usize>) -> usize {
        packets(bytes, 0, |_, tag, p| {
            let at = self.valid;
            let Some(end) = at.checked_add(5 + p.len()) else {
                return false;
            };
            let limit = checkpoint_end.unwrap_or(MAX_FILE_BYTES);
            if tag == FRAME_RUN || tag == FRAME_RUN_COUNTED {
                return checkpoint_end.is_some()
                    && self.address_run(p, limit, tag == FRAME_RUN_COUNTED);
            }
            if end > limit
                || self
                    .record(at, tag, p, proc_limit, checkpoint_end.is_some())
                    .is_none()
            {
                return false;
            }
            self.valid = end;
            true
        })
    }

    fn record(
        &mut self,
        at: usize,
        tag: u8,
        p: &[u8],
        proc_limit: u64,
        checkpoint: bool,
    ) -> Option<()> {
        let capped = match tag {
            CHANNEL => p.len() > MAX_METADATA_BYTES || self.labels.len() >= MAX_FRAMES,
            INSTANCE => p.len() > MAX_METADATA_BYTES || self.ids.len() >= MAX_INSTANCES,
            PROGRAM => p.len() > MAX_METADATA_BYTES || self.programs.len() >= MAX_INSTANCES,
            minute::MINUTE if self.version == MINUTE_VERSION => self.frames.len() >= 1440,
            minute::GAP if self.version == MINUTE_VERSION => self.gaps.len() >= MAX_FRAMES,
            FRAME => self.frames.len() >= MAX_FRAMES,
            PRESENCE => self.masks.len() >= MAX_FRAMES,
            _ => false,
        };
        if capped {
            self.limited = true;
            return None;
        }
        match tag {
            CHANNEL => {
                let (i, c) = serde_json::from_slice::<(usize, HistoryChannel)>(p).ok()?;
                if i == MAX_CHANNELS && i == self.catalog.rows.len() {
                    self.limited = true;
                    return None;
                }
                if i > self.catalog.rows.len()
                    || self
                        .catalog
                        .rows
                        .get(i)
                        .is_some_and(|old| old.key != c.key || old.unit != c.unit)
                    || self.catalog.by_key.get(&c.key).is_some_and(|&j| j != i)
                {
                    return None;
                }
                let j = self.catalog.register(
                    &c.key,
                    &c.unit,
                    &c.group,
                    &c.label,
                    c.device.as_deref(),
                    c.first,
                )?;
                self.catalog.rows[j].service = c.service;
                self.labels.push((c.first, j, c.label));
            }
            INSTANCE => {
                let (i, id) = serde_json::from_slice::<(usize, Identity)>(p).ok()?;
                if i != self.ids.len() {
                    return None;
                }
                self.ids.push(id);
                self.process_bytes += (5 + p.len()) as u64;
            }
            PROGRAM => {
                let (i, name) = serde_json::from_slice::<(usize, String)>(p).ok()?;
                if i != self.programs.len() {
                    return None;
                }
                self.programs.push(name);
                self.process_bytes += (5 + p.len()) as u64;
            }
            // Read with the frame before it.
            PROGRAMS if p.len() == PROGRAMS_LEN => self.process_bytes += (5 + p.len()) as u64,
            MARKER if p.len() == 25 => match p[0] {
                1 => self.boot = i64::from_le_bytes(field(p, 9)?),
                2 | 3 => {
                    if let Some(s) = &mut self.last {
                        s.stop = p[0];
                    }
                }
                _ => return None,
            },
            PRESENCE if p.len() == self.catalog.rows.len().div_ceil(8) => {
                self.masks.push((self.frames.len(), p.to_vec()));
            }
            SYSTEM_ROLL => {
                if !self.disordered
                    && let Some(r) = RollRef::parse(p, self)
                {
                    self.summaries.push(r);
                }
            }
            minute::MINUTE if self.version == MINUTE_VERSION => {
                let minute = Minute::parse(p, self.day)?;
                if self.last.is_some_and(|s| s.ts >= minute.ts) {
                    return None;
                }
                let stamp = Stamp {
                    ts: minute.ts,
                    span: MINUTE_MS,
                    ..Default::default()
                };
                self.first.get_or_insert(stamp);
                self.last = Some(stamp);
                self.frames
                    .push((stamp.ts.rem_euclid(DAY_MS) as u32, at as u32));
                self.process_bytes += minute.process_bytes();
            }
            minute::EDGES if self.version == MINUTE_VERSION && p.len() <= 512 => {
                let edges: [Stamp; 2] = serde_json::from_slice(p).ok()?;
                if edges[1].ts < edges[0].ts
                    || edges.iter().any(|s| {
                        s.ts.div_euclid(DAY_MS) != self.day
                            || !(1..=FRAME_MS).contains(&s.span)
                            || s.stop > 3
                    })
                {
                    return None;
                }
                self.edges = Some(edges);
            }
            minute::SEALED if self.version == MINUTE_VERSION && p == minute::SEAL => (),
            minute::GAP if self.version == MINUTE_VERSION => {
                let g = minute::parse_gap(p, self.day)?;
                if self.gaps.last().is_some_and(|prev| prev.to > g.from) {
                    return None;
                }
                self.gaps.push(g);
            }
            FRAME if self.version != MINUTE_VERSION && !checkpoint => {
                let f = Stored::parse(p)?;
                let end = f.offset.checked_add(f.len as u64)?;
                if f.offset != self.proc_end
                    || end > proc_limit
                    || f.values.len() != self.catalog.bytes
                    || f.ts.div_euclid(DAY_MS) != self.day
                {
                    return None;
                }
                self.proc_end = end;
                // Keep physical offsets for the following records, but expose
                // the first pass through a repeated wall-clock interval.
                if self.last.is_some_and(|s| f.ts <= s.ts) {
                    self.disordered = true;
                    self.summaries.clear();
                    return Some(());
                }
                self.stamp(
                    Stamp {
                        ts: f.ts,
                        span: f.span,
                        boot: self.boot,
                        awake: f.awake,
                        stop: 0,
                    },
                    at,
                );
            }
            _ => return None,
        }
        Some(())
    }
}
fn open_writable(path: &Path) -> io::Result<File> {
    #[cfg(test)]
    if let Some(file) = test_files::open(path, true) {
        return file;
    }
    use std::os::windows::fs::OpenOptionsExt;
    let file = OpenOptions::new()
        .share_mode(1) // One writer per file. Readers remain available throughout recording.
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    #[cfg(test)]
    let file = File::Disk(file);
    Ok(file)
}
fn open_day(dir: &Path, day: i64) -> io::Result<DayFile> {
    let owner = owned_directory(dir)?;
    let path = day_path(dir, day);
    let mut file = open_writable(&path)?;
    let mut bytes = read_bounded(&mut file)?;
    // A day begun by a release writing another version (v1/v2, no longer
    // read) starts over in this one, as an empty or torn header does.
    let other = bytes.starts_with(b"ARGH")
        && bytes.len() >= 5
        && !bytes.starts_with(MAGIC)
        && !bytes.starts_with(MINUTE_MAGIC);
    if (bytes.len() < 5 && MAGIC.starts_with(&bytes)) || other {
        if other {
            maintenance::remove_sidecars(&path)?;
        }
        file.set_len(0)?;
        file.rewind()?;
        file.write_all(MAGIC)?;
        bytes = MAGIC.to_vec();
    }
    let mut idx = Index {
        day,
        valid: 5,
        ..Default::default()
    };
    if !bytes.starts_with(MAGIC) {
        return Err(invalid());
    }
    let writer = {
        let side = path.with_extension("proc");
        let mut proc = open_writable(&side)?;
        idx.scan(&bytes[5..], proc.metadata()?.len(), None);
        if idx.limited {
            return Err(invalid());
        }
        proc.set_len(idx.proc_end)?;
        proc.seek(SeekFrom::End(0))?;
        let mut roll_file = open_writable(&path.with_extension("roll"))?;
        let rolls = if roll_file.metadata()?.len() > MAX_FILE_BYTES as u64 {
            Vec::new()
        } else {
            read_bounded(&mut roll_file)?
        };
        let valid = packets(&rolls, 0, |_, tag, p| {
            tag == 1
                && Roll::header(p, &idx)
                    .and_then(|mut roll| roll.read_rows(p, &idx))
                    .is_some()
        });
        roll_file.set_len(valid as u64)?;
        roll_file.seek(SeekFrom::End(0))?;
        let idx_path = path.with_extension("idx");
        let checkpoints = Checkpoints::open(idx_path.clone(), &bytes[..idx.valid])
            .inspect_err(|e| crate::diagnostics::file_failure("history index open", &idx_path, e))
            .ok();
        DayWriter {
            checkpoints,
            presence: Vec::new(),
            system: SystemRoll::default(),
            roll: Roll::default(),
            roll_network: true,
            roll_file: BufWriter::new(roll_file),
            proc: BufWriter::new(proc),
            pos: idx.proc_end,
            catalog: idx.catalog,
            next_id: idx.ids.len() as u32,
            program_ids: idx
                .programs
                .iter()
                .enumerate()
                .map(|(i, name)| (name.to_lowercase(), i as u32))
                .collect(),
            ids: idx
                .ids
                .into_iter()
                .enumerate()
                .map(|(i, id)| (id, i as u32))
                .collect(),
        }
    };
    file.set_len(idx.valid as u64)?;
    file.seek(SeekFrom::End(0))?;
    Ok(DayFile {
        _owner: owner,
        recorded_until: idx.last.map(|s| s.ts + s.span),
        out: BufWriter::new(file),
        writer,
        day,
    })
}

struct CachedIndex {
    generation: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    len: u64,
    cost: usize,
    /// When it was last read (`INDEX_USES`), to drop the least recently used first.
    used: u64,
    idx: Arc<Index>,
}
/// What the day indexes kept between queries may cost. A day's index is about
/// 0.6 MiB in the current format and 2.2 MiB in the older ones: a week of the
/// older ones fits, so that a week's range read again is not read from disk.
const INDEX_CACHE_BYTES: usize = 24 * 1024 * 1024;
static INDEX_USES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CLEAR_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static INDEXES: LazyLock<Mutex<HashMap<PathBuf, CachedIndex>>> = LazyLock::new(Default::default);
fn history_read<T>(path: &Path, result: io::Result<T>) -> Option<T> {
    result
        .inspect_err(|error| {
            if !matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::UnexpectedEof
            ) {
                crate::diagnostics::file_failure("history read", path, error);
            }
        })
        .ok()
}
fn index(path: &Path) -> Option<Arc<Index>> {
    let generation = CLEAR_GENERATION.load(std::sync::atomic::Ordering::Acquire);
    let meta = history_read(path, metadata(path))?;
    let len = meta.len();
    if len > MAX_FILE_BYTES as u64 {
        return None;
    }
    let modified = meta.modified().ok();
    let created = meta.created().ok();
    let mut cache = INDEXES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = cache.get_mut(path)
        && c.generation == generation
        && c.created == created
        && c.modified == modified
        && c.len == len
    {
        c.used = INDEX_USES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Some(c.idx.clone());
    }
    let mut idx = cache
        .remove(path)
        .filter(|c| c.generation == generation && c.len < len && c.created == created)
        .map(|c| c.idx)
        .unwrap_or_default();
    drop(cache);
    let mut f = history_read(path, File::open(path))?;
    let data = Arc::make_mut(&mut idx);
    if data.valid == 0 {
        let mut magic = [0; 5];
        f.read_exact(&mut magic).ok()?;
        // Other versions (the v1/v2 days of earlier releases) are not read.
        if &magic != MAGIC && &magic != MINUTE_MAGIC {
            return None;
        }
        data.day = day_of(path.file_name()?)?;
        data.version = magic[4];
        data.valid = 5;
        if data.version == VERSION {
            data.load_checkpoints(path, len);
        }
    }
    f.seek(SeekFrom::Start(data.valid as u64)).ok()?;
    let mut tail = Vec::new();
    history_read(
        path,
        f.take(len.saturating_sub(data.valid as u64))
            .read_to_end(&mut tail),
    )?;
    data.scan(&tail, u64::MAX, None);
    // Bound uncommon catalogs and churn too (`INDEX_CACHE_BYTES`).
    let cost = |i: &Index| {
        i.frames.capacity() * 8
            + i.masks.iter().map(|m| 32 + m.1.capacity()).sum::<usize>()
            + i.summaries.capacity() * std::mem::size_of::<RollRef>()
            + i.summaries
                .iter()
                .map(|r| r.bands.len() * std::mem::size_of::<Band>())
                .sum::<usize>()
            + i.gaps.capacity() * std::mem::size_of::<Gap>()
            + i.ids.iter().map(|id| 64 + id.name.len()).sum::<usize>()
            + i.programs.iter().map(|n| 32 + n.len()).sum::<usize>()
            + i.catalog
                .rows
                .iter()
                .map(|c| {
                    256 + c.key.len() * 2
                        + c.unit.len()
                        + c.label.len()
                        + c.group.len()
                        + c.device.as_ref().map_or(0, String::len)
                })
                .sum::<usize>()
            + i.labels.iter().map(|(_, _, s)| 48 + s.len()).sum::<usize>()
    };
    let size = cost(&idx).max(512);
    let mut cache = INDEXES.lock().unwrap_or_else(|e| e.into_inner());
    if size <= INDEX_CACHE_BYTES
        && generation == CLEAR_GENERATION.load(std::sync::atomic::Ordering::Acquire)
    {
        // The days read longest ago go first: a long range read once leaves
        // the recent days that the shorter ranges read.
        let mut total = cache
            .iter()
            .filter(|(p, _)| p.as_path() != path)
            .map(|(_, c)| c.cost)
            .sum::<usize>();
        while total + size > INDEX_CACHE_BYTES {
            let Some(oldest) = cache
                .iter()
                .filter(|(p, _)| p.as_path() != path)
                .min_by_key(|(_, c)| c.used)
                .map(|(p, _)| p.clone())
            else {
                break;
            };
            if let Some(dropped) = cache.remove(&oldest) {
                total = total.saturating_sub(dropped.cost);
            }
        }
        cache.insert(
            path.into(),
            CachedIndex {
                generation,
                modified,
                cost: size,
                created,
                len,
                used: INDEX_USES.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                idx: idx.clone(),
            },
        );
    }
    Some(idx)
}
fn forget(dir: &Path) {
    CLEAR_GENERATION.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    if let Ok(mut cache) = INDEXES.try_lock() {
        cache.retain(|p, _| p.parent() != Some(dir));
    }
}

struct DayRead {
    idx: Arc<Index>,
    base: usize,
    bytes: Vec<u8>,
    proc_base: u64,
    proc: Vec<u8>,
    proc_path: Option<PathBuf>,
    range: std::ops::Range<usize>,
}
fn read_bounded(file: &mut File) -> io::Result<Vec<u8>> {
    let len = file.metadata()?.len();
    if len > MAX_FILE_BYTES as u64 {
        return Err(invalid());
    }
    let mut bytes = vec![0; len as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn read_slice(path: &Path, offset: u64, len: usize) -> Option<Vec<u8>> {
    if len > MAX_FILE_BYTES {
        return None;
    }
    let mut f = history_read(path, File::open(path))?;
    if offset.checked_add(len as u64)? > f.metadata().ok()?.len() {
        return None;
    }
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = vec![0; len];
    history_read(path, f.read_exact(&mut bytes))?;
    Some(bytes)
}
impl DayRead {
    fn open(
        path: &Path,
        day: i64,
        idx: Arc<Index>,
        from: i64,
        to: i64,
        processes: bool,
    ) -> Option<Self> {
        let range = idx.range(day, from, to);
        Self::by_frames(path, idx, range, processes)
    }
    fn by_frames(
        path: &Path,
        idx: Arc<Index>,
        range: std::ops::Range<usize>,
        processes: bool,
    ) -> Option<Self> {
        let first = range.start;
        let last = range.end;
        if first >= last {
            return None;
        }
        let base = idx.frames.get(first)?.1 as usize;
        let end = idx.frames.get(last).map_or(idx.valid, |e| e.1 as usize);
        let bytes = read_slice(path, base as u64, end.checked_sub(base)?)?;
        let mut d = Self {
            idx,
            base,
            bytes,
            proc_base: 0,
            proc: Vec::new(),
            proc_path: None,
            range: first..last,
        };
        if processes && d.idx.version == VERSION {
            let side = path.with_extension("proc");
            let slice = d.stored(first).zip(d.stored(last - 1)).and_then(|(a, b)| {
                let len =
                    usize::try_from(b.offset.checked_add(b.len as u64)?.checked_sub(a.offset)?)
                        .ok()?;
                Some((a.offset, read_slice(&side, a.offset, len)?))
            });
            if let Some((base, bytes)) = slice {
                d.proc_base = base;
                d.proc = bytes;
            } else {
                d.proc_path = Some(side);
            }
        }
        Some(d)
    }
    fn bytes(&self, i: usize) -> Option<&[u8]> {
        self.bytes
            .get((self.idx.frames.get(i)?.1 as usize).checked_sub(self.base)?..)
    }
    fn stored(&self, i: usize) -> Option<Stored<'_>> {
        let (tag, payload) = next_packet(self.bytes(i)?)?;
        let f = Stored::parse(payload)?;
        (tag == FRAME
            && f.ts.div_euclid(DAY_MS) == self.idx.day
            && f.ts.rem_euclid(DAY_MS) == i64::from(self.idx.frames.get(i)?.0)
            && f.values.len() <= self.idx.catalog.bytes)
            .then_some(f)
    }
    fn metrics(&self, f: &Stored<'_>) -> Option<Vec<(usize, Metrics)>> {
        let single;
        let raw = if let Some(path) = &self.proc_path {
            single = read_slice(path, f.offset, f.len)?;
            &single[..]
        } else {
            let offset = usize::try_from(f.offset.checked_sub(self.proc_base)?).ok()?;
            self.proc.get(offset..offset.checked_add(f.len)?)?
        };
        let mut rows = Vec::new();
        decode_rows(raw, |id, m| rows.push((id, m)))?;
        rows.iter()
            .all(|(id, _)| *id < self.idx.ids.len())
            .then_some(rows)
    }
    /// The program counts recorded with frame (or minute) `i`: the packet
    /// right after it, when there is one.
    fn programs(&self, i: usize) -> (Vec<ProgramCount>, u32) {
        self.programs_packet(i)
            .and_then(|p| decode_programs(p, &self.idx.programs))
            .unwrap_or_default()
    }
    fn programs_packet(&self, i: usize) -> Option<&[u8]> {
        let bytes = self.bytes(i)?;
        let (_, first) = next_packet(bytes)?;
        let (tag, p) = next_packet(bytes.get(5 + first.len()..)?)?;
        (tag == PROGRAMS).then_some(p)
    }
    fn minute(&self, i: usize) -> Option<Minute<'_>> {
        let (tag, p) = next_packet(self.bytes(i)?)?;
        let m = Minute::parse(p, self.idx.day)?;
        (tag == minute::MINUTE && m.ts.rem_euclid(DAY_MS) == i64::from(self.idx.frames.get(i)?.0))
            .then_some(m)
    }
    fn row(&self, i: usize) -> Option<Reading<'_>> {
        if self.idx.version == MINUTE_VERSION {
            let m = self.minute(i)?;
            return Some(Reading::Minute(m.ts, m.bands(self.idx.catalog.rows.len())?));
        }
        Some(Reading::New(self.stored(i)?, &self.idx.catalog))
    }
    fn processes(&self, i: usize) -> Vec<FrameProcess> {
        if self.idx.version == MINUTE_VERSION {
            return self
                .minute(i)
                .map(|m| m.processes(&self.idx))
                .unwrap_or_default();
        }
        self.stored(i)
            .and_then(|f| self.metrics(&f))
            .unwrap_or_default()
            .into_iter()
            .map(|(j, m)| m.process(&self.idx.ids[j]))
            .collect()
    }
}
enum Reading<'a> {
    Minute(i64, Vec<Band>),
    New(Stored<'a>, &'a Catalog),
}
impl Reading<'_> {
    fn ts(&self) -> i64 {
        match self {
            Self::Minute(ts, _) => *ts,
            Self::New(f, _) => f.ts,
        }
    }
    fn span(&self) -> i64 {
        match self {
            Self::Minute(_, _) => MINUTE_MS,
            Self::New(f, _) => f.span,
        }
    }
    fn get(&self, i: usize) -> f64 {
        match self {
            Self::Minute(_, bands) => bands.get(i).map_or(f64::NEG_INFINITY, |b| {
                if b.n > 0 {
                    b.sum / f64::from(b.n)
                } else if b.low == f64::NEG_INFINITY {
                    f64::NEG_INFINITY
                } else {
                    f64::NAN
                }
            }),
            Self::New(f, c) => f.value(i, c),
        }
    }
}
fn decode_programs(p: &[u8], names: &[String]) -> Option<(Vec<ProgramCount>, u32)> {
    let (total, slots) = p.split_first_chunk::<2>()?;
    if p.len() != PROGRAMS_LEN {
        return None;
    }
    let mut out = Vec::new();
    for slot in slots.as_chunks::<4>().0 {
        let count = u32::from(u16::from_le_bytes([slot[2], slot[3]]));
        if count == 0 {
            break;
        }
        let id = usize::from(u16::from_le_bytes([slot[0], slot[1]]));
        out.push(ProgramCount {
            name: names.get(id)?.clone(),
            count,
        });
    }
    Some((out, u32::from(u16::from_le_bytes(*total))))
}
fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}
fn visit_days(dir: &Path, from: i64, to: i64, processes: bool, mut visit: impl FnMut(&DayRead)) {
    if from > to {
        return;
    }
    for day in days(dir)
        .into_iter()
        .filter(|&d| d >= from.div_euclid(DAY_MS) && d <= to.div_euclid(DAY_MS))
    {
        let path = day_path(dir, day);
        if let Some(idx) = index(&path)
            && let Some(d) = DayRead::open(&path, day, idx, from, to, processes)
        {
            visit(&d);
        }
    }
}
pub fn frame(dir: &Path, ts: i64) -> Option<HistoryFrame> {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let mut best = None;
    let mut distance = 60_001;
    visit_days(
        dir,
        ts.saturating_sub(60_000),
        ts.saturating_add(60_000),
        true,
        |d| {
            for i in d.range.clone() {
                let Some(row) = d.row(i) else { continue };
                let diff = row.ts().abs_diff(ts);
                if diff < distance {
                    distance = diff;
                    let (programs, program_count) = d.programs(i);
                    best = Some(HistoryFrame {
                        ts: row.ts(),
                        span: row.span(),
                        system: d
                            .idx
                            .catalog
                            .rows
                            .iter()
                            .enumerate()
                            .filter_map(|(j, c)| {
                                let v = row.get(j);
                                (v != f64::NEG_INFINITY).then(|| (c.key.clone(), finite(v)))
                            })
                            .collect(),
                        processes: d.processes(i),
                        programs,
                        program_count,
                    });
                }
            }
        },
    );
    best
}
pub fn moment(dir: &Path, ts: i64) -> Option<HistoryMoment> {
    let f = frame(dir, ts)?;
    Some(HistoryMoment {
        system: legacy_point(f.ts, |k| f.system.get(k).copied().flatten()),
        processes: f.processes.into_iter().map(HistoryProcess::from).collect(),
    })
}
pub fn range(dir: &Path) -> Option<(i64, i64)> {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let days = days(dir);
    let edge = |day: &i64| index(&day_path(dir, *day));
    Some((
        days.iter().find_map(|d| edge(d)?.first.map(|s| s.ts))?,
        days.iter()
            .rev()
            .find_map(|d| edge(d)?.last.map(|s| s.ts))?,
    ))
}
pub fn usage(dir: &Path) -> HistoryUsage {
    usage_with(dir, HistoryOptions::default())
}
pub fn usage_with(dir: &Path, options: HistoryOptions) -> HistoryUsage {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let mut measurement = live(dir).measurement;
    let live_match = measurement.frames > 0 && measurement.recording == options.recording;
    let mut u = HistoryUsage {
        limit: options.limit,
        ..Default::default()
    };
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries
            .flatten()
            .filter(|e| owned_day(&e.file_name()).is_some())
        {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let counter = if matches!(
                entry.path().extension().and_then(OsStr::to_str),
                Some("proc" | "roll")
            ) {
                &mut u.breakdown.processes
            } else {
                &mut u.breakdown.system
            };
            *counter = counter.saturating_add(meta.len());
        }
    }
    for day in days(dir) {
        let path = day_path(dir, day);
        let Some(idx) = index(&path) else { continue };
        let bytes = metadata(&path).map_or(0, |m| m.len());
        let embedded = idx.process_bytes.min(bytes);
        u.breakdown.system = u.breakdown.system.saturating_sub(embedded);
        u.breakdown.processes = u.breakdown.processes.saturating_add(embedded);
        u.days += 1;
        if u.first.is_none() {
            u.first = idx.first.map(|s| s.ts);
        }
        if live_match || idx.frames.is_empty() {
            continue;
        }

        let measure_path = path.with_extension("measure");
        let saved = metadata(&measure_path)
            .ok()
            .and_then(|m| usize::try_from(m.len()).ok())
            .filter(|len| (1..=1024).contains(len))
            .and_then(|len| read_slice(&measure_path, 0, len))
            .and_then(|b| serde_json::from_slice::<Measurement>(&b).ok())
            .filter(|m| m.frames > 0 && m.bytes > 0);
        let candidate = saved.or_else(|| {
            (idx.version != MINUTE_VERSION).then(|| Measurement {
                recording: Recording::default(),
                frames: idx.frames.len() as u64,
                bytes: ["idx", "proc", "roll"].iter().fold(bytes, |sum, ext| {
                    sum.saturating_add(metadata(path.with_extension(ext)).map_or(0, |m| m.len()))
                }),
            })
        });
        if let Some(candidate) = candidate
            && (candidate.recording == options.recording
                || measurement.frames == 0
                || measurement.recording != options.recording)
        {
            measurement = candidate;
        }
    }
    let baseline = options.recording.fallback_frame_bytes() as f64;
    let frame_bytes = if measurement.frames == 0 {
        baseline
    } else {
        measurement.bytes as f64 / measurement.frames as f64 * baseline
            / measurement.recording.fallback_frame_bytes() as f64
    };
    u.estimate.bytes_per_day = (frame_bytes * 17_280.).round() as u64;
    u.breakdown.watch = crate::watch::WatchRecorder::storage_bytes(dir);
    u.bytes = u
        .breakdown
        .system
        .saturating_add(u.breakdown.processes)
        .saturating_add(u.breakdown.watch);
    u
}

#[derive(Debug, Default, Serialize)]
pub struct Column {
    pub mean: Vec<Option<f64>>,
    pub low: Vec<Option<f64>>,
    pub high: Vec<Option<f64>>,
}
#[derive(Debug, Default, Serialize)]
pub struct HistorySeries {
    /// UTC midnight of each sealed day contributing to this query, independent of bucket width.
    #[serde(rename = "mergedDays")]
    pub merged_days: Vec<i64>,
    pub bucket: i64,
    pub ts: Vec<i64>,
    pub span: Vec<i64>,
    /// Whether each point is one sample of the last ten minutes, asked for
    /// at one-second buckets: its `span` is the time since the sample
    /// before, a frame or more at a background cadence of 5 s or longer,
    /// yet it averages nothing. Empty while no point is one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sample: Vec<bool>,
    pub frame: Vec<i64>,
    pub series: BTreeMap<String, Column>,
    pub peak: Vec<i64>,
    pub gaps: Vec<Gap>,
    pub settled: i64,
}
#[derive(Clone, Copy)]
struct Band {
    sum: f64,
    low: f64,
    high: f64,
    n: u32,
}
impl Default for Band {
    fn default() -> Self {
        Self {
            sum: 0.,
            low: f64::INFINITY,
            high: f64::NEG_INFINITY,
            n: 0,
        }
    }
}
impl Band {
    fn add(&mut self, v: f64) {
        if v.is_finite() {
            self.n += 1;
            self.sum += v;
            self.low = self.low.min(v);
            self.high = self.high.max(v);
        }
    }
}
struct Bucket {
    ts: i64,
    span: i64,
    /// One sample, not a frame or an aggregate (`HistorySeries::sample`).
    sample: bool,
    frame: i64,
    peak: i64,
    max: f64,
    bands: Vec<Band>,
}
impl Bucket {
    fn new(ts: i64, span: i64, n: usize) -> Self {
        Self {
            ts,
            span,
            sample: false,
            frame: 0,
            peak: 0,
            max: f64::NEG_INFINITY,
            bands: vec![Band::default(); n],
        }
    }
    fn update(&mut self, frame: i64, peak: f64, values: impl Iterator<Item = f64>) {
        if self.frame == 0 {
            self.frame = frame;
        }
        if peak > self.max {
            self.max = peak;
            self.peak = frame;
        }
        for (b, v) in self.bands.iter_mut().zip(values) {
            b.add(v);
        }
    }
    fn emit(self, out: &mut HistorySeries) {
        // Raw samples and clock changes can produce more timestamps than aligned bins.
        if out.ts.len() >= MAX_BUCKETS || (out.ts.len() + 1) * self.bands.len() > MAX_QUERY_CELLS {
            return;
        }
        out.ts.push(self.ts);
        out.span.push(self.span);
        if self.sample || !out.sample.is_empty() {
            out.sample.resize(out.ts.len() - 1, false);
            out.sample.push(self.sample);
        }
        out.frame.push(self.frame);
        out.peak.push(self.peak);
        for (b, col) in self.bands.into_iter().zip(out.series.values_mut()) {
            col.mean.push((b.n > 0).then(|| b.sum / f64::from(b.n)));
            if out.bucket >= 10_000 || self.span >= MINUTE_MS || !col.low.is_empty() {
                if col.low.len() + 1 < col.mean.len() {
                    col.low.resize(col.mean.len() - 1, None);
                    col.high.resize(col.mean.len() - 1, None);
                }
                col.low.push(finite(b.low));
                col.high.push(finite(b.high));
            }
        }
    }
}
fn align(ts: i64, origin: i64, bucket: i64) -> i64 {
    ts.saturating_sub((ts.rem_euclid(bucket) - origin.rem_euclid(bucket)).rem_euclid(bucket))
}
fn gaps(dir: &Path, from: i64, to: i64) -> Vec<Gap> {
    let mut out = Vec::new();
    visit_gaps(dir, &days(dir), from, to, |g| {
        out.push(g);
        out.len() < MAX_BUCKETS
    });
    out
}
fn visit_gaps(dir: &Path, all: &[i64], from: i64, to: i64, mut visit: impl FnMut(Gap) -> bool) {
    let first = all.partition_point(|&d| d < from.div_euclid(DAY_MS));
    let end = all.partition_point(|&d| d <= to.div_euclid(DAY_MS));
    let older = || {
        all[..first]
            .iter()
            .rev()
            .find_map(|&d| index(&day_path(dir, d))?.gap_end())
    };
    let mut prev = None;
    let crossing =
        |a: Option<Stamp>, b: Option<Stamp>| gap(a?, b?).filter(|g| g.from <= to && g.to >= from);
    for &day in all.get(first..end).unwrap_or_default() {
        let Some(idx) = index(&day_path(dir, day)) else {
            continue;
        };
        if let Some(s) = idx.gap_start() {
            if prev.is_none() && s.ts >= from {
                prev = older();
            }
            if let Some(g) = crossing(prev, Some(s))
                && !visit(g)
            {
                return;
            }
            prev = idx.gap_end();
        }
        let begin = idx.gaps.partition_point(|g| g.to < from);
        for &g in idx.gaps[begin..].iter().take_while(|g| g.from <= to) {
            if !visit(g) {
                return;
            }
        }
    }
    if let Some(a) = prev.or_else(older).filter(|s| s.ts + s.span <= to) {
        let next = all[end..]
            .iter()
            .find_map(|&d| index(&day_path(dir, d))?.gap_start());
        let session = live(dir).first.filter(|s| s.ts > a.ts);
        let next = session
            .filter(|s| next.is_none_or(|b| s.ts < b.ts))
            .or(next);
        if let Some(g) = crossing(Some(a), next) {
            visit(g);
        } else if next.is_none() && a.stop == 2 && a.ts + a.span < to {
            visit(Gap {
                from: a.ts + a.span,
                to,
                reason: "off",
            });
        }
    }
}
pub fn series(
    dir: &Path,
    from: i64,
    to: i64,
    bucket: i64,
    origin: i64,
    keys: &[String],
) -> HistorySeries {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    if keys.len() > MAX_CHANNELS || keys.iter().any(|k| k.len() > MAX_METADATA_BYTES) {
        return HistorySeries {
            bucket,
            settled: from,
            ..Default::default()
        };
    }
    let mut out = HistorySeries {
        bucket,
        settled: from,
        series: keys
            .iter()
            .cloned()
            .map(|k| (k, Column::default()))
            .collect(),
        ..Default::default()
    };
    if to < from
        || !matches!(
            bucket,
            1_000
                | 2_000
                | 5_000
                | 10_000
                | 15_000
                | 30_000
                | 60_000
                | 120_000
                | 300_000
                | 600_000
                | 900_000
                | 1_800_000
                | 3_600_000
                | 7_200_000
                | 10_800_000
                | 21_600_000
        )
    {
        return out;
    }
    let first = align(from, origin, bucket);
    let buckets = align(to, origin, bucket).abs_diff(first) / bucket as u64 + 1;
    if buckets > MAX_BUCKETS as u64 || buckets * out.series.len() as u64 > MAX_QUERY_CELLS as u64 {
        return out;
    }
    let live = live(dir);
    let cut = live
        .samples
        .front()
        .map(|s| {
            if !live.evicted && bucket <= FRAME_MS {
                return s.ts;
            }
            let width = if bucket == 2_000 {
                10_000
            } else {
                bucket.max(FRAME_MS)
            };
            align(s.ts.saturating_add(width - 1), origin, width)
        })
        .unwrap_or(i64::MAX);
    let keys: Vec<_> = out.series.keys().cloned().collect();
    let mut acc: Option<Bucket> = None;
    let indices = |catalog: &Catalog| {
        keys.iter()
            .map(|k| catalog.by_key.get(k).copied().unwrap_or(usize::MAX))
            .collect::<Vec<_>>()
    };
    if first < cut {
        let limit = to.min(cut.saturating_sub(1));
        for day in days(dir)
            .into_iter()
            .filter(|&day| day >= first.div_euclid(DAY_MS) && day <= limit.div_euclid(DAY_MS))
        {
            let path = day_path(dir, day);
            let Some(idx) = index(&path) else { continue };
            let first_ts = if idx.version == MINUTE_VERSION && bucket < MINUTE_MS {
                align(first, 0, MINUTE_MS)
            } else {
                first
            };
            let range = idx.range(day, first_ts, limit);
            if range.is_empty() {
                continue;
            }
            if idx.version == MINUTE_VERSION {
                out.merged_days.push(day * DAY_MS);
            }
            let ids = indices(&idx.catalog);
            let peak = idx
                .catalog
                .by_key
                .get("cpuPeak")
                .copied()
                .unwrap_or(usize::MAX);
            let raw_frames = |range: std::ops::Range<usize>,
                              acc: &mut Option<Bucket>,
                              out: &mut HistorySeries| {
                let Some(d) = DayRead::by_frames(&path, idx.clone(), range, false) else {
                    return;
                };
                for i in d.range.clone() {
                    let Some(r) = d.row(i) else { continue };
                    let ts = r.ts();
                    let resolution = if idx.version == MINUTE_VERSION {
                        MINUTE_MS
                    } else {
                        FRAME_MS
                    };
                    let raw = bucket < resolution;
                    if raw && ts < from && idx.version != MINUTE_VERSION {
                        continue;
                    }
                    let bin = bucket_at(
                        acc,
                        out,
                        if raw { ts } else { align(ts, origin, bucket) },
                        if raw { resolution } else { bucket },
                        keys.len(),
                    );
                    if let Reading::Minute(_, bands) = &r {
                        bin.update(ts, r.get(peak), std::iter::empty());
                        for (b, j) in bin.bands.iter_mut().zip(&ids) {
                            if let Some(v) = bands.get(*j).filter(|b| b.n > 0) {
                                b.n = b.n.saturating_add(v.n);
                                b.sum += v.sum;
                                b.low = b.low.min(v.low);
                                b.high = b.high.max(v.high);
                            }
                        }
                    } else {
                        bin.update(ts, r.get(peak), ids.iter().map(|&j| r.get(j)));
                    }
                }
            };
            let mut cursor = range.start;
            if bucket >= ROLL_MS {
                for summary in &idx.summaries {
                    if summary.first < cursor || summary.last > range.end {
                        continue;
                    }
                    let ts = align(summary.start_ts, origin, bucket);
                    if align(summary.last_ts, origin, bucket) != ts {
                        continue;
                    }
                    raw_frames(cursor..summary.first, &mut acc, &mut out);
                    bucket_at(&mut acc, &mut out, ts, bucket, keys.len()).fold_roll(summary, &ids);
                    cursor = summary.last;
                }
            }
            raw_frames(cursor..range.end, &mut acc, &mut out);
        }
    }
    let mut last_catalog = None;
    let mut ids = Vec::new();
    let mut peak = usize::MAX;
    for s in live
        .samples
        .iter()
        .filter(|s| s.ts >= first.max(cut) && s.ts <= to)
    {
        if bucket == 1000 && s.ts < from {
            continue;
        }
        if last_catalog.is_none_or(|c| !Arc::ptr_eq(c, &s.catalog)) {
            let lookup = |key: &str| {
                s.catalog
                    .iter()
                    .position(|c| c.key == key)
                    .unwrap_or(usize::MAX)
            };
            ids = keys.iter().map(|k| lookup(k)).collect();
            peak = lookup("cpuPeak");
            last_catalog = Some(&s.catalog);
        }
        let raw = bucket == 1000;
        let frame = if live.frames.binary_search(&s.slot).is_ok() {
            s.slot
        } else {
            0
        };
        let get = |i| s.values.get(i).copied().unwrap_or(f64::NAN);
        let bin = bucket_at(
            &mut acc,
            &mut out,
            if raw {
                s.ts
            } else {
                align(s.ts, origin, bucket)
            },
            if raw { s.span } else { bucket },
            keys.len(),
        );
        bin.sample = raw;
        bin.update(frame, get(peak), ids.iter().map(|&j| get(j)));
    }
    if let Some(b) = acc {
        b.emit(&mut out);
    }
    out.gaps = if live.samples.front().is_some_and(|s| s.ts < from) {
        Vec::new()
    } else {
        gaps(dir, from, to)
    };
    for g in live.gaps.iter().filter(|g| g.from <= to && g.to >= from) {
        if out.gaps.last().is_none_or(|last| last.to < g.to) {
            out.gaps.push(*g);
        }
    }
    let latest = live.samples.back().map(|s| s.slot).unwrap_or(to);
    out.settled = align(latest.min(to), origin, bucket).max(first);
    out
}
pub fn channels(dir: &Path, from: i64, to: i64) -> Vec<HistoryChannel> {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    if from > to {
        return Vec::new();
    }
    let mut result = BTreeMap::<String, HistoryChannel>::new();
    let mut merge = |row: &HistoryChannel, first: i64, last: i64, label: &str| {
        if result.len() >= MAX_POINTS && !result.contains_key(&row.key) {
            return false;
        }
        let c = result
            .entry(row.key.clone())
            .or_insert_with(|| HistoryChannel {
                first,
                last,
                ..row.clone()
            });
        c.first = c.first.min(first);
        if last >= c.last {
            c.last = last;
            if c.label != label {
                c.label = label.into();
            }
        }
        true
    };
    for day in days(dir)
        .into_iter()
        .filter(|&d| d >= from.div_euclid(DAY_MS) && d <= to.div_euclid(DAY_MS))
    {
        let Some(idx) = index(&day_path(dir, day)) else {
            continue;
        };
        let start = day * DAY_MS;
        let from = if idx.version == MINUTE_VERSION {
            align(from, 0, MINUTE_MS)
        } else {
            from
        };
        let ts = |i: usize| start + i64::from(idx.frames[i].0);
        let first = idx
            .frames
            .partition_point(|&(t, _)| start + i64::from(t) < from);
        let end = idx
            .frames
            .partition_point(|&(t, _)| start + i64::from(t) <= to);
        if end <= first {
            continue;
        }
        let mut bounds = vec![None; idx.catalog.rows.len()];
        let mut observe = |j: usize, a: usize, b: usize| {
            bounds[j].get_or_insert((ts(a), ts(a))).1 = ts(b);
        };
        for (m, (begin, mask)) in idx.masks.iter().enumerate() {
            let a = first.max(*begin);
            let b = end.min(idx.masks.get(m + 1).map_or(end, |e| e.0));
            if a >= b {
                continue;
            }
            for j in 0..idx.catalog.rows.len() {
                if mask
                    .get(j / 8)
                    .is_some_and(|byte| byte & (1 << (j % 8)) != 0)
                {
                    observe(j, a, b - 1);
                }
            }
        }
        for (j, bounds) in bounds.into_iter().enumerate() {
            if let Some((first, last)) = bounds {
                let c = &idx.catalog.rows[j];
                let label = idx
                    .labels
                    .iter()
                    .rev()
                    .find(|(t, i, _)| *i == j && *t <= last)
                    .map_or(c.label.as_str(), |e| e.2.as_str());
                if !merge(c, first, last, label) {
                    return Vec::new();
                }
            }
        }
    }
    let live = live(dir);
    let (front, back) = live.samples.as_slices();
    for samples in [front, back]
        .into_iter()
        .flat_map(|part| part.chunk_by(|a, b| Arc::ptr_eq(&a.catalog, &b.catalog)))
    {
        let Some(sample) = samples.first() else {
            continue;
        };
        for (j, c) in sample.catalog.iter().enumerate() {
            let mut observed = samples.iter().filter(|s| {
                (from..=to).contains(&s.ts)
                    && s.values.get(j).is_some_and(|&v| v != f64::NEG_INFINITY)
            });
            if let Some(first) = observed.next() {
                let last = observed.next_back().unwrap_or(first);
                if !merge(c, first.ts, last.ts, &c.label) {
                    return Vec::new();
                }
            }
        }
    }
    result.into_values().collect()
}

#[derive(Debug, Default, Serialize)]
pub struct HistoryTable {
    pub recorded: i64,
    pub processes: Vec<TableProcess>,
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableProcess {
    pub name: String,
    pub pid: u32,
    pub create_time: Option<i64>,
    pub cpu: f64,
    pub cpu_max: f64,
    pub mem: u64,
    pub io: u64,
    pub net: Option<u64>,
    pub disk: Option<u64>,
    pub disk_read: Option<u64>,
    pub disk_write: Option<u64>,
    pub gpu: f64,
    pub gpu_mem: Option<u64>,
    pub power: Option<f64>,
    pub threads: Option<u32>,
    pub handles: Option<u32>,
    pub first: i64,
    pub last: i64,
    pub seen: i64,
    pub trend: Vec<Option<f64>>,
}
#[derive(Default, Clone, Copy)]
struct Totals {
    first: i64,
    last: i64,
    seen: i64,
    cpu: f64,
    gpu: f64,
    cpu_max: f64,
    mem: u64,
    mem_sum: f64,
    io: f64,
    net: f64,
    net_ms: i64,
    read: Option<f64>,
    write: Option<f64>,
    disk: f64,
    disk_ms: i64,
    gpu_mem: Option<u64>,
    power: f64,
    power_ms: i64,
    threads: Option<u32>,
    handles: Option<u32>,
}
impl Totals {
    fn observation(m: Metrics, ts: i64, span: i64) -> Self {
        let ms = span as f64;
        Self {
            first: ts,
            last: ts,
            seen: span,
            cpu: m.cpu * ms,
            gpu: m.gpu * ms,
            cpu_max: m.cpu,
            mem: m.mem,
            mem_sum: m.mem as f64 * ms,
            io: m.io * ms,
            net: m.net * ms,
            net_ms: if m.net.is_finite() { span } else { 0 },
            read: m.disk_read.map(|v| v * ms),
            write: m.disk_write.map(|v| v * ms),
            disk: m
                .disk_read
                .zip(m.disk_write)
                .map_or(0., |(r, w)| (r + w) * ms),
            disk_ms: if m.disk_read.is_some() && m.disk_write.is_some() {
                span
            } else {
                0
            },
            gpu_mem: m.gpu_mem,
            power: m.power.unwrap_or_default() * ms,
            power_ms: if m.power.is_some() { span } else { 0 },
            threads: m.threads,
            handles: m.handles,
        }
    }
    fn merge(&mut self, b: Self) {
        if self.seen == 0 {
            *self = b;
            return;
        }
        self.first = self.first.min(b.first);
        self.last = self.last.max(b.last);
        self.seen += b.seen;
        self.cpu += b.cpu;
        self.gpu += b.gpu;
        self.cpu_max = self.cpu_max.max(b.cpu_max);
        self.mem = self.mem.max(b.mem);
        self.mem_sum += b.mem_sum;
        self.io += b.io;
        self.net_ms += b.net_ms;
        if b.net.is_finite() {
            self.net = if self.net.is_finite() {
                self.net + b.net
            } else {
                b.net
            };
        }
        for (sum, v) in [(&mut self.read, b.read), (&mut self.write, b.write)] {
            if let Some(v) = v {
                *sum.get_or_insert(0.) += v;
            }
        }
        self.disk += b.disk;
        self.disk_ms += b.disk_ms;
        self.gpu_mem = self.gpu_mem.max(b.gpu_mem);
        self.power += b.power;
        self.power_ms += b.power_ms;
        self.threads = self.threads.max(b.threads);
        self.handles = self.handles.max(b.handles);
    }
    fn trend(&self, key: &str) -> (f64, i64) {
        match key {
            "mem" => (self.mem_sum, self.seen),
            "disk" => (self.disk, self.disk_ms),
            "net" if self.net.is_nan() => (0., 0),
            "net" => (self.net, self.net_ms),
            "gpu" => (self.gpu, self.seen),
            _ => (self.cpu, self.seen),
        }
    }
    fn encode(self, out: &mut Vec<u8>) {
        for v in [
            self.first,
            self.last,
            self.seen,
            self.disk_ms,
            self.power_ms,
        ] {
            var(out, v as u64);
        }
        for v in [
            self.mem,
            self.gpu_mem.map_or(0, |v| v.saturating_add(1)),
            self.threads.map_or(0, |v| u64::from(v) + 1),
            self.handles.map_or(0, |v| u64::from(v) + 1),
        ] {
            var(out, v);
        }
        for v in [
            self.cpu,
            self.gpu,
            self.cpu_max,
            self.mem_sum,
            self.io,
            self.net,
            self.read.unwrap_or(f64::NAN),
            self.write.unwrap_or(f64::NAN),
            self.disk,
            self.power,
        ] {
            out.extend(v.to_le_bytes());
        }
    }
    fn decode(raw: &mut &[u8]) -> Option<Self> {
        let mut n = || take_var(raw);
        let first = n()? as i64;
        let last = n()? as i64;
        let seen = n()? as i64;
        let disk_ms = n()? as i64;
        let power_ms = n()? as i64;
        let mem = n()?;
        let gpu_mem = n()?.checked_sub(1);
        let threads = n()?.checked_sub(1).map(u32::try_from).transpose().ok()?;
        let handles = n()?.checked_sub(1).map(u32::try_from).transpose().ok()?;
        let mut f = || {
            let (v, r) = raw.split_at_checked(8)?;
            *raw = r;
            Some(f64::from_le_bytes(v.try_into().ok()?))
        };
        let mut totals = Self {
            net_ms: 0,
            first,
            last,
            seen,
            disk_ms,
            power_ms,
            mem,
            gpu_mem,
            threads,
            handles,
            cpu: finite(f()?)?,
            gpu: finite(f()?)?,
            cpu_max: finite(f()?)?,
            mem_sum: finite(f()?)?,
            io: finite(f()?)?,
            net: f()?,
            read: finite(f()?),
            write: finite(f()?),
            disk: finite(f()?)?,
            power: finite(f()?)?,
        };
        if totals.net.is_infinite() {
            return None;
        }
        if totals.net.is_finite() {
            totals.net_ms = seen;
        }
        Some(totals)
    }
}
const ROLL_MS: i64 = 900_000;
const PROCESS_ROLL_MS: i64 = 300_000;
#[derive(Default)]
struct Roll {
    first: i64,
    end: i64,
    recorded: i64,
    rows: HashMap<usize, Totals>,
}
impl Roll {
    fn header(p: &[u8], idx: &Index) -> Option<Self> {
        let first = i64::from_le_bytes(field(p, 0)?);
        let end = i64::from_le_bytes(field(p, 8)?);
        let recorded = i64::from_le_bytes(field(p, 16)?);
        if idx.disordered
            || first.div_euclid(DAY_MS) != idx.day
            || end <= first
            || first.div_euclid(PROCESS_ROLL_MS) != (end - 1).div_euclid(PROCESS_ROLL_MS)
            || !(1..=end.checked_sub(first)?).contains(&recorded)
            || first < idx.first?.ts
            || end > idx.last?.ts.checked_add(idx.last?.span)?
            || idx
                .frames
                .binary_search_by_key(&(first.rem_euclid(DAY_MS) as u32), |r| r.0)
                .is_err()
        {
            return None;
        }
        Some(Self {
            first,
            end,
            recorded,
            ..Default::default()
        })
    }
    fn read_rows(&mut self, p: &[u8], idx: &Index) -> Option<()> {
        let mut raw = p.get(24..)?;
        while !raw.is_empty() {
            let id = usize::try_from(take_var(&mut raw)?).ok()?;
            let t = Totals::decode(&mut raw)?;
            if id >= idx.ids.len()
                || !(1..=self.recorded).contains(&t.seen)
                || !(0..=t.seen).contains(&t.disk_ms)
                || !(0..=t.seen).contains(&t.power_ms)
                || t.first < self.first
                || t.last < t.first
                || t.last >= self.end
                || self.rows.insert(id, t).is_some()
            {
                return None;
            }
        }
        Some(())
    }
    fn write(&mut self, out: &mut impl Write) -> io::Result<u64> {
        if self.recorded == 0 {
            return Ok(0);
        }
        let mut p = Vec::new();
        for v in [self.first, self.end, self.recorded] {
            p.extend(v.to_le_bytes());
        }
        for (&id, t) in &self.rows {
            var(&mut p, id as u64);
            t.encode(&mut p);
        }
        packet(out, 1, &p)?;
        out.flush()?;
        self.recorded = 0;
        self.rows.clear();
        Ok((5 + p.len()) as u64)
    }
}
struct TableQuery<'a> {
    from: i64,
    to: i64,
    key: &'a str,
    points: usize,
    recorded: i64,
    rows: HashMap<Identity, Instance>,
    cells: usize,
    name_bytes: usize,
    exhausted: bool,
}
#[derive(Default)]
struct Instance {
    totals: Totals,
    trend: Vec<(usize, f64, i64)>,
}
impl TableQuery<'_> {
    fn new(from: i64, to: i64, key: &str, points: usize) -> TableQuery<'_> {
        TableQuery {
            from,
            to,
            key,
            points,
            cells: 0,
            name_bytes: 0,
            exhausted: false,
            recorded: 0,
            rows: HashMap::new(),
        }
    }
    fn bin(&self, ts: i64) -> usize {
        (((i128::from(ts) - i128::from(self.from)) * self.points as i128
            / (i128::from(self.to) - i128::from(self.from))) as usize)
            .min(self.points.saturating_sub(1))
    }
    fn bin_end(&self, bin: usize, finish: i64) -> i64 {
        if self.points == 0 {
            return finish;
        }
        let delta = i128::from(self.to) - i128::from(self.from);
        let bound = i128::from(self.from)
            + (delta * (bin as i128 + 1) + self.points as i128 - 1) / self.points as i128;
        finish.min(bound as i64)
    }
    fn add(&mut self, id: &Identity, t: Totals, bin: usize) {
        if self.exhausted {
            return;
        }
        let full = self.rows.len() >= MAX_INSTANCES;
        let row = if let Some(r) = self.rows.get_mut(id) {
            r
        } else {
            self.name_bytes += id.name.len();
            if full || self.name_bytes > 32 << 20 {
                self.exhausted = true;
                return;
            }
            self.rows.entry(id.clone()).or_default()
        };
        row.totals.merge(t);
        if self.points != 0 {
            let (v, n) = t.trend(self.key);
            if n > 0 {
                if let Some(last) = row.trend.last_mut().filter(|last| last.0 == bin) {
                    last.1 += v;
                    last.2 += n;
                } else {
                    if self.cells >= MAX_QUERY_CELLS {
                        self.exhausted = true;
                        return;
                    }
                    self.cells += 1;
                    row.trend.push((bin, v, n));
                }
            }
        }
    }
    fn detail(&mut self, path: &Path, day: i64, idx: Arc<Index>, start: i64, end: i64) {
        if end <= start {
            return;
        }
        let resolution = if idx.version == MINUTE_VERSION {
            MINUTE_MS
        } else {
            FRAME_MS
        };
        let Some(d) = DayRead::open(
            path,
            day,
            idx,
            start.saturating_sub(resolution - 1),
            end - 1,
            true,
        ) else {
            return;
        };
        for i in d.range.clone() {
            if self.exhausted {
                return;
            }
            if d.idx.version == MINUTE_VERSION {
                if let Some(m) = d.minute(i) {
                    m.table(self, &d.idx, start, end);
                }
                continue;
            }
            let Some(r) = d.row(i) else { continue };
            let ts = r.ts();
            let mut a = ts.max(start);
            let finish = (ts + r.span()).min(end);
            if a >= finish {
                continue;
            }
            let rows = match &r {
                Reading::New(f, _) => d.metrics(f).unwrap_or_default(),
                Reading::Minute(_, _) => Vec::new(),
            };
            self.recorded += finish - a;
            while a < finish {
                let bin = self.bin(a);
                let next = self.bin_end(bin, finish);
                let span = next - a;
                match &r {
                    Reading::New(_, _) => {
                        for &(j, m) in &rows {
                            self.add(&d.idx.ids[j], Totals::observation(m, ts, span), bin);
                        }
                    }
                    Reading::Minute(_, _) => (),
                }
                a = next;
            }
        }
    }
    fn day(&mut self, path: &Path, day: i64, idx: Arc<Index>) {
        let mut start = self.from.max(day * DAY_MS);
        let end = self.to.min((day + 1) * DAY_MS);
        if end - start >= PROCESS_ROLL_MS
            && idx.version == VERSION
            && let Ok(bytes) =
                File::open(path.with_extension("roll")).and_then(|mut f| read_bounded(&mut f))
        {
            packets(&bytes, 0, |_, tag, p| {
                if self.exhausted || tag != 1 {
                    return false;
                }
                let Some(mut roll) = Roll::header(p, &idx) else {
                    return false;
                };
                let (a, b, n) = (roll.first, roll.end, roll.recorded);
                if a < start || b > end || self.bin(a) != self.bin(b - 1) {
                    return true;
                }
                if roll.read_rows(p, &idx).is_none() {
                    return false;
                }
                let bin = self.bin(a);
                self.detail(path, day, idx.clone(), start, a);
                self.recorded += n;
                for (j, t) in roll.rows {
                    self.add(&idx.ids[j], t, bin);
                }
                start = b;
                true
            });
        }
        self.detail(path, day, idx, start, end);
    }
}
/// CPU and GPU are time-weighted over all recorded time. An omitted instance
/// contributes zero. Trends average only its observed intervals; absent bins are null.
pub fn table(dir: &Path, from: i64, to: i64, trend_key: &str, trend_points: usize) -> HistoryTable {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    if from >= to || trend_points > MAX_POINTS {
        return HistoryTable::default();
    }
    let mut q = TableQuery::new(from, to, trend_key, trend_points);
    for day in days(dir)
        .into_iter()
        .filter(|&d| d >= from.div_euclid(DAY_MS) && d <= (to - 1).div_euclid(DAY_MS))
    {
        let path = day_path(dir, day);
        if let Some(idx) = index(&path) {
            q.day(&path, day, idx);
            if q.exhausted {
                return HistoryTable::default();
            }
        }
    }
    let mut rows: Vec<_> = q.rows.into_iter().collect();
    rows.sort_unstable_by(|(ia, a), (ib, b)| {
        b.totals
            .cpu
            .total_cmp(&a.totals.cpu)
            .then_with(|| b.totals.io.total_cmp(&a.totals.io))
            .then_with(|| b.totals.mem.cmp(&a.totals.mem))
            .then_with(|| ia.pid.cmp(&ib.pid))
            .then_with(|| ia.created.cmp(&ib.created))
            .then_with(|| ia.name.cmp(&ib.name))
    });
    let recorded = q.recorded;
    let processes = rows
        .into_iter()
        .take(500)
        .map(|(id, row)| {
            let t = row.totals;
            let bytes = |v: f64| (v / 1000.).round() as u64;
            TableProcess {
                name: id.name.to_string(),
                pid: id.pid,
                create_time: (id.created > 0).then_some(id.created),
                cpu: t.cpu / recorded as f64,
                cpu_max: t.cpu_max,
                mem: t.mem,
                io: bytes(t.io),
                net: finite(t.net).map(bytes),
                disk: (t.disk_ms > 0).then(|| bytes(t.disk)),
                disk_read: t.read.map(bytes),
                disk_write: t.write.map(bytes),
                gpu: t.gpu / recorded as f64,
                gpu_mem: t.gpu_mem,
                power: (t.power_ms > 0).then(|| t.power / t.power_ms as f64),
                threads: t.threads,
                handles: t.handles,
                first: t.first,
                last: t.last,
                seen: t.seen,
                trend: {
                    let mut points = vec![None; trend_points];
                    for (bin, v, n) in row.trend {
                        points[bin] = Some(v / n as f64);
                    }
                    points
                },
            }
        })
        .collect();
    HistoryTable {
        recorded,
        processes,
    }
}

// Embedded quarter-hour statistics cover finished groups of five-second frames.
// They accelerate broad queries without changing retention or frame detail.
const SYSTEM_ROLL: u8 = 7;
const PRESENCE: u8 = 8;
#[derive(Default)]
struct SystemRoll {
    start: i64,
    last: i64,
    count: u32,
    peak: i64,
    max: f64,
    bands: Vec<Band>,
}
impl SystemRoll {
    fn add(&mut self, ts: i64, values: &[f64], peak: f32) {
        if self.count == 0 {
            self.start = ts;
            self.max = f64::NEG_INFINITY;
        }
        self.count += 1;
        self.last = ts;
        if f64::from(peak) > self.max {
            self.max = f64::from(peak);
            self.peak = ts;
        }
        self.bands.resize(values.len(), Band::default());
        for (b, &v) in self.bands.iter_mut().zip(values) {
            b.add(v);
        }
    }
    fn finish(&mut self, out: &mut Vec<u8>) -> io::Result<()> {
        if self.count == 0 {
            return Ok(());
        }
        let mut bytes = Vec::new();
        for v in [self.start, self.last, self.peak] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend(self.count.to_le_bytes());
        bytes.extend(self.max.to_le_bytes());
        for b in &self.bands {
            bytes.extend(b.n.to_le_bytes());
            for v in [b.sum, b.low, b.high] {
                bytes.extend(v.to_le_bytes());
            }
        }
        packet(out, SYSTEM_ROLL, &bytes)?;
        self.count = 0;
        self.bands.clear();
        Ok(())
    }
}
#[derive(Clone)]
struct RollRef {
    first: usize,
    last: usize,
    start_ts: i64,
    last_ts: i64,
    peak: i64,
    max: f64,
    bands: Box<[Band]>,
}
impl RollRef {
    fn parse(p: &[u8], idx: &Index) -> Option<Self> {
        let start_ts = i64::from_le_bytes(field(p, 0)?);
        let last_ts = i64::from_le_bytes(field(p, 8)?);
        let peak = i64::from_le_bytes(field(p, 16)?);
        let count = u32::from_le_bytes(field(p, 24)?);
        let max = finite(f64::from_le_bytes(field(p, 28)?))?;
        let last = idx.frames.len();
        let first = last.checked_sub(count as usize)?;
        let frames = idx.frames.get(first..last)?;
        let (values, tail) = p.get(36..)?.as_chunks::<28>();
        if !tail.is_empty()
            || values.len() != idx.catalog.rows.len()
            || start_ts.div_euclid(DAY_MS) != idx.day
            || last_ts.div_euclid(DAY_MS) != idx.day
            || start_ts.div_euclid(ROLL_MS) != last_ts.div_euclid(ROLL_MS)
            || !(start_ts..=last_ts).contains(&peak)
            || i64::from(frames.first()?.0) != start_ts.rem_euclid(DAY_MS)
            || i64::from(frames.last()?.0) != last_ts.rem_euclid(DAY_MS)
            || frames
                .binary_search_by_key(&(peak.rem_euclid(DAY_MS) as u32), |e| e.0)
                .is_err()
            || idx.summaries.last().is_some_and(|r| r.last > first)
        {
            return None;
        }
        let mut bands = Vec::with_capacity(values.len());
        for v in values {
            let n = u32::from_le_bytes(field(v, 0)?);
            let sum = finite(f64::from_le_bytes(field(v, 4)?))?;
            let low = f64::from_le_bytes(field(v, 12)?);
            let high = f64::from_le_bytes(field(v, 20)?);
            if n > count
                || (n > 0 && (!low.is_finite() || !high.is_finite() || low > high))
                || (n == 0 && (sum != 0. || low != f64::INFINITY || high != f64::NEG_INFINITY))
            {
                return None;
            }
            bands.push(Band { n, sum, low, high });
        }
        Some(Self {
            first,
            last,
            start_ts,
            last_ts,
            peak,
            max,
            bands: bands.into_boxed_slice(),
        })
    }
}
impl Bucket {
    fn fold_roll(&mut self, r: &RollRef, ids: &[usize]) {
        if self.frame == 0 {
            self.frame = r.start_ts;
        }
        if r.max > self.max {
            self.max = r.max;
            self.peak = r.peak;
        }
        for (band, &id) in self.bands.iter_mut().zip(ids) {
            if let Some(b) = r.bands.get(id) {
                band.n = band.n.saturating_add(b.n);
                band.sum += b.sum;
                band.low = band.low.min(b.low);
                band.high = band.high.max(b.high);
            }
        }
    }
}

fn bucket_at<'a>(
    acc: &'a mut Option<Bucket>,
    out: &mut HistorySeries,
    ts: i64,
    span: i64,
    n: usize,
) -> &'a mut Bucket {
    if acc.as_ref().is_some_and(|b| b.ts != ts)
        && let Some(b) = acc.take()
    {
        b.emit(out);
    }
    acc.get_or_insert_with(|| Bucket::new(ts, span, n))
}

// Checkpoints contain only frame addresses, metadata and system summaries.
// A run ends at a gap or metadata change. Its two clocks suffice to join it
// to the adjacent runs; reading an older day never decodes every system frame.
const FRAME_RUN: u8 = 9;
/// A run of frames each followed by its program counts (`PROGRAMS`): the
/// same record, at a stride that includes them.
const FRAME_RUN_COUNTED: u8 = 16;
#[derive(Default)]
struct AddressRun {
    first: Option<Stamp>,
    last: Stamp,
    refs: Vec<u8>,
    proc_end: u64,
    /// Its frames are each followed by their program counts.
    counted: bool,
}
impl AddressRun {
    fn finish(&mut self, bytes: &mut Vec<u8>) {
        let Some(first) = self.first.take() else {
            return;
        };
        let mut body = Vec::with_capacity(48 + self.refs.len());
        for s in [first, self.last] {
            body.extend(s.ts.to_le_bytes());
            body.extend(s.awake.to_le_bytes());
            body.extend((s.span as u32).to_le_bytes());
        }
        body.extend(self.proc_end.to_le_bytes());
        body.append(&mut self.refs);
        bytes.push(if std::mem::take(&mut self.counted) {
            FRAME_RUN_COUNTED
        } else {
            FRAME_RUN
        });
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(body);
    }
}
struct Checkpoints {
    written: u64,
    out: BufWriter<File>,
    base: usize,
    end: usize,
    pending: Vec<u8>,
    run: AddressRun,
    path: PathBuf,
}
impl Checkpoints {
    fn open(path: PathBuf, main: &[u8]) -> io::Result<Self> {
        let file = open_writable(&path)?;
        file.set_len(0)?;
        // Rebuild the optional index from the primary's verified record boundaries.
        // A same-size edited index may end in the middle of a primary packet.
        let mut cp = Self {
            written: 0,
            out: BufWriter::new(file),
            base: 5,
            end: 5,
            pending: Vec::new(),
            run: AddressRun::default(),
            path,
        };
        let mut remaining = main.get(5..).ok_or_else(invalid)?;
        while !remaining.is_empty() {
            let (_, p) = next_packet(remaining).ok_or_else(invalid)?;
            let (record, tail) = remaining.split_at(5 + p.len());
            cp.capture(record)?;
            remaining = tail;
        }
        Ok(cp)
    }
    fn capture(&mut self, main: &[u8]) -> io::Result<()> {
        let mut summary = false;
        let end = self
            .end
            .checked_add(main.len())
            .filter(|&v| v <= MAX_FILE_BYTES)
            .ok_or_else(invalid)?;
        let used = packets(main, 0, |at, tag, p| {
            if tag == FRAME {
                let Some(f) = Stored::parse(p) else {
                    return false;
                };
                let stamp = Stamp {
                    ts: f.ts,
                    awake: f.awake,
                    span: f.span,
                    ..Default::default()
                };
                if self.run.first.is_some()
                    && self.run.last.ts.checked_add(self.run.last.span) != Some(f.ts)
                {
                    self.run.finish(&mut self.pending);
                }
                self.run.first.get_or_insert(stamp);
                self.run.last = stamp;
                self.run.proc_end = f.offset + f.len as u64;
                self.run
                    .refs
                    .extend((f.ts.rem_euclid(DAY_MS) as u32).to_le_bytes());
                self.run.refs.extend(((self.end + at) as u32).to_le_bytes());
            } else if tag == PROGRAMS {
                // Read with their frame from the main file: copied, every
                // frame would end a run. The run records that its frames
                // have them (`FRAME_RUN_COUNTED`).
                self.run.counted = true;
            } else {
                self.run.finish(&mut self.pending);
                self.pending.extend(&main[at..at + 5 + p.len()]);
                summary |= tag == SYSTEM_ROLL;
            }
            true
        });
        if used != main.len() {
            return Err(invalid());
        }
        self.end = end;
        if summary || self.end - self.base >= 512 * 1024 {
            self.commit()?;
        }
        Ok(())
    }
    fn commit(&mut self) -> io::Result<()> {
        if self.base == self.end {
            return Ok(());
        }
        self.run.finish(&mut self.pending);
        let mut body = Vec::with_capacity(8 + self.pending.len());
        body.extend((self.base as u32).to_le_bytes());
        body.extend((self.end as u32).to_le_bytes());
        body.append(&mut self.pending);
        packet(&mut self.out, 1, &body)?;
        self.written += (body.len() + 5) as u64;
        self.out.flush()?;
        self.base = self.end;
        Ok(())
    }
}
impl DayWriter {
    fn index_append(&mut self, bytes: &[u8]) {
        if let Some(cp) = &mut self.checkpoints
            && let Err(e) = cp.capture(bytes)
        {
            crate::diagnostics::file_failure("history index append", &cp.path, &e);
            self.checkpoints = None;
        }
    }
}
impl DayFile {
    fn marker(&mut self, kind: u8, ts: i64, boot: i64, awake: i64) -> io::Result<()> {
        let mut bytes = Vec::new();
        write_marker(&mut bytes, kind, ts, boot, awake)?;
        self.out.write_all(&bytes)?;
        self.out.flush()?;
        self.writer.index_append(&bytes);
        Ok(())
    }
}
impl Index {
    fn load_checkpoints(&mut self, path: &Path, main_len: u64) {
        let Some(bytes) = File::open(path.with_extension("idx"))
            .ok()
            .and_then(|mut f| read_bounded(&mut f).ok())
        else {
            return;
        };
        self.checkpoints(&bytes, main_len);
    }

    fn checkpoints(&mut self, bytes: &[u8], main_len: u64) {
        let mut bad = false;
        packets(bytes, 0, |_, tag, p| {
            let Some((header, records)) = p.split_first_chunk::<8>() else {
                return false;
            };
            let [lo, hi] = header.as_chunks::<4>().0 else {
                return false;
            };
            let base = u32::from_le_bytes(*lo) as usize;
            let end = u32::from_le_bytes(*hi) as usize;
            if tag != 1 || base != self.valid || end <= base || end as u64 > main_len {
                return false;
            }
            if self.scan(records, u64::MAX, Some(end)) != records.len() || self.valid != end {
                bad = true;
                return false;
            }
            true
        });
        if bad {
            *self = Self {
                day: self.day,
                version: VERSION,
                valid: 5,
                ..Default::default()
            };
        }
    }

    fn address_run(&mut self, p: &[u8], limit: usize, counted: bool) -> bool {
        let Some((header, refs)) = p.split_first_chunk::<48>() else {
            return false;
        };
        let (refs, tail) = refs.as_chunks::<8>();
        if refs.is_empty() || !tail.is_empty() || refs.len() > MAX_FRAMES - self.frames.len() {
            return false;
        }
        let stamp = |i| -> Option<Stamp> {
            let ts = i64::from_le_bytes(field(header, i)?);
            let span = i64::from(u32::from_le_bytes(field(header, i + 16)?));
            let awake = i64::from_le_bytes(field(header, i + 8)?);
            (ts.div_euclid(DAY_MS) == self.day && (1..=FRAME_MS).contains(&span)).then_some(Stamp {
                ts,
                span,
                awake,
                boot: self.boot,
                stop: 0,
            })
        };
        let (Some(first), Some(last), Some(proc_end)) = (
            stamp(0),
            stamp(20),
            field(header, 40).map(u64::from_le_bytes),
        ) else {
            return false;
        };
        // Fall back to the primary scan, which can omit individual repeated frames.
        if self.last.is_some_and(|s| first.ts <= s.ts) {
            return false;
        }
        let base = self.valid;
        let width =
            5 + FRAME_PREFIX + self.catalog.bytes + if counted { 5 + PROGRAMS_LEN } else { 0 };
        let Some(end) = refs
            .len()
            .checked_mul(width)
            .and_then(|n| base.checked_add(n))
            .filter(|&n| n <= limit)
        else {
            return false;
        };
        let mut prev = None;
        for (i, pair) in refs.iter().enumerate() {
            let [t, off] = pair.as_chunks::<4>().0 else {
                return false;
            };
            let ts = u32::from_le_bytes(*t) as i64;
            if ts >= DAY_MS
                || u32::from_le_bytes(*off) as usize != base + i * width
                || prev.is_some_and(|a| !(1..=FRAME_MS).contains(&(ts - a)))
                || (i == 0 && ts != first.ts.rem_euclid(DAY_MS))
                || (i == 1 && ts != first.ts.rem_euclid(DAY_MS) + first.span)
                || (i + 1 == refs.len() && ts != last.ts.rem_euclid(DAY_MS))
            {
                return false;
            }
            prev = Some(ts);
        }
        if proc_end < self.proc_end
            || proc_end - self.proc_end > (refs.len() * MAX_FRAME_ROWS * 160) as u64
        {
            return false;
        }
        self.stamp(first, base);
        for (i, r) in refs.iter().enumerate().skip(1) {
            let [t, ..] = r.as_chunks::<4>().0 else {
                return false;
            };
            self.frames
                .push((u32::from_le_bytes(*t), (base + i * width) as u32));
        }
        self.proc_end = proc_end;
        self.last = Some(last);
        self.valid = end;
        // The counts a scan of the primary would have read with each frame.
        if counted {
            self.process_bytes += (refs.len() * (5 + PROGRAMS_LEN)) as u64;
        }
        true
    }
    fn range(&self, day: i64, from: i64, to: i64) -> std::ops::Range<usize> {
        let start = day * DAY_MS;
        self.frames
            .partition_point(|&(t, _)| start + i64::from(t) < from)
            ..self
                .frames
                .partition_point(|&(t, _)| start + i64::from(t) <= to)
    }
}

#[cfg(test)]
mod tests {
    mod damage;
    mod events;
    mod retention;
    use super::*;
    use crate::hardware::Hardware;
    use crate::process::ProcessRow;
    use crate::system::SystemStats;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("argus-history-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        prepare_directory(&dir, None).unwrap();
        dir
    }

    fn proc(pid: u32, name: &str, cpu: f32, mem: u64) -> ProcessRow {
        ProcessRow {
            pid,
            create_time: 1,
            name: name.into(),
            cpu,
            private_ws: mem,
            working_set: mem,
            commit: mem,
            gpu_dedicated: Some(0),
            gpu_shared: Some(0),
            threads: 1,
            handles: 1,
            session_id: 1,
            base_priority: 8,
            ..ProcessRow::default()
        }
    }

    fn snapshot(ts: i64, cpu: f32, procs: Vec<ProcessRow>) -> Snapshot {
        Snapshot {
            seq: 0,
            timestamp: ts,
            system: SystemStats {
                cpu,
                mem_used: 8 << 30,
                mem_total: 16 << 30,
                ..Default::default()
            },
            hardware: Hardware::default(),
            processes: procs,
            sessions: Vec::new(),
            leaks: Vec::new(),
            process_network: false,
        }
    }

    fn battery(percent: f32, rate: Option<i32>) -> crate::hardware::BatteryStats {
        crate::hardware::BatteryStats {
            info: std::sync::Arc::new(crate::hardware::BatteryInfo {
                id: "0".to_string(),
                name: String::new(),
                manufacturer: String::new(),
                serial: String::new(),
                chemistry: String::new(),
                designed_capacity: None,
                full_capacity: None,
                cycle_count: None,
                manufactured: None,
            }),
            percent,
            capacity: None,
            rate,
            voltage: None,
            state: if rate.is_some_and(|r| r < 0) {
                "discharging"
            } else {
                "charging"
            },
            plugged_in: false,
            critical: false,
            remaining: None,
            temperature: None,
        }
    }

    #[test]
    fn bucket_addresses_a_real_frame_and_the_earliest_cpu_peak() {
        let dir = temp_dir("peak-frame");
        let base = 20_738 * DAY_MS - FRAME_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for (offset, cpu, name) in [
            (0, 98., "burst.exe"),
            (5000, 2., "quiet.exe"),
            (10000, 2., "quiet.exe"),
            (15000, 0., "end.exe"),
        ] {
            rec.record(&snapshot(
                base + offset,
                cpu,
                vec![proc(42, name, cpu, 1024)],
            ));
        }
        let bucket = legacy_series(&dir, base, base + 10000, 1).remove(0);
        assert_eq!((bucket.cpu, bucket.cpu_peak), (34., 98.));
        assert_eq!(bucket.ts, base + 5000);
        assert_eq!(bucket.cpu_peak_ts, base);
        assert_eq!(bucket.frame_ts, base);
        assert_eq!(
            moment(&dir, bucket.cpu_peak_ts).unwrap().processes[0].name,
            "burst.exe"
        );
        // Equal peaks keep the earliest actual frame, including across UTC midnight.
        rec.clear().unwrap();
        for offset in [0, 5000, 10000] {
            rec.record(&snapshot(base + offset, 98., Vec::new()));
        }
        assert_eq!(
            legacy_series(&dir, base, base + 10000, 1)[0].cpu_peak_ts,
            base
        );
        // An average timestamp can lie in a long gap. Its representative cannot.
        rec.clear().unwrap();
        for offset in [0, 600000, 605000] {
            rec.record(&snapshot(
                base + offset,
                1.,
                vec![proc(42, "sample.exe", 1., 1024)],
            ));
        }
        let bucket = legacy_series(&dir, base, base + 7 * DAY_MS, 500).remove(0);
        assert!(moment(&dir, bucket.ts).is_none());
        assert!(moment(&dir, bucket.frame_ts).is_some());
        assert!(moment(&dir, bucket.cpu_peak_ts).is_some());
        drop(rec);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn days_of_other_versions_are_not_read_and_restart_when_recorded() {
        let base = 20_737 * DAY_MS;
        for version in [1u8, 2] {
            let dir = temp_dir(&format!("other-version-{version}"));
            fs::create_dir_all(&dir).unwrap();
            let path = day_path(&dir, 20_737);
            let mut old = b"ARGH".to_vec();
            old.push(version);
            old.extend([2, 0, 0, 0, 0, 1, 2, 3]);
            fs::write(&path, &old).unwrap();
            fs::write(path.with_extension("idx"), [1, 2, 3]).unwrap();
            assert!(frame(&dir, base).is_none());
            assert!(moment(&dir, base).is_none());
            assert!(range(&dir).is_none());
            assert!(channels(&dir, base, base + DAY_MS).is_empty());
            assert!(legacy_series(&dir, base, base + DAY_MS, 10).is_empty());
            let mut rec = HistoryRecorder::new(dir.clone());
            rec.record(&snapshot(base, 3., vec![proc(7, "a.exe", 1., 1024)]));
            rec.record(&snapshot(base + FRAME_MS, 1., Vec::new()));
            drop(rec);
            assert!(fs::read(&path).unwrap().starts_with(MAGIC));
            assert_eq!(frame(&dir, base).unwrap().processes[0].name, "a.exe");
            assert_ne!(
                fs::read(path.with_extension("idx")).ok(),
                Some(vec![1, 2, 3])
            );
            fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn frames_keep_program_counts_of_every_process_in_their_last_sample() {
        let dir = temp_dir("programs");
        let day = 20_737;
        let base = day * DAY_MS;
        let path = day_path(&dir, day);
        let mut processes = Vec::new();
        let mut pid = 100;
        let mut add = |name: &str, n: u32| {
            for _ in 0..n {
                pid += 1;
                processes.push(proc(pid, name, 0., 1024));
            }
        };
        // Two spellings are one program, under the first seen.
        add("svchost.exe", 5);
        add("Chrome.exe", 3);
        add("chrome.exe", 2);
        for name in [
            "i.exe", "b.exe", "a.exe", "h.exe", "c.exe", "g.exe", "d.exe", "f.exe", "e.exe",
        ] {
            add(name, 1);
        }
        let mut rec = HistoryRecorder::new(dir.clone());
        // Only the frame's last sample counts.
        rec.record(&snapshot(base, 1., vec![proc(1, "early.exe", 0., 1024)]));
        rec.record(&snapshot(base + 1000, 1., processes.clone()));
        rec.record(&snapshot(base + FRAME_MS, 1., processes[..5].to_vec()));
        rec.record(&snapshot(base + 2 * FRAME_MS, 1., Vec::new()));
        drop(rec);
        let counts = |list: &[(&str, u32)]| {
            list.iter()
                .map(|&(name, count)| ProgramCount {
                    name: name.into(),
                    count,
                })
                .collect::<Vec<_>>()
        };
        let check = |dir: &Path| {
            let first = frame(dir, base).unwrap();
            assert_eq!(first.program_count, 11);
            assert_eq!(
                first.programs,
                counts(&[
                    ("Chrome.exe", 5),
                    ("svchost.exe", 5),
                    ("a.exe", 1),
                    ("b.exe", 1),
                    ("c.exe", 1),
                    ("d.exe", 1),
                    ("e.exe", 1),
                    ("f.exe", 1),
                ])
            );
            let second = frame(dir, base + FRAME_MS).unwrap();
            assert_eq!(second.programs, counts(&[("svchost.exe", 5)]));
            let json = serde_json::to_value(&second).unwrap();
            assert_eq!(json["programCount"], 1);
            assert_eq!(
                json["programs"],
                serde_json::json!([{"name": "svchost.exe", "count": 5}])
            );
            let empty = frame(dir, base + 2 * FRAME_MS).unwrap();
            assert!(empty.programs.is_empty());
            assert_eq!(empty.program_count, 0);
        };
        check(&dir);
        // Recording on in the same day keeps the dictionary.
        let mut rec = HistoryRecorder::new(dir.clone());
        rec.record(&snapshot(
            base + 3 * FRAME_MS,
            1.,
            vec![
                proc(9, "SVCHOST.EXE", 0., 1024),
                proc(10, "new.exe", 0., 1024),
            ],
        ));
        rec.record(&snapshot(base + 4 * FRAME_MS, 1., Vec::new()));
        drop(rec);
        let later = counts(&[("new.exe", 1), ("svchost.exe", 1)]);
        assert_eq!(frame(&dir, base + 3 * FRAME_MS).unwrap().programs, later);
        // The checkpoints hold the dictionary but not the counts of every
        // frame, whose runs they mark; read through them and without them.
        let mut tags = Vec::new();
        packets(
            &fs::read(path.with_extension("idx")).unwrap(),
            0,
            |_, _, p| {
                packets(p.get(8..).unwrap_or_default(), 0, |_, tag, _| {
                    tags.push(tag);
                    true
                });
                true
            },
        );
        assert!(tags.contains(&PROGRAM) && tags.contains(&FRAME_RUN_COUNTED));
        assert!(!tags.contains(&PROGRAMS));
        forget(&dir);
        check(&dir);
        let through = index(&path).unwrap().process_bytes;
        fs::remove_file(path.with_extension("idx")).unwrap();
        forget(&dir);
        check(&dir);
        assert_eq!(index(&path).unwrap().process_bytes, through);
        // A merged minute keeps the counts of its last frame that had some.
        minute::merge(&dir, day, 0).unwrap();
        forget(&dir);
        let merged = frame(&dir, base).unwrap();
        assert_eq!((merged.programs, merged.program_count), (later, 2));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn persists_separate_instances_and_resolved_creation_time() {
        let dir = temp_dir("instances");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        let mut first = proc(42, "worker.exe", 20., 1024);
        first.create_time = base - 1000;
        let mut second = first.clone();
        second.create_time = base + 1000;
        rec.record(&snapshot(base, 20., vec![first]));
        rec.record(&snapshot(base + 1000, 20., vec![second]));
        rec.record(&snapshot(base + FRAME_MS, 1., Vec::new()));
        drop(rec);
        let frame = moment(&dir, base).unwrap();
        let mut created = frame
            .processes
            .iter()
            .map(|p| p.create_time)
            .collect::<Vec<_>>();
        created.sort();
        assert_eq!(created, vec![Some(base - 1000), Some(base + 1000)]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn records_battery_level_and_rate() {
        let dir = temp_dir("battery");
        let base = 20_737 * DAY_MS + 3_600_000;
        let mut rec = HistoryRecorder::new(dir.clone());
        // Frame A: 40% drawing 10 W; frame B: 50% charging 5 W.
        for i in 0..10 {
            let mut s = snapshot(base + i * 1000, 5.0, Vec::new());
            let (pct, rate) = if i < 5 {
                (40.0, Some(-10_000))
            } else {
                (50.0, Some(5_000))
            };
            s.hardware.batteries.push(battery(pct, rate));
            rec.record(&s);
        }
        // A machine without a battery writes frames without battery data.
        // (The sample at +15 s only closes frame C; frames need a successor.)
        for i in 10..16 {
            rec.record(&snapshot(base + i * 1000, 5.0, Vec::new()));
        }
        drop(rec);
        let m = moment(&dir, base + 1_000).unwrap();
        assert_eq!(m.system.battery, Some(40.0));
        assert_eq!(m.system.battery_rate, Some(-10_000.0));
        let m = moment(&dir, base + 6_000).unwrap();
        assert_eq!(m.system.battery, Some(50.0));
        assert_eq!(m.system.battery_rate, Some(5_000.0));
        // The battery-less frames keep both absent.
        let m = moment(&dir, base + 12_000).unwrap();
        assert_eq!(m.system.battery, None);
        assert_eq!(m.system.battery_rate, None);
        // Buckets carry the average of the frames that had a battery.
        let points = legacy_series(&dir, base, base + 15_000, 3);
        assert_eq!(points[0].battery, Some(40.0));
        assert_eq!(points[1].battery, Some(50.0));
        assert_eq!(points[2].battery, None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn averages_multiple_batteries() {
        let dir = temp_dir("batteries");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        // Six one-second samples: the first frame is complete and written.
        for i in 0..6 {
            let mut s = snapshot(base + i * 1000, 5.0, Vec::new());
            // Two batteries: 20% drawing 8 W, 80% with an unknown rate.
            s.hardware.batteries.push(battery(20.0, Some(-8_000)));
            s.hardware.batteries.push(battery(80.0, None));
            rec.record(&s);
        }
        drop(rec);
        let m = moment(&dir, base).unwrap();
        assert_eq!(m.system.battery, Some(50.0));
        assert_eq!(m.system.battery_rate, Some(-8_000.0));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn day_names_round_trip() {
        for day in [0, 19_000, 20_737, -1, 11_016] {
            assert_eq!(day_number(&day_name(day)), Some(day));
        }
        assert_eq!(day_name(20_737), "2026-10-11");
    }

    #[test]
    fn one_pending_process_does_not_inflate_other_process_history() {
        let dir = temp_dir("mixed-pending");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..6 {
            let ready = proc(1, "ready.exe", 20., 1024);
            let mut new = proc(2, "new.exe", 40., 1024);
            new.instance_key = Some("2:seq:50".into());
            new.create_time = if i < 4 { -1 } else { 123 };
            new.counters_pending = i < 4;
            rec.record(&snapshot(base + i * 1000, 10., vec![ready, new]));
        }
        drop(rec);
        let data = moment(&dir, base + 1000).unwrap();
        assert_eq!(data.processes.iter().find(|p| p.pid == 1).unwrap().cpu, 20.);
        assert_eq!(data.processes.iter().find(|p| p.pid == 2).unwrap().cpu, 40.);
        assert_eq!(
            data.processes
                .iter()
                .find(|p| p.pid == 2)
                .unwrap()
                .create_time,
            Some(123)
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reused_readings_do_not_repeat_or_dilute_history() {
        let dir = temp_dir("reused-processes");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..6 {
            let mut process = proc(42, "busy.exe", if i == 0 { 30. } else { 90. }, 1 << 30);
            process.sample_reused = i > 0;
            rec.record(&snapshot(base + i * 1000, 10. + i as f32, vec![process]));
        }
        drop(rec);
        let m = moment(&dir, base + 1000).unwrap();
        assert_eq!(m.system.cpu, 12.);
        assert_eq!(m.processes.len(), 1);
        assert_eq!(m.processes[0].cpu, 30.);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unavailable_process_counters_do_not_repeat_or_dilute_history() {
        let dir = temp_dir("stale-processes");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..6 {
            let mut process = proc(42, "busy.exe", if i == 0 { 30. } else { 90. }, 1 << 30);
            process.stale = i > 0;
            rec.record(&snapshot(base + i * 1000, 10. + i as f32, vec![process]));
        }
        drop(rec);
        let m = moment(&dir, base + 1000).unwrap();
        assert_eq!(m.system.cpu, 12.);
        assert_eq!(m.processes.len(), 1);
        assert_eq!(m.processes[0].cpu, 30.);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn records_and_reads_back() {
        let dir = temp_dir("rw");
        let base = 20_737 * DAY_MS + 3_600_000; // 2026-10-11 01:00 UTC
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..12 {
            let ts = base + i * 1000;
            rec.record(&snapshot(
                ts,
                10.0 + i as f32,
                vec![
                    proc(42, "busy.exe", 30.0, 1 << 30),
                    proc(7, "idle.exe", 0.0, 10 << 20),
                ],
            ));
        }
        drop(rec);
        // Shutdown also persists the two-sample final frame.
        let (first, last) = range(&dir).unwrap();
        assert_eq!((first, last), (base, base + 10_000));
        let m = moment(&dir, base + 1_000).unwrap();
        assert_eq!(m.system.ts, base);
        assert!((m.system.cpu - 12.0).abs() < 0.01); // average of 10..=14
        assert_eq!(m.system.cpu_peak, 14.0);
        let busy = m.processes.iter().find(|p| p.name == "busy.exe").unwrap();
        assert_eq!((busy.pid, busy.cpu, busy.mem), (42, 30.0, 1 << 30));
        let points = legacy_series(&dir, base, base + 10_000, 100);
        assert_eq!(points.len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cuts_a_torn_record_and_keeps_appending() {
        let dir = temp_dir("torn");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..11 {
            rec.record(&snapshot(
                base + i * 1000,
                5.0,
                vec![proc(1, "a.exe", 1.0, 1 << 20)],
            ));
        }
        drop(rec);
        let path = day_path(&dir, 20_737);
        let len = fs::metadata(&path).unwrap().len();
        // A crash mid-write leaves half a frame behind.
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[FRAME, 1, 2, 3])
            .unwrap();
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 20..26 {
            rec.record(&snapshot(
                base + i * 1000,
                6.0,
                vec![proc(1, "a.exe", 1.0, 1 << 20)],
            ));
        }
        drop(rec);
        assert!(fs::metadata(&path).unwrap().len() > len);
        let points = legacy_series(&dir, base, base + 60_000, 60);
        assert_eq!(
            points.iter().map(|p| p.ts).collect::<Vec<_>>(),
            vec![
                base,
                base + 5_000,
                base + 10_000,
                base + 20_000,
                base + 25_000
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_moments_across_midnight() {
        let dir = temp_dir("midnight");
        let midnight = 20_738 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        // One sample per frame: 23:59:50, 23:59:55, 00:00:00, 00:00:05 (UTC).
        for i in -2..=2 {
            rec.record(&snapshot(
                midnight + i * 5_000,
                1.0,
                vec![proc(1, "a.exe", 1.0, 1 << 20)],
            ));
        }
        drop(rec);
        assert_eq!(days(&dir), vec![20_737, 20_738]);
        // The closest frame is in the other day's file.
        assert_eq!(moment(&dir, midnight - 2_000).unwrap().system.ts, midnight);
        assert_eq!(
            moment(&dir, midnight + 30_000).unwrap().system.ts,
            midnight + 10_000
        );
        assert_eq!(
            moment(&dir, midnight - 60_000).unwrap().system.ts,
            midnight - 10_000
        );
        assert!(moment(&dir, midnight + 70_001).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clears_and_records_afresh() {
        let dir = temp_dir("clear");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..11 {
            rec.record(&snapshot(
                base + i * 1000,
                5.0,
                vec![proc(1, "a.exe", 1.0, 1 << 20)],
            ));
        }
        assert!(usage(&dir).bytes > 0);
        rec.clear().unwrap();
        assert!(days(&dir).is_empty());
        let cleared = usage(&dir);
        assert_eq!((cleared.days, cleared.bytes, cleared.first), (0, 0, None));
        assert!(cleared.estimate.bytes_per_day > 0);
        for i in 20..31 {
            rec.record(&snapshot(
                base + i * 1000,
                6.0,
                vec![proc(2, "b.exe", 1.0, 1 << 20)],
            ));
        }
        drop(rec);
        // A new file with its own name table.
        let m = moment(&dir, base + 20_000).unwrap();
        assert_eq!(m.processes[0].name, "b.exe");
        assert_eq!(usage(&dir).first, Some(base + 20_000));
        clear(&dir).unwrap();
        assert!(days(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn adds_channels_and_preserves_unknowns_across_midnight() {
        let dir = temp_dir("channels");
        let midnight = 20_738 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for n in 0..20 {
            let mut s = snapshot(midnight - 10_000 + n * 1000, n as f32, vec![]);
            s.system.mem_used = (n as u64 + 1) * 1024;
            if n >= 5 {
                s.hardware.cpu_sensors = Some(crate::hardware::CpuSensors {
                    board_sensor: Some(if n < 10 { "chip-a" } else { "chip-b" }.into()),
                    board_fans: vec![crate::hardware::CpuFanReading {
                        channel: 2,
                        rpm: 900.,
                    }],
                    core_voltage: (n % 2 == 1).then_some(1.25),
                    ..Default::default()
                });
            }
            rec.record(&s);
        }
        drop(rec);
        let list = channels(&dir, midnight - 10_000, midnight + 8_000);
        let f = list.iter().find(|c| c.key == "fan.2").unwrap();
        assert_eq!((f.first, f.last), (midnight - 5000, midnight + 5000));
        assert!(f.service);
        assert_eq!(f.device.as_deref(), Some("2"));
        assert!(f.label.starts_with("chip-b"));
        assert!(channels(&dir, midnight - 9000, midnight - 1)[0].last < midnight);
        assert!(
            channels(&dir, midnight - 10_000, midnight - 6000)
                .iter()
                .all(|c| c.key != "fan.2")
        );
        assert_eq!(frame(&dir, midnight - 1000).unwrap().ts, midnight);
        assert_eq!(
            frame(&dir, midnight - 5000).unwrap().system["cpuVoltage"],
            Some(1.25)
        );
        assert_eq!(
            frame(&dir, midnight - 10_000).unwrap().system["memUsed"],
            Some(3072.)
        );
        let keys = ["fan.2".into(), "missing".into(), "cpuPeak".into()];
        let points = series(
            &dir,
            midnight - 10_000,
            midnight + 9999,
            10_000,
            midnight,
            &keys,
        );
        assert_eq!(points.series["fan.2"].mean, vec![Some(900.); 2]);
        assert_eq!(points.series["missing"].mean, vec![None; 2]);
        assert_eq!(points.series["cpuPeak"].low, vec![Some(4.), Some(14.)]);
        assert_eq!(points.series["cpuPeak"].high, vec![Some(9.), Some(19.)]);
        assert_eq!(points.peak, vec![midnight - 5000, midnight + 5000]);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn live_cutover_owns_whole_buckets_and_alignment_survives_panning() {
        let dir = temp_dir("live-cut");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..618 {
            rec.record(&snapshot(base + i * 1000, i as f32, vec![]));
        }
        let keys = ["cpu".into()];
        let a = series(&dir, base, base + 40_999, 1000, base, &keys);
        assert_eq!(
            &a.ts[..5],
            &[
                base,
                base + 5000,
                base + 10_000,
                base + 15_000,
                base + 20_000
            ]
        );
        assert_eq!(&a.span[..5], &[5000, 5000, 5000, 5000, 1000]);
        // Recorded frames before the cut, the samples themselves after it.
        assert_eq!(&a.sample[..5], &[false, false, false, false, true]);
        assert_eq!(
            &a.series["cpu"].mean[..5],
            &[Some(2.), Some(7.), Some(12.), Some(17.), Some(20.)]
        );
        assert_eq!(a.ts[5], base + 21000);
        let a = series(&dir, base + 31_400, base + 59_999, 10_000, base, &keys);
        let b = series(&dir, base + 38_000, base + 59_999, 10_000, base, &keys);
        assert!(a.sample.is_empty());
        assert_eq!(a.ts, b.ts);
        assert_eq!(a.series["cpu"].mean, b.series["cpu"].mean);
        assert_eq!(a.series["cpu"].low[0], Some(30.));
        assert_eq!(a.series["cpu"].high[0], Some(39.));
        let fresh = series(&dir, base + 615_000, base + 617_000, 1000, base, &keys);
        assert_eq!(fresh.frame, vec![0; 3]);
        assert_eq!(fresh.settled, base + 615_000);
        rec.clear().unwrap();
        assert!(
            series(&dir, base, base + 617_000, 1000, base, &keys)
                .ts
                .is_empty()
        );
        drop(rec);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn samples_a_background_cadence_apart_stay_samples() {
        let dir = temp_dir("slow-samples");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        // Minimized at 5 s: the sampler wakes a little early or late.
        let mut at = base;
        for (i, step) in [0, 5003, 4998, 5003, 4998].into_iter().enumerate() {
            at += step;
            rec.record(&snapshot(at, i as f32, vec![]));
        }
        let keys = ["cpu".into()];
        // Each spans the time since the one before, a frame or more, and
        // stays one sample rather than reading as an average.
        let raw = series(&dir, base, at + 999, 1000, base, &keys);
        assert_eq!(raw.span, vec![1000, 5003, 4998, 5003, 4998]);
        assert_eq!(raw.sample, vec![true; 5]);
        let frames = series(&dir, base, at + 4999, 5000, base, &keys);
        assert!(frames.sample.is_empty());
        drop(rec);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn marker_reasons_survive_reopen_and_ranges_inside_empty_days() {
        let dir = temp_dir("gap-markers");
        let base = 20_737 * DAY_MS;
        // Supply clock evidence without sleeping the developer's machine.
        let mut rec = HistoryRecorder::new(dir.clone());
        let emit = |rec: &mut HistoryRecorder, offset: i64, boot: i64, awake: i64| {
            rec.boot = Some(boot);
            rec.record(&snapshot(base + offset, 1., vec![]));
            rec.window.awake = awake;
        };
        emit(&mut rec, 0, 12345, 1000);
        rec.stop(true);
        let mut rec = HistoryRecorder::new(dir.clone());
        emit(&mut rec, 30_000, 12345, 31_000);
        emit(&mut rec, 60_000, 12345, 32_000);
        rec.stop(false);
        let mut rec = HistoryRecorder::new(dir.clone());
        emit(&mut rec, 120_000, 12345, 92_000);
        // Simulate a crash after a committed frame, before a stop marker.
        rec.flush_window();
        rec.file = None;
        drop(rec);
        let mut rec = HistoryRecorder::new(dir.clone());
        emit(&mut rec, 180_000, 12345, 152_000);
        drop(rec);
        let mut rec = HistoryRecorder::new(dir.clone());
        emit(&mut rec, 3 * DAY_MS, 987654, 100);
        drop(rec);
        assert_eq!(
            gaps(&dir, base, base + 3 * DAY_MS)
                .iter()
                .map(|g| g.reason)
                .collect::<Vec<_>>(),
            ["off", "sleep", "notRunning", "notRunning", "shutdown"]
        );
        assert_eq!(
            gaps(&dir, base + DAY_MS, base + 2 * DAY_MS)[0].reason,
            "shutdown"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn table_clips_spans_and_agrees_with_saved_rollups() {
        let dir = temp_dir("table");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..360 {
            let mut p = proc(12, "worker.exe", 20., 8192);
            p.io_read = 80;
            p.disk_read = Some(60);
            p.disk_write = (i != 0).then_some(40);
            p.power = (i % 2 == 0).then_some(0.5);
            p.gpu = 8.;
            p.gpu_dedicated = Some(4096);
            p.net_sent = 10;
            p.threads = 3;
            p.handles = 17;
            let mut s = snapshot(
                base + i * 5000,
                1.,
                if i % 3 == 2 { vec![] } else { vec![p] },
            );
            if i == 0 {
                s.processes[0].gpu_dedicated = None;
            }
            rec.record(&s);
        }
        drop(rec);
        let t = table(&dir, base + 2500, base + 15_000, "cpu", 5);
        assert_eq!(t.recorded, 12500);
        let p = &t.processes[0];
        assert_eq!(p.seen, 7500);
        assert_eq!(p.cpu, 12.);
        assert_eq!(p.gpu, 4.8);
        assert_eq!(p.power, Some(0.5));
        assert_eq!(
            (p.io, p.disk_read, p.disk_write, p.disk, p.net),
            (600, Some(450), Some(200), Some(500), Some(75))
        );
        assert_eq!(p.gpu_mem, Some(4096));
        assert_eq!(p.trend, [Some(20.), Some(20.), Some(20.), None, None]);
        assert_eq!((p.threads, p.handles), (Some(3), Some(17)));
        let end = base + 1_800_000;
        let rolled = serde_json::to_value(table(&dir, base, end, "disk", 2)).unwrap();
        fs::remove_file(day_path(&dir, 20_737).with_extension("roll")).unwrap();
        assert_eq!(
            rolled,
            serde_json::to_value(table(&dir, base, end, "disk", 2)).unwrap()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recovery_matches_process_tail_and_discards_incomplete_rollup() {
        let dir = temp_dir("torn-pair");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..3 {
            rec.record(&snapshot(
                base + i * FRAME_MS,
                2.,
                vec![proc(7, "task", 3., 12345)],
            ));
        }
        drop(rec);
        let p = day_path(&dir, 20_737).with_extension("proc");
        let file = OpenOptions::new().write(true).open(&p).unwrap();
        file.set_len(file.metadata().unwrap().len() - 1).unwrap();
        drop(file);
        let mut rec = HistoryRecorder::new(dir.clone());
        rec.record(&snapshot(base + 30_000, 7., vec![proc(8, "new", 2., 999)]));
        drop(rec);
        assert_eq!(
            legacy_series(&dir, base, base + 30_000, 20)
                .iter()
                .map(|p| p.ts)
                .collect::<Vec<_>>(),
            [base, base + 5000, base + 30_000]
        );
        assert_eq!(frame(&dir, base + 30_000).unwrap().processes[0].mem, 999);
        let expected = table(&dir, base, base + ROLL_MS, "cpu", 1);
        fs::remove_file(day_path(&dir, 20_737).with_extension("roll")).unwrap();
        assert_eq!(
            serde_json::to_value(expected).unwrap(),
            serde_json::to_value(table(&dir, base, base + ROLL_MS, "cpu", 1)).unwrap()
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn system_summaries_match_frames_and_skip_process_file() {
        let dir = temp_dir("system-roll");
        let base = 20_737 * DAY_MS;
        let mut r = HistoryRecorder::new(dir.clone());
        for i in 0..400 {
            let mut s = snapshot(
                base + i * 5000,
                (i % 19) as f32,
                vec![proc(17, "busy", 12., 1 << 20)],
            );
            s.system.mem_used = (1u64 << 35) + i as u64;
            r.record(&s);
        }
        drop(r);
        fs::remove_file(day_path(&dir, 20_737).with_extension("proc")).unwrap();
        let keys = ["cpuPeak".into(), "memUsed".into(), "missing".into()];
        let result = series(&dir, base, base + 2 * ROLL_MS - 1, ROLL_MS, base, &keys);
        assert_eq!(result.ts, [base, base + ROLL_MS]);
        assert_eq!(result.peak[0], base + 18 * FRAME_MS);
        let memory = &result.series["memUsed"];
        assert_eq!(memory.low[0], Some((1u64 << 35) as f64));
        assert_eq!(memory.high[0], Some(((1u64 << 35) + 179) as f64));
        assert_eq!(memory.mean[0], Some((1u64 << 35) as f64 + 89.5));
        assert_eq!(result.series["missing"].high, [None, None]);
        let offset = series(
            &dir,
            base + 1000,
            base + ROLL_MS + 999,
            ROLL_MS,
            base + 1000,
            &keys,
        );
        assert_eq!(
            offset.series["memUsed"].mean[0],
            Some((1u64 << 35) as f64 + 90.5)
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn process_rates_preserve_fractional_means_and_independent_divisors() {
        let dir = temp_dir("fractional");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..5 {
            let mut p = proc(7, "task", 2., 4097);
            p.io_read = u64::from(i == 0);
            p.net_received = u64::from(i > 1);
            p.disk_read = (i < 2).then_some(i as u64);
            p.disk_write = None;
            p.gpu_dedicated = Some(4097);
            p.power = Some(0.);
            rec.record(&snapshot(base + i * 1000, 0., vec![p]));
        }
        drop(rec);
        let f = frame(&dir, base).unwrap();
        let p = &f.processes[0];
        assert_eq!(
            (p.io, p.net, p.disk_read, p.disk_write, p.power),
            (0.2, 0.6, Some(0.5), None, Some(0.))
        );
        assert_eq!(p.gpu_mem, Some(4097));
        let q = table(&dir, base, base + FRAME_MS, "net", 1);
        assert_eq!((q.processes[0].io, q.processes[0].net), (1, Some(3)));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn checkpoints_match_primary_when_missing_or_torn() {
        let dir = temp_dir("index-recovery");
        let base = 20_737 * DAY_MS;
        let mut r = HistoryRecorder::new(dir.clone());
        for i in 0..400 {
            r.record(&snapshot(
                base + i * 5000,
                ((i + 7) % 23) as f32,
                vec![proc(3, "test", 8., 4096)],
            ));
        }
        drop(r);
        let get = || {
            forget(&dir);
            serde_json::to_value(series(
                &dir,
                base,
                base + 2 * ROLL_MS,
                ROLL_MS,
                base,
                &["cpu".into()],
            ))
            .unwrap()
        };
        let expected = get();
        let path = day_path(&dir, 20_737).with_extension("idx");
        let f = OpenOptions::new().write(true).open(&path).unwrap();
        f.set_len(f.metadata().unwrap().len() - 3).unwrap();
        drop(f);
        assert_eq!(get(), expected);
        fs::remove_file(&path).unwrap();
        assert_eq!(get(), expected);
        let mut r = HistoryRecorder::new(dir.clone());
        r.record(&snapshot(base + 3 * ROLL_MS, 1., vec![]));
        drop(r);
        assert_eq!(get(), expected);
        assert_eq!(range(&dir).unwrap().1, base + 3 * ROLL_MS);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn prunes_old_days() {
        let dir = temp_dir("prune");
        fs::create_dir_all(&dir).unwrap();
        for day in [20_729, 20_730, 20_736] {
            fs::write(day_path(&dir, day), MAGIC).unwrap();
        }
        maintenance::maintain_at(
            &dir,
            HistoryOptions {
                limit: HistoryLimit {
                    days: Some(7),
                    bytes: None,
                },
                detail: HistoryDetail::Full,
                ..Default::default()
            },
            20_737,
            20_737,
            || false,
        )
        .unwrap();
        assert_eq!(days(&dir), vec![20_736]);
        let _ = fs::remove_dir_all(&dir);
    }
}
