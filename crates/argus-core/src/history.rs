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
//! Layout of `<YYYY-MM-DD>.hist`, little endian: the magic `ARGH` and a version
//! byte, then records, each starting with a tag byte:
//! - `1` name: index u16, length u8, UTF-8 bytes;
//! - `2` frame: time i64 (Unix ms), CPU f32, CPU peak f32, GPU f32, memory used
//!   u64, memory total u64, disk u64 (B/s), network u64 (B/s), battery f32
//!   (percent, -1 when absent), battery rate f32 (mW, NaN when unknown),
//!   entry count u8, then per entry: name u16, PID u32, CPU f32, memory u32
//!   (KiB), I/O u32 (B/s), GPU f32, network u32 (B/s).

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::collector::Snapshot;

/// Length of one frame.
pub const FRAME_MS: i64 = 5_000;
/// Days of history kept.
pub const KEEP_DAYS: i64 = 7;

const MAGIC: &[u8; 5] = b"ARGH\x02";
const TAG_NAME: u8 = 1;
const TAG_FRAME: u8 = 2;
const FRAME_HEAD: usize = 8 + 4 * 3 + 8 * 4 + 4 * 2 + 1;
const ENTRY: usize = 2 + 4 + 4 + 4 + 4 + 4 + 4;
const DAY_MS: i64 = 86_400_000;

/// System totals for one frame (or one chart bucket).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    /// Unix ms: the frame's start, or a bucket's average.
    pub ts: i64,
    /// Average CPU, percent.
    pub cpu: f32,
    /// Highest CPU sample in the frame (or bucket).
    pub cpu_peak: f32,
    /// Busiest GPU engine, percent.
    pub gpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Bytes per second, all disks.
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

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryProcess {
    pub name: String,
    pub pid: u32,
    /// Averages over the frame.
    pub cpu: f32,
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
    name: Arc<str>,
    pid: u32,
    cpu: f64,
    mem: u64,
    io: f64,
    gpu: f64,
    net: f64,
}

#[derive(Default)]
struct Window {
    start: i64,
    samples: u32,
    cpu: f64,
    cpu_peak: f32,
    gpu: f64,
    mem_used: u64,
    mem_total: u64,
    disk: f64,
    net: f64,
    battery: f64,
    battery_n: u32,
    battery_rate: f64,
    battery_rate_n: u32,
    procs: HashMap<(u32, i64), ProcAcc>,
}

