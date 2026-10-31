//! A visual move and fade of a laid-out subtree (`Window::with_visual_transform`):
//! layout, hit testing and element bounds stay at the final place, so a cached
//! view inside replays its last paint instead of rendering every frame. Live
//! transforms (`live`, `LiveMotions`) go further: their values change between
//! frames while the view that painted them replays.
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// `child` drawn `offset` away from where it is laid out, at `opacity`.
pub(super) fn shifted(child: impl IntoElement, offset: Point<Pixels>, opacity: f32) -> Shifted {
    Shifted {
        child: child.into_any_element(),
        offset,
        opacity,
        viewport: None,
    }
}

pub(super) struct Shifted {
    child: AnyElement,
    offset: Point<Pixels>,
    opacity: f32,
    viewport: Option<Rc<Cell<Bounds<Pixels>>>>,
}

impl Shifted {
    /// Share the stationary parent clip with a layer painted outside this subtree.
    pub(super) fn within_viewport(mut self, viewport: Option<Rc<Cell<Bounds<Pixels>>>>) -> Self {
        self.viewport = viewport;
        self
    }
}

impl IntoElement for Shifted {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Shifted {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(viewport) = &self.viewport {
            viewport.set(window.content_mask().bounds);
        }
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.offset == Point::default() && self.opacity >= 1. {
            self.child.paint(window, cx);
        } else {
            let mut paint = |window: &mut Window| {
                window.with_visual_transform(self.offset, self.opacity, |window| {
                    self.child.paint(window, cx)
                })
            };
            if let Some(viewport) = &self.viewport {
                window.with_visual_clip(viewport.get(), paint);
            } else {
                paint(window);
            }
        }
    }
}

/// `child` drawn only inside its own bounds, their sides moved in by `left`
/// and `right` (out when negative), without an overflow clip: a cached view
/// inside replays under a new clip instead of rendering again, as it would
/// when an overflow clip changes.
pub(super) fn clipped(child: impl IntoElement, left: Pixels, right: Pixels) -> Clipped {
    Clipped {
        child: child.into_any_element(),
        left,
        right,
    }
}

pub(super) struct Clipped {
    child: AnyElement,
    left: Pixels,
    right: Pixels,
}

impl IntoElement for Clipped {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Clipped {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let clip = Bounds {
            origin: point(bounds.origin.x + self.left, bounds.origin.y),
            size: size(
                (bounds.size.width - self.left - self.right).max(px(0.)),
                bounds.size.height,
            ),
        };
        window.with_visual_clip(clip, |window| self.child.paint(window, cx));
    }
}

/// The point of an element's bounds a live transform scales about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Anchor {
    TopLeft,
    Center,
}

/// `child` drawn inside `transform` about its `anchor`
/// (`Window::with_live_transform`), laid out and hit-tested in place. The
/// transform's value is read whenever the window draws, so an animation
/// (`LiveMotions`) moves, scales and fades the child every frame while the
/// view around it replays its last paint instead of rendering again. The
/// anchor is written to `anchor_at` when the child prepaints, so that charts
/// inside, which the chart layer draws, take the same transform (`charts::Live`).
pub(super) fn live(
    child: impl IntoElement,
    transform: LiveTransform,
    anchor: Anchor,
    anchor_at: Rc<Cell<Point<Pixels>>>,
) -> Live {
    Live {
        child: child.into_any_element(),
        transform,
        anchor,
        anchor_at,
    }
}

pub(super) struct Live {
    child: AnyElement,
    transform: LiveTransform,
    anchor: Anchor,
    anchor_at: Rc<Cell<Point<Pixels>>>,
}

impl Live {
    fn anchor(&self, bounds: Bounds<Pixels>) -> Point<Pixels> {
        match self.anchor {
            Anchor::TopLeft => bounds.origin,
            Anchor::Center => bounds.center(),
        }
    }
}

impl IntoElement for Live {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Live {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.anchor_at.set(self.anchor(bounds));
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let anchor = self.anchor(bounds);
        window.with_live_transform(&self.transform, anchor, |window| {
            self.child.paint(window, cx)
        });
    }
}

/// What runs once after an animation's last frame.
pub(super) type Done = Box<dyn FnOnce(&mut App)>;

/// The most an animation advances from one frame to the next: a refresh at
/// 60 Hz. A slower frame (the page rendering again, new curves rasterized)
/// delays the animation rather than skipping part of it.
const MAX_STEP: Duration = Duration::from_micros(16_667);

/// One animation of live transforms: `step` sets their values for a time
/// since it began, and `done` runs once after its last frame (to render the
/// view without the animation's extra elements).
struct LiveMotion {
    key: SharedString,
    /// The frame it was last stepped in; `None` until the first frame after it started.
    last: Option<Instant>,
    elapsed: Duration,
    duration: Duration,
    step: Box<dyn Fn(Duration)>,
    done: Option<Done>,
}

/// The animations that move parts of the window with live transforms. The
/// chart layer, drawn in every frame, steps them before the cached views
/// replay (`tick`) and asks for the next frame while any runs; the views
/// they move are not rendered meanwhile. An animation begins at the first
/// frame drawn after it starts, so that a slow first frame (new curves are
/// rasterized before it) does not use up part of it, and advances by at most
/// `MAX_STEP` a frame.
#[derive(Default)]
pub(super) struct LiveMotions {
    motions: RefCell<Vec<LiveMotion>>,
}

impl LiveMotions {
    /// Starts an animation, replacing the one under the same `key`, and sets
    /// its first values for the frame being drawn.
    pub(super) fn start(
        &self,
        key: impl Into<SharedString>,
        duration: Duration,
        step: impl Fn(Duration) + 'static,
        done: impl FnOnce(&mut App) + 'static,
    ) {
        let key = key.into();
        step(Duration::ZERO);
        let mut motions = self.motions.borrow_mut();
        motions.retain(|motion| motion.key != key);
        motions.push(LiveMotion {
            key,
            last: None,
            elapsed: Duration::ZERO,
            duration,
            step: Box::new(step),
            done: Some(Box::new(done)),
        });
    }

    /// How far the animation under `key` has come, if it still runs.
    pub(super) fn elapsed(&self, key: &str) -> Option<Duration> {
        self.motions
            .borrow()
            .iter()
            .find(|motion| motion.key.as_ref() == key)
            .map(|motion| motion.elapsed)
    }

    /// Ends the animation under `key` where it is, without its `done`.
    pub(super) fn stop(&self, key: &str) {
        self.motions
            .borrow_mut()
            .retain(|motion| motion.key.as_ref() != key);
    }

    pub(super) fn active(&self) -> bool {
        !self.motions.borrow().is_empty()
    }

    /// Sets every animation's values for `now`; those past their end take
    /// their last values and leave, and their `done` runs after this frame.
    pub(super) fn tick(&self, now: Instant, cx: &mut App) {
        for done in self.step(now) {
            cx.defer(done);
        }
    }

    /// Sets every animation's values for `now` and returns the `done` of
    /// those that ended.
    pub(super) fn step(&self, now: Instant) -> Vec<Done> {
        let mut finished = Vec::new();
        self.motions.borrow_mut().retain_mut(|motion| {
            if let Some(last) = motion.last {
                motion.elapsed += now.saturating_duration_since(last).min(MAX_STEP);
            }
            motion.last = Some(now);
            (motion.step)(motion.elapsed.min(motion.duration));
            if motion.elapsed < motion.duration {
                return true;
            }
            finished.extend(motion.done.take());
            false
        });
        finished
    }
}
