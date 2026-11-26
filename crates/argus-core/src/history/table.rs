//! The process table of a range: time-weighted totals per instance and
//! their trends, from the summaries where whole ones fit.
use super::*;

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

pub(super) struct TableQuery<'a> {
    from: i64,
    to: i64,
    key: &'a str,
    points: usize,
    pub(super) recorded: i64,
    pub(super) rows: HashMap<Identity, Instance>,
    cells: usize,
    name_bytes: usize,
    pub(super) exhausted: bool,
}
#[derive(Default)]
pub(super) struct Instance {
    pub(super) totals: Totals,
    trend: Vec<(usize, f64, i64)>,
}
impl TableQuery<'_> {
    pub(super) fn new(from: i64, to: i64, key: &str, points: usize) -> TableQuery<'_> {
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
    pub(super) fn bin(&self, ts: i64) -> usize {
        (((i128::from(ts) - i128::from(self.from)) * self.points as i128
            / (i128::from(self.to) - i128::from(self.from))) as usize)
            .min(self.points.saturating_sub(1))
    }
    pub(super) fn bin_end(&self, bin: usize, finish: i64) -> i64 {
        if self.points == 0 {
            return finish;
        }
        let delta = i128::from(self.to) - i128::from(self.from);
        let bound = i128::from(self.from)
            + (delta * (bin as i128 + 1) + self.points as i128 - 1) / self.points as i128;
        finish.min(bound as i64)
    }
    pub(super) fn add(&mut self, id: &Identity, t: Totals, bin: usize) {
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
    pub(super) fn day(&mut self, path: &Path, day: i64, idx: Arc<Index>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{proc, snapshot, temp_dir};

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
}
