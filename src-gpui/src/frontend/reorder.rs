//! Dragging an item of a list to another place, as the components page and
//! the taskbar monitor's panel arrange the monitors' readings: blocks
//! stacked top to bottom at their own heights, or items flowing in lines (a
//! grid of options, chips that wrap). The dragged item follows the pointer,
//! drawn by its list over the others while its place stays as a tinted
//! slot; the others make room at once and glide there (`flip`). Nothing is
//! saved until the item is let go.
use super::flip::Flips;
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

/// What is dragged: the drag's value. What it carries is in `Reorder`.
pub(super) struct Lifted;

/// What follows the pointer as the platform sees it: nothing, as the list
/// draws the item itself.
pub(super) struct Unseen;

impl Render for Unseen {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// How a list lays its items out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Flow {
    /// Top to bottom, `gap` apart.
    Stack { gap: f32 },
    /// Left to right `gap` apart, a new line `line_gap` below where the
    /// next item does not fit.
    Lines { gap: f32, line_gap: f32 },
}

/// An item being dragged.
#[derive(Clone, Debug)]
pub(super) struct Lift {
    /// The list it is of, and the part of the list it was taken in: one of
    /// several showing the same items in one order (a device's options).
    pub list: String,
    pub part: String,
    pub key: String,
    pub flow: Flow,
    /// The items' order as the drag has arranged it, and as it was.
    pub order: Vec<String>,
    pub initial: Vec<String>,
    /// The pointer when it took the item, and now, in the window.
    pub grab: Point<Pixels>,
    pub pointer: Point<Pixels>,
    /// Where the item was drawn when taken.
    pub start: Bounds<Pixels>,
    /// Each place of a flowing list as drawn when the item was taken, in
    /// order: the item takes the place under the pointer. Places that stay
    /// put keep the item from swinging between two of different widths.
    pub slots: Vec<Bounds<Pixels>>,
}

impl Lift {
    /// Where the item is drawn: where it was taken, moved as far as the
    /// pointer has.
    pub fn origin(&self) -> Point<Pixels> {
        self.start.origin + (self.pointer - self.grab)
    }

    /// The order it was let go in, when that differs from the one it began
    /// in.
    pub fn moved(&self) -> Option<&[String]> {
        (self.order != self.initial).then_some(self.order.as_slice())
    }
}

type Drawn = Rc<RefCell<HashMap<(String, String, String), Bounds<Pixels>>>>;

/// The lists of a view that can be arranged by dragging: the item dragged,
/// where the items were last drawn, and those gliding to new places.
#[derive(Default)]
pub(super) struct Reorder {
    lift: RefCell<Option<Lift>>,
    /// Where the button went down on an item that can be dragged: a drag
    /// begins only once the pointer has moved a little, and the item keeps
    /// to the pointer from where it was pressed.
    pressed: Cell<Option<Point<Pixels>>>,
    /// Each item's bounds as last drawn, and each part's (with no item),
    /// in the window, by list, part and item.
    drawn: Drawn,
    x: Flips,
    y: Flips,
}

impl Reorder {
    /// Notes where an item of `list` (or with `key` empty, the part that
    /// holds them) is drawn: for a canvas laid over it.
    pub fn recorder(
        &self,
        list: &str,
        part: &str,
        key: &str,
    ) -> impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static {
        let drawn = self.drawn.clone();
        let at = (list.to_owned(), part.to_owned(), key.to_owned());
        move |bounds, _, _| {
            drawn.borrow_mut().insert(at.clone(), bounds);
        }
    }

    /// Notes where an item (or with `key` empty, a part) is drawn.
    #[cfg(test)]
    pub fn note(&self, list: &str, part: &str, key: &str, bounds: Bounds<Pixels>) {
        self.drawn
            .borrow_mut()
            .insert((list.to_owned(), part.to_owned(), key.to_owned()), bounds);
    }

    /// Where an item (or with `key` empty, a part) was last drawn.
    pub fn bounds(&self, list: &str, part: &str, key: &str) -> Option<Bounds<Pixels>> {
        self.drawn
            .borrow()
            .get(&(list.to_owned(), part.to_owned(), key.to_owned()))
            .copied()
    }

    /// The item dragged, when it is of `list` or of a list whose name
    /// begins with `list` and `/`.
    pub fn lifted(&self, list: &str) -> Option<Lift> {
        self.lift
            .borrow()
            .as_ref()
            .filter(|lift| of_list(&lift.list, list))
            .cloned()
    }

    /// The button went down at `at` on an item that can be dragged.
    pub fn press(&self, at: Point<Pixels>) {
        self.pressed.set(Some(at));
    }

    /// The item dragged, of whichever list.
    pub fn current(&self) -> Option<Lift> {
        self.lift.borrow().clone()
    }

