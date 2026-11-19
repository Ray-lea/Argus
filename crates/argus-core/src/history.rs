//! Append-only system and process history, partitioned by UTC day. The live
//! sampler publishes ten minutes of unrounded system samples independently of
//! the five-second frames. Times and spans are milliseconds. System readings
//! average finite samples independently; CPU peak is their maximum. Process
//! working set, dedicated GPU memory, threads and handles are frame maxima.
//!
//! # Version 4
//!
//! `YYYY-MM-DD.hist` begins with `ARGH\x04`. Each record is a u8 tag, u32
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
//! ULEB128 identity index, flags u8, nonzero CPU/GPU f32 (bits 0/1), working
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
//! V2/V3 day files retain their original name table, fixed system header and
//! process entries (v3 adds creation time). Existing days continue in their
//! own encoding. Older process fields and split disk/network counters are
//! unknown. Legacy queries project v4 back into the original JSON structures.

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
/// Days of history kept.
pub const KEEP_DAYS: i64 = 7;

const MAGIC: &[u8; 5] = b"ARGH\x04";
const MAGIC_V3: &[u8; 5] = b"ARGH\x03";
const LEGACY_MAGIC: &[u8; 5] = b"ARGH\x02";
const TAG_NAME: u8 = 1;
const TAG_FRAME: u8 = 2;
const FRAME_HEAD: usize = 8 + 4 * 3 + 8 * 4 + 4 * 2 + 1;
const ENTRY: usize = 2 + 4 + 4 + 4 + 4 + 4 + 4;
const ENTRY_V3: usize = ENTRY + 8;
const DAY_MS: i64 = 86_400_000;
// A normal day holds 17,280 frames. Reject oversized files before allocating or repairing.
const MAX_FILE_BYTES: usize = 128 << 20;
const MAX_METADATA_BYTES: usize = 16 << 10;
const MAX_CHANNELS: usize = 512;
const MAX_INSTANCES: usize = 65_536;
const MAX_FRAME_ROWS: usize = 4096;
// Ten minutes at the sampler's shortest supported cadence (250 ms), inclusive.
const MAX_RECENT: usize = 2401;
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
    day: i64,
    out: BufWriter<File>,
    names: HashMap<Arc<str>, u16>,
    next_name: usize,
    legacy: bool,
    v4: Option<Writer4>,
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
    selection: Selection,
}

