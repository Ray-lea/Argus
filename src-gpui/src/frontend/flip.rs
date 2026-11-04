//! FLIP reordering (First, Last, Invert, Play): an item laid out at a new
//! place is drawn back where it was and glides to where it now is. Layout and
//! hit testing stay at the new place; only the drawing moves.
use gpui_kit::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long a moved item takes to glide into its place.
pub(super) const GLIDE: Duration = Duration::from_millis(220);

#[derive(Clone, Copy)]
struct Glide {
    from: f32,
    started: Instant,
}

impl Glide {
    fn offset(&self, now: Instant) -> f32 {
        let t = now.saturating_duration_since(self.started).as_secs_f32() / GLIDE.as_secs_f32();
        // Exactly where it was at the start and in place at the end: the
        // curve itself lands a hair off either.
        if t <= 0. {
            self.from
        } else if t >= 1. {
            0.
        } else {
            self.from * (1. - super::theme::ease_out(t))
        }
    }
}

/// Items gliding to new places along one axis, by key.
#[derive(Default)]
pub(super) struct Flips {
    glides: RefCell<HashMap<String, Glide>>,
    /// Where each item was last laid out, for `place`.
    places: RefCell<HashMap<String, f32>>,
}

impl Flips {
    /// `key` was laid out `by` further along: it is drawn back where it was
    /// (where an earlier glide had taken it so far) and glides from there.
    pub(super) fn moved(&self, key: &str, by: f32, now: Instant) {
        let mut glides = self.glides.borrow_mut();
        let left = glides.get(key).map_or(0., |glide| glide.offset(now));
        glides.insert(
            key.to_owned(),
            Glide {
                from: left - by,
                started: now,
            },
        );
    }

    /// Notes where `key` is laid out, gliding it from its last place if that differs.
    pub(super) fn place(&self, key: &str, place: f32, now: Instant) {
        let previous = self.places.borrow_mut().insert(key.to_owned(), place);
        if let Some(previous) = previous
            && previous != place
        {
            self.moved(key, place - previous, now);
        }
    }

    /// Notes where `key` is laid out without gliding it there (it is under
    /// the pointer, or motion is reduced).
    pub(super) fn quiet(&self, key: &str, place: f32) {
        self.places.borrow_mut().insert(key.to_owned(), place);
        self.glides.borrow_mut().remove(key);
    }

    /// How far from its layout `key` is drawn now.
    pub(super) fn offset(&self, key: &str, now: Instant) -> f32 {
        self.glides
            .borrow()
            .get(key)
            .map_or(0., |glide| glide.offset(now))
    }

    /// Whether anything still glides; finished glides are dropped.
    pub(super) fn active(&self, now: Instant) -> bool {
        let mut glides = self.glides.borrow_mut();
        glides.retain(|_, glide| now < glide.started + GLIDE);
        !glides.is_empty()
    }

    /// Forgets every place and glide (the items were laid out anew).
    pub(super) fn clear(&self) {
        self.glides.borrow_mut().clear();
        self.places.borrow_mut().clear();
    }
}

/// An item dragged along a list: it stays under the pointer while the list
/// rearranges around it.
#[derive(Clone)]
pub(super) struct Drag {
    pub key: String,
    /// The pointer when it took the item, and now, along the list's axis.
    pub grab: f32,
    pub pointer: f32,
    /// Where the item was laid out when taken.
    pub start: f32,
}

impl Drag {
    /// Where the item is drawn: its start moved as far as the pointer has.
    pub(super) fn at(&self) -> f32 {
        self.start + self.pointer - self.grab
    }
}

/// `child` drawn after the window's other content and clipped to `clip`
/// rather than to its ancestors: a table cell's content gliding past the
/// cells beside it, which would otherwise cut it off. Laid out where it is.
pub(super) fn escaping(child: impl IntoElement, clip: Bounds<Pixels>, priority: usize) -> Escaping {
    Escaping {
        child: Some(child.into_any_element()),
        clip,
        priority,
    }
}

pub(super) struct Escaping {
    child: Option<AnyElement>,
    clip: Bounds<Pixels>,
    priority: usize,
}

impl IntoElement for Escaping {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Escaping {
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
        let child = self.child.as_mut().expect("escaping child");
        (child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let child = self.child.take().expect("escaping child");
        let offset = window.element_offset();
        window.defer_draw(
            child,
            offset,
            self.priority,
            Some(ContentMask { bounds: self.clip }),
        );
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::{Flips, GLIDE};
    use std::time::{Duration, Instant};

    #[test]
    fn a_moved_item_starts_where_it_was_and_settles_in_place() {
        let flips = Flips::default();
        let now = Instant::now();
        flips.place("cpu", 100., now);
        assert_eq!(flips.offset("cpu", now), 0.);
        flips.place("cpu", 40., now);
        assert_eq!(flips.offset("cpu", now), 60.);
        let half = flips.offset("cpu", now + GLIDE / 2);
        assert!(half > 0. && half < 60.);
        assert_eq!(flips.offset("cpu", now + GLIDE), 0.);
        assert!(!flips.active(now + GLIDE + Duration::from_millis(1)));
    }

    #[test]
    fn a_second_move_continues_from_where_the_first_had_taken_it() {
        let flips = Flips::default();
        let now = Instant::now();
        flips.moved("pid", 30., now);
        let later = now + GLIDE / 2;
        let midway = flips.offset("pid", later);
        flips.moved("pid", -30., later);
        assert!((flips.offset("pid", later) - (midway + 30.)).abs() < 1e-4);
        assert!(flips.active(later));
    }
}