    pub fn dragging(&self) -> bool {
        self.lift.borrow().is_some()
    }

    /// Takes `key` of `list`, its items in `order`, pressed where the
    /// button went down (`press`) and now under the pointer at `pointer`.
    /// False when it was not drawn yet.
    pub fn take(
        &self,
        list: &str,
        part: &str,
        key: &str,
        flow: Flow,
        order: Vec<String>,
        pointer: Point<Pixels>,
    ) -> bool {
        let Some(start) = self.bounds(list, part, key) else {
            return false;
        };
        let slots = match flow {
            Flow::Stack { .. } => Vec::new(),
            Flow::Lines { .. } => order
                .iter()
                .filter_map(|item| self.bounds(list, part, item))
                .collect(),
        };
        let grab = self.pressed.take().unwrap_or(pointer);
        *self.lift.borrow_mut() = Some(Lift {
            list: list.to_owned(),
            part: part.to_owned(),
            key: key.to_owned(),
            flow,
            initial: order.clone(),
            order,
            grab,
            pointer,
            start,
            slots,
        });
        true
    }

    /// Follows the pointer at `pointer`: the dragged item takes the place
    /// it has come to and the items it passed glide to theirs. True when
    /// the order changed.
    pub fn follow(&self, pointer: Point<Pixels>, animate: bool, now: Instant) -> bool {
        let Some(mut lift) = self.lift.borrow().clone() else {
            return false;
        };
        lift.pointer = pointer;
        let before = lift.order.clone();
        match lift.flow {
            Flow::Stack { .. } => {
                // One neighbour at a time, as long as the item's middle
                // passes theirs.
                for _ in 0..lift.order.len() {
                    let places = self.places(&lift, &lift.order);
                    let Some(height) = places.get(&lift.key).map(|place| place.size.height) else {
                        break;
                    };
                    let middle = f32::from(lift.origin().y + height / 2.);
                    let middles = places
                        .iter()
                        .filter(|(key, _)| **key != lift.key)
                        .map(|(key, place)| (key.clone(), f32::from(place.center().y)))
                        .collect::<HashMap<_, _>>();
                    let Some(order) = stepped(&lift.order, &lift.key, middle, &middles) else {
                        break;
                    };
                    lift.order = order;
                }
            }
            Flow::Lines { .. } => {
                if let Some(place) = slot_at(&lift.slots, pointer)
                    && let Some(at) = lift.order.iter().position(|key| *key == lift.key)
                    && place != at
                {
                    let key = lift.order.remove(at);
                    lift.order.insert(place.min(lift.order.len()), key);
                }
            }
        }
        let changed = lift.order != before;
        if changed && animate {
            // Every item glides, the dragged one too: where other parts
            // show it, it moves like the rest (its own place is the slot,
            // drawn without glides).
            let (old, new) = (self.places(&lift, &before), self.places(&lift, &lift.order));
            for (key, place) in &new {
                if let Some(was) = old.get(key) {
                    self.glide(&shared(&lift.list, key), place.origin - was.origin, now);
                }
            }
        }
        *self.lift.borrow_mut() = Some(lift);
        changed
    }

    /// Lets go of the dragged item: it glides from under the pointer into
    /// its place. Returns the drag, with the order it arranged.
    pub fn release(&self, animate: bool, now: Instant) -> Option<Lift> {
        let lift = self.lift.borrow_mut().take()?;
        if animate && let Some(place) = self.places(&lift, &lift.order).get(&lift.key) {
            // Only in the part it was taken from, from where it was drawn:
            // a glide its list began meanwhile still runs and adds to this.
            let flip = shared(&lift.list, &lift.key);
            let running = point(px(self.x.offset(&flip, now)), px(self.y.offset(&flip, now)));
            let by = place.origin - lift.origin() + running;
            self.glide(&own(&lift.list, &lift.part, &lift.key), by, now);
        }
        Some(lift)
    }

    /// `flip` was laid out `by` further: it glides from where it was.
    fn glide(&self, flip: &str, by: Point<Pixels>, now: Instant) {
        if by.x != px(0.) {
            self.x.moved(flip, f32::from(by.x), now);
        }
        if by.y != px(0.) {
            self.y.moved(flip, f32::from(by.y), now);
        }
    }

    /// Forgets a drag without arranging anything.
    pub fn drop_lift(&self) {
        self.lift.borrow_mut().take();
    }

    /// How far from its place `key` of `list` is drawn in `part` while
    /// it glides.
    pub fn offset(&self, list: &str, part: &str, key: &str, now: Instant) -> Point<Pixels> {
        let (all, one) = (shared(list, key), own(list, part, key));
        point(
            px(self.x.offset(&all, now) + self.x.offset(&one, now)),
            px(self.y.offset(&all, now) + self.y.offset(&one, now)),
        )
    }

