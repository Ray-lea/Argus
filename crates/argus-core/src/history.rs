//! History: a compact record of what the machine and its busiest processes
//! were doing, kept on disk for a week so the history page can look back.
//!
//! Every five seconds the recorder writes one frame to the file of the
//! current (UTC) day: system totals averaged over the window, plus the
//! processes that stood out in it (busiest by CPU, memory, I/O, GPU and
//! network). A process name is written once per file and then referred to by
//! index. Files are append-only; a torn record at the end (a crash mid-write)
//! is cut off when the file is opened again.
//!
//! Layout of `<YYYY-MM-DD>.v5.hist`, little endian: the magic `ARGH` and a version
//! byte, then records, each starting with a tag byte:
//! - `1` name: index u16, length u8, UTF-8 bytes;
//! - `2` frame: time i64 (Unix ms), CPU f32, CPU peak f32, GPU f32, memory used
//!   u64, memory total u64, disk u64 (B/s), network u64 (B/s), battery f32
//!   (percent, -1 when absent), battery rate f32 (mW, NaN when unknown),
//!   disk coverage (sample/valid/partial counts, three u32, v4+), GPU and
//!   network coverage (v5), three missing-reason bitsets (v5), entry
//!   count u8, then per entry: name u16, PID u32, CPU f32, memory u32
//!   (KiB), I/O u32 (B/s), GPU f32, network u32 (B/s), creation time i64
//!   (Unix ms, v3+; nonpositive means unknown), network coverage (three u32,
//!   v4+), I/O and GPU coverage (v5), three missing-reason bitsets (v5).
//!   V2/V3 `<YYYY-MM-DD>.hist` and V4 `.v4.hist` remain readable and untouched.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::collector::Snapshot;
use crate::measurement::{self, Measurement};

/// Length of one frame.
pub const FRAME_MS: i64 = 5_000;
/// Days of history kept.
pub const KEEP_DAYS: i64 = 7;

const MAGIC: &[u8; 5] = b"ARGH\x05";
const V4_MAGIC: &[u8; 5] = b"ARGH\x04";
const PREVIOUS_MAGIC: &[u8; 5] = b"ARGH\x03";
const LEGACY_MAGIC: &[u8; 5] = b"ARGH\x02";
const TAG_NAME: u8 = 1;
const TAG_FRAME: u8 = 2;
const FRAME_HEAD: usize = 8 + 4 * 3 + 8 * 4 + 4 * 2 + 1;
const ENTRY: usize = 2 + 4 + 4 + 4 + 4 + 4 + 4;
const ENTRY_V3: usize = ENTRY + 8;
const COVERAGE_BYTES: usize = 12;
const ENTRY_V4: usize = ENTRY_V3 + COVERAGE_BYTES;
const ENTRY_V5: usize = ENTRY_V4 + 2 * COVERAGE_BYTES + 3;
const DAY_MS: i64 = 86_400_000;

/// Availability of one metric. Partial samples contain only a known subset
/// of devices. Legacy frames have no stored availability, never inferred zeros.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricCoverage {
    pub samples: u32,
    pub valid: u32,
    pub partial: u32,
    pub legacy_frames: u32,
    pub missing: u8,
}

impl MetricCoverage {
    fn observe(&mut self, state: Measurement) {
        self.samples += 1;
        self.valid += u32::from(state.valid);
        self.partial += u32::from(state.valid && state.partial);
        self.missing |= state.missing;
    }
    fn write(&self, buf: &mut Vec<u8>) {
        for count in [self.samples, self.valid, self.partial] {
            buf.extend_from_slice(&count.to_le_bytes());
        }
    }
}

#[derive(Default)]
struct MetricAcc {
    sum: f64,
    coverage: MetricCoverage,
}
impl MetricAcc {
    fn record(&mut self, value: f64, state: Measurement) {
        self.coverage.observe(state);
        if state.valid {
            self.sum += value;
        }
    }
    fn mean(&self) -> f64 {
        self.sum / f64::from(self.coverage.valid.max(1))
    }
}

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
    pub gpu_coverage: Option<MetricCoverage>,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Bytes per second from available disk counters.
    pub disk: u64,
    pub disk_coverage: Option<MetricCoverage>,
    /// Bytes per second, all adapters.
    pub net: u64,
    pub net_coverage: Option<MetricCoverage>,
    /// Battery charge, percent, averaged over the frame; None on machines
    /// without a battery (or in frames recorded while it was absent).
    pub battery: Option<f32>,
    /// Battery power flow, mW (positive while charging), averaged over the
    /// frame; None when no battery reports a rate.
    pub battery_rate: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
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
    pub io_coverage: Option<MetricCoverage>,
    pub gpu: f32,
    pub gpu_coverage: Option<MetricCoverage>,
    pub net: u64,
    pub net_coverage: Option<MetricCoverage>,
}

/// What the recorded history takes on disk.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryUsage {
    /// Size of all day files.
    pub bytes: u64,
    /// Unix ms of the oldest recorded frame.
    pub first: Option<i64>,
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
    measured: bool,
    missing: u32,
    name: Arc<str>,
    pid: u32,
    create_time: i64,
    cpu: f64,
    mem: u64,
    io: MetricAcc,
    gpu: MetricAcc,
    net: MetricAcc,
}

impl ProcAcc {
    fn divisor(&self, samples: u32) -> f64 {
        samples.saturating_sub(self.missing).max(1) as f64
    }
}

#[derive(Default)]
struct Window {
    start: i64,
    samples: u32,
    cpu: f64,
    cpu_peak: f32,
    gpu: MetricAcc,
    mem_used: u64,
    mem_total: u64,
    disk: MetricAcc,
    net: MetricAcc,
    battery: f64,
    battery_n: u32,
    battery_rate: f64,
    battery_rate_n: u32,
    procs: HashMap<Arc<str>, ProcAcc>,
}

