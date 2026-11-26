use crate::{Pixels, Point, Window, point, px};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
    time::Instant,
};

/// Chromium's pixels per wheel line on Windows (`kScrollbarPixelsPerLine`):
/// 100 px per notch at the default three lines.
pub const PIXELS_PER_LINE: f32 = 100. / 3.;

struct Glide {
    from: Point<Pixels>,
    to: Point<Pixels>,
    start: Instant,
    /// Seconds; `None` until the first frame, which clamps the target to
    /// the scrollable range and then times the segment.
    duration: Option<f32>,
    /// The timing function's initial slope, carrying a retargeted glide's velocity.
    slope: f32,
    /// The velocity (px/s along the larger axis) when the segment began.
    velocity: f32,
    /// The offset the glide last wrote; anything else means someone else scrolled.
    written: Point<Pixels>,
}

thread_local! {
    static GLIDES: RefCell<HashMap<usize, Glide>> = RefCell::new(HashMap::new());
}

/// The segment duration in seconds for a distance (cc's kInverseDelta).
fn segment(distance: f32) -> f32 {
    const RAMP_START: f32 = 120.;
    const RAMP_END: f32 = 480.;
    const MIN_FRAMES: f32 = 6.;
    const MAX_FRAMES: f32 = 12.;
    let slope = (MIN_FRAMES - MAX_FRAMES) / (RAMP_END - RAMP_START);
    let frames = (MAX_FRAMES + (distance.abs() - RAMP_START) * slope).clamp(MIN_FRAMES, MAX_FRAMES);
    frames / 60.
}

/// The larger component of a delta, keeping its sign (cc's MaximumDimension).
fn major(delta: Point<Pixels>) -> f32 {
    let (x, y) = (f32::from(delta.x), f32::from(delta.y));
    if x.abs() > y.abs() { x } else { y }
}

/// cubic-bezier(0.42, 0.42 * slope, 0.58, 1): progress at `x`, and its slope.
fn ease(slope: f32, x: f32) -> (f32, f32) {
    let (x1, y1, x2, y2) = (0.42f32, 0.42 * slope, 0.58f32, 1f32);
    let (cx, cy) = (3. * x1, 3. * y1);
    let (bx, by) = (3. * (x2 - x1) - cx, 3. * (y2 - y1) - cy);
    let (ax, ay) = (1. - cx - bx, 1. - cy - by);
    let curve_x = |t: f32| ((ax * t + bx) * t + cx) * t;
    let speed_x = |t: f32| (3. * ax * t + 2. * bx) * t + cx;
    let mut t = x;
    for _ in 0..8 {
        let error = curve_x(t) - x;
        let speed = speed_x(t);
        if error.abs() < 1e-6 || speed.abs() < 1e-6 {
            break;
        }
        t = (t - error / speed).clamp(0., 1.);
    }
    if (curve_x(t) - x).abs() > 1e-4 {
        let (mut low, mut high) = (0f32, 1f32);
        for _ in 0..32 {
            t = (low + high) / 2.;
            if curve_x(t) < x { low = t } else { high = t }
        }
    }
    let y = ((ay * t + by) * t + cy) * t;
    let dy = (3. * ay * t + 2. * by) * t + cy;
    let dx = speed_x(t);
    (y, if dx.abs() < 1e-6 { 0. } else { dy / dx })
}

impl Glide {
    fn progress(&self, now: Instant) -> f32 {
        match self.duration {
            Some(duration) if duration > 0. => {
                (now.saturating_duration_since(self.start).as_secs_f32() / duration).min(1.)
            }
            _ => 0.,
        }
    }

    fn value(&self, now: Instant) -> Point<Pixels> {
        let (y, _) = ease(self.slope, self.progress(now));
        self.from + (self.to - self.from) * y
    }

    /// Velocity in px/s along the larger axis of the segment.
    fn velocity(&self, now: Instant) -> f32 {
        match self.duration {
            Some(duration) if duration > 0. => {
                let x = self.progress(now);
                if x >= 1. {
                    return 0.;
                }
                ease(self.slope, x).1 * major(self.to - self.from) / duration
            }
            _ => self.velocity,
        }
    }
}

/// Where a running glide is heading, or `current` when none is.
pub fn target(key: usize, current: Point<Pixels>) -> Point<Pixels> {
    GLIDES.with_borrow(|glides| match glides.get(&key) {
        Some(glide) if glide.written == current => glide.to,
        _ => current,
    })
}

/// Glides from `current` to `to`, continuing a running glide's motion.
pub fn glide_to(key: usize, current: Point<Pixels>, to: Point<Pixels>) {
    let now = Instant::now();
    GLIDES.with_borrow_mut(|glides| {
        let (from, velocity) = match glides.get(&key) {
            Some(glide) if glide.written == current => (glide.value(now), glide.velocity(now)),
            _ => (current, 0.),
        };
        glides.insert(
            key,
            Glide {
                from,
                to,
                start: now,
                duration: None,
                slope: 0.,
                velocity,
                written: current,
            },
        );
    });
}

