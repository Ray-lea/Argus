//! Detect excursions in UTC minutes, then attribute only the returned intervals.
use super::*;
use std::sync::Weak;

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Cpu,
    Temp,
    Mem,
    Gpu,
    Net,
    Disk,
}

#[derive(Debug, Serialize)]
pub struct HistoryTop {
    pub name: String,
    pub share: f64,
    pub mean: f64,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum HistoryEvent {
    Resource {
        kind: ResourceKind,
        from: i64,
        to: i64,
        peak: f64,
        #[serde(rename = "peakAt")]
        peak_at: i64,
        top: Option<HistoryTop>,
    },
    Gap {
        kind: &'static str,
        #[serde(flatten)]
        gap: Gap,
    },
}

#[derive(Debug, Default, Serialize)]
pub struct HistoryEvents {
    pub events: Vec<HistoryEvent>,
    pub truncated: bool,
}

const MB: f64 = (1 << 20) as f64;
struct Rules {
    resources: [(ResourceKind, f64, i64); 6],
    gap_minutes: i64,
    merge_minutes: i64,
}
const RULES: Rules = Rules {
    resources: [
        (ResourceKind::Cpu, 85., 3),
        (ResourceKind::Temp, 90., 2),
        (ResourceKind::Mem, 85., 1),
        (ResourceKind::Gpu, 75., 15),
        (ResourceKind::Net, 15. * MB, 3),
        (ResourceKind::Disk, 80. * MB, 3),
    ],
    gap_minutes: 10,
    merge_minutes: 3,
};
const LIMIT: usize = 500;

#[derive(Clone, Copy)]
struct Peak {
    value: f64,
    at: i64,
}
impl Default for Peak {
    fn default() -> Self {
        Self {
            value: f64::NEG_INFINITY,
            at: 0,
        }
    }
}
impl Peak {
    fn include(&mut self, other: Self) {
        if other.value.is_finite()
            && (other.value > self.value || (other.value == self.value && other.at < self.at))
        {
            *self = other;
        }
    }
}

struct EventMinute {
    slot: i64,
    above: [bool; 6],
    peaks: [Peak; 6],
}
#[derive(Default)]
struct Accumulator {
    means: [Mean; 6],
    total_memory: Mean,
    peaks: [Peak; 6],
}
impl Accumulator {
    fn finish(self, slot: i64) -> EventMinute {
        let above = std::array::from_fn(|k| {
            let mut mean = self.means[k].get().unwrap_or(f64::NAN);
            if RULES.resources[k].0 == ResourceKind::Mem {
                mean = percent(mean, self.total_memory.get().unwrap_or(f64::NAN));
            }
            mean.is_finite() && mean >= RULES.resources[k].1
        });
        EventMinute {
            slot,
            above,
            peaks: self.peaks,
        }
    }
}

fn percent(used: f64, total: f64) -> f64 {
    if used.is_finite() && used >= 0. && total.is_finite() && total > 0. {
        (used / total * 100.).min(100.)
    } else {
        f64::NAN
    }
}

/// Rows are paired before deriving disk throughput and memory pressure. Sealed
/// days store marginal bands, so independent read/write maxima cannot be added.
struct Observation {
    values: [f64; 6],
    peaks: [f64; 6],
    total_memory: f64,
}
impl Observation {
    fn read(row: &Reading<'_>, keys: &[usize; 10]) -> Self {
        let [
            cpu,
            cpu_peak,
            temp,
            used,
            total,
            gpu,
            net,
            read,
            write,
            disk,
        ] = *keys;
        let value = |i| row.get(i);
        let high = |i: usize| match row {
            Reading::Minute(_, bands) => bands
                .get(i)
                .filter(|b| b.n > 0)
                .map_or(f64::NAN, |b| b.high),
            _ => value(i),
        };
        let constant = |i: usize| match row {
            Reading::Minute(_, bands) => bands.get(i).is_some_and(|b| b.n > 0 && b.low == b.high),
            _ => true,
        };
        let total_memory = value(total);
        let mem = if percent(value(used), total_memory).is_finite() {
            value(used)
        } else {
            f64::NAN
        };
        let disk_rate = if disk != usize::MAX {
            value(disk)
        } else {
            value(read) + value(write)
        };
        let disk_peak = if disk != usize::MAX {
            high(disk)
        } else if constant(read) || constant(write) {
            high(read) + high(write)
        } else {
            disk_rate
        };
        let mem_peak = percent(if constant(total) { high(used) } else { mem }, total_memory);
        Self {
            values: [
                value(cpu),
                value(temp),
                mem,
                value(gpu),
                value(net),
                disk_rate,
            ],
            peaks: [
                finite(high(cpu_peak))
                    .unwrap_or(f64::NEG_INFINITY)
                    .max(high(cpu)),
                high(temp),
                finite(mem_peak).unwrap_or_else(|| percent(mem, total_memory)),
                high(gpu),
                high(net),
                finite(disk_peak).unwrap_or(disk_rate),
            ],
            total_memory,
        }
    }
    fn add(&self, acc: &mut Accumulator, at: i64) {
        if self.values[2].is_finite() {
            acc.total_memory.add(self.total_memory);
        }
        for (k, &v) in self.values.iter().enumerate() {
            if v.is_finite() {
                acc.means[k].add(v);
                acc.peaks[k].include(Peak {
                    value: self.peaks[k],
                    at,
                });
            }
        }
    }
}

fn scan_day(path: &Path, idx: &Arc<Index>, from: i64, to: i64) -> Option<Arc<[EventMinute]>> {
    let resolution = if idx.version == MINUTE_VERSION {
        MINUTE_MS
    } else {
        FRAME_MS
    };
    let day = DayRead::open(
        path,
        idx.day,
        idx.clone(),
        from - resolution + 1,
        to - 1,
        false,
    )?;
    let keys = [
        "cpu",
        "cpuPeak",
        "cpuTemp",
        "memUsed",
        "memTotal",
        "gpu",
        "netReceive",
        "diskRead",
        "diskWrite",
        "disk",
    ]
    .map(|key| idx.catalog.by_key.get(key).copied().unwrap_or(usize::MAX));
    let mut minutes = Vec::new();
    let mut current = None;
    let mut acc = Accumulator::default();
    for i in day.range.clone() {
        let Some(row) = day.row(i) else { continue };
        let mut start = row.ts().max(from);
        let next = idx
            .frames
            .get(i + 1)
            .map_or(to, |f| idx.day * DAY_MS + i64::from(f.0));
        let end = (row.ts() + row.span()).min(to).min(next);
        if start >= end {
            continue;
        }
        if idx.version == MINUTE_VERSION
            && (start != row.ts() || end != row.ts() + MINUTE_MS)
            && !day
                .minute(i)
                .is_some_and(|m| m.intervals().any(|(a, b)| a < end && b > start))
        {
            continue;
        }
        let obs = Observation::read(&row, &keys);
        while start < end {
            let slot = align(start, 0, MINUTE_MS);
            if current.is_some_and(|s| s > slot) {
                break;
            }
            if current != Some(slot)
                && let Some(prev) = current.replace(slot)
            {
                minutes.push(std::mem::take(&mut acc).finish(prev));
            }
            obs.add(&mut acc, start);
            start = end.min(slot + MINUTE_MS);
        }
    }
    if let Some(slot) = current {
        minutes.push(acc.finish(slot));
    }
    Some(minutes.into())
}

// An appended, replaced or invalidated file gets a new Index allocation (a
// Weak also detaches on Arc::make_mut). Never pin file indexes or process data.
struct CachedDay {
    index: Weak<Index>,
    minutes: Arc<[EventMinute]>,
}
static DAYS: Mutex<VecDeque<CachedDay>> = Mutex::new(VecDeque::new());
struct ScanDay {
    path: PathBuf,
    idx: Arc<Index>,
    from: i64,
    to: i64,
    minutes: Option<Arc<[EventMinute]>>,
}
impl ScanDay {
    fn open(dir: &Path, day: i64, from: i64, to: i64) -> Option<Self> {
        let path = day_path(dir, day);
        let idx = index(&path)?;
        let from = from.max(day * DAY_MS);
        let to = to.min((day + 1) * DAY_MS);
        let minutes = if to - from == DAY_MS {
            let key = Arc::downgrade(&idx);
            DAYS.lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|d| d.index.ptr_eq(&key))
                .map(|d| d.minutes.clone())
        } else {
            None
        };
        Some(Self {
            path,
            idx,
            from,
            to,
            minutes,
        })
    }
    fn load(&mut self) {
        if self.minutes.is_some() {
            return;
        }
        let Some(minutes) = scan_day(&self.path, &self.idx, self.from, self.to) else {
            self.minutes = Some(Arc::default());
            return;
        };
        if self.to - self.from == DAY_MS {
            let key = Arc::downgrade(&self.idx);
            let mut cache = DAYS.lock().unwrap_or_else(|e| e.into_inner());
            cache.retain(|d| d.index.strong_count() != 0 && !d.index.ptr_eq(&key));
            if cache.len() >= 32 {
                cache.pop_front();
            }
            cache.push_back(CachedDay {
                index: key,
                minutes: minutes.clone(),
            });
        }
        self.minutes = Some(minutes);
    }
}
fn visit_minutes(
    dir: &Path,
    days: &[i64],
    from: i64,
    to: i64,
    mut visit: impl FnMut(&EventMinute),
) {
    for group in days.chunks(4) {
        let mut batch: Vec<_> = group
            .iter()
            .filter_map(|&d| ScanDay::open(dir, d, from, to))
            .collect();
        // Bound concurrent main-file buffers to one decoder's existing byte budget.
        // Keep one day on the command thread. Failed thread creation falls back inline.
        let bytes: usize = batch.iter().map(|d| d.idx.valid).sum();
        if bytes <= MAX_FILE_BYTES
            && let Some((last, others)) = batch.split_last_mut()
        {
            std::thread::scope(|scope| {
                for d in others
                    .iter_mut()
                    .filter(|d| d.minutes.is_none() && d.idx.frames.len() >= 720)
                {
                    let _ = std::thread::Builder::new().spawn_scoped(scope, || d.load());
                }
                last.load();
            });
        }
        for mut d in batch {
            d.load();
            for m in d.minutes.as_deref().unwrap_or_default() {
                visit(m);
            }
        }
    }
}