struct DayFile {
    day: i64,
    out: BufWriter<File>,
    names: HashMap<Arc<str>, u16>,
    version: u8,
}

pub struct HistoryRecorder {
    dir: PathBuf,
    file: Option<DayFile>,
    window: Window,
}

impl HistoryRecorder {
    pub fn new(dir: PathBuf) -> Self {
        Self {
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
        clear(&self.dir)
    }

    /// Adds a sample; writes the previous frame once a sample of the next one arrives.
    pub fn record(&mut self, s: &Snapshot) {
        let slot = s.timestamp - s.timestamp.rem_euclid(FRAME_MS);
        if self.window.samples > 0 && slot != self.window.start {
            let window = std::mem::take(&mut self.window);
            // Losing a frame (disk full, folder gone) is not worth failing a sample over.
            if let Err(error) = self.write(&window) {
                crate::diagnostics::file_failure(
                    "history frame write",
                    &day_path(&self.dir, window.start.div_euclid(DAY_MS)),
                    &error,
                );
            }
        }
        let w = &mut self.window;
        if w.samples == 0 {
            w.start = slot;
        }
        w.samples += 1;
        w.cpu += s.system.cpu as f64;
        w.cpu_peak = w.cpu_peak.max(s.system.cpu);
        w.gpu.record(
            s.hardware
                .gpus
                .iter()
                .filter(|g| g.util_state.valid)
                .map(|g| g.util)
                .fold(0.0, f32::max) as f64,
            Measurement::combine(s.hardware.gpus.iter().map(|g| g.util_state)),
        );
        w.mem_used = s.system.mem_used;
        w.mem_total = s.system.mem_total;
        let disk_sum = s
            .hardware
            .disks
            .iter()
            .filter_map(|d| {
                d.counter_state
                    .valid
                    .then_some(d.read)
                    .flatten()
                    .zip(d.write)
                    .map(|(read, write)| read.saturating_add(write))
            })
            .fold(0u64, u64::saturating_add);
        w.disk.record(
            disk_sum as f64,
            Measurement::combine(s.hardware.disks.iter().map(|d| d.counter_state)),
        );
        w.net.record(
            s.hardware
                .nics
                .iter()
                .filter(|n| n.rate_state.valid)
                .map(|n| n.rx.saturating_add(n.tx))
                .sum::<u64>() as f64,
            s.hardware.network_state,
        );
        // Across batteries the level is averaged and the flows summed, so a
        // single battery (the common case) is exact; partial rates (only some
        // batteries reporting) keep their sum.
        let mut battery = 0.0f64;
        let mut rate = 0.0f64;
        let mut has_rate = false;
        for b in &s.hardware.batteries {
            battery += b.percent as f64;
            if let Some(r) = b.rate {
                rate += r as f64;
                has_rate = true;
            }
        }
        if !s.hardware.batteries.is_empty() {
            w.battery += battery / s.hardware.batteries.len() as f64;
            w.battery_n += 1;
        }
        if has_rate {
            w.battery_rate += rate;
            w.battery_rate_n += 1;
        }
        for p in &s.processes {
            let acc = w
                .procs
                .entry(
                    p.instance_key
                        .clone()
                        .unwrap_or_else(|| format!("{}:{}", p.pid, p.create_time).into()),
                )
                .or_insert_with(|| ProcAcc {
                    name: p.name.clone(),
                    pid: p.pid,
                    ..Default::default()
                });
            if p.create_time > 0 {
                acc.create_time = p.create_time;
            }
            let state = if p.stale || p.sample_reused {
                Measurement::missing(measurement::STALE)
            } else if p.counters_pending {
                Measurement::missing(measurement::BASELINE)
            } else {
                Measurement::ready()
            };
            acc.io.record(
                p.io_read
                    .saturating_add(p.io_write)
                    .saturating_add(p.io_other) as f64,
                if !state.valid || p.io_valid {
                    state
                } else {
                    Measurement::missing(measurement::BASELINE)
                },
            );
            acc.gpu
                .record(p.gpu as f64, if state.valid { p.gpu_state } else { state });
            acc.net.record(
                p.net_sent.saturating_add(p.net_received) as f64,
                if !state.valid {
                    state
                } else if s.process_network {
                    p.net_state
                } else {
                    Measurement::missing(if p.net_state.missing != 0 {
                        p.net_state.missing
                    } else {
                        measurement::UNAVAILABLE
                    })
                },
            );
            if p.stale || p.sample_reused || p.counters_pending {
                acc.missing += 1;
                continue;
            }
            acc.measured = true;
            acc.cpu += p.cpu as f64;
            acc.mem = acc.mem.max(p.private_ws);
        }
    }

    fn write(&mut self, w: &Window) -> io::Result<()> {
        let day = w.start.div_euclid(DAY_MS);
        if self.file.as_ref().is_none_or(|f| f.day != day) {
            self.file = None;
            fs::create_dir_all(&self.dir)?;
            prune(&self.dir, day);
            self.file = Some(open_day(&self.dir, day)?);
        }
        let file = self.file.as_mut().expect("opened above");
        let n = w.samples as f64;
        let mut procs: Vec<&ProcAcc> = w.procs.values().filter(|p| p.measured).collect();
        let mut picked: Vec<&ProcAcc> = Vec::new();
        let mut take = |key: &dyn Fn(&ProcAcc) -> f64, count: usize, floor: f64| {
            procs.sort_by(|a, b| key(b).total_cmp(&key(a)));
            for p in procs.iter().take(count).filter(|p| key(p) > floor) {
                if !picked.iter().any(|q| std::ptr::eq(*q, *p)) {
                    picked.push(*p);
                }
            }
        };
        take(&|p| p.cpu / p.divisor(w.samples), 6, 0.5);
        take(&|p| p.mem as f64, 6, 0.0);
        take(&|p| p.io.mean(), 3, 0.0);
        take(&|p| p.gpu.mean(), 3, 0.5);
        take(&|p| p.net.mean(), 3, 0.0);

        let mut buf =
            Vec::with_capacity(FRAME_HEAD + 3 * COVERAGE_BYTES + 4 + picked.len() * ENTRY_V5);
        let mut indices = Vec::with_capacity(picked.len());
        for p in &picked {
            let index = match file.names.get(&p.name) {
                Some(&i) => i,
                None => {
                    let Ok(i) = u16::try_from(file.names.len()) else {
                        continue;
                    };
                    let bytes = truncate_utf8(&p.name, 255);
                    buf.push(TAG_NAME);
                    buf.extend_from_slice(&i.to_le_bytes());
                    buf.push(bytes.len() as u8);
                    buf.extend_from_slice(bytes);
                    file.names.insert(p.name.clone(), i);
                    i
                }
            };
            indices.push((index, *p));
        }
        buf.push(TAG_FRAME);
        buf.extend_from_slice(&w.start.to_le_bytes());
        buf.extend_from_slice(&((w.cpu / n) as f32).to_le_bytes());
        buf.extend_from_slice(&w.cpu_peak.to_le_bytes());
        buf.extend_from_slice(&(w.gpu.mean() as f32).to_le_bytes());
        buf.extend_from_slice(&w.mem_used.to_le_bytes());
        buf.extend_from_slice(&w.mem_total.to_le_bytes());
        buf.extend_from_slice(&(w.disk.mean() as u64).to_le_bytes());
        buf.extend_from_slice(&(w.net.mean() as u64).to_le_bytes());
        // -1 and NaN stand in for absence: levels are 0–100, rates are finite.
        let battery = (w.battery_n > 0).then_some(w.battery / w.battery_n as f64);
        buf.extend_from_slice(&(battery.map_or(-1.0, |v| v as f32)).to_le_bytes());
        let rate = (w.battery_rate_n > 0).then_some(w.battery_rate / w.battery_rate_n as f64);
        buf.extend_from_slice(&(rate.map_or(f32::NAN, |v| v as f32)).to_le_bytes());
        if file.version >= 4 {
            w.disk.coverage.write(&mut buf);
        }
        if file.version >= 5 {
            w.gpu.coverage.write(&mut buf);
            w.net.coverage.write(&mut buf);
            buf.extend_from_slice(&[
                w.disk.coverage.missing,
                w.gpu.coverage.missing,
                w.net.coverage.missing,
            ]);
        }
        buf.push(indices.len() as u8);
        let clamp = |v: f64| v.clamp(0.0, u32::MAX as f64) as u32;
        for (index, p) in indices {
            let process_n = p.divisor(w.samples);
            buf.extend_from_slice(&index.to_le_bytes());
            buf.extend_from_slice(&p.pid.to_le_bytes());
            buf.extend_from_slice(&((p.cpu / process_n) as f32).to_le_bytes());
            buf.extend_from_slice(&clamp((p.mem / 1024) as f64).to_le_bytes());
            buf.extend_from_slice(&clamp(p.io.mean()).to_le_bytes());
            buf.extend_from_slice(&(p.gpu.mean() as f32).to_le_bytes());
            buf.extend_from_slice(&clamp(p.net.mean()).to_le_bytes());
            if file.version >= 3 {
                buf.extend_from_slice(&p.create_time.to_le_bytes());
            }
            if file.version >= 4 {
                p.net.coverage.write(&mut buf);
            }
            if file.version >= 5 {
                p.io.coverage.write(&mut buf);
                p.gpu.coverage.write(&mut buf);
                buf.extend_from_slice(&[
                    p.net.coverage.missing,
                    p.io.coverage.missing,
                    p.gpu.coverage.missing,
                ]);
            }
        }
        // One write and flush per frame, so readers and crashes see whole frames.
        file.out.write_all(&buf)?;
        file.out.flush()
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
    dir.join(format!("{}.v5.hist", day_name(day)))
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
    let name = file_name.to_str()?.strip_suffix(".hist")?;
    day_number(
        name.strip_suffix(".v5")
            .or_else(|| name.strip_suffix(".v4"))
            .unwrap_or(name),
    )
}

fn day_number(name: &str) -> Option<i64> {
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
    Some(era * 146_097 + doe - 719_468)
}

/// Opens a day file for appending: validates what is there (cutting off a
/// torn last record) and reloads its name table.
fn open_day(dir: &Path, day: i64) -> io::Result<DayFile> {
    let path = day_path(dir, day);
    let mut names = HashMap::new();
    let mut valid = 0u64;
    let version = 5;
    // Only a missing file starts empty: truncating after a transient read
    // failure would erase the day recorded so far.
    let existing = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if let Some(bytes) = existing {
        if !bytes.starts_with(MAGIC) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected history writer version",
            ));
        }
        let mut table = Vec::new();
        valid = parse(&bytes, &mut table, |_, _, _| true) as u64;
        for (i, name) in table.into_iter().enumerate() {
            names.insert(Arc::from(name.as_str()), i as u16);
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    file.set_len(valid)?;
    let mut out = BufWriter::new(file);
    use std::io::Seek;
    out.seek(io::SeekFrom::End(0))?;
    if valid == 0 {
        out.write_all(MAGIC)?;
        out.flush()?;
    }
    Ok(DayFile {
        day,
        out,
        names,
        version,
    })
}

/// Deletes day files older than the retention window.
fn prune(dir: &Path, today: i64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if day_of(&entry.file_name()).is_some_and(|d| d <= today - KEEP_DAYS)
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
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        if day_of(&entry.file_name()).is_some() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

// ---- Reading -----------------------------------------------------------------------

fn supported(bytes: &[u8]) -> bool {
    [MAGIC, V4_MAGIC, PREVIOUS_MAGIC, LEGACY_MAGIC]
        .iter()
        .any(|magic| bytes.starts_with(*magic))
}

fn coverage(bytes: &[u8], offset: usize) -> MetricCoverage {
    let at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    MetricCoverage {
        samples: at(offset),
        valid: at(offset + 4),
        partial: at(offset + 8),
        legacy_frames: 0,
        missing: if at(offset + 4) < at(offset) {
            measurement::UNAVAILABLE
        } else {
            0
        },
    }
}

fn coverage_with_reason(bytes: &[u8], offset: usize, reason: usize) -> MetricCoverage {
    let mut coverage = coverage(bytes, offset);
    coverage.missing = bytes[reason] & 0x0f;
    coverage
}

#[derive(Default, Clone)]
struct BucketMetric {
    sum: f64,
    legacy_sum: f64,
    new_frames: u32,
    coverage: MetricCoverage,
}

impl BucketMetric {
    fn add(&mut self, value: f64, coverage: &Option<MetricCoverage>) {
        if let Some(coverage) = coverage {
            self.new_frames += 1;
            self.sum += value * f64::from(coverage.valid);
            self.coverage.samples = self.coverage.samples.saturating_add(coverage.samples);
            self.coverage.valid = self.coverage.valid.saturating_add(coverage.valid);
            self.coverage.partial = self.coverage.partial.saturating_add(coverage.partial);
            self.coverage.missing |= coverage.missing;
        } else {
            self.coverage.legacy_frames += 1;
            self.legacy_sum += value;
        }
    }
    fn mean(&self) -> f64 {
        if self.new_frames > 0 {
            self.sum / f64::from(self.coverage.valid.max(1))
        } else {
            self.legacy_sum / f64::from(self.coverage.legacy_frames.max(1))
        }
    }
    fn coverage(self) -> Option<MetricCoverage> {
        (self.new_frames > 0).then_some(self.coverage)
    }
}

/// Walks the records of one file, calling `frame` with each frame's system
/// point, its raw entries and the names defined so far; stops early when
/// `frame` returns false. Returns the length of the valid prefix.
fn parse(
    bytes: &[u8],
    names: &mut Vec<String>,
    mut frame: impl FnMut(&HistoryPoint, &[u8], &[String]) -> bool,
) -> usize {
    if !supported(bytes) {
        return 0;
    }
    let version = bytes[4];
    let entry_size = match version {
        2 => ENTRY,
        3 => ENTRY_V3,
        4 => ENTRY_V4,
        _ => ENTRY_V5,
    };
    let frame_head = FRAME_HEAD
        + if version >= 5 {
            COVERAGE_BYTES * 3 + 3
        } else if version == 4 {
            COVERAGE_BYTES
        } else {
            0
        };
    let mut at = MAGIC.len();
    let mut valid = at;
    let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    while at < bytes.len() {
        match bytes[at] {
            TAG_NAME => {
                if at + 4 > bytes.len() {
                    break;
                }
                let len = bytes[at + 3] as usize;
                let end = at + 4 + len;
                if end > bytes.len() {
                    break;
                }
                let index = u16_at(bytes, at + 1) as usize;
                let name = String::from_utf8_lossy(&bytes[at + 4..end]).into_owned();
                if index == names.len() {
                    names.push(name);
                }
                at = end;
            }
            TAG_FRAME => {
                if at + 1 + frame_head > bytes.len() {
                    break;
                }
                let b = &bytes[at + 1..];
                let f32_at = |i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
                let u64_at = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
                let point = HistoryPoint {
                    ts: i64::from_le_bytes(b[0..8].try_into().unwrap()),
                    frame_ts: i64::from_le_bytes(b[0..8].try_into().unwrap()),
                    cpu_peak_ts: i64::from_le_bytes(b[0..8].try_into().unwrap()),
                    cpu: f32_at(8),
                    cpu_peak: f32_at(12),
                    gpu: f32_at(16),
                    gpu_coverage: (version >= 5).then(|| coverage_with_reason(b, 72, 97)),
                    mem_used: u64_at(20),
                    mem_total: u64_at(28),
                    disk: u64_at(36),
                    disk_coverage: (version >= 4).then(|| {
                        if version >= 5 {
                            coverage_with_reason(b, 60, 96)
                        } else {
                            coverage(b, 60)
                        }
                    }),
                    net: u64_at(44),
                    net_coverage: (version >= 5).then(|| coverage_with_reason(b, 84, 98)),
                    battery: {
                        let v = f32_at(52);
                        (v >= 0.0).then_some(v)
                    },
                    battery_rate: {
                        let v = f32_at(56);
                        v.is_finite().then_some(v)
                    },
                };
                let count = b[frame_head - 1] as usize;
                let end = at + 1 + frame_head + count * entry_size;
                if end > bytes.len() {
                    break;
                }
                at = end;
                valid = at;
                if !frame(&point, &bytes[end - count * entry_size..end], names) {
                    return valid;
                }
                continue;
            }
            _ => break,
        }
        valid = at;
    }
    valid
}

fn entries(raw: &[u8], names: &[String], version: u8) -> Vec<HistoryProcess> {
    raw.chunks_exact(match version {
        2 => ENTRY,
        3 => ENTRY_V3,
        4 => ENTRY_V4,
        _ => ENTRY_V5,
    })
    .map(|e| {
        let u32_at = |i: usize| u32::from_le_bytes(e[i..i + 4].try_into().unwrap());
        let f32_at = |i: usize| f32::from_le_bytes(e[i..i + 4].try_into().unwrap());
        let name = u16::from_le_bytes([e[0], e[1]]) as usize;
        HistoryProcess {
            name: names.get(name).cloned().unwrap_or_default(),
            pid: u32_at(2),
            create_time: if version < 3 {
                None
            } else {
                let created = i64::from_le_bytes(e[ENTRY..ENTRY_V3].try_into().unwrap());
                (created > 0).then_some(created)
            },
            cpu: f32_at(6),
            mem: u32_at(10) as u64 * 1024,
            io: u32_at(14) as u64,
            io_coverage: (version >= 5).then(|| coverage_with_reason(e, ENTRY_V4, ENTRY_V5 - 2)),
            gpu: f32_at(18),
            gpu_coverage: (version >= 5)
                .then(|| coverage_with_reason(e, ENTRY_V4 + COVERAGE_BYTES, ENTRY_V5 - 1)),
            net: u32_at(22) as u64,
            net_coverage: (version >= 4).then(|| {
                if version >= 5 {
                    coverage_with_reason(e, ENTRY_V3, ENTRY_V5 - 3)
                } else {
                    coverage(e, ENTRY_V3)
                }
            }),
        }
    })
    .collect()
}

/// Day files in the folder, oldest first.
fn days(dir: &Path) -> Vec<i64> {
    let mut days: Vec<i64> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| day_of(&e.file_name()))
        .collect();
    days.sort_unstable();
    days.dedup();
    days
}

fn read_day(dir: &Path, day: i64) -> Vec<Vec<u8>> {
    // V4 starts beside a legacy day file, preserving that file byte for byte.
    // The new file wins if an upgrade overlaps a timestamp already recorded.
    [
        day_path(dir, day),
        dir.join(format!("{}.v4.hist", day_name(day))),
        dir.join(format!("{}.hist", day_name(day))),
    ]
    .into_iter()
    .filter_map(|path| {
        let bytes = fs::read(&path)
            .inspect_err(|error| {
                if error.kind() != io::ErrorKind::NotFound {
                    crate::diagnostics::file_failure("history read", &path, error);
                }
            })
            .ok()?;
        supported(&bytes).then_some(bytes)
    })
    .collect()
}

/// Unix ms of the oldest frame.
fn first_frame(dir: &Path, days: &[i64]) -> Option<i64> {
    for &day in days {
        let mut first = None;
        for bytes in read_day(dir, day) {
            parse(&bytes, &mut Vec::new(), |p, _, _| {
                first = Some(first.map_or(p.ts, |at: i64| at.min(p.ts)));
                true
            });
        }
        if first.is_some() {
            return first;
        }
    }
    None
}

/// Size on disk and the oldest frame.
pub fn usage(dir: &Path) -> HistoryUsage {
    let bytes = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| day_of(&e.file_name()).is_some())
        // Through the path, not the directory listing: the listing's size of
        // today's file (open for appending) lags behind.
        .filter_map(|e| fs::metadata(e.path()).ok())
        .map(|m| m.len())
        .sum();
    HistoryUsage {
        bytes,
        first: first_frame(dir, &days(dir)),
    }
}

