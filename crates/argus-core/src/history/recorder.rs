//! Recording: samples gather into five-second frames, each written once a
//! sample of the next one arrives. The last ten minutes of samples are
//! published for the readers (`live`).
use super::*;

#[derive(Default)]
pub(super) struct ProcAcc {
    pub(super) disk_read: Mean,
    pub(super) disk_write: Mean,
    pub(super) power: Mean,
    pub(super) gpu_mem: Option<u64>,
    pub(super) threads: u32,
    pub(super) handles: u32,
    measured: bool,
    missing: u32,
    pub(super) name: Arc<str>,
    pub(super) pid: u32,
    pub(super) create_time: i64,
    pub(super) cpu: f64,
    pub(super) mem: u64,
    pub(super) io: f64,
    pub(super) gpu: f64,
    pub(super) net: f64,
}

impl ProcAcc {
    pub(super) fn divisor(&self, samples: u32) -> f64 {
        samples.saturating_sub(self.missing).max(1) as f64
    }
}

#[derive(Default)]
pub(super) struct Window {
    /// How many processes the programs that run the most had in the last
    /// sample, and how many programs it had (`programs_of`).
    pub(super) programs: Vec<(Arc<str>, u32)>,
    pub(super) program_count: u32,
    pub(super) values: Vec<Mean>,
    pub(super) present: Vec<bool>,
    pub(super) awake: i64,
    boot: i64,
    last: i64,
    interval: i64,
    pub(super) start: i64,
    pub(super) samples: u32,
    pub(super) cpu_peak: f32,
    procs: HashMap<Arc<str>, ProcAcc>,
}

impl Window {
    pub(super) fn span(&self) -> i64 {
        (self.last - self.start + self.interval)
            .clamp(1, FRAME_MS - self.start.rem_euclid(FRAME_MS))
    }
}

pub struct HistoryRecorder {
    dir: PathBuf,
    file: Option<DayFile>,
    window: Window,
    pub(super) catalog: Catalog,
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
pub(super) fn select<T: Copy>(
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

#[derive(Default, Clone, Copy)]
pub(super) struct Mean {
    pub(super) sum: f64,
    pub(super) n: u32,
}
impl Mean {
    pub(super) fn add(&mut self, v: f64) {
        if v.is_finite() {
            self.sum += v;
            self.n = self.n.saturating_add(1);
        }
    }
    pub(super) fn get(self) -> Option<f64> {
        (self.n > 0).then(|| self.sum / f64::from(self.n))
    }
}

#[derive(Clone)]
pub(super) struct Sample {
    pub(super) ts: i64,
    pub(super) span: i64,
    pub(super) slot: i64,
    pub(super) catalog: Arc<Vec<HistoryChannel>>,
    pub(super) values: Vec<f64>,
}
#[derive(Clone, Default)]
pub(super) struct Live {
    pub(super) measurement: Measurement,
    pub(super) evicted: bool,
    previous: Option<Stamp>,
    pub(super) first: Option<Stamp>,
    pub(super) gaps: VecDeque<Gap>,
    catalog: Arc<Vec<HistoryChannel>>,
    pub(super) samples: VecDeque<Arc<Sample>>,
    pub(super) frames: VecDeque<i64>,
}
type SharedLive = Arc<ArcSwap<Live>>;
static ACTIVE: LazyLock<ArcSwap<HashMap<PathBuf, SharedLive>>> = LazyLock::new(Default::default);
pub(super) fn live(dir: &Path) -> Arc<Live> {
    ACTIVE
        .load()
        .get(dir)
        .map(|s| s.load_full())
        .unwrap_or_default()
}
pub(super) fn unix_ms() -> i64 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{proc, snapshot, temp_dir};

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
}