#[derive(Default)]
struct Latest {
    entries: BTreeMap<(i64, Option<ResourceKind>), HistoryEvent>,
    truncated: bool,
}
impl Latest {
    fn insert(&mut self, from: i64, kind: Option<ResourceKind>, event: HistoryEvent) {
        self.entries.insert((from, kind), event);
        if self.entries.len() > LIMIT {
            self.entries.pop_first();
            self.truncated = true;
        }
    }
}

struct Segment {
    from: i64,
    to: i64,
    peak: Peak,
}
impl Segment {
    fn emit(self, kind: ResourceKind, out: &mut Latest) {
        out.insert(
            self.from,
            Some(kind),
            HistoryEvent::Resource {
                kind,
                from: self.from,
                to: self.to,
                peak: self.peak.value,
                peak_at: self.peak.at,
                top: None,
            },
        );
    }
}
#[derive(Default)]
struct Detector {
    run: Option<Segment>,
    qualified: Option<Segment>,
    bridge_peak: Peak,
}
impl Detector {
    fn finish_run(&mut self, k: usize, out: &mut Latest) {
        let Some(mut run) = self.run.take() else {
            return;
        };
        let (kind, _, duration) = RULES.resources[k];
        if run.to - run.from < duration * MINUTE_MS {
            self.bridge_peak.include(run.peak);
            return;
        }
        if let Some(prev) = self.qualified.take() {
            if run.from - prev.to <= RULES.merge_minutes * MINUTE_MS {
                run.from = prev.from;
                run.peak.include(prev.peak);
                run.peak.include(self.bridge_peak);
            } else {
                prev.emit(kind, out);
            }
        }
        self.qualified = Some(run);
        self.bridge_peak = Peak::default();
    }
    fn add(&mut self, k: usize, m: &EventMinute, from: i64, to: i64, out: &mut Latest) {
        if !m.above[k] || self.run.as_ref().is_some_and(|r| r.to != from) {
            self.finish_run(k, out);
        }
        if m.above[k] {
            let run = self.run.get_or_insert(Segment {
                from,
                to,
                peak: m.peaks[k],
            });
            run.to = to;
            run.peak.include(m.peaks[k]);
        } else {
            self.bridge_peak.include(m.peaks[k]);
        }
    }
}

