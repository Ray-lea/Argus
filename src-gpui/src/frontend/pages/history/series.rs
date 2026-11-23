//! What the page draws from: readings in buckets aligned to absolute time (so
//! a sliding window moves the curve without cutting it anew), kept and merged
//! as newer answers arrive, and the clock arithmetic around them.
use serde_json::Value;
use std::collections::HashMap;

pub(super) const SECOND: i64 = 1_000;
pub(super) const MINUTE: i64 = 60 * SECOND;
pub(super) const HOUR: i64 = 60 * MINUTE;
pub(super) const DAY: i64 = 24 * HOUR;
/// One recorded frame (`FRAME_MS` in `argus_core::history`).
pub(super) const FRAME: i64 = argus_core::history::FRAME_MS;
/// Bucket widths: one second (five minutes on screen) up to six hours.
pub(super) const NICE: [i64; 16] = [
    SECOND,
    2 * SECOND,
    5 * SECOND,
    10 * SECOND,
    15 * SECOND,
    30 * SECOND,
    MINUTE,
    2 * MINUTE,
    5 * MINUTE,
    10 * MINUTE,
    15 * MINUTE,
    30 * MINUTE,
    HOUR,
    2 * HOUR,
    3 * HOUR,
    6 * HOUR,
];
/// About one point per this many pixels: dense enough to read, sparse enough to keep
/// a spike visible.
const PIXELS_PER_POINT: f64 = 2.6;
/// Buckets this wide or wider carry their lowest and highest readings too.
pub(super) const BANDED: i64 = 10 * SECOND;
/// How far the corners of the steps (`Points::line`) are rounded, in pixels.
pub(super) const STEP_CORNER: f32 = 2.5;
/// The widest spacing of readings recorded while running: the slowest background
/// refresh plus a frame. Points further apart than this break the line.
const MAX_CADENCE: i64 = 35 * SECOND;

/// The bucket for a window of `span` drawn `width` pixels wide.
pub(super) fn bucket_for(span: i64, width: f32) -> i64 {
    let target = span as f64 / (f64::from(width) / PIXELS_PER_POINT).max(60.);
    NICE.into_iter()
        .find(|bucket| *bucket as f64 >= target)
        .unwrap_or(NICE[NICE.len() - 1])
}

/// The narrowest window a range zooms to: a fifth of it, from 30 s to 10 min.
pub(super) fn min_span(range: i64) -> i64 {
    (range / 5).clamp(30 * SECOND, 10 * MINUTE)
}

/// A window (or an axis's range) part of the way `e` from `from` to `to`,
/// zooming at an even pace: its width changes by the same factor in equal
/// steps, about the one place the two windows have in common (the newest
/// reading of a window following the clock, the time under the pointer for
/// the wheel, an axis's zero). Windows as wide only move.
pub(super) fn zoom_between(from: (f64, f64), to: (f64, f64), e: f64) -> (f64, f64) {
    let (was, will) = (from.1 - from.0, to.1 - to.0);
    let linear = (from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e);
    if !(was > 0. && will > 0.) || (will / was - 1.).abs() < 1e-9 {
        return linear;
    }
    // From `from.0`, which keeps the products small next to timestamps.
    let fixed = (to.0 - from.0) * was / (was - will);
    let scale = (will / was).powf(e);
    let start = from.0 + fixed * (1. - scale);
    let zoomed = (start, start + was * scale);
    if zoomed.0.is_finite() && zoomed.1.is_finite() {
        zoomed
    } else {
        linear
    }
}

/// Why nothing was recorded for a while.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GapReason {
    Sleep,
    Shutdown,
    NotRunning,
    Off,
    Unknown,
}

impl GapReason {
    pub(super) fn parse(text: &str) -> Self {
        match text {
            "sleep" => Self::Sleep,
            "shutdown" => Self::Shutdown,
            "notRunning" => Self::NotRunning,
            "off" => Self::Off,
            _ => Self::Unknown,
        }
    }
    /// The locale key naming it.
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Sleep => "history.gap.sleep",
            Self::Shutdown => "history.gap.shutdown",
            Self::NotRunning => "history.gap.notRunning",
            Self::Off => "history.gap.off",
            Self::Unknown => "history.gap.unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Gap {
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) reason: GapReason,
}

/// One reading's values, one per point.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Column {
    pub(super) mean: Vec<Option<f64>>,
    pub(super) low: Vec<Option<f64>>,
    pub(super) high: Vec<Option<f64>>,
}

/// Which of a column's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Part {
    Mean,
    Low,
    High,
}

/// The answer to a points query, and the newer ones merged into it.
#[derive(Clone, Debug, Default)]
pub(super) struct Points {
    /// The bucket width the points were asked for (wider ones from merged records
    /// keep their own `span`).
    pub(super) bucket: i64,
    /// Bucket starts are `origin + k·bucket`.
    pub(super) origin: i64,
    /// What was asked for: the readings and the window.
    pub(super) keys: Vec<String>,
    pub(super) from: i64,
    pub(super) to: i64,
    /// Point start times, ascending.
    pub(super) ts: Vec<i64>,
    /// How long each point covers.
    pub(super) span: Vec<i64>,
    /// Whether each point is one sample (`HistorySeries::sample`): its span
    /// is the time since the sample before, however long.
    pub(super) sample: Vec<bool>,
    /// A recorded frame inside each point, whose processes can be shown (0: none yet).
    pub(super) frame: Vec<i64>,
    /// The frame with the highest CPU sample, per point.
    pub(super) peak: Vec<Option<i64>>,
    pub(super) columns: HashMap<String, Column>,
    /// The recorded gaps, by when they begin.
    pub(super) gaps: Vec<Gap>,
    /// Points before this time are final; later ones may still change.
    pub(super) settled: i64,
    /// The latest end among the gaps up to each one (`gaps_across`), made
    /// when first asked for and again after the gaps change.
    reach: std::cell::OnceCell<Vec<i64>>,
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|v| v.is_finite())
}

fn numbers(value: &Value) -> Vec<Option<f64>> {
    value
        .as_array()
        .map(|list| list.iter().map(number).collect())
        .unwrap_or_default()
}

fn times(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .map(|list| list.iter().map(|v| v.as_i64().unwrap_or(0)).collect())
        .unwrap_or_default()
}

