//! A lane's axes: their ranges and rows, easing to new ranges as the data
//! changes (`AxisEase`), and the rows with their labels (`Mark`), sliding
//! and cross-fading while they do.
use super::series::{BANDED, Points, rows_for, zoom_between};
use super::state::Lane;
use super::*;

/// A lane's axis, the overlay's, and the rows dividing them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Axes {
    pub(super) left: (f64, f64),
    pub(super) right: Option<(f64, f64)>,
    /// The overlay shares the left axis (same unit).
    pub(super) shared: bool,
    pub(super) rows: u32,
}

/// A lane's axes easing to new ranges (`super::transition::AXIS`), geometrically about
/// the value the two ranges have at the same height (zero, mostly), as the
/// live charts' tops do: the readings they are for, the ranges shown when
/// the latest change began, the axes they go to, the rows fading out (those
/// of the axes shown before, and how opaque they were then), and when.
#[derive(Clone, Debug)]
pub(super) struct AxisEase {
    readings: (String, Option<String>),
    from: ((f64, f64), Option<(f64, f64)>),
    to: Axes,
    leaving: Option<(Axes, f32)>,
    started: Option<Instant>,
}

/// A lane's axes at a moment of their ease: the ranges shown, and the rows
/// either side (`Mark`) of the axes they go to and of those they leave.
pub(super) struct Eased {
    pub(super) axes: Axes,
    pub(super) left: Vec<Mark>,
    pub(super) right: Vec<Mark>,
    pub(super) moving: bool,
}

/// A row of an axis: its value, its height on the plot (0 at the top, 1 at
/// the bottom), how opaque, and the top of the axis it is a row of (which
/// unit its label is in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Mark {
    pub(super) value: f64,
    pub(super) at: f32,
    pub(super) alpha: f32,
    pub(super) top: f64,
}

impl AxisEase {
    pub(super) fn new(axes: Axes, readings: (String, Option<String>)) -> Self {
        Self {
            readings,
            from: (axes.left, axes.right),
            to: axes,
            leaving: None,
            started: None,
        }
    }

    /// The axes it goes to.
    pub(super) fn target(&self) -> Axes {
        self.to
    }

    /// How far the latest change has come, eased out (1 when done).
    fn progress(&self, now: Instant) -> f32 {
        let Some(started) = self.started else {
            return 1.;
        };
        let t = now.saturating_duration_since(started).as_secs_f32()
            / super::transition::AXIS.as_secs_f32();
        if t >= 1. {
            1.
        } else {
            1. - (1. - t.max(0.)).powi(3)
        }
    }

    /// The axes of a lane showing `readings` at `now`, easing towards
    /// `target`: a new target eases on from the ranges shown, the rows shown
    /// more fading out. Another reading starts at its own axes; an overlay
    /// coming or going eases them.
    pub(super) fn toward(
        &mut self,
        target: Axes,
        readings: (String, Option<String>),
        now: Instant,
        motion: bool,
    ) -> Eased {
        if !motion || readings.0 != self.readings.0 {
            *self = Self::new(target, readings);
        } else if self.to != target {
            let e = self.progress(now);
            let shown = self.eased(now).axes;
            let leaving = match self.leaving.filter(|_| e < 1.) {
                Some((axes, alpha)) if alpha * (1. - e) > e => (axes, alpha * (1. - e)),
                _ => (self.to, e),
            };
            *self = Self {
                readings,
                from: (shown.left, shown.right),
                to: target,
                leaving: Some(leaving),
                started: Some(now),
            };
        }
        self.eased(now)
    }