fn top(dir: &Path, days: &[i64], kind: ResourceKind, from: i64, to: i64) -> Option<HistoryTop> {
    let metric = match kind {
        ResourceKind::Cpu | ResourceKind::Temp => "cpu",
        ResourceKind::Gpu => "gpu",
        ResourceKind::Mem => "mem",
        ResourceKind::Net => "net",
        ResourceKind::Disk => "disk",
    };
    let mut query = TableQuery::new(from, to, metric, 0);
    for &day in overlapping(days, from, to) {
        let path = day_path(dir, day);
        if let Some(idx) = index(&path) {
            query.day(&path, day, idx);
            if query.exhausted {
                return None;
            }
        }
    }
    let mut names = BTreeMap::<String, (Arc<str>, f64)>::new();
    for (id, row) in query.rows {
        let (sum, ms) = row.totals.trend(metric);
        if ms > 0 && sum.is_finite() && sum > 0. {
            let group = names
                .entry(id.name.to_ascii_lowercase())
                .or_insert((id.name.clone(), 0.));
            if id.name < group.0 {
                group.0 = id.name;
            }
            group.1 += sum;
        }
    }
    let total = names.values().map(|g| g.1).sum::<f64>();
    let (name, sum) = names
        .into_values()
        .reduce(|a, b| if b.1 > a.1 { b } else { a })?;
    (total.is_finite() && query.recorded > 0).then(|| HistoryTop {
        name: name.to_string(),
        share: sum / total,
        mean: sum / query.recorded as f64,
    })
}