    /// Whether anything still glides: frames to draw. A dragged item needs
    /// none of its own; it moves as the pointer does.
    pub fn moving(&self, now: Instant) -> bool {
        // Both run: each drops its finished glides.
        let x = self.x.active(now);
        let y = self.y.active(now);
        x || y
    }

    /// Where each item of the dragged list would be in `order`, from the
    /// sizes drawn last and where its part is: stacked from the part's
    /// top, or flowing in lines across the part's width.
    fn places(&self, lift: &Lift, order: &[String]) -> HashMap<String, Bounds<Pixels>> {
        let Some(part) = self.bounds(&lift.list, &lift.part, "") else {
            return HashMap::new();
        };
        let drawn = order
            .iter()
            .filter_map(|key| {
                self.bounds(&lift.list, &lift.part, key)
                    .map(|bounds| (key.clone(), bounds))
            })
            .collect::<Vec<_>>();
        let mut places = HashMap::new();
        match lift.flow {
            // Each keeps its own left edge: an item may reach past its
            // part's sides (a grip in the margin).
            Flow::Stack { gap } => {
                let mut top = part.top();
                for (key, bounds) in drawn {
                    places.insert(key, Bounds::new(point(bounds.left(), top), bounds.size));
                    top += bounds.size.height + px(gap);
                }
            }
            Flow::Lines { gap, line_gap } => {
                let (mut x, mut y, mut line) = (px(0.), px(0.), px(0.));
                for (key, Bounds { size, .. }) in drawn {
                    // A little over the width still fits: sizes are rounded
                    // to device pixels, each item's on its own.
                    if x > px(0.) && x + size.width > part.size.width + px(2.) {
                        x = px(0.);
                        y += line + px(line_gap);
                        line = px(0.);
                    }
                    places.insert(key, Bounds::new(part.origin + point(x, y), size));
                    x += size.width + px(gap);
                    line = line.max(size.height);
                }
            }
        }
        places
    }
}

/// What an item glides by in every part of its list, and in one part only.
fn shared(list: &str, key: &str) -> String {
    format!("{list}\u{1f}{key}")
}

fn own(list: &str, part: &str, key: &str) -> String {
    format!("{list}\u{1f}{part}\u{1f}{key}")
}

