//! One observed run's readings over a window, in the shape the history page
//! draws the system's (`history::HistorySeries`), so that the observation
//! page draws them with the same lanes, navigator and tooltips.
use super::{SAMPLE_MS, SeriesQuery, WatchPoint, WatchState};
use crate::history::{Column, Gap, HistorySeries};
use std::collections::VecDeque;

/// Full-resolution snapshots of one (PID, creation time) in this session.
#[derive(Default)]
pub(super) struct RecentRun {
    samples: VecDeque<(WatchPoint, i64)>,
    evicted: bool,
}

impl RecentRun {
    pub(super) fn latest(&self) -> Option<&WatchPoint> {
        self.samples.back().map(|(p, _)| p)
    }

    pub(super) fn push(&mut self, point: WatchPoint, span: i64) {
        if let Some((last, _)) = self.samples.back_mut()
            && last.ts == point.ts
            && last.state == point.state
        {
            *last = point;
            return;
        }
        if self.samples.len() >= crate::history::MAX_RECENT {
            self.samples.pop_front();
            self.evicted = true;
        }
        self.samples.push_back((point, span));
    }

    pub(super) fn prune(&mut self, now: i64) -> bool {
        let cutoff = now.saturating_sub(crate::history::RECENT_MS);
        while self.samples.front().is_some_and(|(p, _)| p.ts < cutoff) {
            self.samples.pop_front();
            self.evicted = true;
        }
        !self.samples.is_empty()
    }
}

/// The widest bucket asked for (six hours), and the most points one answer
/// holds; a window that would need more answers empty.
const MAX_BUCKET: i64 = 6 * 60 * 60 * 1000;
const MAX_POINTS: i64 = 20_000;
const MAX_KEYS: usize = 32;
/// Buckets this wide or wider carry their lowest and highest readings too,
/// as the history's do.
const BANDED: i64 = 10_000;
/// The slowest background cadence plus one disk recording interval.
pub(super) const MAX_SPACING_MS: i64 = 30_000 + SAMPLE_MS;

/// The start of the bucket `ts` falls in.
fn align(ts: i64, origin: i64, bucket: i64) -> i64 {
    ts - (ts - origin).rem_euclid(bucket)
}