/// Unix ms of the first and the last recorded frame.
pub fn range(dir: &Path) -> Option<(i64, i64)> {
    let days = days(dir);
    let first = first_frame(dir, &days);
    let mut last = None;
    for &day in days.iter().rev() {
        for bytes in read_day(dir, day) {
            parse(&bytes, &mut Vec::new(), |p, _, _| {
                last = Some(last.map_or(p.ts, |at: i64| at.max(p.ts)));
                true
            });
        }
        if last.is_some() {
            break;
        }
    }
    Some((first?, last?))
}

/// System totals between `from` and `to` (Unix ms), averaged into at most
/// `buckets` points; empty buckets are left out, so gaps (Argus not running)
/// stay gaps. The CPU peak is the highest of the bucket.
pub fn series(dir: &Path, from: i64, to: i64, buckets: usize) -> Vec<HistoryPoint> {
    if to <= from || buckets == 0 {
        return Vec::new();
    }
    #[derive(Default, Clone)]
    struct Acc {
        n: u32,
        ts: f64,
        frame_ts: i64,
        cpu_peak_ts: i64,
        cpu: f64,
        cpu_peak: f32,
        gpu: BucketMetric,
        mem_used: f64,
        mem_total: u64,
        disk: BucketMetric,
        net: BucketMetric,
        battery: f64,
        battery_n: u32,
        battery_rate: f64,
        battery_rate_n: u32,
    }
    let mut acc = vec![Acc::default(); buckets];
    let span = (to - from) as f64;
    for day in days(dir) {
        if (day + 1) * DAY_MS <= from || day * DAY_MS > to {
            continue;
        }
        let files = read_day(dir, day);
        let mut seen = (files.len() > 1).then(HashSet::new);
        for bytes in files {
            parse(&bytes, &mut Vec::new(), |p, _, _| {
                if p.ts >= from && p.ts <= to {
                    if seen.as_mut().is_some_and(|seen| !seen.insert(p.ts)) {
                        return true;
                    }
                    let i = (((p.ts - from) as f64 / span) * buckets as f64) as usize;
                    let a = &mut acc[i.min(buckets - 1)];
                    if a.n == 0 || p.ts < a.frame_ts {
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
                    a.gpu.add(p.gpu as f64, &p.gpu_coverage);
                    a.mem_used += p.mem_used as f64;
                    a.mem_total = p.mem_total;
                    a.disk.add(p.disk as f64, &p.disk_coverage);
                    a.net.add(p.net as f64, &p.net_coverage);
                    if let Some(v) = p.battery {
                        a.battery += v as f64;
                        a.battery_n += 1;
                    }
                    if let Some(v) = p.battery_rate {
                        a.battery_rate += v as f64;
                        a.battery_rate_n += 1;
                    }
                }
                true
            });
        }
    }
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
                gpu: a.gpu.mean() as f32,
                gpu_coverage: a.gpu.coverage(),
                mem_used: (a.mem_used / n) as u64,
                mem_total: a.mem_total,
                disk: a.disk.mean() as u64,
                disk_coverage: a.disk.coverage(),
                net: a.net.mean() as u64,
                net_coverage: a.net.coverage(),
                battery: (a.battery_n > 0).then_some((a.battery / a.battery_n as f64) as f32),
                battery_rate: (a.battery_rate_n > 0)
                    .then_some((a.battery_rate / a.battery_rate_n as f64) as f32),
            }
        })
        .collect()
}