/// `name` is `list`, or one of its lists (`list/…`).
fn of_list(name: &str, list: &str) -> bool {
    name == list
        || name
            .strip_prefix(list)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Where the dragged item belongs in `order` once its middle is at
/// `middle`: past the next item whose middle it has passed, or before the
/// previous one, one at a time; none while it stays between them.
pub(super) fn stepped(
    order: &[String],
    dragged: &str,
    middle: f32,
    middles: &HashMap<String, f32>,
) -> Option<Vec<String>> {
    let at = order.iter().position(|key| key == dragged)?;
    let known = |index: &usize| middles.contains_key(&order[*index]);
    let mut moved = order.to_vec();
    if let Some(next) = (at + 1..order.len()).find(known)
        && middle > middles[&order[next]]
    {
        let key = moved.remove(at);
        moved.insert(next, key);
        return Some(moved);
    }
    if let Some(previous) = (0..at).rev().find(known)
        && middle < middles[&order[previous]]
    {
        let key = moved.remove(at);
        moved.insert(previous, key);
        return Some(moved);
    }
    None
}

/// The place under `pointer` among `slots`; else the nearest on its line
/// (past a line's end, its last place), or with none on its line, the
/// nearest of all.
pub(super) fn slot_at(slots: &[Bounds<Pixels>], pointer: Point<Pixels>) -> Option<usize> {
    if let Some(index) = slots.iter().position(|slot| slot.contains(&pointer)) {
        return Some(index);
    }
    let across = |slot: &Bounds<Pixels>| {
        f32::from(
            (slot.left() - pointer.x)
                .max(pointer.x - slot.right())
                .max(px(0.)),
        )
    };
    let down = |slot: &Bounds<Pixels>| {
        f32::from(
            (slot.top() - pointer.y)
                .max(pointer.y - slot.bottom())
                .max(px(0.)),
        )
    };
    let nearest = |distance: &dyn Fn(&Bounds<Pixels>) -> f32, on_line: bool| {
        slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| !on_line || down(slot) == 0.)
            .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))
            .map(|(index, _)| index)
    };
    nearest(&across, true).or_else(|| {
        nearest(
            &|slot: &Bounds<Pixels>| across(slot).powi(2) + down(slot).powi(2),
            false,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{Flow, Reorder, of_list, slot_at, stepped};
    use gpui_kit::{Bounds, point, px, size};
    use std::collections::HashMap;
    use std::time::Instant;

    fn keys(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn a_block_passes_a_neighbour_once_its_middle_does() {
        let order = keys(&["cpu", "disk", "network"]);
        // A short block over a tall one: it passes once its middle passes
        // the tall one's, and does not swing back right after.
        let middles = HashMap::from([("disk".to_owned(), 100.), ("network".to_owned(), 200.)]);
        assert_eq!(stepped(&order, "cpu", 90., &middles), None);
        let moved = stepped(&order, "cpu", 101., &middles).unwrap();
        assert_eq!(moved, keys(&["disk", "cpu", "network"]));
        let middles = HashMap::from([("disk".to_owned(), 80.), ("network".to_owned(), 200.)]);
        assert_eq!(stepped(&moved, "cpu", 101., &middles), None);
        // And back up past the one before.
        assert_eq!(
            stepped(&moved, "cpu", 70., &middles).unwrap(),
            keys(&["cpu", "disk", "network"])
        );
    }

    #[test]
    fn an_item_takes_the_place_under_the_pointer_or_the_nearest() {
        let slot = |x: f32, y: f32| Bounds::new(point(px(x), px(y)), size(px(50.), px(30.)));
        let slots = [slot(0., 0.), slot(60., 0.), slot(0., 40.)];
        assert_eq!(slot_at(&slots, point(px(70.), px(10.))), Some(1));
        assert_eq!(slot_at(&slots, point(px(10.), px(50.))), Some(2));
        // Between two: the nearer; past a line's end: its last place,
        // though one of the line above is nearer.
        assert_eq!(slot_at(&slots, point(px(56.), px(10.))), Some(1));
        assert_eq!(slot_at(&slots, point(px(90.), px(60.))), Some(2));
        // Off every line: the nearest of all.
        assert_eq!(slot_at(&slots, point(px(90.), px(35.))), Some(1));
        assert_eq!(slot_at(&slots, point(px(10.), px(90.))), Some(2));
        assert_eq!(slot_at(&[], point(px(0.), px(0.))), None);
    }

    #[test]
    fn lists_name_their_own_lists() {
        assert!(of_list("desktopMetrics", "desktopMetrics"));
        assert!(of_list("desktopMetrics/cpu", "desktopMetrics"));
        assert!(!of_list("desktopMetricsX", "desktopMetrics"));
        assert!(!of_list("taskbarMetrics/cpu", "desktopMetrics"));
    }

    #[test]
    fn a_dragged_chip_moves_the_others_and_glides_into_place() {
        let reorder = Reorder::default();
        let chip =
            |x: f32, y: f32, width: f32| Bounds::new(point(px(x), px(y)), size(px(width), px(28.)));
        // A part 140 wide: chips 80 and 40 wide on the first line, a third
        // 60 wide on the next, 6 apart.
        let part = Bounds::new(point(px(0.), px(0.)), size(px(140.), px(62.)));
        for (key, bounds) in [
            ("", part),
            ("a", chip(0., 0., 80.)),
            ("b", chip(86., 0., 40.)),
            ("c", chip(0., 34., 60.)),
        ] {
            reorder.note("chips", "", key, bounds);
        }
        let order = keys(&["a", "b", "c"]);
        let now = Instant::now();
        // Pressed at (10, 10); the drag begins a few pixels on.
        reorder.press(point(px(10.), px(10.)));
        let flow = Flow::Lines {
            gap: 6.,
            line_gap: 6.,
        };
        assert!(reorder.take("chips", "", "a", flow, order, point(px(13.), px(10.))));
        assert_eq!(
            reorder.lifted("chips").unwrap().origin(),
            point(px(3.), px(0.))
        );
        assert!(reorder.lifted("chips").is_some() && reorder.lifted("other").is_none());
        // Over the place "c" had: "a" takes it; the others flow up, "b"
        // gliding from the first line's end to its start.
        assert!(reorder.follow(point(px(20.), px(45.)), true, now));
        let lift = reorder.lifted("chips").unwrap();
        assert_eq!(lift.order, keys(&["b", "c", "a"]));
        assert_eq!(lift.origin(), point(px(10.), px(35.)));
        assert_eq!(
            reorder.offset("chips", "", "b", now),
            point(px(86.), px(0.))
        );
        // Still there: nothing changes.
        assert!(!reorder.follow(point(px(22.), px(46.)), true, now));
        // Let go: it glides from under the pointer to its place, at the
        // start of the second line (40 + 6 + 60 + 6 + 80 > 140).
        let lift = reorder.release(true, now).unwrap();
        assert_eq!(lift.moved(), Some(keys(&["b", "c", "a"]).as_slice()));
        assert_eq!(
            reorder.offset("chips", "", "a", now),
            point(px(12.), px(2.))
        );
        assert!(!reorder.dragging() && reorder.moving(now));
    }
}