impl Points {
    /// Reads an answer to `history_points` asked with `args`.
    pub(super) fn parse(answer: &Value, args: &Value) -> Self {
        let ts = times(&answer["ts"]);
        let n = ts.len();
        let fit = |mut list: Vec<Option<f64>>| {
            list.resize(n, None);
            list
        };
        let mut span = times(&answer["span"]);
        let bucket = answer["bucket"]
            .as_i64()
            .or_else(|| args["bucket"].as_i64())
            .unwrap_or(FRAME)
            .max(1);
        span.resize(n, bucket.max(SECOND));
        let mut sample: Vec<bool> = answer["sample"]
            .as_array()
            .map(|list| list.iter().map(|v| v.as_bool() == Some(true)).collect())
            .unwrap_or_default();
        sample.resize(n, false);
        let mut frame = times(&answer["frame"]);
        frame.resize(n, 0);
        let mut peak = answer["peak"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|v| v.as_i64().filter(|at| *at > 0))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        peak.resize(n, None);
        let columns = answer["series"]
            .as_object()
            .map(|series| {
                series
                    .iter()
                    .map(|(key, column)| {
                        (
                            key.clone(),
                            Column {
                                mean: fit(numbers(&column["mean"])),
                                low: fit(numbers(&column["low"])),
                                high: fit(numbers(&column["high"])),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let gaps = answer["gaps"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|gap| {
                        Some(Gap {
                            from: gap["from"].as_i64()?,
                            to: gap["to"].as_i64()?,
                            reason: GapReason::parse(gap["reason"].as_str().unwrap_or("")),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut gaps = gaps;
        gaps.sort_by_key(|gap| gap.from);
        let keys = args["keys"]
            .as_array()
            .map(|keys| {
                keys.iter()
                    .filter_map(|k| k.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let from = args["from"].as_i64().unwrap_or(0);
        let to = args["to"].as_i64().unwrap_or(from);
        Self {
            bucket,
            origin: args["origin"].as_i64().unwrap_or(0),
            keys,
            from,
            to,
            settled: answer["settled"]
                .as_i64()
                .unwrap_or_else(|| ts.last().copied().unwrap_or(from)),
            ts,
            span,
            sample,
            frame,
            peak,
            columns,
            gaps,
            reach: Default::default(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.ts.is_empty()
    }

    /// Whether these points answer the same question as a query for `bucket`,
    /// `origin` and `keys` (so a newer answer can be merged in).
    pub(super) fn matches(&self, bucket: i64, origin: i64, keys: &[String]) -> bool {
        self.bucket == bucket && self.origin == origin && self.keys == keys
    }

    /// Takes in an answer for a later part of the same query: its points replace
    /// those from where it starts on, and its gaps those after its start.
    pub(super) fn merge(&mut self, newer: Points) {
        let start = newer
            .ts
            .first()
            .copied()
            .unwrap_or(newer.from)
            .min(newer.from);
        let keep = self.ts.partition_point(|at| *at < start);
        self.ts.truncate(keep);
        self.span.truncate(keep);
        self.sample.truncate(keep);
        self.frame.truncate(keep);
        self.peak.truncate(keep);
        for column in self.columns.values_mut() {
            column.mean.truncate(keep);
            column.low.truncate(keep);
            column.high.truncate(keep);
        }
        let added = newer.ts.len();
        for (key, column) in &newer.columns {
            let entry = self.columns.entry(key.clone()).or_insert_with(|| Column {
                mean: vec![None; keep],
                low: vec![None; keep],
                high: vec![None; keep],
            });
            entry.mean.extend_from_slice(&column.mean);
            entry.low.extend_from_slice(&column.low);
            entry.high.extend_from_slice(&column.high);
        }
        for (key, column) in self.columns.iter_mut() {
            if !newer.columns.contains_key(key) {
                column.mean.resize(keep + added, None);
                column.low.resize(keep + added, None);
                column.high.resize(keep + added, None);
            }
        }
        self.ts.extend_from_slice(&newer.ts);
        self.span.extend_from_slice(&newer.span);
        self.sample.extend_from_slice(&newer.sample);
        self.frame.extend_from_slice(&newer.frame);
        self.peak.extend_from_slice(&newer.peak);
        self.gaps.retain(|gap| gap.to <= start);
        self.gaps.extend(newer.gaps);
        self.gaps.sort_by_key(|gap| gap.from);
        self.reach = Default::default();
        self.to = self.to.max(newer.to);
        self.settled = newer.settled.max(self.settled.min(start));
    }

    /// Two answers drawn as one while the window eases from the one to the
    /// other: the finer points (the newer of two as fine) where they cover,
    /// the coarser around them, so that a short range's per-second readings
    /// stay at the end of a long range's buckets as the window widens, and
    /// come in at its end as it narrows. The steps join where they meet
    /// (`steps`).
    pub(super) fn spliced(&self, other: &Points) -> Points {
        let (fine, coarse) = if self.bucket <= other.bucket {
            (self, other)
        } else {
            (other, self)
        };
        let (Some(&first), Some(&last), Some(&last_span)) =
            (fine.ts.first(), fine.ts.last(), fine.span.last())
        else {
            return coarse.clone();
        };
        let stop = last.saturating_add(last_span);
        let before = coarse.ts.partition_point(|at| *at < first);
        let after = coarse.ts.partition_point(|at| *at < stop).max(before);
        let picks: Vec<(&Points, usize)> = (0..before)
            .map(|i| (coarse, i))
            .chain((0..fine.ts.len()).map(|i| (fine, i)))
            .chain((after..coarse.ts.len()).map(|i| (coarse, i)))
            .collect();
        let value = |points: &Points, key: &str, part: Part, i: usize| points.value(key, part, i);
        let mut columns = HashMap::new();
        for key in coarse.columns.keys().chain(fine.columns.keys()) {
            if columns.contains_key(key) {
                continue;
            }
            let part = |part: Part| {
                picks
                    .iter()
                    .map(|(points, i)| value(points, key, part, *i))
                    .collect::<Vec<_>>()
            };
            columns.insert(
                key.clone(),
                Column {
                    mean: part(Part::Mean),
                    low: part(Part::Low),
                    high: part(Part::High),
                },
            );
        }
        let mut gaps: Vec<Gap> = coarse
            .gaps
            .iter()
            .filter(|gap| gap.to <= first || gap.from >= stop)
            .chain(&fine.gaps)
            .cloned()
            .collect();
        gaps.sort_by_key(|gap| gap.from);
        Points {
            bucket: coarse.bucket,
            origin: coarse.origin,
            keys: coarse.keys.clone(),
            from: coarse.from.min(fine.from),
            to: coarse.to.max(fine.to),
            ts: picks.iter().map(|(p, i)| p.ts[*i]).collect(),
            span: picks.iter().map(|(p, i)| p.span[*i]).collect(),
            sample: picks.iter().map(|(p, i)| p.single(*i)).collect(),
            frame: picks
                .iter()
                .map(|(p, i)| p.frame.get(*i).copied().unwrap_or(0))
                .collect(),
            peak: picks
                .iter()
                .map(|(p, i)| p.peak.get(*i).copied().flatten())
                .collect(),
            columns,
            gaps,
            settled: if fine.to >= coarse.to {
                fine.settled
            } else {
                coarse.settled
            },
            reach: Default::default(),
        }
    }

    /// The points as far as `cutoff`, as the curves draw them while following
    /// a range in buckets: the bucket still filling left out (its value moved
    /// with every reading), and one coming in cut short there, its values
    /// eased in from those before it as far as it has come.
    pub(super) fn until(&self, cutoff: i64) -> Points {
        let keep = self.ts.partition_point(|at| *at < cutoff);
        let whole = keep == self.ts.len()
            && self
                .ts
                .last()
                .zip(self.span.last())
                .is_none_or(|(at, span)| at + span <= cutoff);
        if whole {
            return self.clone();
        }
        let mut out = self.clone();
        out.ts.truncate(keep);
        out.span.truncate(keep);
        out.sample.truncate(keep);
        out.frame.truncate(keep);
        out.peak.truncate(keep);
        for column in out.columns.values_mut() {
            column.mean.truncate(keep);
            column.low.truncate(keep);
            column.high.truncate(keep);
        }
        out.gaps.retain(|gap| gap.from < cutoff);
        out.to = out.to.min(cutoff);
        out.settled = out.settled.min(cutoff);
        out.reach = Default::default();
        if let Some(last) = keep.checked_sub(1)
            && cutoff - out.ts[last] < out.span[last]
        {
            let come = (cutoff - out.ts[last]) as f64 / out.span[last] as f64;
            out.span[last] = (cutoff - out.ts[last]).max(1);
            if let Some(before) = last.checked_sub(1)
                && out.joins(before)
            {
                let eased = 1. - (1. - come.clamp(0., 1.)).powi(3);
                for column in out.columns.values_mut() {
                    for list in [&mut column.mean, &mut column.low, &mut column.high] {
                        if let (Some(was), Some(is)) = (list[before], list[last]) {
                            list[last] = Some(was + (is - was) * eased);
                        }
                    }
                }
            }
        }
        out
    }

    /// Forgets points that ended before `before`, so a window following the
    /// clock for hours keeps a bounded list.
    pub(super) fn trim(&mut self, before: i64) {
        let drop = self
            .ts
            .iter()
            .zip(&self.span)
            .take_while(|(at, span)| *at + *span < before)
            .count();
        if drop == 0 {
            return;
        }
        self.ts.drain(..drop);
        self.span.drain(..drop);
        self.sample.drain(..drop.min(self.sample.len()));
        self.frame.drain(..drop);
        self.peak.drain(..drop);
        for column in self.columns.values_mut() {
            column.mean.drain(..drop.min(column.mean.len()));
            column.low.drain(..drop.min(column.low.len()));
            column.high.drain(..drop.min(column.high.len()));
        }
        self.gaps.retain(|gap| gap.to >= before);
        self.reach = Default::default();
        self.from = self.from.max(before);
    }

    /// The newest time a reading was taken in: the end of the last point that
    /// is settled, else the start of the one still filling.
    pub(super) fn newest(&self) -> Option<i64> {
        let last = self.ts.len().checked_sub(1)?;
        let end = self.ts[last] + self.span[last];
        Some(if end <= self.settled.max(self.ts[last]) {
            end
        } else {
            self.settled.max(self.ts[last])
        })
    }

    /// Whether point `i` aggregates several records (placed at its middle)
    /// rather than being one sample or frame (placed at its start). A
    /// sample a background cadence of 5 s or more after the one before spans
    /// a frame or more, and is still one sample.
    pub(super) fn aggregated(&self, i: usize) -> bool {
        self.span[i] > FRAME && !self.single(i)
    }

    /// Whether point `i` is one sample of the last minutes, spanning the
    /// time since the sample before.
    pub(super) fn single(&self, i: usize) -> bool {
        self.sample.get(i).copied().unwrap_or(false)
    }

    /// The moment point `i` stands for, to tell whether it is in a window (the
    /// axes, moving to a point): a sample or frame at its start, a bucket at
    /// its middle, the bucket still filling no later than `end`. Its step is
    /// drawn across its whole span (`steps`).
    pub(super) fn x_time(&self, i: usize, end: i64) -> i64 {
        if !self.aggregated(i) {
            return self.ts[i];
        }
        let middle = self.ts[i] + self.span[i] / 2;
        if self.ts[i] + self.span[i] > self.settled {
            middle.min(end.max(self.ts[i]))
        } else {
            middle
        }
    }

    pub(super) fn value(&self, key: &str, part: Part, i: usize) -> Option<f64> {
        let column = self.columns.get(key)?;
        let list = match part {
            Part::Mean => &column.mean,
            Part::Low => &column.low,
            Part::High => &column.high,
        };
        list.get(i).copied().flatten()
    }

    pub(super) fn has(&self, key: &str) -> bool {
        self.columns
            .get(key)
            .is_some_and(|column| column.mean.iter().any(Option::is_some))
    }

    /// Whether the line joins point `i` to the next: not across a recorded gap,
    /// nor across a hole wider than the slowest cadence and three points.
    pub(super) fn joins(&self, i: usize) -> bool {
        let Some(next) = self.ts.get(i + 1) else {
            return false;
        };
        let end = self.ts[i] + self.span[i];
        if self.gaps_across(end - 1, *next).next().is_some() {
            return false;
        }
        let widest = self.span[i].max(self.span[i + 1]);
        next - end <= (3 * widest).max(MAX_CADENCE)
    }

    /// The recorded gaps the lanes show hatched: those the line breaks for,
    /// and those before its first point or after its last. A gap inside what
    /// a point covers (a short one in a wide bucket, with records either side
    /// of it in the bucket) leaves the line whole and is not shown: hatched,
    /// it lay over a line that ran on through it.
    pub(super) fn shown_gaps(&self) -> Vec<&Gap> {
        let (Some(&first), Some(&last), Some(&last_span)) =
            (self.ts.first(), self.ts.last(), self.span.last())
        else {
            return self.gaps.iter().collect();
        };
        let mut broken = vec![false; self.gaps.len()];
        for i in 0..self.ts.len() - 1 {
            if !self.joins(i) {
                for at in self.gaps_after_at(i) {
                    broken[at] = true;
                }
            }
        }
        self.gaps
            .iter()
            .enumerate()
            .filter(|(at, gap)| gap.to <= first || gap.from >= last + last_span || broken[*at])
            .map(|(_, gap)| gap)
            .collect()
    }

    /// Where in `gaps` the gaps are that begin before `before` and end after
    /// `after`: the gaps are sorted by start, and the latest end among those
    /// up to each one rises with them, so both bounds are found by halving.
    fn gaps_across_at(&self, after: i64, before: i64) -> impl Iterator<Item = usize> + '_ {
        let reach = self.reach.get_or_init(|| {
            let mut latest = i64::MIN;
            self.gaps
                .iter()
                .map(|gap| {
                    latest = latest.max(gap.to);
                    latest
                })
                .collect()
        });
        let upto = self.gaps.partition_point(|gap| gap.from < before);
        let first = reach[..upto].partition_point(|end| *end <= after);
        (first..upto).filter(move |at| self.gaps[*at].to > after)
    }

    fn gaps_across(&self, after: i64, before: i64) -> impl Iterator<Item = &Gap> {
        self.gaps_across_at(after, before).map(|at| &self.gaps[at])
    }

    /// The recorded gaps between point `i` and the next: those the line
    /// breaks across (`joins`), by where they are in `gaps`.
    fn gaps_after_at(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let before = self.ts.get(i + 1).copied().unwrap_or(i64::MIN);
        let end = self.ts[i] + self.span[i];
        self.gaps_across_at(end - 1, before)
    }

    fn gaps_after(&self, i: usize) -> impl Iterator<Item = &Gap> {
        self.gaps_after_at(i).map(|at| &self.gaps[at])
    }

    /// Where the step of point `i` (beginning at `at`) stops when the line
    /// breaks after it: where the recorded gap after it begins, so that the
    /// line meets the gap's hatching, else at the end of what the point
    /// covers. The per-second readings of the last minutes cover less than
    /// the frame a gap is counted from: the last one holds on.
    fn run_end(&self, i: usize, at: f64) -> f64 {
        let Some(&next) = self.ts.get(i + 1) else {
            return at;
        };
        let stop = match self.gaps_after(i).map(|gap| gap.from).min() {
            Some(from) => from.min(next),
            None => self.ts[i].saturating_add(self.span[i]).min(next),
        };
        (stop as f64).max(at)
    }

    /// Where the step of point `i` (at `at`) begins when the line resumes
    /// after a recorded gap: where the gap ends, so that it meets the
    /// hatching there too, the point's value held back to it.
    fn run_start(&self, i: usize, at: f64) -> f64 {
        let Some(previous) = i.checked_sub(1) else {
            return at;
        };
        self.gaps_after(previous)
            .map(|gap| gap.to)
            .max()
            .map_or(at, |to| (to as f64).min(at))
    }

    /// The values of `key` as a time plot draws them, in steps: each point's
    /// value flat across what it covers (a sample until the next one, a bucket
    /// across its span), rising or falling straight to the next value where
    /// that begins. An unknown value, and a break where the line does not join
    /// the next point, leave it out. `scale` maps a value onto the plot's axis
    /// (a reading on the right axis).
    pub(super) fn line(
        &self,
        key: &str,
        part: Part,
        end: i64,
        scale: impl Fn(f64) -> f64,
    ) -> Vec<(f64, Option<f64>)> {
        self.steps(end, |i| self.value(key, part, i).map(&scale))
    }

    /// A band's edges in steps as the line's, broken where it breaks: the
    /// lowest of `low` up to the highest of `high` (the highest samples of
    /// `cpuPeak` above the lowest of `cpu`, say).
    pub(super) fn band(&self, low: &str, high: &str, end: i64) -> Vec<(f64, Option<(f64, f64)>)> {
        self.steps(end, |i| {
            let below = self
                .value(low, Part::Low, i)
                .or_else(|| self.value(low, Part::Mean, i));
            let above = self
                .value(high, Part::High, i)
                .or_else(|| self.value(high, Part::Mean, i));
            below.zip(above)
        })
    }

    /// Each point's `value` held from where the point begins to where the
    /// next one does: across a sample's or a bucket's own span, a point that
    /// breaks the line held to the gap after it (`run_end`) and one resuming
    /// after a gap held back to where it ends (`run_start`), the newest up to
    /// the newest reading (`end`).
    fn steps<T: Copy>(
        &self,
        end: i64,
        value: impl Fn(usize) -> Option<T>,
    ) -> Vec<(f64, Option<T>)> {
        let count = self.ts.len();
        let mut out = Vec::with_capacity(2 * count + 8);
        for i in 0..count {
            let start = self.ts[i] as f64;
            let begin = if i > 0 && !self.joins(i - 1) {
                self.run_start(i, start)
            } else {
                start
            };
            let stop = if i + 1 == count {
                self.ts[i]
                    .saturating_add(self.span[i])
                    .min(end.max(self.ts[i])) as f64
            } else if self.joins(i) {
                self.ts[i + 1] as f64
            } else {
                self.run_end(i, start)
            };
            let held = value(i);
            out.push((begin, held));
            if stop > begin {
                out.push((stop, held));
            }
            if i + 1 < count && !self.joins(i) {
                let next = self.run_start(i + 1, self.ts[i + 1] as f64);
                out.push(((stop + next) / 2., None));
            }
        }
        out
    }

    /// The point whose step the line draws at `at` (`steps`): each point from
    /// where it begins to where the next one does, one before a break up to
    /// the gap after it and one after a break from where that gap ends, the
    /// newest across its own span. None where the line breaks.
    pub(super) fn index_at(&self, at: i64) -> Option<usize> {
        let count = self.ts.len();
        let after = self.ts.partition_point(|t| *t <= at);
        // Held back to the end of the gap before it.
        if after > 0
            && after < count
            && !self.joins(after - 1)
            && self.run_start(after, self.ts[after] as f64) <= at as f64
        {
            return Some(after);
        }
        let i = after.checked_sub(1)?;
        let stop = if i + 1 == count {
            self.ts[i].saturating_add(self.span[i]) as f64
        } else if self.joins(i) {
            self.ts[i + 1] as f64
        } else {
            self.run_end(i, self.ts[i] as f64)
        };
        ((at as f64) < stop).then_some(i)
    }

    /// The gap that holds `at`, if any.
    pub(super) fn gap_at(&self, at: i64) -> Option<Gap> {
        self.gaps
            .iter()
            .copied()
            .find(|gap| gap.from <= at && at < gap.to)
    }

    /// Highest value of `key` among points drawn in `from..=to` (a bucket either side).
    pub(super) fn highest(
        &self,
        key: &str,
        part: Part,
        from: i64,
        to: i64,
        end: i64,
    ) -> Option<f64> {
        self.fold(key, part, from, to, end, f64::max)
    }

    pub(super) fn lowest(
        &self,
        key: &str,
        part: Part,
        from: i64,
        to: i64,
        end: i64,
    ) -> Option<f64> {
        self.fold(key, part, from, to, end, f64::min)
    }

    fn fold(
        &self,
        key: &str,
        part: Part,
        from: i64,
        to: i64,
        end: i64,
        f: impl Fn(f64, f64) -> f64,
    ) -> Option<f64> {
        let margin = self.bucket;
        (0..self.ts.len())
            .filter(|i| {
                let at = self.x_time(*i, end);
                at >= from - margin && at <= to + margin
            })
            .filter_map(|i| self.value(key, part, i))
            .reduce(f)
    }
}

/// The top of an axis: a round value just above `value`. Percentages stop at
/// 10, 20, 25, 40, 50, 75 or 100.
pub(super) fn nice_top(value: f64, percent: bool) -> f64 {
    let value = if value > 0. && value.is_finite() {
        value
    } else if percent {
        10.
    } else {
        1.
    };
    if percent {
        return [10., 20., 25., 40., 50., 75., 100.]
            .into_iter()
            .find(|step| value * 1.06 <= *step)
            .unwrap_or(100.);
    }
    let exponent = 10f64.powf(value.log10().floor());
    [1., 1.5, 2., 2.5, 3., 4., 5., 6., 8., 10.]
        .into_iter()
        .map(|k| k * exponent)
        .find(|step| value * 1.06 <= *step)
        .unwrap_or(10. * exponent)
}

/// How many rows divide an axis of `range` into round steps: 25 in five, 40 in
/// four, 75 in three.
pub(super) fn rows_for(range: f64) -> u32 {
    let range = (range * 1000.).round() / 1000.;
    for rows in [4u32, 5, 3] {
        let step = range / f64::from(rows);
        if (step - step.round()).abs() < 1e-6
            || (step < 1. && (step * 10. - (step * 10.).round()).abs() < 1e-6)
        {
            return rows;
        }
    }
    4
}

/// Clock tick spacing for a window of `span` drawn `width` pixels wide: labels at
/// least this far apart.
const TICK_GAP: f32 = 86.;
const TICK_STEPS: [i64; 18] = [
    5 * SECOND,
    10 * SECOND,
    15 * SECOND,
    30 * SECOND,
    MINUTE,
    2 * MINUTE,
    5 * MINUTE,
    10 * MINUTE,
    15 * MINUTE,
    30 * MINUTE,
    HOUR,
    2 * HOUR,
    3 * HOUR,
    6 * HOUR,
    12 * HOUR,
    DAY,
    2 * DAY,
    7 * DAY,
];

pub(super) fn tick_step(span: i64, width: f32) -> i64 {
    let room = (width / TICK_GAP).max(1.) as f64;
    TICK_STEPS
        .into_iter()
        .find(|step| span as f64 / *step as f64 <= room)
        .unwrap_or(TICK_STEPS[TICK_STEPS.len() - 1])
}

/// Ticks on local clock boundaries inside the window.
pub(super) fn ticks(from: i64, to: i64, step: i64) -> Vec<i64> {
    crate::regional::clock_ticks(from, to, step)
}

#[cfg(test)]
mod tests {
    use super::{
        DAY, FRAME, GapReason, HOUR, MINUTE, Part, Points, SECOND, bucket_for, min_span, nice_top,
        rows_for, tick_step, zoom_between,
    };
    use serde_json::json;

    fn args(from: i64, to: i64, bucket: i64) -> serde_json::Value {
        json!({"from": from, "to": to, "bucket": bucket, "origin": 0, "keys": ["cpu"]})
    }

    #[test]
    fn buckets_keep_about_one_point_per_few_pixels() {
        assert_eq!(bucket_for(5 * MINUTE, 1000.), SECOND);
        assert_eq!(bucket_for(HOUR, 1000.), 10 * SECOND);
        assert_eq!(bucket_for(DAY, 1000.), 5 * MINUTE);
        assert_eq!(bucket_for(30 * DAY, 1000.), 2 * HOUR);
        assert_eq!(bucket_for(365 * DAY, 1000.), 6 * HOUR);
        assert_eq!(min_span(5 * MINUTE), MINUTE);
        assert_eq!(min_span(7 * DAY), 10 * MINUTE);
    }

    #[test]
    fn axes_stop_on_round_values_and_rows_divide_them_evenly() {
        assert_eq!(nice_top(8.9, true), 10.);
        assert_eq!(nice_top(18., true), 20.);
        assert_eq!(nice_top(19., true), 25., "six percent of headroom");
        assert_eq!(nice_top(70., true), 75.);
        assert_eq!(nice_top(99., true), 100.);
        assert_eq!(nice_top(1.3e6, false), 1.5e6);
        assert_eq!(nice_top(0., false), 1.5, "an empty axis still has a top");
        assert_eq!(rows_for(100.), 4);
        assert_eq!(rows_for(25.), 5);
        assert_eq!(rows_for(75.), 5);
        assert_eq!(rows_for(1.5), 5);
        assert_eq!(rows_for(0.9), 3);
        assert_eq!(tick_step(5 * MINUTE, 1000.), 30 * SECOND);
    }

    #[test]
    fn a_newer_answer_replaces_the_points_from_its_start_on() {
        let first = json!({"bucket": 1000, "ts": [0, 1000, 2000], "span": [1000, 1000, 1000],
            "frame": [0, 0, 0], "series": {"cpu": {"mean": [1, 2, 3]}}, "settled": 2000});
        let mut points = Points::parse(&first, &args(0, 3000, 1000));
        let newer = json!({"bucket": 1000, "ts": [2000, 3000], "span": [1000, 1000],
            "frame": [0, 0], "series": {"cpu": {"mean": [4, 5]}}, "settled": 3000});
        points.merge(Points::parse(&newer, &args(2000, 4000, 1000)));
        assert_eq!(points.ts, vec![0, 1000, 2000, 3000]);
        assert_eq!(
            points.columns["cpu"].mean,
            vec![Some(1.), Some(2.), Some(4.), Some(5.)]
        );
        assert_eq!(points.settled, 3000);
        points.trim(1500);
        assert_eq!(points.ts, vec![1000, 2000, 3000]);
        assert_eq!(points.columns["cpu"].mean.len(), 3);
    }

    #[test]
    fn lines_break_at_recorded_gaps_and_wide_holes_only() {
        let answer = json!({"bucket": FRAME, "ts": [0, FRAME, 2 * FRAME, 10 * MINUTE, 10 * MINUTE + FRAME, 30 * MINUTE],
            "span": [FRAME, FRAME, FRAME, FRAME, FRAME, FRAME],
            "series": {"cpu": {"mean": [1, 2, 3, 4, 5, 6]}},
            "gaps": [{"from": 3 * FRAME, "to": 10 * MINUTE, "reason": "sleep"}], "settled": 40 * MINUTE});
        let points = Points::parse(&answer, &args(0, 40 * MINUTE, FRAME));
        assert!(points.joins(0) && points.joins(1));
        assert!(!points.joins(2), "a recorded gap breaks the line");
        assert!(points.joins(3));
        assert!(
            !points.joins(4),
            "twenty minutes without a reading break it too"
        );
        let line = points.line("cpu", Part::Mean, 40 * MINUTE, |v| v);
        assert_eq!(line.iter().filter(|(_, v)| v.is_none()).count(), 2);
        assert_eq!(
            points.gap_at(5 * MINUTE).map(|g| g.reason),
            Some(GapReason::Sleep)
        );
        assert_eq!(points.index_at(FRAME + 10), Some(1));
        assert_eq!(points.index_at(5 * MINUTE), None);
    }

    #[test]
    fn a_broken_line_holds_its_last_value_up_to_the_gap() {
        // Frames at 0 and 5 s; Argus stopped 2.5 s into the second one.
        let answer = json!({"bucket": SECOND, "ts": [0, FRAME, 10 * MINUTE], "span": [FRAME, FRAME, FRAME],
            "series": {"cpu": {"mean": [1, 2, 3]}},
            "gaps": [{"from": FRAME + 2_500, "to": 10 * MINUTE, "reason": "notRunning"}],
            "settled": 20 * MINUTE});
        let points = Points::parse(&answer, &args(0, 20 * MINUTE, SECOND));
        let line = points.line("cpu", Part::Mean, 20 * MINUTE, |v| v);
        let held = (FRAME + 2_500) as f64;
        assert!(
            line.contains(&(held, Some(2.))),
            "the value holds to where the gap begins"
        );
        // The newest point holds across its own frame, no further: its successor
        // is not in yet.
        assert_eq!(line.last(), Some(&((10 * MINUTE + FRAME) as f64, Some(3.))));
        // Without a recorded gap, the value holds to the end of its frame.
        let answer = json!({"bucket": SECOND, "ts": [0, 30 * MINUTE], "span": [FRAME, FRAME],
            "series": {"cpu": {"mean": [1, 2]}}, "settled": 40 * MINUTE});
        let points = Points::parse(&answer, &args(0, 40 * MINUTE, SECOND));
        let line = points.line("cpu", Part::Mean, 40 * MINUTE, |v| v);
        assert_eq!(line[1], (FRAME as f64, Some(1.)));
        assert_eq!(line[2].1, None);
    }

    #[test]
    fn values_hold_flat_across_their_points_in_steps() {
        // Seconds: each value flat until the next begins, rising or falling
        // straight there; the newest up to the newest reading.
        let answer = json!({"bucket": SECOND, "ts": [0, SECOND, 2 * SECOND],
            "span": [SECOND, SECOND, SECOND],
            "series": {"cpu": {"mean": [1, 3, 2]}}, "settled": 2_500});
        let points = Points::parse(&answer, &args(0, MINUTE, SECOND));
        let line = points.line("cpu", Part::Mean, 2_500, |v| v);
        let second = SECOND as f64;
        assert_eq!(
            line,
            vec![
                (0., Some(1.)),
                (second, Some(1.)),
                (second, Some(3.)),
                (2. * second, Some(3.)),
                (2. * second, Some(2.)),
                (2_500., Some(2.)),
            ]
        );
    }

    #[test]
    fn the_point_at_a_moment_is_the_step_drawn_there() {
        // Samples at 0 and 1.3 s (one late), then Argus stopped from 2.3 s to
        // 9.5 s and the next came at 10 s.
        let answer = json!({"bucket": SECOND, "ts": [0, 1_300, 10_000],
            "span": [SECOND, SECOND, SECOND], "series": {"cpu": {"mean": [1, 2, 3]}},
            "gaps": [{"from": 2_300, "to": 9_500, "reason": "notRunning"}], "settled": 11_000});
        let points = Points::parse(&answer, &args(0, MINUTE, SECOND));
        // Held until the next sample begins, not handed to the nearer one.
        assert_eq!(points.index_at(1_200), Some(0));
        assert_eq!(points.index_at(1_300), Some(1));
        // Up to the gap, none in it, the next held back to where it ends.
        assert_eq!(points.index_at(2_299), Some(1));
        assert_eq!(points.index_at(5_000), None);
        assert_eq!(points.index_at(9_600), Some(2));
        // The newest across its own span, no further.
        assert_eq!(points.index_at(10_999), Some(2));
        assert_eq!(points.index_at(11_000), None);
    }

    #[test]
    fn samples_a_background_cadence_apart_are_no_averages() {
        // A recorded frame, then samples taken minimized at 5 s, waking a
        // little early or late: each spans the time since the one before.
        let answer = json!({"bucket": SECOND, "ts": [0, FRAME, 2 * FRAME + 3, 3 * FRAME + 1],
            "span": [FRAME, FRAME, FRAME + 3, FRAME - 2], "sample": [false, true, true, true],
            "series": {"cpu": {"mean": [1, 2, 3, 4]}}, "settled": 4 * FRAME});
        let points = Points::parse(&answer, &args(0, MINUTE, SECOND));
        for i in 0..4 {
            assert!(!points.aggregated(i));
            assert_eq!(points.x_time(i, MINUTE), points.ts[i]);
        }
        assert!(!points.single(0) && points.single(2));
        // A newer answer and a coarser one keep each point's kind in place.
        let mut merged = points.clone();
        merged.merge(Points::parse(
            &json!({"bucket": SECOND, "ts": [3 * FRAME + 1, 4 * FRAME], "span": [FRAME - 2, FRAME + 5],
                "sample": [true, true], "series": {"cpu": {"mean": [4, 5]}}, "settled": 5 * FRAME}),
            &args(3 * FRAME, MINUTE, SECOND),
        ));
        assert_eq!(merged.sample, vec![false, true, true, true, true]);
        let coarse = Points::parse(
            &json!({"bucket": MINUTE, "ts": [-MINUTE], "span": [MINUTE],
                "series": {"cpu": {"mean": [1]}}, "settled": 0}),
            &args(-MINUTE, MINUTE, MINUTE),
        );
        let spliced = points.spliced(&coarse);
        assert_eq!(spliced.sample, vec![false, false, true, true, true]);
        assert!(spliced.aggregated(0) && !spliced.aggregated(3));
    }

    #[test]
    fn only_gaps_the_line_breaks_for_are_shown() {
        // 10-s buckets: a 3-s gap inside the second leaves the line whole; a
        // gap of minutes after the third breaks it; one after the last stays.
        let answer = json!({"bucket": 10 * SECOND,
            "ts": [0, 10 * SECOND, 20 * SECOND, 10 * MINUTE],
            "span": [10 * SECOND, 10 * SECOND, 10 * SECOND, 10 * SECOND],
            "series": {"cpu": {"mean": [1, 2, 3, 4]}},
            "gaps": [{"from": 12 * SECOND, "to": 15 * SECOND, "reason": "sleep"},
                {"from": 30 * SECOND, "to": 10 * MINUTE, "reason": "sleep"},
                {"from": 10 * MINUTE + 10 * SECOND, "to": 11 * MINUTE, "reason": "sleep"}],
            "settled": 20 * MINUTE});
        let points = Points::parse(&answer, &args(0, 20 * MINUTE, 10 * SECOND));
        assert_eq!(points.gaps.len(), 3);
        let shown = points
            .shown_gaps()
            .iter()
            .map(|gap| gap.from)
            .collect::<Vec<_>>();
        assert_eq!(shown, vec![30 * SECOND, 10 * MINUTE + 10 * SECOND]);
    }

    #[test]
    fn a_line_meets_the_gap_on_both_sides() {
        // Per-second readings stop 3 s in; the gap is counted from the end of
        // the 5-s frame, and the next reading comes 1.5 s after the gap ends.
        let answer = json!({"bucket": SECOND, "ts": [0, SECOND, 2 * SECOND, MINUTE + 1_500],
            "span": [SECOND, SECOND, SECOND, SECOND],
            "series": {"cpu": {"mean": [1, 2, 3, 4]}},
            "gaps": [{"from": FRAME, "to": MINUTE, "reason": "sleep"}],
            "settled": 2 * MINUTE});
        let points = Points::parse(&answer, &args(0, 2 * MINUTE, SECOND));
        let line = points.line("cpu", Part::Mean, 2 * MINUTE, |v| v);
        assert!(
            line.contains(&(FRAME as f64, Some(3.))),
            "the last value holds to where the gap begins"
        );
        assert!(
            line.contains(&(MINUTE as f64, Some(4.))),
            "the next run begins where the gap ends"
        );
        let gap = line.iter().position(|p| p.1.is_none()).expect("a break");
        assert!((FRAME as f64) < line[gap].0 && line[gap].0 < MINUTE as f64);
    }

    #[test]
    fn buckets_sit_at_their_middle_and_the_filling_one_stays_inside_the_window() {
        let answer = json!({"bucket": 10_000, "ts": [0, 10_000], "span": [10_000, 10_000],
            "series": {"cpu": {"mean": [1, 2], "low": [0, 1], "high": [3, 4]}}, "settled": 10_000});
        let points = Points::parse(&answer, &args(0, 20_000, 10_000));
        assert_eq!(points.x_time(0, 12_000), 5_000);
        assert_eq!(points.x_time(1, 12_000), 12_000);
        assert_eq!(points.x_time(1, 19_000), 15_000);
        assert_eq!(
            points.highest("cpu", Part::High, 0, 20_000, 12_000),
            Some(4.)
        );
        assert_eq!(points.lowest("cpu", Part::Low, 0, 20_000, 12_000), Some(0.));
    }

    #[test]
    fn a_zoom_keeps_its_common_place_and_an_even_pace() {
        let close =
            |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
        // Widening a window that ends at the newest reading: the end stays, the
        // width doubles in each half.
        let now = 1.8e12;
        let (from, to) = ((now - 100., now), (now - 400., now));
        assert!(close(zoom_between(from, to, 0.), from));
        assert!(close(zoom_between(from, to, 1.), to));
        assert!(close(zoom_between(from, to, 0.5), (now - 200., now)));
        // About the pointer, a third of the way across.
        let middle = zoom_between((0., 90.), (20., 50.), 0.5);
        let third = |w: (f64, f64)| w.0 + (w.1 - w.0) / 3.;
        assert!((third(middle) - 30.).abs() < 1e-9);
        assert!(((middle.1 - middle.0) - (90_f64 * 30.).sqrt()).abs() < 1e-9);
        // As wide: it only moves.
        assert!(close(zoom_between((0., 10.), (20., 30.), 0.25), (5., 15.)));
        // An axis growing from zero keeps zero at the bottom.
        assert!(close(
            zoom_between((0., 100.), (0., 1000.), 0.5),
            (0., 1000_f64.sqrt() * 10.)
        ));
    }

    #[test]
    fn spliced_answers_keep_the_finer_points_where_they_cover() {
        let coarse = Points::parse(
            &json!({"bucket": MINUTE, "ts": [0, MINUTE, 2 * MINUTE, 3 * MINUTE, 4 * MINUTE],
                "span": [MINUTE, MINUTE, MINUTE, MINUTE, MINUTE],
                "series": {"cpu": {"mean": [1, 2, 3, 4, 5]}, "gpu": {"mean": [9, 9, 9, 9, 9]}},
                "settled": 5 * MINUTE}),
            &args(0, 5 * MINUTE, MINUTE),
        );
        let fine = Points::parse(
            &json!({"bucket": SECOND, "ts": [MINUTE + 30 * SECOND, MINUTE + 31 * SECOND],
                "span": [SECOND, SECOND], "series": {"cpu": {"mean": [7, 8]}},
                "settled": MINUTE + 32 * SECOND}),
            &args(MINUTE + 30 * SECOND, MINUTE + 32 * SECOND, SECOND),
        );
        for points in [coarse.spliced(&fine), fine.spliced(&coarse)] {
            assert_eq!(
                points.ts,
                vec![
                    0,
                    MINUTE,
                    MINUTE + 30 * SECOND,
                    MINUTE + 31 * SECOND,
                    2 * MINUTE,
                    3 * MINUTE,
                    4 * MINUTE
                ]
            );
            let cpu: Vec<_> = (0..points.ts.len())
                .map(|i| points.value("cpu", Part::Mean, i))
                .collect();
            assert_eq!(
                cpu,
                vec![
                    Some(1.),
                    Some(2.),
                    Some(7.),
                    Some(8.),
                    Some(3.),
                    Some(4.),
                    Some(5.)
                ]
            );
            // A reading the finer answer lacks is unknown where it covers.
            assert_eq!(points.value("gpu", Part::Mean, 2), None);
            assert_eq!(points.value("gpu", Part::Mean, 4), Some(9.));
            assert_eq!(points.bucket, MINUTE);
            assert_eq!(points.settled, 5 * MINUTE);
        }
    }

    #[test]
    fn gaps_found_by_halving_are_those_a_scan_finds() {
        // Points every 10 s with holes, and gaps of every kind: inside a
        // point, across several, overlapping one another, before and after.
        let mut seed = 7_u64;
        let mut next = |below: i64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            ((seed >> 33) % below as u64) as i64
        };
        let mut ts = Vec::new();
        let mut at = 0;
        for _ in 0..300 {
            ts.push(at);
            at += 10 * SECOND * (1 + i64::from(next(10) == 0) * next(8));
        }
        let gaps: Vec<_> = (0..80)
            .map(|_| {
                let from = next(at + 60 * SECOND) - 30 * SECOND;
                json!({"from": from, "to": from + 1 + next(90 * SECOND), "reason": "sleep"})
            })
            .collect();
        let answer = json!({"bucket": 10 * SECOND, "ts": ts, "span": vec![10 * SECOND; ts.len()],
            "series": {"cpu": {"mean": vec![1; ts.len()]}}, "gaps": gaps, "settled": at});
        let points = Points::parse(&answer, &args(0, at, 10 * SECOND));
        let scan = |i: usize| -> Vec<(i64, i64)> {
            let Some(&next) = points.ts.get(i + 1) else {
                return Vec::new();
            };
            let end = points.ts[i] + points.span[i];
            points
                .gaps
                .iter()
                .filter(|gap| gap.from < next && gap.to > end - 1)
                .map(|gap| (gap.from, gap.to))
                .collect()
        };
        let mut broken = 0;
        for i in 0..points.ts.len() {
            let found: Vec<_> = points.gaps_after(i).map(|gap| (gap.from, gap.to)).collect();
            assert_eq!(found, scan(i), "the gaps after point {i}");
            broken += usize::from(!points.joins(i));
        }
        assert!(
            broken > 10,
            "the line breaks often enough to test ({broken})"
        );
        let shown: Vec<_> = points
            .shown_gaps()
            .iter()
            .map(|gap| (gap.from, gap.to))
            .collect();
        let first = points.ts[0];
        let stop = points.ts[points.ts.len() - 1] + points.span[points.ts.len() - 1];
        let expected: Vec<_> = points
            .gaps
            .iter()
            .filter(|gap| {
                gap.to <= first
                    || gap.from >= stop
                    || (0..points.ts.len() - 1)
                        .any(|i| !points.joins(i) && scan(i).contains(&(gap.from, gap.to)))
            })
            .map(|gap| (gap.from, gap.to))
            .collect();
        assert_eq!(shown, expected);
    }

    #[test]
    fn following_buckets_end_at_the_newest_complete_one_and_draw_the_next_on() {
        let answer = json!({"bucket": MINUTE, "ts": [0, MINUTE, 2 * MINUTE],
            "span": [MINUTE, MINUTE, MINUTE],
            "series": {"cpu": {"mean": [10, 50, 90], "high": [20, 60, 99]}},
            "settled": 2 * MINUTE});
        let points = Points::parse(&answer, &args(0, 3 * MINUTE, MINUTE));
        // The bucket still filling is left out.
        let complete = points.until(points.settled);
        assert_eq!(complete.ts, vec![0, MINUTE]);
        assert_eq!(complete.value("cpu", Part::Mean, 1), Some(50.));
        // Halfway on: cut short there, its value most of the way from the one before.
        let coming = points.until(MINUTE + 30 * SECOND);
        assert_eq!(coming.span, vec![MINUTE, 30 * SECOND]);
        let mean = coming.value("cpu", Part::Mean, 1).expect("a value");
        assert!(
            (mean - (10. + 40. * 0.875)).abs() < 1e-9,
            "eased out: {mean}"
        );
        // All of it: as it was.
        assert_eq!(points.until(3 * MINUTE).ts, points.ts);
    }
}