/// The frame closest to `ts`, if one lies within a minute of it.
pub fn moment(dir: &Path, ts: i64) -> Option<HistoryMoment> {
    let day = ts.div_euclid(DAY_MS);
    let mut best: Option<(i64, HistoryMoment)> = None;
    // A moment within a minute of midnight may sit in the neighbouring file.
    let first = (ts - 60_000).div_euclid(DAY_MS).min(day);
    let last = (ts + 60_000).div_euclid(DAY_MS).max(day);
    for d in first..=last {
        for bytes in read_day(dir, d) {
            let mut names = Vec::new();
            parse(&bytes, &mut names, |p, raw, names| {
                let dist = (p.ts - ts).abs();
                if dist <= 60_000 && best.as_ref().is_none_or(|(b, _)| dist < *b) {
                    best = Some((
                        dist,
                        HistoryMoment {
                            system: p.clone(),
                            processes: entries(raw, names, bytes[4]),
                        },
                    ));
                }
                // A clock correction may put later records before earlier ones.
                true
            });
        }
    }
    best.map(|(_, m)| m)
}

#[cfg(test)]
mod tests {
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
            io_valid: true,
            disk_read: None,
            disk_write: None,
            gpu: 0.0,
            gpu_state: Measurement::ready(),
            gpu_engine: None,
            gpu_engines: Vec::new(),
            gpu_dedicated: Some(0),
            gpu_shared: Some(0),
            net_sent: 0,
            net_received: 0,
            net_state: Measurement::ready(),
            net_reused: false,
            threads: 1,
            handles: 1,
            session_id: 1,
            base_priority: 8,
            suspended: false,
            frozen: false,
            efficiency: false,
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
    fn gpu_network_and_io_keep_independent_coverage_and_missing_reasons() {
        use crate::hardware::{GpuInfo, GpuStats, GpuUtilSource, NicInfo, NicStats};
        let dir = temp_dir("all-metric-coverage");
        let base = 20_737 * DAY_MS;
        let mut recorder = HistoryRecorder::new(dir.clone());
        let ready = Measurement::ready();
        let denied = Measurement::failure(5);
        let stale = Measurement::missing(measurement::STALE);
        let baseline = Measurement::missing(measurement::BASELINE);
        for (i, state, gpu, io_valid) in [
            (0, ready, 0., true),
            (1, denied, 99., false),
            (2, ready, 12., true),
            (3, stale, 99., false),
        ] {
            let mut row = proc(42, "worker.exe", 2., 1024);
            row.gpu = gpu;
            row.gpu_state = state;
            row.io_read = if i == 2 {
                100
            } else if i == 0 {
                0
            } else {
                999
            };
            row.io_valid = io_valid;
            row.net_received = if i == 2 {
                200
            } else if i == 0 {
                0
            } else {
                999
            };
            row.net_state = if i == 1 {
                stale
            } else if i == 3 {
                baseline
            } else {
                ready
            };
            let net_state = row.net_state;
            let net = row.net_received;
            let mut sample = snapshot(base + i * 1000, 2., vec![row]);
            sample.process_network = true;
            sample.hardware.network_state = net_state;
            sample.hardware.nics.push(NicStats {
                info: Arc::new(NicInfo {
                    id: "1".into(),
                    name: "test".into(),
                    description: String::new(),
                    kind: "ethernet",
                    link_speed: 1000,
                    mac: None,
                    ipv4: vec![],
                    ipv6: vec![],
                }),
                rx: net,
                tx: 0,
                rate_state: net_state,
            });
            sample.hardware.gpus.push(GpuStats {
                info: Arc::new(GpuInfo {
                    id: "1".into(),
                    name: "test".into(),
                    dedicated_total: 0,
                    shared_total: 0,
                    engines: vec!["3D".into()],
                    temperature_max: None,
                    intel_integrated: false,
                    driver_version: None,
                    driver_date: None,
                    direct_x: None,
                }),
                util: gpu,
                util_source: GpuUtilSource::Windows,
                util_state: state,
                engines: vec![gpu],
                dedicated_used: 0,
                shared_used: 0,
                temperature: None,
                fan_rpm: None,
                power: None,
                board_power: None,
                board_power_limit: None,
                power_source: None,
                clock_mhz: None,
                clock_max_mhz: None,
                voltage_mv: None,
                memory_clock_mhz: None,
                core_vid_mv: None,
                kernel_sensor_state: None,
                kernel_sensor_supported: false,
                integrated_sensors: None,
            });
            recorder.record(&sample);
        }
        recorder.record(&snapshot(base + FRAME_MS, 0., vec![]));
        let recorded = moment(&dir, base).unwrap();
        assert_eq!(recorded.system.gpu, 6.);
        assert_eq!(recorded.system.net, 100);
        let row = &recorded.processes[0];
        assert_eq!((row.io, row.gpu, row.net), (50, 6., 100));
        for coverage in [
            &recorded.system.gpu_coverage,
            &recorded.system.net_coverage,
            &row.io_coverage,
            &row.gpu_coverage,
            &row.net_coverage,
        ] {
            let coverage = coverage.as_ref().unwrap();
            assert_eq!((coverage.samples, coverage.valid), (4, 2));
        }
        assert_eq!(
            row.gpu_coverage.as_ref().unwrap().missing,
            measurement::ACCESS_DENIED | measurement::STALE
        );
        assert_eq!(
            row.io_coverage.as_ref().unwrap().missing,
            measurement::BASELINE
        );
        assert_eq!(
            row.net_coverage.as_ref().unwrap().missing,
            measurement::STALE | measurement::BASELINE
        );
        let aggregate = series(&dir, base, base + FRAME_MS - 1, 1).remove(0);
        assert_eq!(aggregate.gpu_coverage, recorded.system.gpu_coverage);
        assert_eq!(aggregate.net_coverage, recorded.system.net_coverage);
        assert_eq!((aggregate.gpu, aggregate.net), (6., 100));
        drop(recorder);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unavailable_disk_and_network_are_not_idle_or_zero_weighted() {
        let dir = temp_dir("metric-coverage");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for (offset, disk, network, net) in [
            (0, Some(0), true, 0),
            (5000, None, false, 999),
            (10000, Some(1000), true, 100),
            (11000, None, false, 999),
            (15000, Some(2000), true, 200),
        ] {
            rec.record(&metric_sample(base + offset, vec![disk], network, net));
        }
        let idle = moment(&dir, base).unwrap();
        let missing = moment(&dir, base + 5000).unwrap();
        assert_eq!((idle.system.disk, missing.system.disk), (0, 0));
        assert_eq!(idle.system.disk_coverage.as_ref().unwrap().valid, 1);
        assert_eq!(missing.system.disk_coverage.as_ref().unwrap().valid, 0);
        assert_eq!(idle.processes[0].net_coverage.as_ref().unwrap().valid, 1);
        assert_eq!(missing.processes[0].net_coverage.as_ref().unwrap().valid, 0);
        let mixed = moment(&dir, base + 10000).unwrap();
        assert_eq!((mixed.system.disk, mixed.processes[0].net), (1000, 100));
        assert_eq!(
            mixed.system.disk_coverage,
            Some(MetricCoverage {
                samples: 2,
                valid: 1,
                missing: measurement::UNAVAILABLE,
                ..Default::default()
            })
        );
        assert_eq!(
            mixed.processes[0].net_coverage,
            Some(MetricCoverage {
                samples: 2,
                valid: 1,
                missing: measurement::UNAVAILABLE,
                ..Default::default()
            })
        );
        let aggregate = series(&dir, base + 5000, base + 14999, 1).remove(0);
        assert_eq!(aggregate.disk, 1000);
        assert_eq!(
            aggregate.disk_coverage,
            Some(MetricCoverage {
                samples: 3,
                valid: 1,
                missing: measurement::UNAVAILABLE,
                ..Default::default()
            })
        );
        drop(rec);
        fs::remove_dir_all(dir).unwrap();
    }

    fn metric_sample(ts: i64, disks: Vec<Option<u64>>, network: bool, net: u64) -> Snapshot {
        let mut process = proc(42, "worker.exe", 2., 1024);
        process.net_received = net;
        let mut snapshot = snapshot(ts, 2., vec![process]);
        snapshot.process_network = network;
        for (index, disk) in disks.into_iter().enumerate() {
            snapshot.hardware.disks.push(crate::hardware::DiskStats {
                info: Arc::new(crate::hardware::DiskInfo {
                    index: index as u32,
                    model: "test disk".into(),
                    size: 1 << 30,
                    bus: "usb",
                    ssd: None,
                    removable: true,
                    volumes: vec![],
                }),
                active: disk.map(|_| 0.),
                read: disk,
                write: disk.map(|_| 0),
                counter_state: disk.map_or(Measurement::default(), |_| Measurement::ready()),
                response_ms: None,
                temperature: None,
                temperature_warning: None,
                performance_volumes: None,
            });
        }
        snapshot
    }

    fn legacy_frame(version: u8, ts: i64, disk: u64) -> Vec<u8> {
        let mut bytes = b"ARGH".to_vec();
        bytes.push(version);
        bytes.extend([TAG_NAME, 0, 0, 10]);
        bytes.extend(b"worker.exe");
        bytes.push(TAG_FRAME);
        let mut head = vec![0; FRAME_HEAD + if version == 4 { COVERAGE_BYTES } else { 0 }];
        head[..8].copy_from_slice(&ts.to_le_bytes());
        head[36..44].copy_from_slice(&disk.to_le_bytes());
        *head.last_mut().unwrap() = 1;
        if version == 4 {
            head[60..64].copy_from_slice(&1u32.to_le_bytes());
            head[64..68].copy_from_slice(&1u32.to_le_bytes());
        }
        bytes.extend(head);
        let mut entry = vec![
            0;
            match version {
                2 => ENTRY,
                3 => ENTRY_V3,
                _ => ENTRY_V4,
            }
        ];
        entry[2..6].copy_from_slice(&42u32.to_le_bytes());
        if version >= 3 {
            entry[ENTRY..ENTRY_V3].copy_from_slice(&(ts - 1000).to_le_bytes());
        }
        if version == 4 {
            entry[ENTRY_V3..ENTRY_V3 + 4].copy_from_slice(&1u32.to_le_bytes());
            entry[ENTRY_V3 + 4..ENTRY_V3 + 8].copy_from_slice(&1u32.to_le_bytes());
        }
        bytes.extend(entry);
        bytes
    }

    #[test]
    fn v4_coverage_survives_upgrade_without_inventing_other_metrics() {
        let dir = temp_dir("upgrade-v4");
        let base = 20_737 * DAY_MS;
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join(format!("{}.v4.hist", day_name(20_737)));
        let bytes = legacy_frame(4, base, 900);
        fs::write(&old, &bytes).unwrap();
        let legacy = moment(&dir, base).unwrap();
        assert_eq!(legacy.system.disk_coverage.as_ref().unwrap().valid, 1);
        assert!(legacy.system.gpu_coverage.is_none());
        assert!(legacy.system.net_coverage.is_none());
        assert!(legacy.processes[0].io_coverage.is_none());
        assert!(legacy.processes[0].gpu_coverage.is_none());
        assert_eq!(legacy.processes[0].net_coverage.as_ref().unwrap().valid, 1);
        let mut recorder = HistoryRecorder::new(dir.clone());
        recorder.record(&metric_sample(base + FRAME_MS, vec![Some(1000)], true, 10));
        recorder.record(&metric_sample(base + 2 * FRAME_MS, vec![Some(0)], true, 0));
        assert_eq!(fs::read(&old).unwrap(), bytes);
        let aggregate = series(&dir, base, base + FRAME_MS, 1).remove(0);
        assert_eq!(aggregate.disk, 950);
        assert_eq!(aggregate.disk_coverage.as_ref().unwrap().valid, 2);
        assert_eq!(aggregate.gpu_coverage.as_ref().unwrap().legacy_frames, 1);
        drop(recorder);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preserves_old_day_files_and_deduplicates_upgrade_overlap() {
        let base = 20_737 * DAY_MS;
        for version in [2, 3] {
            let dir = temp_dir(&format!("upgrade-{version}"));
            fs::create_dir_all(&dir).unwrap();
            let old = dir.join(format!("{}.hist", day_name(20737)));
            let bytes = legacy_frame(version, base, 900);
            fs::write(&old, &bytes).unwrap();
            assert_eq!(moment(&dir, base).unwrap().system.disk_coverage, None);
            let mut rec = HistoryRecorder::new(dir.clone());
            rec.record(&metric_sample(base, vec![Some(1000)], true, 10));
            rec.record(&metric_sample(base + FRAME_MS, vec![Some(0)], true, 0));
            assert_eq!(fs::read(&old).unwrap(), bytes);
            assert_eq!(range(&dir), Some((base, base)));
            let points = series(&dir, base, base + FRAME_MS, 10);
            assert_eq!(points.len(), 1);
            assert_eq!(points[0].disk, 1000);
            assert_eq!(moment(&dir, base).unwrap().system.disk, 1000);
            drop(rec);
            clear(&dir).unwrap();
            assert!(!old.exists());
            assert!(!day_path(&dir, 20737).exists());
            fs::remove_dir(dir).unwrap();
        }
    }

    #[test]
    fn mixed_legacy_and_new_buckets_keep_legacy_coverage_unknown() {
        let dir = temp_dir("mixed-legacy-coverage");
        fs::create_dir_all(&dir).unwrap();
        let base = 20737 * DAY_MS;
        fs::write(
            dir.join(format!("{}.hist", day_name(20737))),
            legacy_frame(3, base, 900),
        )
        .unwrap();
        let mut rec = HistoryRecorder::new(dir.clone());
        rec.record(&metric_sample(base + FRAME_MS, vec![Some(1000)], true, 0));
        rec.record(&metric_sample(base + 2 * FRAME_MS, vec![Some(0)], true, 0));
        let aggregate = series(&dir, base, base + 2 * FRAME_MS, 1).remove(0);
        assert_eq!(aggregate.disk, 1000);
        assert_eq!(aggregate.frame_ts, base);
        assert_eq!(
            aggregate.disk_coverage,
            Some(MetricCoverage {
                samples: 1,
                valid: 1,
                partial: 0,
                legacy_frames: 1,
                missing: 0,
            })
        );
        drop(rec);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn partial_device_sums_and_backwards_clock_remain_explicit() {
        let dir = temp_dir("partial-clock");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        rec.record(&metric_sample(
            base + 120000,
            vec![Some(100), Some(200)],
            true,
            0,
        ));
        rec.record(&metric_sample(base, vec![Some(500), None], true, 0));
        rec.record(&metric_sample(
            base + FRAME_MS,
            vec![Some(0), Some(0)],
            true,
            0,
        ));
        let frame = moment(&dir, base).unwrap();
        assert_eq!(frame.system.ts, base);
        assert_eq!(frame.system.disk, 500);
        assert_eq!(frame.system.disk_coverage.as_ref().unwrap().partial, 1);
        assert_eq!(range(&dir), Some((base, base + 120000)));
        drop(rec);
        fs::remove_dir_all(dir).unwrap();
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
        let bucket = series(&dir, base, base + 10000, 1).remove(0);
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
        assert_eq!(series(&dir, base, base + 10000, 1)[0].cpu_peak_ts, base);
        // An average timestamp can lie in a long gap. Its representative cannot.
        rec.clear().unwrap();
        for offset in [0, 600000, 605000] {
            rec.record(&snapshot(
                base + offset,
                1.,
                vec![proc(42, "sample.exe", 1., 1024)],
            ));
        }
        let bucket = series(&dir, base, base + 7 * DAY_MS, 500).remove(0);
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
        let path = dir.join(format!("{}.hist", day_name(20_737)));
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
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            moment(&dir, base + FRAME_MS).unwrap().processes[0].create_time,
            Some(1)
        );
        assert_eq!(series(&dir, base, base + 2 * FRAME_MS, 10).len(), 2);
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
        let points = series(&dir, base, base + 15_000, 3);
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
        // 12 one-second samples: frames at base and base+5s are complete.
        let (first, last) = range(&dir).unwrap();
        assert_eq!((first, last), (base, base + 5_000));
        let m = moment(&dir, base + 1_000).unwrap();
        assert_eq!(m.system.ts, base);
        assert!((m.system.cpu - 12.0).abs() < 0.01); // average of 10..=14
        assert_eq!(m.system.cpu_peak, 14.0);
        let busy = m.processes.iter().find(|p| p.name == "busy.exe").unwrap();
        assert_eq!((busy.pid, busy.cpu, busy.mem), (42, 30.0, 1 << 30));
        let points = series(&dir, base, base + 10_000, 100);
        assert_eq!(points.len(), 2);
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
        let points = series(&dir, base, base + 60_000, 60);
        assert_eq!(
            points.iter().map(|p| p.ts).collect::<Vec<_>>(),
            vec![base, base + 5_000, base + 20_000]
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
            midnight + 5_000
        );
        assert_eq!(
            moment(&dir, midnight - 60_000).unwrap().system.ts,
            midnight - 10_000
        );
        assert!(moment(&dir, midnight + 70_000).is_none());
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