impl HistoryRecorder {
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
            selection: Selection::default(),
            dir,
            file: None,
            window: Window::default(),
        }
    }

    /// Deletes everything recorded so far, the frame in progress included;
    /// recording carries on into a new file.
    pub fn clear(&mut self) -> io::Result<()> {
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
        if self.stopped {
            return;
        }
        let slot = align(s.timestamp, 0, FRAME_MS);
        if self.window.samples > 0 && slot != align(self.window.start, 0, FRAME_MS) {
            if slot == align(self.window.start, 0, FRAME_MS).saturating_add(FRAME_MS) {
                self.window.interval = slot - self.window.last;
            }
            self.flush_window();
        }
        let first_sample = self.live.samples.is_empty();
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
            .live
            .samples
            .back()
            .is_some_and(|p| p.ts.div_euclid(DAY_MS) != slot.div_euclid(DAY_MS))
        {
            self.catalog = Catalog::default();
        }
        let values = readings(&mut self.catalog, s);
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
        w.samples += 1;
        w.values.resize(values.len(), Mean::default());
        w.present.resize(values.len(), false);
        for (i, &v) in values.iter().enumerate() {
            w.values[i].add(v);
            w.present[i] |= v != f64::NEG_INFINITY;
        }
        w.cpu_peak = w.cpu_peak.max(s.system.cpu);
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
                .is_some_and(|p| p.ts < s.timestamp.saturating_sub(600_000))
        {
            self.live.evicted = true;
            self.live.samples.pop_front();
        }
        while self.live.frames.len() > MAX_RECENT
            || self
                .live
                .frames
                .front()
                .is_some_and(|&ts| ts < s.timestamp.saturating_sub(605_000))
        {
            self.live.frames.pop_front();
        }
        while self.live.gaps.len() > MAX_RECENT
            || self
                .live
                .gaps
                .front()
                .is_some_and(|g| g.to < s.timestamp.saturating_sub(600_000))
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
                acc.missing += 1;
                continue;
            }
            acc.measured = true;
            if let Some(v) = p.disk_read {
                acc.disk_read.add(v as f64);
            }
            if let Some(v) = p.disk_write {
                acc.disk_write.add(v as f64);
            }
            if let Some(v) = p.power {
                acc.power.add(v as f64);
            }
            acc.gpu_mem = acc.gpu_mem.max(p.gpu_dedicated);
            acc.threads = acc.threads.max(p.threads);
            acc.handles = acc.handles.max(p.handles);
            acc.cpu += p.cpu as f64;
            acc.mem = acc.mem.max(p.private_ws);
            acc.io += p.io_read as f64 + p.io_write as f64 + p.io_other as f64;
            acc.gpu += p.gpu as f64;
            acc.net += p.net_sent as f64 + p.net_received as f64;
        }
    }

    fn open(&mut self, ts: i64, boot: i64, awake: i64) -> io::Result<()> {
        let day = ts.div_euclid(DAY_MS);
        if self.file.as_ref().is_some_and(|f| f.day == day) {
            return Ok(());
        }
        self.file = None;
        fs::create_dir_all(&self.dir)?;
        prune(&self.dir, day);
        let mut file = open_day(&self.dir, day)?;
        forget(&self.dir);
        if file.v4.is_some() {
            file.marker(1, ts, boot, awake)?;
        }
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
            Ok(()) => {
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
        if let Some(file) = self.file.as_mut().filter(|f| f.v4.is_some()) {
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
        self.shared = SharedLive::default();
        self.live = Live::default();
        self.file = None;
    }
    fn write(&mut self, w: &Window) -> io::Result<()> {
        self.open(w.start, w.boot, w.awake)?;
        let file = self.file.as_mut().ok_or_else(invalid)?;
        let mut procs: Vec<&ProcAcc> = w.procs.values().filter(|p| p.measured).collect();
        let mut picked: Vec<&ProcAcc> = Vec::new();
        let mut take = |key: &dyn Fn(&ProcAcc) -> f64, count: usize, floor: f64| {
            let order = |a: &&ProcAcc, b: &&ProcAcc| {
                key(b)
                    .total_cmp(&key(a))
                    .then_with(|| a.pid.cmp(&b.pid))
                    .then_with(|| a.create_time.cmp(&b.create_time))
            };
            let n = count.min(procs.len());
            if n < procs.len() {
                procs.select_nth_unstable_by(n, order);
            }
            let leaders = &mut procs[..n];
            leaders.sort_unstable_by(order);
            for p in leaders.iter().filter(|p| key(p) > floor) {
                if !picked.iter().any(|q| std::ptr::eq(*q, *p)) {
                    picked.push(*p);
                }
            }
        };
        let scale = self.selection.multiplier;
        take(&|p| p.cpu / p.divisor(w.samples), 6 * scale, 0.5);
        take(&|p| p.mem as f64, 6 * scale, 0.0);
        take(&|p| p.io / p.divisor(w.samples), 3 * scale, 0.0);
        take(&|p| p.gpu / p.divisor(w.samples), 3 * scale, 0.5);
        take(&|p| p.net / p.divisor(w.samples), 3 * scale, 0.0);

        take(
            &|p| p.disk_read.get().unwrap_or_default() + p.disk_write.get().unwrap_or_default(),
            3 * scale,
            0.,
        );
        if self.selection.all {
            procs.truncate(MAX_FRAME_ROWS);
            picked = procs;
        }
        if let Some(writer) = &mut file.v4 {
            return writer.frame(&mut file.out, &self.catalog, w, &picked);
        }
        let old = legacy_point(w.start, |key| {
            self.catalog
                .by_key
                .get(key)
                .and_then(|&i| w.values.get(i)?.get())
        });
        let mut buf = Vec::with_capacity(FRAME_HEAD + 1 + picked.len() * ENTRY);
        let mut indices = Vec::with_capacity(picked.len());
        for p in picked.iter().take(usize::from(u8::MAX)) {
            let index = match file.names.get(&p.name) {
                Some(&i) => i,
                None => {
                    let Ok(i) = u16::try_from(file.next_name) else {
                        continue;
                    };
                    let bytes = truncate_utf8(&p.name, 255);
                    buf.push(TAG_NAME);
                    buf.extend_from_slice(&i.to_le_bytes());
                    buf.push(bytes.len() as u8);
                    buf.extend_from_slice(bytes);
                    file.names.insert(p.name.clone(), i);
                    file.next_name += 1;
                    i
                }
            };
            indices.push((index, *p));
        }
        buf.push(TAG_FRAME);
        buf.extend_from_slice(&w.start.to_le_bytes());
        buf.extend_from_slice(&old.cpu.to_le_bytes());
        buf.extend_from_slice(&w.cpu_peak.to_le_bytes());
        buf.extend_from_slice(&old.gpu.to_le_bytes());
        buf.extend_from_slice(&old.mem_used.to_le_bytes());
        buf.extend_from_slice(&old.mem_total.to_le_bytes());
        buf.extend_from_slice(&old.disk.to_le_bytes());
        buf.extend_from_slice(&old.net.to_le_bytes());
        // -1 and NaN stand in for absence: levels are 0–100, rates are finite.
        buf.extend_from_slice(&old.battery.unwrap_or(-1.).to_le_bytes());
        buf.extend_from_slice(&old.battery_rate.unwrap_or(f32::NAN).to_le_bytes());
        buf.push(indices.len() as u8);
        let clamp = |v: f64| v.clamp(0.0, u32::MAX as f64) as u32;
        for (index, p) in indices {
            let process_n = p.divisor(w.samples);
            buf.extend_from_slice(&index.to_le_bytes());
            buf.extend_from_slice(&p.pid.to_le_bytes());
            buf.extend_from_slice(&((p.cpu / process_n) as f32).to_le_bytes());
            buf.extend_from_slice(&clamp((p.mem / 1024) as f64).to_le_bytes());
            buf.extend_from_slice(&clamp(p.io / process_n).to_le_bytes());
            buf.extend_from_slice(&((p.gpu / process_n) as f32).to_le_bytes());
            buf.extend_from_slice(&clamp(p.net / process_n).to_le_bytes());
            if !file.legacy {
                buf.extend_from_slice(&p.create_time.to_le_bytes());
            }
        }
        // One write and flush per frame, so readers and crashes see whole frames.
        file.out.write_all(&buf)?;
        file.out.flush()
    }
}

impl Drop for HistoryRecorder {
    fn drop(&mut self) {
        self.stop(false);
    }
}
struct Selection {
    multiplier: usize,
    all: bool,
}
impl Default for Selection {
    fn default() -> Self {
        Self {
            multiplier: 1,
            all: false,
        }
    }
}
fn truncate_utf8(s: &str, max: usize) -> &[u8] {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s.as_bytes()[..end]
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
                .or_else(|| text.strip_suffix(".idx"))?,
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

/// Deletes day files older than the retention window.
fn prune(dir: &Path, today: i64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if owned_day(&entry.file_name()).is_some_and(|d| d <= today - KEEP_DAYS)
            && let Err(error) = fs::remove_file(entry.path())
        {
            crate::diagnostics::file_failure("history retention cleanup", &entry.path(), &error);
        }
    }
}

/// Deletes day files that have aged out. Recording does this whenever it
/// starts a new file; this is for when history is not being recorded.
pub fn prune_expired(dir: &Path) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    prune(dir, now.div_euclid(DAY_MS));
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

fn entries(raw: &[u8], names: &[String], legacy: bool) -> Vec<HistoryProcess> {
    raw.chunks_exact(if legacy { ENTRY } else { ENTRY_V3 })
        .filter_map(|e| {
            let name = u16::from_le_bytes(field(e, 0)?) as usize;
            Some(HistoryProcess {
                name: names.get(name)?.clone(),
                pid: u32::from_le_bytes(field(e, 2)?),
                create_time: if legacy {
                    None
                } else {
                    let ts = i64::from_le_bytes(field(e, ENTRY)?);
                    (ts > 0).then_some(ts)
                },
                cpu: f32::from_le_bytes(field(e, 6)?),
                mem: u64::from(u32::from_le_bytes(field(e, 10)?)) * 1024,
                io: u32::from_le_bytes(field(e, 14)?) as u64,
                gpu: f32::from_le_bytes(field(e, 18)?),
                net: u32::from_le_bytes(field(e, 22)?) as u64,
            })
        })
        .collect()
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
    visit_days(dir, from, to, false, |d| {
        for i in d.range.clone() {
            let Some(row) = d.row(i) else { continue };
            let p = legacy_point(row.ts(), |k| finite(row.get(*d.idx.catalog.by_key.get(k)?)));
            if p.ts >= from && p.ts <= to {
                let i = ((p.ts.abs_diff(from) as f64 / span) * buckets as f64) as usize;
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
fn readings(catalog: &mut Catalog, s: &Snapshot) -> Vec<f64> {
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
            self.n += 1;
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
#[derive(Debug, Clone, Serialize)]
pub struct HistoryFrame {
    pub ts: i64,
    pub span: i64,
    pub system: BTreeMap<String, Option<f64>>,
    pub processes: Vec<FrameProcess>,
}

const CHANNEL: u8 = 3;
const INSTANCE: u8 = 4;
const MARKER: u8 = 5;
const V4_FRAME: u8 = 6;
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
struct Writer4 {
    checkpoints: Option<Checkpoints>,
    presence: Vec<u8>,
    system: SystemRoll,
    roll: Roll,
    roll_file: BufWriter<File>,
    proc: BufWriter<File>,
    pos: u64,
    catalog: Catalog,
    ids: HashMap<Identity, u32>,
    next_id: u32,
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
impl Writer4 {
    fn frame(
        &mut self,
        out: &mut impl Write,
        catalog: &Catalog,
        w: &Window,
        picked: &[&ProcAcc],
    ) -> io::Result<()> {
        let mut metadata = Vec::new();
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
            && self.roll.first.div_euclid(PROCESS_ROLL_MS) != w.start.div_euclid(PROCESS_ROLL_MS)
        {
            self.roll.write(&mut self.roll_file)?;
        }
        if self.roll.recorded == 0 {
            self.roll.first = w.start;
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
            var(&mut rows, id.into());
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
            for v in [
                rates[0].0 as u64,
                rates[1].0 as u64,
                p.threads as u64,
                p.handles as u64,
            ] {
                var(&mut rows, v);
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
        packet(&mut metadata, V4_FRAME, &frame)?;
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
        Ok(())
    }
}
impl Drop for Writer4 {
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
        let id = usize::try_from(take_var(&mut raw)?).ok()?;
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
        let net = take_var(&mut raw)?;
        let threads = Some(u32::try_from(take_var(&mut raw)?).ok()?);
        let handles = Some(u32::try_from(take_var(&mut raw)?).ok()?);
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
            Some(net as f64),
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
impl From<HistoryProcess> for FrameProcess {
    fn from(p: HistoryProcess) -> Self {
        Self {
            name: p.name,
            pid: p.pid,
            create_time: p.create_time,
            cpu: p.cpu,
            gpu: p.gpu,
            mem: p.mem,
            io: p.io as f64,
            net: p.net as f64,
            ..Default::default()
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

const OLD_KEYS: [(&str, &str, &str); 9] = [
    ("cpu", "percent", "cpu"),
    ("cpuPeak", "percent", "cpu"),
    ("gpu", "percent", "gpu"),
    ("memUsed", "bytes", "memory"),
    ("memTotal", "bytes", "memory"),
    ("disk", "bytesPerSecond", "disk"),
    ("network", "bytesPerSecond", "network"),
    ("battery", "percent", "battery"),
    ("batteryRate", "milliwatts", "battery"),
];
#[derive(Clone, Copy, Default)]
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
    catalog: Catalog,
    ids: Vec<Identity>,
    names: Vec<String>,
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
    headers: Vec<[u8; FRAME_HEAD]>,
}
impl Index {
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
            if tag == FRAME_RUN {
                return checkpoint_end.is_some() && self.address_run(p, limit);
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
            V4_FRAME => self.frames.len() >= MAX_FRAMES,
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
                if let Some(r) = RollRef::parse(p, self) {
                    self.summaries.push(r);
                }
            }
            V4_FRAME if !checkpoint => {
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

    /// Legacy records interleave system and process bytes. Keep only the headers
    /// in the bounded index and seek over the variable-length process arrays.
    fn legacy(&mut self, file: &mut (impl Read + Seek), len: usize) -> io::Result<()> {
        for (key, unit, group) in OLD_KEYS {
            self.catalog.register(key, unit, group, "", None, 0);
        }
        let mut at = self.valid.max(5);
        file.seek(SeekFrom::Start(at as u64))?;
        let entry = if self.version == 2 { ENTRY } else { ENTRY_V3 };
        while at < len {
            let mut tag = [0];
            file.read_exact(&mut tag)?;
            match tag[0] {
                TAG_NAME if len - at >= 4 => {
                    let mut head = [0; 3];
                    file.read_exact(&mut head)?;
                    let size = usize::from(head[2]);
                    if len - at < 4 + size {
                        break;
                    }
                    let mut name = vec![0; size];
                    file.read_exact(&mut name)?;
                    let id = u16::from_le_bytes([head[0], head[1]]) as usize;
                    if id != self.names.len() {
                        break;
                    }
                    self.names.push(String::from_utf8_lossy(&name).into_owned());
                    self.process_bytes += (size + 4) as u64;
                    at += size + 4;
                }
                TAG_FRAME if len - at > FRAME_HEAD => {
                    let mut head = [0; FRAME_HEAD];
                    file.read_exact(&mut head)?;
                    let rows = usize::from(head[60]) * entry;
                    if len - at < 1 + FRAME_HEAD + rows {
                        break;
                    }
                    let ts = i64::from_le_bytes(field(&head, 0).ok_or_else(invalid)?);
                    if self.frames.len() >= MAX_FRAMES {
                        self.limited = true;
                        break;
                    }
                    if ts.div_euclid(DAY_MS) != self.day {
                        break;
                    }
                    self.stamp(
                        Stamp {
                            ts,
                            span: FRAME_MS,
                            awake: -1,
                            ..Default::default()
                        },
                        at,
                    );
                    self.headers.push(head);
                    file.seek(SeekFrom::Current(rows as i64))?;
                    self.process_bytes += rows as u64;
                    at += 1 + FRAME_HEAD + rows;
                }
                _ => break,
            }
        }
        self.valid = at;
        Ok(())
    }
}
fn open_writable(path: &Path) -> io::Result<File> {
    #[cfg(test)]
    if let Some(file) = test_files::open(path, true) {
        return file;
    }
    let file = OpenOptions::new()
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
    let path = day_path(dir, day);
    let mut file = open_writable(&path)?;
    let mut bytes = read_bounded(&mut file)?;
    if bytes.len() < 5 && MAGIC.starts_with(&bytes) {
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
    let mut names = HashMap::new();
    let mut next_name = 0;
    let mut v4 = None;
    if bytes.starts_with(MAGIC) {
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
        v4 = Some(Writer4 {
            checkpoints,
            presence: Vec::new(),
            system: SystemRoll::default(),
            roll: Roll::default(),
            roll_file: BufWriter::new(roll_file),
            proc: BufWriter::new(proc),
            pos: idx.proc_end,
            catalog: idx.catalog,
            next_id: idx.ids.len() as u32,
            ids: idx
                .ids
                .into_iter()
                .enumerate()
                .map(|(i, id)| (id, i as u32))
                .collect(),
        });
    } else if bytes.starts_with(LEGACY_MAGIC) || bytes.starts_with(MAGIC_V3) {
        idx.version = bytes[4];
        idx.legacy(&mut std::io::Cursor::new(&bytes), bytes.len())?;
        if idx.limited {
            return Err(invalid());
        }
        next_name = idx.names.len();
        names = idx
            .names
            .into_iter()
            .enumerate()
            .map(|(i, n)| (Arc::from(n), i as u16))
            .collect();
    } else {
        return Err(invalid());
    }
    file.set_len(idx.valid as u64)?;
    file.seek(SeekFrom::End(0))?;
    Ok(DayFile {
        out: BufWriter::new(file),
        names,
        next_name,
        legacy: bytes.starts_with(LEGACY_MAGIC),
        v4,
        day,
    })
}

struct CachedIndex {
    generation: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    len: u64,
    cost: usize,
    idx: Arc<Index>,
}
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
    if let Some(c) = cache.get(path)
        && c.generation == generation
        && c.created == created
        && c.modified == modified
        && c.len == len
    {
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
        if &magic != MAGIC && &magic != MAGIC_V3 && &magic != LEGACY_MAGIC {
            return None;
        }
        data.day = day_of(path.file_name()?)?;
        data.version = magic[4];
        data.valid = 5;
        if data.version == 4 {
            data.load_checkpoints(path, len);
        }
    }
    if data.version == 4 {
        f.seek(SeekFrom::Start(data.valid as u64)).ok()?;
        let mut tail = Vec::new();
        history_read(
            path,
            f.take(len.saturating_sub(data.valid as u64))
                .read_to_end(&mut tail),
        )?;
        data.scan(&tail, u64::MAX, None);
    } else {
        data.legacy(&mut f, usize::try_from(len).ok()?).ok()?;
    }
    // The normal 30-day frame index is about 4 MiB. Bound uncommon catalogs and churn too.
    let cost = |i: &Index| {
        i.frames.capacity() * 8
            + i.headers.capacity() * FRAME_HEAD
            + i.masks.iter().map(|m| 32 + m.1.capacity()).sum::<usize>()
            + i.summaries.capacity() * std::mem::size_of::<RollRef>()
            + i.summaries
                .iter()
                .map(|r| r.bands.len() * std::mem::size_of::<Band>())
                .sum::<usize>()
            + i.gaps.capacity() * std::mem::size_of::<Gap>()
            + i.ids.iter().map(|id| 64 + id.name.len()).sum::<usize>()
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
            + i.names.iter().map(|n| 32 + n.len()).sum::<usize>()
    };
    let size = cost(&idx).max(512);
    let mut cache = INDEXES.lock().unwrap_or_else(|e| e.into_inner());
    if size <= 12 * 1024 * 1024
        && generation == CLEAR_GENERATION.load(std::sync::atomic::Ordering::Acquire)
    {
        if cache.values().map(|c| c.cost).sum::<usize>() + size > 12 * 1024 * 1024 {
            cache.clear();
        }
        cache.insert(
            path.into(),
            CachedIndex {
                generation,
                modified,
                cost: size,
                created,
                len,
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
        let bytes = if idx.version == 4 || processes {
            read_slice(path, base as u64, end.checked_sub(base)?)?
        } else {
            Vec::new()
        };
        let mut d = Self {
            idx,
            base,
            bytes,
            proc_base: 0,
            proc: Vec::new(),
            proc_path: None,
            range: first..last,
        };
        if processes && d.idx.version == 4 {
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
        (tag == V4_FRAME
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
    fn row(&self, i: usize) -> Option<Reading<'_>> {
        if self.idx.version == 4 {
            Some(Reading::New(self.stored(i)?, &self.idx.catalog))
        } else {
            Some(Reading::Old(self.idx.headers.get(i)?))
        }
    }
    fn processes(&self, i: usize) -> Vec<FrameProcess> {
        if self.idx.version == 4 {
            self.stored(i)
                .and_then(|f| self.metrics(&f))
                .unwrap_or_default()
                .into_iter()
                .map(|(j, m)| m.process(&self.idx.ids[j]))
                .collect()
        } else {
            let Some(b) = self.bytes(i).and_then(|b| b.get(1..)) else {
                return Vec::new();
            };
            let v2 = self.idx.version == 2;
            let Some(head) = self.idx.headers.get(i) else {
                return Vec::new();
            };
            let Some(raw) = b.get(
                FRAME_HEAD..FRAME_HEAD + usize::from(head[60]) * if v2 { ENTRY } else { ENTRY_V3 },
            ) else {
                return Vec::new();
            };
            entries(raw, &self.idx.names, v2)
                .into_iter()
                .map(FrameProcess::from)
                .collect()
        }
    }
}
enum Reading<'a> {
    New(Stored<'a>, &'a Catalog),
    Old(&'a [u8; FRAME_HEAD]),
}
impl Reading<'_> {
    fn ts(&self) -> i64 {
        match self {
            Self::New(f, _) => f.ts,
            Self::Old(b) => i64::from_le_bytes(*b.first_chunk().unwrap_or(&[0; 8])),
        }
    }
    fn span(&self) -> i64 {
        match self {
            Self::New(f, _) => f.span,
            Self::Old(_) => FRAME_MS,
        }
    }
    fn get(&self, i: usize) -> f64 {
        match self {
            Self::New(f, c) => f.value(i, c),
            Self::Old(b) => match i {
                0..=2 | 7..=8 => {
                    let offset = if i < 3 { 8 + 4 * i } else { 52 + 4 * (i - 7) };
                    let v = field(*b, offset).map_or(f64::NAN, |v| f32::from_le_bytes(v) as f64);
                    if i == 7 && v < 0. { f64::NAN } else { v }
                }
                3..=6 => {
                    field(*b, 20 + 8 * (i - 3)).map_or(f64::NAN, |v| u64::from_le_bytes(v) as f64)
                }
                _ => f64::NAN,
            },
        }
    }
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
    let mut u = HistoryUsage::default();
    for day in days(dir) {
        let path = day_path(dir, day);
        let bytes = metadata(&path).map_or(0, |m| m.len());
        let Some(idx) = index(&path) else { continue };
        let proc = if idx.version == 4 {
            metadata(path.with_extension("proc"))
                .map_or(0, |m| m.len())
                .saturating_add(metadata(path.with_extension("roll")).map_or(0, |m| m.len()))
        } else {
            0
        };
        u.days += 1;
        u.breakdown.processes = u
            .breakdown
            .processes
            .saturating_add(proc)
            .saturating_add(idx.process_bytes);
        u.breakdown.system = u
            .breakdown
            .system
            .saturating_add(bytes.saturating_sub(idx.process_bytes))
            .saturating_add(metadata(path.with_extension("idx")).map_or(0, |m| m.len()));
        if u.first.is_none() {
            u.first = idx.first.map(|s| s.ts);
        }
    }
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
    pub bucket: i64,
    pub ts: Vec<i64>,
    pub span: Vec<i64>,
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
        out.frame.push(self.frame);
        out.peak.push(self.peak);
        for (b, col) in self.bands.into_iter().zip(out.series.values_mut()) {
            col.mean.push((b.n > 0).then(|| b.sum / f64::from(b.n)));
            if out.bucket >= 10_000 {
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
    let all = days(dir);
    let first = all.partition_point(|&d| d < from.div_euclid(DAY_MS));
    let end = all.partition_point(|&d| d <= to.div_euclid(DAY_MS));
    let older = || {
        all[..first]
            .iter()
            .rev()
            .find_map(|&d| index(&day_path(dir, d))?.last)
    };
    let mut prev = None;
    let mut out = Vec::new();
    let crossing =
        |a: Option<Stamp>, b: Option<Stamp>| gap(a?, b?).filter(|g| g.from <= to && g.to >= from);
    for &day in all.get(first..end).unwrap_or_default() {
        if out.len() >= MAX_BUCKETS {
            break;
        }
        let Some(idx) = index(&day_path(dir, day)) else {
            continue;
        };
        if let Some(s) = idx.first {
            if prev.is_none() && s.ts >= from {
                prev = older();
            }
            if let Some(g) = crossing(prev, Some(s)) {
                out.push(g);
            }
            prev = idx.last;
        }
        let begin = idx.gaps.partition_point(|g| g.to < from);
        out.extend(
            idx.gaps[begin..]
                .iter()
                .take_while(|g| g.from <= to)
                .take(MAX_BUCKETS.saturating_sub(out.len())),
        );
    }
    if let Some(a) = prev.or_else(older).filter(|s| s.ts + s.span <= to) {
        let next = all[end..]
            .iter()
            .find_map(|&d| index(&day_path(dir, d))?.first);
        let session = live(dir).first.filter(|s| s.ts > a.ts);
        let next = session
            .filter(|s| next.is_none_or(|b| s.ts < b.ts))
            .or(next);
        if let Some(g) = crossing(Some(a), next) {
            out.push(g);
        } else if next.is_none() && a.stop == 2 && a.ts + a.span < to {
            out.push(Gap {
                from: a.ts + a.span,
                to,
                reason: "off",
            });
        }
    }
    out
}
pub fn series(
    dir: &Path,
    from: i64,
    to: i64,
    bucket: i64,
    origin: i64,
    keys: &[String],
) -> HistorySeries {
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
            let range = idx.range(day, first, limit);
            if range.is_empty() {
                continue;
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
                    let raw = bucket < FRAME_MS;
                    if raw && ts < from {
                        continue;
                    }
                    bucket_at(
                        acc,
                        out,
                        if raw { ts } else { align(ts, origin, bucket) },
                        if raw { FRAME_MS } else { bucket },
                        keys.len(),
                    )
                    .update(ts, r.get(peak), ids.iter().map(|&j| r.get(j)));
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
        bucket_at(
            &mut acc,
            &mut out,
            if raw {
                s.ts
            } else {
                align(s.ts, origin, bucket)
            },
            if raw { s.span } else { bucket },
            keys.len(),
        )
        .update(frame, get(peak), ids.iter().map(|&j| get(j)));
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
        if idx.version == 4 {
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
        } else {
            for (i, header) in idx.headers.iter().enumerate().take(end).skip(first) {
                let r = Reading::Old(header);
                for j in 0..OLD_KEYS.len() {
                    if r.get(j).is_finite() {
                        observe(j, i, i);
                    }
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
    pub net: u64,
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
        self.net += b.net;
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
            "net" => (self.net, self.seen),
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
        Some(Self {
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
            net: finite(f()?)?,
            read: finite(f()?),
            write: finite(f()?),
            disk: finite(f()?)?,
            power: finite(f()?)?,
        })
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
        if first.div_euclid(DAY_MS) != idx.day
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
    fn write(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.recorded == 0 {
            return Ok(());
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
        Ok(())
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
    fn bin(&self, ts: i64) -> usize {
        (((i128::from(ts) - i128::from(self.from)) * self.points as i128
            / (i128::from(self.to) - i128::from(self.from))) as usize)
            .min(self.points.saturating_sub(1))
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
        let Some(d) = DayRead::open(
            path,
            day,
            idx,
            start.saturating_sub(FRAME_MS - 1),
            end - 1,
            true,
        ) else {
            return;
        };
        for i in d.range.clone() {
            if self.exhausted {
                return;
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
                Reading::Old(_) => Vec::new(),
            };
            self.recorded += finish - a;
            while a < finish {
                let bin = self.bin(a);
                let next = if self.points == 0 {
                    finish
                } else {
                    let delta = i128::from(self.to) - i128::from(self.from);
                    let bound = i128::from(self.from)
                        + (delta * (bin as i128 + 1) + self.points as i128 - 1)
                            / self.points as i128;
                    finish.min(bound as i64)
                };
                let span = next - a;
                match &r {
                    Reading::New(_, _) => {
                        for &(j, m) in &rows {
                            self.add(&d.idx.ids[j], Totals::observation(m, ts, span), bin);
                        }
                    }
                    Reading::Old(_) => {
                        for p in d.processes(i) {
                            let id = Identity {
                                name: p.name.into(),
                                pid: p.pid,
                                created: p.create_time.unwrap_or_default(),
                            };
                            self.add(
                                &id,
                                Totals::observation(
                                    Metrics {
                                        cpu: p.cpu.into(),
                                        gpu: p.gpu.into(),
                                        mem: p.mem,
                                        io: p.io,
                                        net: p.net,
                                        ..Default::default()
                                    },
                                    ts,
                                    span,
                                ),
                                bin,
                            );
                        }
                    }
                }
                a = next;
            }
        }
    }
    fn day(&mut self, path: &Path, day: i64, idx: Arc<Index>) {
        let mut start = self.from.max(day * DAY_MS);
        let end = self.to.min((day + 1) * DAY_MS);
        if end - start >= PROCESS_ROLL_MS
            && idx.version == 4
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
    if from >= to || trend_points > MAX_POINTS {
        return HistoryTable::default();
    }
    let mut q = TableQuery {
        from,
        to,
        key: trend_key,
        points: trend_points,
        cells: 0,
        name_bytes: 0,
        exhausted: false,
        recorded: 0,
        rows: HashMap::new(),
    };
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
                net: bytes(t.net),
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
#[derive(Default)]
struct AddressRun {
    first: Option<Stamp>,
    last: Stamp,
    refs: Vec<u8>,
    proc_end: u64,
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
        bytes.push(FRAME_RUN);
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(body);
    }
}
struct Checkpoints {
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
            if tag == V4_FRAME {
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
        self.out.flush()?;
        self.base = self.end;
        Ok(())
    }
}
impl Writer4 {
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
        if let Some(writer) = &mut self.v4 {
            writer.index_append(&bytes);
        }
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
                version: 4,
                valid: 5,
                ..Default::default()
            };
        }
    }

    fn address_run(&mut self, p: &[u8], limit: usize) -> bool {
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
        let base = self.valid;
        let width = 5 + FRAME_PREFIX + self.catalog.bytes;
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
    use super::*;
    use crate::hardware::Hardware;
    use crate::process::ProcessRow;
    use crate::system::SystemStats;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("argus-history-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn proc(pid: u32, name: &str, cpu: f32, mem: u64) -> ProcessRow {
        ProcessRow {
            listed_by_roster: false,
            counters_pending: false,
            sequence_number: None,
            instance_key: None,
            stale: false,
            sample_reused: false,
            counters: Default::default(),
            pid,
            ppid: 0,
            create_time: 1,
            name: name.into(),
            path: None,
            description: None,
            company: None,
            version: None,
            facts: Default::default(),
            icon: None,
            web_view: None,
            windows_process: false,
            cpu,
            cpu_time: 0,
            private_ws: mem,
            working_set: mem,
            commit: mem,
            io_read: 0,
            io_write: 0,
            io_other: 0,
            disk_read: None,
            disk_write: None,
            gpu: 0.0,
            gpu_engine: None,
            gpu_engines: Vec::new(),
            gpu_dedicated: Some(0),
            gpu_shared: Some(0),
            net_sent: 0,
            net_received: 0,
            power: None,
            power_trend: None,
            threads: 1,
            handles: 1,
            session_id: 1,
            base_priority: 8,
            suspended: false,
            frozen: false,
            efficiency: false,
            io_priority: None,
            memory_priority: None,
            app_window: false,
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
    fn keeps_legacy_frames_and_appends_without_inventing_identity() {
        let dir = temp_dir("legacy-identity");
        let base = 20_737 * DAY_MS;
        fs::create_dir_all(&dir).unwrap();
        let mut bytes = LEGACY_MAGIC.to_vec();
        bytes.extend_from_slice(&[TAG_NAME, 0, 0, 10]);
        bytes.extend_from_slice(b"worker.exe");
        bytes.push(TAG_FRAME);
        let mut head = vec![0u8; FRAME_HEAD];
        head[..8].copy_from_slice(&base.to_le_bytes());
        head[60] = 1;
        bytes.extend_from_slice(&head);
        let mut entry = vec![0u8; ENTRY];
        entry[2..6].copy_from_slice(&42u32.to_le_bytes());
        bytes.extend_from_slice(&entry);
        let path = day_path(&dir, 20_737);
        fs::write(&path, &bytes).unwrap();
        let old = moment(&dir, base).unwrap();
        assert_eq!(old.processes[0].create_time, None);
        let mut rec = HistoryRecorder::new(dir.clone());
        rec.record(&snapshot(
            base + FRAME_MS,
            1.,
            vec![proc(42, "worker.exe", 1., 1024)],
        ));
        rec.record(&snapshot(base + 2 * FRAME_MS, 1., Vec::new()));
        drop(rec);
        assert!(fs::read(&path).unwrap().starts_with(&bytes));
        assert_eq!(
            moment(&dir, base + FRAME_MS).unwrap().processes[0].create_time,
            None
        );
        assert_eq!(legacy_series(&dir, base, base + 2 * FRAME_MS, 10).len(), 3);
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
            .write_all(&[TAG_FRAME, 1, 2, 3])
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
        assert_eq!(usage(&dir), HistoryUsage::default());
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
    fn v4_adds_channels_and_preserves_unknowns_across_midnight() {
        let dir = temp_dir("v4-channels");
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
        assert_eq!(
            &a.series["cpu"].mean[..5],
            &[Some(2.), Some(7.), Some(12.), Some(17.), Some(20.)]
        );
        assert_eq!(a.ts[5], base + 21000);
        let a = series(&dir, base + 31_400, base + 59_999, 10_000, base, &keys);
        let b = series(&dir, base + 38_000, base + 59_999, 10_000, base, &keys);
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
    fn v3_reads_combined_rates_and_appends_without_changing_format() {
        let dir = temp_dir("v3");
        let base = 20_737 * DAY_MS;
        fs::create_dir_all(&dir).unwrap();
        let mut bytes = MAGIC_V3.to_vec();
        bytes.push(TAG_FRAME);
        let mut head = [0; FRAME_HEAD];
        head[..8].copy_from_slice(&base.to_le_bytes());
        head[36..44].copy_from_slice(&1234u64.to_le_bytes());
        bytes.extend(head);
        fs::write(day_path(&dir, 20_737), &bytes).unwrap();
        let mut rec = HistoryRecorder::new(dir.clone());
        rec.record(&snapshot(
            base + 5000,
            4.,
            vec![proc(7, "test.exe", 12., 10 << 20)],
        ));
        drop(rec);
        assert!(
            fs::read(day_path(&dir, 20_737))
                .unwrap()
                .starts_with(&bytes)
        );
        assert_eq!(frame(&dir, base).unwrap().system["disk"], Some(1234.));
        let p = frame(&dir, base + 5000).unwrap().processes.remove(0);
        assert_eq!(p.create_time, Some(1));
        assert_eq!(p.threads, None);
        assert_eq!(
            series(&dir, base, base + 6000, 5000, base, &["diskRead".into()]).series["diskRead"]
                .mean,
            vec![None, None]
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
            (600, Some(450), Some(200), Some(500), 75)
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
        assert_eq!((q.processes[0].io, q.processes[0].net), (1, 3));
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
        prune(&dir, 20_737);
        assert_eq!(days(&dir), vec![20_736]);
        let _ = fs::remove_dir_all(&dir);
    }
}