    fn eased(&self, now: Instant) -> Eased {
        let e = self.progress(now);
        let left = zoom_between(self.from.0, self.to.left, f64::from(e));
        let right = match (self.from.1, self.to.right) {
            (Some(from), Some(to)) => Some(zoom_between(from, to, f64::from(e))),
            (_, to) => to,
        };
        let axes = Axes {
            left,
            right,
            ..self.to
        };
        let leaving = self
            .leaving
            .filter(|_| e < 1.)
            .map(|(axes, alpha)| (axes, alpha * (1. - e)));
        let rows = self.to.rows;
        Eased {
            axes,
            left: marks(
                left,
                (self.to.left, rows),
                leaving.map(|(axes, alpha)| ((axes.left, axes.rows), alpha)),
                e,
            ),
            right: match (right, self.to.right) {
                (Some(shown), Some(to)) => marks(
                    shown,
                    (to, rows),
                    leaving.and_then(|(axes, alpha)| axes.right.map(|r| ((r, axes.rows), alpha))),
                    e,
                ),
                _ => Vec::new(),
            },
            moving: e < 1.,
        }
    }
}

/// An axis's range, and how many rows divide it.
type Rows = ((f64, f64), u32);

/// The rows an axis draws over the range `shown`: those of the range it goes
/// to, as opaque as the change has come (`e`), and those of the range it
/// leaves, fading out; a value both have stays whole. Rows off the plot are
/// left out.
pub(super) fn marks(
    shown: (f64, f64),
    to: Rows,
    leaving: Option<(Rows, f32)>,
    e: f32,
) -> Vec<Mark> {
    let height = shown.1 - shown.0;
    if height.is_nan() || height <= 0. {
        return Vec::new();
    }
    let rows = |((low, high), rows): Rows| {
        let rows = rows.max(1);
        (0..=rows).map(move |row| {
            let value = high - (high - low) * f64::from(row) / f64::from(rows);
            (value, high)
        })
    };
    let near = |a: f64, b: f64| (a - b).abs() <= height.abs() * 1e-6;
    let going: Vec<(f64, f64)> = leaving
        .map(|(axis, _)| rows(axis).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for (value, top) in rows(to) {
        let kept = leaving.is_none() || going.iter().any(|(v, _)| near(*v, value));
        out.push(Mark {
            value,
            at: (1. - (value - shown.0) / height) as f32,
            alpha: if kept { 1. } else { e },
            top,
        });
    }
    if let Some((_, alpha)) = leaving {
        for (value, top) in going {
            if !rows(to).any(|(v, _)| near(v, value)) {
                out.push(Mark {
                    value,
                    at: (1. - (value - shown.0) / height) as f32,
                    alpha,
                    top,
                });
            }
        }
    }
    out.retain(|mark| mark.at >= -0.002 && mark.at <= 1.002 && mark.alpha > 0.004);
    out
}

/// The rows of two axes at once, each where its own range puts it: those of
/// the axes a range switch leaves, fading out, and those of the axes it goes
/// to, as opaque as `alpha`. A row both have in one place stays whole.
pub(super) fn crossfade_marks(leaving: Rows, arriving: Rows, alpha: f32) -> Vec<Mark> {
    let rows = |((low, high), rows): Rows, alpha: f32| {
        let rows = rows.max(1);
        (0..=rows)
            .map(|row| {
                let at = row as f32 / rows as f32;
                Mark {
                    value: high - (high - low) * f64::from(at),
                    at,
                    alpha,
                    top: high,
                }
            })
            .collect::<Vec<_>>()
    };
    let old = rows(leaving, 1. - alpha);
    let mut out = rows(arriving, alpha);
    let near = |a: &Mark, b: &Mark| {
        (a.at - b.at).abs() < 1e-4 && (a.value - b.value).abs() <= a.value.abs().max(1.) * 1e-9
    };
    for mark in &mut out {
        if old.iter().any(|kept| near(kept, mark)) {
            mark.alpha = 1.;
        }
    }
    let gone: Vec<Mark> = old
        .into_iter()
        .filter(|mark| !out.iter().any(|kept| near(kept, mark)))
        .collect();
    out.extend(gone);
    out.retain(|mark| mark.alpha > 0.004);
    out
}

impl Desktop {
    /// The axes of a lane over the window `from..to`.
    pub(super) fn history_axes(
        &self,
        lane: &Lane,
        points: Option<&Points>,
        (from, to): (i64, i64),
        bucket: i64,
        height: f32,
    ) -> Axes {
        let end = self.history_clock();
        let banded = bucket >= BANDED && self.history.band;
        let reading = self.history_reading(&lane.reading);
        let empty = Points::default();
        let points = points.unwrap_or(&empty);
        let mut left = reading.map_or((0., 100.), |r| r.axis_range(points, from, to, end, banded));
        let overlay = lane
            .overlay
            .as_deref()
            .and_then(|id| self.history_reading(id));
        let mut right = overlay.map(|r| r.axis_range(points, from, to, end, false));
        let shared = matches!((reading, overlay), (Some(a), Some(b)) if a.unit == b.unit);
        if shared && let Some(r) = right {
            left = (left.0.min(r.0), left.1.max(r.1));
            right = Some(left);
        }
        let range = left.1 - left.0;
        let unit = reading.map_or(1., |r| r.label_unit(left.1.abs().max(left.0.abs())));
        let scaled = range / unit;
        let half = scaled / 2.;
        let round_half = (half - half.round()).abs() < 1e-6
            || (half < 1. && (half * 10. - (half * 10.).round()).abs() < 1e-6);
        let rows = if height < 50. {
            1
        } else if height < 100. {
            if round_half { 2 } else { 1 }
        } else {
            rows_for(scaled)
        };
        Axes {
            left,
            right,
            shared,
            rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Axes, AxisEase, crossfade_marks, marks};
    use std::time::{Duration, Instant};

    fn axes(top: f64, rows: u32) -> Axes {
        Axes {
            left: (0., top),
            right: None,
            shared: false,
            rows,
        }
    }

    #[test]
    fn settled_rows_divide_the_axis_evenly_and_whole() {
        let rows = marks((0., 100.), ((0., 100.), 4), None, 1.);
        let at: Vec<f32> = rows.iter().map(|mark| mark.at).collect();
        assert_eq!(at, vec![0., 0.25, 0.5, 0.75, 1.]);
        assert!(rows.iter().all(|mark| mark.alpha == 1.));
        assert_eq!(rows[0].value, 100.);
    }

    #[test]
    fn rows_cross_fade_and_a_value_both_have_stays_whole() {
        // Halfway from 0–100 to 0–200 on a range shown at 0–150.
        let rows = marks(
            (0., 150.),
            ((0., 200.), 2),
            Some((((0., 100.), 2), 0.5)),
            0.5,
        );
        let zero = rows.iter().find(|mark| mark.value == 0.).expect("zero");
        assert_eq!(zero.alpha, 1.);
        // 100 is a row of both: whole; 50 only of the axis left, fading.
        let both = rows
            .iter()
            .find(|mark| mark.value == 100.)
            .map(|mark| mark.alpha);
        assert_eq!(both, Some(1.));
        let going = rows
            .iter()
            .find(|mark| mark.value == 50.)
            .map(|mark| mark.alpha);
        assert_eq!(going, Some(0.5));
        // 200 lies above the range shown: left out.
        assert!(!rows.iter().any(|mark| mark.value == 200.));
        let both = crossfade_marks(((0., 100.), 4), ((0., 100.), 4), 0.3);
        assert!(both.iter().all(|mark| mark.alpha == 1.));
        assert_eq!(both.len(), 5);
    }

    #[test]
    fn a_new_top_eases_and_another_reading_starts_afresh() {
        let start = Instant::now();
        let mut ease = AxisEase::new(axes(100., 4), ("cpu".into(), None));
        let now = ease.toward(axes(200., 4), ("cpu".into(), None), start, true);
        assert!(now.moving);
        assert_eq!(now.axes.left, (0., 100.));
        let later = ease.toward(
            axes(200., 4),
            ("cpu".into(), None),
            start + Duration::from_secs(1),
            true,
        );
        assert!(!later.moving);
        assert_eq!(later.axes.left, (0., 200.));
        let other = ease.toward(axes(10., 2), ("battery".into(), None), start, true);
        assert!(!other.moving);
        assert_eq!(other.axes.left, (0., 10.));
    }
}