struct DayFile {
    day: i64,
    out: BufWriter<File>,
    names: HashMap<Arc<str>, u16>,
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
            let _ = self.write(&window);
        }
        let w = &mut self.window;
        if w.samples == 0 {
            w.start = slot;
        }
        w.samples += 1;
        w.cpu += s.system.cpu as f64;
        w.cpu_peak = w.cpu_peak.max(s.system.cpu);
        w.gpu += s.hardware.gpus.iter().map(|g| g.util).fold(0.0, f32::max) as f64;
        w.mem_used = s.system.mem_used;
        w.mem_total = s.system.mem_total;
        w.disk += s
            .hardware
            .disks
            .iter()
            .map(|d| d.read + d.write)
            .sum::<u64>() as f64;
        w.net += s.hardware.nics.iter().map(|n| n.rx + n.tx).sum::<u64>() as f64;
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
                .entry((p.pid, p.create_time))
                .or_insert_with(|| ProcAcc {
                    name: p.name.clone(),
                    pid: p.pid,
                    ..Default::default()
                });
            acc.cpu += p.cpu as f64;
            acc.mem = acc.mem.max(p.private_ws);
            acc.io += (p.io_read + p.io_write + p.io_other) as f64;
            acc.gpu += p.gpu as f64;
            acc.net += (p.net_sent + p.net_received) as f64;
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
        let mut procs: Vec<&ProcAcc> = w.procs.values().collect();
        let mut picked: Vec<&ProcAcc> = Vec::new();
        let mut take = |key: fn(&ProcAcc) -> f64, count: usize, floor: f64| {
            procs.sort_by(|a, b| key(b).total_cmp(&key(a)));
            for p in procs.iter().take(count).filter(|p| key(p) > floor) {
                if !picked.iter().any(|q| std::ptr::eq(*q, *p)) {
                    picked.push(*p);
                }
            }
        };
        take(|p| p.cpu, 6, 0.5 * n);
        take(|p| p.mem as f64, 6, 0.0);
        take(|p| p.io, 3, 0.0);
        take(|p| p.gpu, 3, 0.5 * n);
        take(|p| p.net, 3, 0.0);

        let mut buf = Vec::with_capacity(FRAME_HEAD + 1 + picked.len() * ENTRY);
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
        buf.extend_from_slice(&((w.gpu / n) as f32).to_le_bytes());
        buf.extend_from_slice(&w.mem_used.to_le_bytes());
        buf.extend_from_slice(&w.mem_total.to_le_bytes());
        buf.extend_from_slice(&((w.disk / n) as u64).to_le_bytes());
        buf.extend_from_slice(&((w.net / n) as u64).to_le_bytes());
        // -1 and NaN stand in for absence: levels are 0–100, rates are finite.
        let battery = (w.battery_n > 0).then_some(w.battery / w.battery_n as f64);
        buf.extend_from_slice(&(battery.map_or(-1.0, |v| v as f32)).to_le_bytes());
        let rate = (w.battery_rate_n > 0).then_some(w.battery_rate / w.battery_rate_n as f64);
        buf.extend_from_slice(&(rate.map_or(f32::NAN, |v| v as f32)).to_le_bytes());
        buf.push(indices.len() as u8);
        let clamp = |v: f64| v.clamp(0.0, u32::MAX as f64) as u32;
        for (index, p) in indices {
            buf.extend_from_slice(&index.to_le_bytes());
            buf.extend_from_slice(&p.pid.to_le_bytes());
            buf.extend_from_slice(&((p.cpu / n) as f32).to_le_bytes());
            buf.extend_from_slice(&clamp((p.mem / 1024) as f64).to_le_bytes());
            buf.extend_from_slice(&clamp(p.io / n).to_le_bytes());
            buf.extend_from_slice(&((p.gpu / n) as f32).to_le_bytes());
            buf.extend_from_slice(&clamp(p.net / n).to_le_bytes());
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
    if let Ok(bytes) = fs::read(&path)
        && bytes.starts_with(MAGIC)
    {
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
    Ok(DayFile { day, out, names })
}

/// Deletes day files older than the retention window.
fn prune(dir: &Path, today: i64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if day_of(&entry.file_name()).is_some_and(|d| d <= today - KEEP_DAYS) {
            let _ = fs::remove_file(entry.path());
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

/// Walks the records of one file, calling `frame` with each frame's system
/// point, its raw entries and the names defined so far; stops early when
/// `frame` returns false. Returns the length of the valid prefix.
fn parse(
    bytes: &[u8],
    names: &mut Vec<String>,
    mut frame: impl FnMut(&HistoryPoint, &[u8], &[String]) -> bool,
) -> usize {
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
                if at + 1 + FRAME_HEAD > bytes.len() {
                    break;
                }
                let b = &bytes[at + 1..];
                let f32_at = |i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
                let u64_at = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
                let point = HistoryPoint {
                    ts: i64::from_le_bytes(b[0..8].try_into().unwrap()),
                    cpu: f32_at(8),
                    cpu_peak: f32_at(12),
                    gpu: f32_at(16),
                    mem_used: u64_at(20),
                    mem_total: u64_at(28),
                    disk: u64_at(36),
                    net: u64_at(44),
                    battery: {
                        let v = f32_at(52);
                        (v >= 0.0).then_some(v)
                    },
                    battery_rate: {
                        let v = f32_at(56);
                        v.is_finite().then_some(v)
                    },
                };
                let count = b[60] as usize;
                let end = at + 1 + FRAME_HEAD + count * ENTRY;
                if end > bytes.len() {
                    break;
                }
                at = end;
                valid = at;
                if !frame(&point, &bytes[end - count * ENTRY..end], names) {
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

fn entries(raw: &[u8], names: &[String]) -> Vec<HistoryProcess> {
    raw.as_chunks::<ENTRY>()
        .0
        .iter()
        .map(|e| {
            let u32_at = |i: usize| u32::from_le_bytes(e[i..i + 4].try_into().unwrap());
            let f32_at = |i: usize| f32::from_le_bytes(e[i..i + 4].try_into().unwrap());
            let name = u16::from_le_bytes([e[0], e[1]]) as usize;
            HistoryProcess {
                name: names.get(name).cloned().unwrap_or_default(),
                pid: u32_at(2),
                cpu: f32_at(6),
                mem: u32_at(10) as u64 * 1024,
                io: u32_at(14) as u64,
                gpu: f32_at(18),
                net: u32_at(22) as u64,
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
    days
}

fn read_day(dir: &Path, day: i64) -> Option<Vec<u8>> {
    let bytes = fs::read(day_path(dir, day)).ok()?;
    bytes.starts_with(MAGIC).then_some(bytes)
}

/// Unix ms of the oldest frame.
fn first_frame(dir: &Path, days: &[i64]) -> Option<i64> {
    for &day in days {
        let Some(bytes) = read_day(dir, day) else {
            continue;
        };
        let mut first = None;
        parse(&bytes, &mut Vec::new(), |p, _, _| {
            first = Some(p.ts);
            false
        });
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
        let Some(bytes) = read_day(dir, day) else {
            continue;
        };
        parse(&bytes, &mut Vec::new(), |p, _, _| {
            last = Some(p.ts);
            true
        });
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
        cpu: f64,
        cpu_peak: f32,
        gpu: f64,
        mem_used: f64,
        mem_total: u64,
        disk: f64,
        net: f64,
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
        let Some(bytes) = read_day(dir, day) else {
            continue;
        };
        parse(&bytes, &mut Vec::new(), |p, _, _| {
            if p.ts >= from && p.ts <= to {
                let i = (((p.ts - from) as f64 / span) * buckets as f64) as usize;
                let a = &mut acc[i.min(buckets - 1)];
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
            true
        });
    }
    acc.into_iter()
        .filter(|a| a.n > 0)
        .map(|a| {
            let n = a.n as f64;
            HistoryPoint {
                ts: (a.ts / n) as i64,
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

/// The frame closest to `ts`, if one lies within a minute of it.
pub fn moment(dir: &Path, ts: i64) -> Option<HistoryMoment> {
    let day = ts.div_euclid(DAY_MS);
    let mut best: Option<(i64, HistoryMoment)> = None;
    // A moment within a minute of midnight may sit in the neighbouring file.
    let first = (ts - 60_000).div_euclid(DAY_MS).min(day);
    let last = (ts + 60_000).div_euclid(DAY_MS).max(day);
    for d in first..=last {
        let Some(bytes) = read_day(dir, d) else {
            continue;
        };
        let mut names = Vec::new();
        parse(&bytes, &mut names, |p, raw, names| {
            let dist = (p.ts - ts).abs();
            if dist <= 60_000 && best.as_ref().is_none_or(|(b, _)| dist < *b) {
                best = Some((
                    dist,
                    HistoryMoment {
                        system: p.clone(),
                        processes: entries(raw, names),
                    },
                ));
            }
            // Frames are in time order: past the target and getting further, stop.
            !(p.ts > ts && dist > 60_000)
        });
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
            pid,
            ppid: 0,
            create_time: 1,
            name: name.into(),
            path: None,
            description: None,
            company: None,
            icon: None,
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
            gpu_dedicated: 0,
            gpu_shared: 0,
            net_sent: 0,
            net_received: 0,
            threads: 1,
            handles: 1,
            session_id: 1,
            base_priority: 8,
            suspended: false,
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