fn add_gap(out: &mut HistorySeries, query: &SeriesQuery, from: i64, to: i64) {
    if from < to && to >= query.from && from <= query.to {
        out.gaps.push(Gap {
            from,
            to,
            reason: "missing",
        });
    }
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

struct Bucket {
    ts: i64,
    span: i64,
    sample: bool,
    bands: Vec<Band>,
}

impl Bucket {
    fn emit(self, out: &mut HistorySeries, keys: &[String], banded: bool) {
        out.ts.push(self.ts);
        out.span.push(self.span);
        out.frame.push(0);
        out.peak.push(0);
        if self.sample || !out.sample.is_empty() {
            out.sample.resize(out.ts.len() - 1, false);
            out.sample.push(self.sample);
        }
        for (key, band) in keys.iter().zip(self.bands) {
            let column = out.series.entry(key.clone()).or_default();
            column
                .mean
                .push((band.n > 0).then(|| band.sum / f64::from(band.n)));
            if banded {
                column.low.push((band.n > 0).then_some(band.low));
                column.high.push((band.n > 0).then_some(band.high));
            }
        }
    }
}

/// The run's `points`, oldest first, over the query's window: each record
/// as one sample at the source resolution (one-second queries for recent
/// snapshots), else averages over aligned buckets with extremes (`BANDED` and
/// wider). Where samples went missing between two records the answer holds
/// a gap. An answer the window or keys would make too large is empty.
pub(super) fn series<'a, I: Iterator<Item = &'a WatchPoint>>(
    saved: impl FnOnce(i64) -> I,
    recent: Option<&'a RecentRun>,
    query: &SeriesQuery,
) -> HistorySeries {
    let bucket = query.bucket;
    let mut out = HistorySeries {
        bucket,
        settled: query.from,
        series: query
            .keys
            .iter()
            .map(|key| (key.clone(), Column::default()))
            .collect(),
        ..Default::default()
    };
    if !(1..=MAX_BUCKET).contains(&bucket)
        || query.to < query.from
        || query.to.saturating_sub(query.from) / bucket > MAX_POINTS
        || query.keys.len() > MAX_KEYS
    {
        return out;
    }
    let banded = bucket >= BANDED;
    let first = align(query.from, query.origin, bucket);
    let mut acc: Option<Bucket> = None;
    // The last record with readings, and whether samples went missing since.
    let mut previous: Option<(i64, i64)> = None;
    let mut missing = None;
    let mut missing_end = 0;
    let mut newest = None;
    // Keep a partially evicted aggregate bucket on the disk side. Every
    // timestamp then contributes exactly once, also for incremental queries.
    let cut = recent
        .and_then(|run| {
            let ts = run.samples.front()?.0.ts;
            Some(if run.evicted && bucket > 1_000 {
                align(ts.saturating_add(bucket - 1), query.origin, bucket)
            } else {
                ts
            })
        })
        .unwrap_or(i64::MAX);
    let disk = (first < cut)
        .then(|| saved(first))
        .into_iter()
        .flatten()
        .take_while(|p| p.ts < cut)
        .map(|p| (p, None));
    let live = recent
        .into_iter()
        .flat_map(|r| &r.samples)
        .filter(|(p, _)| p.ts >= cut)
        .map(|(p, span)| (p, Some(*span)));
    let mut points = disk.chain(live).peekable();
    while let Some((point, live_span)) = points.next() {
        if !point.has_any_metric() {
            if point.state == WatchState::Gap {
                let start =
                    previous.map_or(point.ts, |(ts, span)| ts.saturating_add(span).min(point.ts));
                missing.get_or_insert(start);
                missing_end = point
                    .ts
                    .saturating_add(live_span.unwrap_or(SAMPLE_MS).max(1));
            } else if missing.is_some() {
                missing_end = point.ts;
            }
            continue;
        }
        let delta = previous.map(|(ts, _)| point.ts.saturating_sub(ts));
        // Slower background sampling is valid. Recent samples carry explicit
        // interruptions; old files can also have a hole without a marker.
        let gap = missing.take().or_else(|| {
            previous
                .filter(|_| delta.is_some_and(|d| d > MAX_SPACING_MS))
                .map(|(ts, span)| ts.saturating_add(span))
        });
        if let Some(from) = gap {
            add_gap(&mut out, query, from, point.ts);
        }
        if point.ts > query.to {
            break;
        }
        let span = live_span
            .or(delta.filter(|d| *d > 0 && gap.is_none()))
            .or_else(|| {
                points
                    .peek()
                    .filter(|(p, span)| span.is_none() && p.has_any_metric())
                    .map(|(p, _)| p.ts - point.ts)
                    .filter(|d| (1..=MAX_SPACING_MS).contains(d))
            })
            .unwrap_or(SAMPLE_MS);
        previous = Some((point.ts, span));
        let raw = bucket
            <= if live_span.is_some() {
                1_000
            } else {
                SAMPLE_MS
            };
        if point.ts < if raw { query.from } else { first } {
            continue;
        }
        let ts = if raw {
            point.ts
        } else {
            align(point.ts, query.origin, bucket)
        };
        newest = Some(ts);
        if acc.as_ref().is_some_and(|b| b.ts != ts)
            && let Some(done) = acc.take()
        {
            done.emit(&mut out, &query.keys, banded);
        }
        let current = acc.get_or_insert_with(|| Bucket {
            ts,
            span: if raw { span } else { bucket },
            sample: raw,
            bands: vec![Band::default(); query.keys.len()],
        });
        for (key, band) in query.keys.iter().zip(&mut current.bands) {
            if let Some(value) = point.metric(key) {
                band.sum += value;
                band.low = band.low.min(value);
                band.high = band.high.max(value);
                band.n += 1;
            }
        }
    }
    if let Some(done) = acc {
        done.emit(&mut out, &query.keys, banded);
    }
    if let Some(from) = missing {
        add_gap(&mut out, query, from, missing_end.min(query.to));
    }
    out.settled = newest.unwrap_or(query.from);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(ts: i64, memory: u64) -> WatchPoint {
        WatchPoint {
            memory: Some(memory),
            cpu: Some(1.5),
            ..WatchPoint::marker(ts, WatchState::Recording)
        }
    }

    fn query(from: i64, to: i64, bucket: i64) -> SeriesQuery {
        SeriesQuery {
            pid: 1,
            create_time: 1,
            from,
            to,
            bucket,
            origin: 0,
            keys: vec!["memory".into(), "cpu".into(), "handles".into()],
        }
    }

    #[test]
    fn records_are_samples_and_wider_buckets_average_with_their_extremes() {
        let points = [
            point(0, 10),
            point(5_000, 20),
            point(10_000, 30),
            point(15_000, 50),
        ];
        let raw = series(|_| points.iter(), None, &query(0, 20_000, 1_000));
        assert_eq!(raw.ts, vec![0, 5_000, 10_000, 15_000]);
        assert_eq!(raw.span, vec![SAMPLE_MS; 4]);
        assert_eq!(raw.sample, vec![true; 4]);
        assert_eq!(raw.series["memory"].mean[1], Some(20.));
        // Recorded readings only: a field no record has stays unknown.
        assert_eq!(raw.series["handles"].mean, vec![None; 4]);
        assert_eq!(raw.settled, 15_000);
        let wide = series(|_| points.iter(), None, &query(0, 20_000, 10_000));
        assert!(wide.sample.is_empty());
        assert_eq!(wide.ts, vec![0, 10_000]);
        assert_eq!(wide.series["memory"].mean, vec![Some(15.), Some(40.)]);
        assert_eq!(wide.series["memory"].low, vec![Some(10.), Some(30.)]);
        assert_eq!(wide.series["memory"].high, vec![Some(20.), Some(50.)]);
    }

    #[test]
    fn recent_samples_replace_disk_points_and_preserve_real_intervals_at_the_cut() {
        let saved = [
            point(0, 10),
            point(5_500, 20),
            point(11_500, 30),
            point(17_250, 40),
        ];
        let mut recent = RecentRun::default();
        for (ts, span) in [
            (5_500, 1_000),
            (6_000, 500),
            (11_500, 5_500),
            (12_520, 1_020),
            (17_250, 4_730),
        ] {
            recent.push(point(ts, 70), span);
        }
        recent.prune(6_000 + crate::history::RECENT_MS);
        let read = |q: &SeriesQuery| series(|_| saved.iter(), Some(&recent), q);
        let raw = read(&query(0, 18_000, 1_000));
        assert_eq!(raw.ts, [0, 5_500, 6_000, 11_500, 12_520, 17_250]);
        assert_eq!(raw.span, [5_500, 5_500, 500, 5_500, 1_020, 4_730]);
        assert_eq!(
            raw.series["memory"].mean,
            [
                Some(10.),
                Some(20.),
                Some(70.),
                Some(70.),
                Some(70.),
                Some(70.)
            ]
        );
        assert!(raw.gaps.is_empty());
        let tail = read(&query(12_500, 18_000, 1_000));
        assert_eq!(tail.span, [1_020, 4_730]);
        // An evicted partial bucket comes from disk; the next bucket is wholly live.
        let combined = read(&query(0, 18_000, 5_000));
        assert_eq!(combined.ts, [0, 5_500, 10_000, 15_000]);
        assert_eq!(combined.sample, [true, true, false, false]);
        assert_eq!(
            combined.series["memory"].mean,
            [Some(10.), Some(20.), Some(70.), Some(70.)]
        );
        assert!(!recent.prune(17_251 + crate::history::RECENT_MS));
        let old = query(11_500, 18_000, 1_000).recorded(&saved);
        assert_eq!(old.span, [6_000, 5_750]);
    }

    #[test]
    fn trailing_missing_snapshots_end_the_reading_at_the_first_missing_tick() {
        let mut recent = RecentRun::default();
        recent.push(point(30_000, 15), 30_000);
        recent.push(WatchPoint::marker(31_000, WatchState::Gap), 1_000);
        recent.push(WatchPoint::marker(32_000, WatchState::Gap), 1_000);
        let q = query(30_000, 33_000, 1_000);
        let result = series(|_| std::iter::empty(), Some(&recent), &q);
        assert_eq!(result.span, [30_000]);
        assert_eq!(result.gaps.len(), 1);
        assert_eq!((result.gaps[0].from, result.gaps[0].to), (31_000, 33_000));
    }

    #[test]
    fn missing_samples_leave_a_gap_and_markers_draw_nothing() {
        let points = [
            point(0, 10),
            WatchPoint::marker(5_000, WatchState::Gap),
            point(20_000, 30),
            point(60_000, 40),
            WatchPoint::marker(65_000, WatchState::Exited),
        ];
        let answer = series(|_| points.iter(), None, &query(0, 70_000, 1_000));
        assert_eq!(answer.ts, vec![0, 20_000, 60_000]);
        let gaps: Vec<_> = answer
            .gaps
            .iter()
            .map(|g| (g.from, g.to, g.reason))
            .collect();
        assert_eq!(
            gaps,
            vec![(5_000, 20_000, "missing"), (25_000, 60_000, "missing")]
        );
        // Outside the window: no points, no gaps, and it settles at its start.
        let before = series(|_| points.iter(), None, &query(100_000, 110_000, 1_000));
        assert!(before.ts.is_empty() && before.gaps.is_empty());
        assert_eq!(before.settled, 100_000);
    }

    #[test]
    fn oversized_questions_get_empty_answers() {
        let points = [point(0, 10)];
        assert!(
            series(|_| points.iter(), None, &query(0, 10_000, 0))
                .ts
                .is_empty()
        );
        assert!(
            series(|_| points.iter(), None, &query(10, 0, 1_000))
                .ts
                .is_empty()
        );
        assert!(
            series(
                |_| points.iter(),
                None,
                &query(0, MAX_POINTS * 1_000 + 2_000, 1_000)
            )
            .ts
            .is_empty()
        );
    }
}