thread_local! {
    static CLAIMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A wheel event starts its dispatch: no scroller has taken it yet.
pub fn new_event() {
    CLAIMED.set(false);
}

/// A scroller moved for the event being dispatched; outer scrollers leave it.
pub fn claim() {
    CLAIMED.set(true);
}

/// Whether a scroller already moved for the event being dispatched.
pub fn claimed() -> bool {
    CLAIMED.get()
}

/// Ends a running glide where it is.
pub fn cancel(key: usize) {
    GLIDES.with_borrow_mut(|glides| glides.remove(&key));
}

/// Whether a glide is running.
pub fn running(key: usize) -> bool {
    GLIDES.with_borrow(|glides| glides.contains_key(&key))
}

/// Whether a glide is running and not past its end. The scroller's
/// prepaint takes the last step and ends the glide; one whose scroller
/// is no longer drawn stays in the list, and stops counting here once it
/// would have ended (the longest segment, until its first step times it).
pub(super) fn stepping(key: usize) -> bool {
    let now = Instant::now();
    GLIDES.with_borrow(|glides| {
        glides.get(&key).is_some_and(|glide| {
            let length = glide.duration.unwrap_or_else(|| segment(0.));
            now.saturating_duration_since(glide.start).as_secs_f32() <= length + 0.1
        })
    })
}

thread_local! {
    /// The offsets whose glides ask for isolated frames (`ScrollHandle::isolate_glides`).
    static ISOLATED: RefCell<Vec<Weak<RefCell<Point<Pixels>>>>> = const { RefCell::new(Vec::new()) };
}

/// The glides of `offset` ask for isolated frames from now on.
pub(super) fn isolate(offset: &Rc<RefCell<Point<Pixels>>>) {
    ISOLATED.with_borrow_mut(|isolated| {
        isolated.retain(|known| known.strong_count() > 0);
        if !isolated
            .iter()
            .any(|known| std::ptr::eq(known.as_ptr(), Rc::as_ptr(offset)))
        {
            isolated.push(Rc::downgrade(offset));
        }
    });
}

/// Whether the glides of `offset` ask for isolated frames.
pub(super) fn isolated(offset: &Rc<RefCell<Point<Pixels>>>) -> bool {
    ISOLATED.with_borrow(|isolated| {
        isolated
            .iter()
            .any(|known| std::ptr::eq(known.as_ptr(), Rc::as_ptr(offset)))
    })
}

/// Moves the offset along a running glide, keeping the target inside the
/// scrollable range, and asks for the next frame until the glide ends:
/// an isolated one (`Window::request_isolated_animation_frame`) if the
/// scroller's handle asked for that.
pub(super) fn advance(
    key: usize,
    offset: &mut Point<Pixels>,
    max: Point<Pixels>,
    isolated: bool,
    window: &mut Window,
) {
    GLIDES.with_borrow_mut(|glides| {
        let Some(glide) = glides.get_mut(&key) else {
            return;
        };
        if *offset != glide.written {
            glides.remove(&key);
            return;
        }
        let clamp = |p: Point<Pixels>| point(p.x.clamp(-max.x, px(0.)), p.y.clamp(-max.y, px(0.)));
        let to = clamp(glide.to);
        if glide.duration.is_none() || to != glide.to {
            // Time the segment from where it began, toward the reachable target.
            if glide.duration.is_some() {
                let now = Instant::now();
                glide.velocity = glide.velocity(now);
                glide.from = glide.value(now);
                glide.start = now;
            }
            glide.to = to;
            let distance = major(to - glide.from);
            if distance.abs() < 0.01 {
                *offset = to;
                glides.remove(&key);
                return;
            }
            let duration = segment(distance);
            glide.duration = Some(duration);
            glide.slope = (glide.velocity * duration / distance).clamp(-1000., 1000.);
        }
        let now = Instant::now();
        // On whole device pixels, each frame of a glide moves the content
        // by a whole number of them: the renderer can copy it instead of
        // drawing it again (`ScrollRegion`). An edge off the device grid
        // (a fractional scale factor, or the scroll max rounded to two
        // decimals) snaps past itself, so the snapped offset is clamped
        // again here: the prepaint's own clamp would otherwise move it off
        // what the glide wrote, the next step would take that for someone
        // else's scroll and end the glide, and a glide leaving that edge
        // would never get away from it.
        if glide.progress(now) >= 1. {
            // Land exactly at a targeted edge. Snapping inward leaves a
            // remainder that claims the next notch meant for the parent.
            let snapped = clamp(window.pixel_snap_point(to));
            *offset = point(
                if to.x == -max.x { to.x } else { snapped.x },
                if to.y == -max.y { to.y } else { snapped.y },
            );
            glides.remove(&key);
        } else {
            *offset = clamp(window.pixel_snap_point(clamp(glide.value(now))));
            glide.written = *offset;
        }
        // Once more after the last step: what reads the offset while
        // rendering, before this prepaint moves it (a pinned group
        // header), shows where the glide ended. On whole pixels, the
        // last step can be a pixel or two.
        if isolated {
            window.request_isolated_animation_frame();
        } else {
            window.request_animation_frame();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_follow_chromium_inverse_delta() {
        assert!((segment(100.) - 0.2).abs() < 1e-6);
        assert!((segment(-120.) - 0.2).abs() < 1e-6);
        assert!((segment(300.) - 0.15).abs() < 1e-6);
        assert!((segment(900.) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn ease_in_out_matches_css_and_carries_slope() {
        let (half, slope) = ease(0., 0.5);
        assert!((half - 0.5).abs() < 1e-4);
        assert!(slope > 1.5);
        assert!(ease(0., 0.).1.abs() < 1e-4);
        assert!((ease(2., 0.).1 - 2.).abs() < 1e-3);
        assert!((ease(0., 1.).0 - 1.).abs() < 1e-5);
    }
}
