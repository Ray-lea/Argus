//! How the page moves between states: a range switch's wait for its points
//! and cross-fade (`Crossfade`), the newest bucket drawn on while following
//! (`Reveal`), and an overlay coming in or going (`OverlayFade`).
use std::time::{Duration, Instant};

/// The longest a new range's window waits for its points before it eases
/// without them (days not read since the start, read from disk).
pub(super) const HOLD: Duration = Duration::from_millis(500);
/// How long a lane's axis takes to ease to a new range, as the live charts'
/// tops do (`charts::RESCALE`).
pub(super) const AXIS: Duration = Duration::from_millis(420);
/// How long a bucket that completes takes to draw on while following (`Reveal`).
pub(super) const REVEAL: Duration = Duration::from_millis(450);
/// How long an overlay takes to come in or go (`OverlayFade`).
pub(super) const OVERLAY: Duration = Duration::from_millis(300);

/// A range switch on screen: the window drawn before it, which the curves
/// fading out stay drawn for, the navigator's range then, and how far the
/// curves drawn for the window it goes to have faded in. Both move and scale
/// with the window shown.
#[derive(Clone, Copy, Debug)]
pub(super) struct Crossfade {
    pub(super) from: (f64, f64),
    pub(super) extent_from: (f64, f64),
    pub(super) alpha: f32,
}

/// How far the new curves have faded in at `t` of a range switch: from a
/// tenth of the way, smoothly, whole a little before the zoom ends.
pub(super) fn fade_in(t: f32) -> f32 {
    let t = ((t - 0.1) / 0.75).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Where the curves end while following a range in buckets: easing from
/// where they ended to the newest complete bucket when one more completes.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Reveal {
    pub(super) from: f64,
    pub(super) to: f64,
    pub(super) started: Option<Instant>,
}

impl Reveal {
    pub(super) fn at(&self, now: Instant) -> f64 {
        let Some(started) = self.started else {
            return self.to;
        };
        let t = now.saturating_duration_since(started).as_secs_f64() / REVEAL.as_secs_f64();
        if t >= 1. {
            self.to
        } else {
            self.from + (self.to - self.from) * (1. - (1. - t).powi(3))
        }
    }
}

/// A lane's overlay coming in or going: which reading, the right axis it went
/// with (for one going), whether it comes, and since when.
#[derive(Clone, Debug)]
pub(super) struct OverlayFade {
    pub(super) id: String,
    pub(super) right: Option<(f64, f64)>,
    pub(super) coming: bool,
    pub(super) started: Instant,
}

impl OverlayFade {
    /// How opaque it is at `now`; none once it has come or gone.
    pub(super) fn alpha(&self, now: Instant) -> Option<f32> {
        let t = now.saturating_duration_since(self.started).as_secs_f32() / OVERLAY.as_secs_f32();
        if t >= 1. {
            return None;
        }
        let e = ease_in_out(t);
        Some(if self.coming { e } else { 1. - e })
    }
}

/// Eases in and out (cubic).
pub(super) fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}
