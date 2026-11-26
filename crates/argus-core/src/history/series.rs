//! Chart series: frames, summaries and live samples in buckets aligned to
//! absolute time, and the gaps between recordings.
use super::*;

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
pub(super) struct Band {
    pub(super) sum: f64,
    pub(super) low: f64,
    pub(super) high: f64,
    pub(super) n: u32,
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
    pub(super) fn add(&mut self, v: f64) {
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

pub(super) fn align(ts: i64, origin: i64, bucket: i64) -> i64 {
    ts.saturating_sub((ts.rem_euclid(bucket) - origin.rem_euclid(bucket)).rem_euclid(bucket))
}

pub(super) fn gaps(dir: &Path, from: i64, to: i64) -> Vec<Gap> {
    let mut out = Vec::new();
    visit_gaps(dir, &days(dir), from, to, |g| {
        out.push(g);
        out.len() < MAX_BUCKETS
    });
    out
}
pub(super) fn visit_gaps(
    dir: &Path,
    all: &[i64],
    from: i64,
    to: i64,
    mut visit: impl FnMut(Gap) -> bool,
) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{snapshot, temp_dir};

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
}