fn overlapping(days: &[i64], from: i64, to: i64) -> &[i64] {
    let first = days.partition_point(|d| *d < from.div_euclid(DAY_MS));
    let end = days.partition_point(|d| *d <= (to - 1).div_euclid(DAY_MS));
    &days[first..end]
}

/// Half-open Unix millisecond interval. None denotes an invalid request.
pub fn events(dir: &Path, from: i64, to: i64) -> Option<HistoryEvents> {
    if from >= to || from.abs_diff(to) > (400 * DAY_MS) as u64 {
        return None;
    }
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let days = days(dir);
    let mut found = Latest::default();
    let mut detectors: [Detector; 6] = Default::default();
    visit_minutes(dir, overlapping(&days, from, to), from, to, |m| {
        for (k, d) in detectors.iter_mut().enumerate() {
            d.add(
                k,
                m,
                m.slot.max(from),
                (m.slot + MINUTE_MS).min(to),
                &mut found,
            );
        }
    });
    for (k, mut d) in detectors.into_iter().enumerate() {
        d.finish_run(k, &mut found);
        if let Some(s) = d.qualified {
            s.emit(RULES.resources[k].0, &mut found);
        }
    }
    visit_gaps(dir, &days, from, to, |mut gap| {
        gap.from = gap.from.max(from);
        gap.to = gap.to.min(to);
        if gap.to.saturating_sub(gap.from) >= RULES.gap_minutes * MINUTE_MS {
            found.insert(gap.from, None, HistoryEvent::Gap { kind: "gap", gap });
        }
        true
    });
    let mut events: Vec<_> = found.entries.into_values().rev().collect();
    for e in &mut events {
        if let HistoryEvent::Resource {
            from,
            to,
            kind,
            top: lead,
            ..
        } = e
        {
            *lead = top(dir, &days, *kind, *from, *to);
        }
    }
    Some(HistoryEvents {
        events,
        truncated: found.truncated,
    })
}
