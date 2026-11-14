//! Hover hints on table cells: the interface's native `title` tooltips.
//!
//! Tables lay out their visible rows again on every scroll frame, and a
//! `.tooltip()` on each cell would add a hitbox and four mouse listeners per
//! cell to every one of those frames. Instead only the row under the pointer
//! reports where its hinted parts were laid out ([`Tips::target`]), a table
//! forwards its pointer moves, and one tracker picks the part under the
//! pointer. Its hint shows after GPUI's tooltip delay in the window's tooltip
//! slot, where `.tooltip()` puts its own, and hides on the same occasions: the
//! pointer leaving the part, a press, the wheel or the keyboard.
//!
//! A row reports when its list renders, and a list does not render for the
//! pointer moving over its rows (their hover fills change in place). A row the
//! pointer comes onto is therefore asked to report once the pointer has stayed
//! on it ([`REPORT_AFTER`]): the list renders once for a pointer that rests,
//! not once for every row a moving pointer crosses. The hint's delay still
//! counts from when the pointer came onto the part.
use super::*;
use std::hash::{DefaultHasher, Hash, Hasher};

/// GPUI's `DEFAULT_TOOLTIP_SHOW_DELAY`, the delay of every other tooltip.
const SHOW_DELAY: Duration = Duration::from_millis(500);
/// How long the pointer stays on a row before the row reports its parts:
/// half the hint's delay, so that a late frame still leaves the hint on time.
const REPORT_AFTER: Duration = Duration::from_millis(250);
/// Most pointer moves kept for a row that is waited for ([`Waiting::trail`]).
const TRAIL: usize = 256;
/// Widest a hint runs before it wraps, as the performance page's hints do.
const WIDTH: f32 = 420.;

/// A hinted part's identity: the same part of the same row keeps it across
/// renders, another row in its place gets another one.
pub(super) fn id(parts: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    parts.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone, Default)]
pub(super) struct Tips(Rc<RefCell<State>>);

#[derive(Default)]
struct State {
    /// The hinted parts reported since the hinting table last rendered.
    targets: Vec<Target>,
    /// The pointer moved over a table since the last press, wheel or exit:
    /// as with `.tooltip()`, a hint waits for a move after those.
    armed: bool,
    /// The part under the pointer.
    hovered: Option<u64>,
    pending: Option<Task<()>>,
    shown: Option<Shown>,
    /// The view that puts the hint in the window ([`TipLayer`]).
    layer: Option<EntityId>,
    /// The view that renders the hinting table.
    table: Option<EntityId>,
    /// Where the hinting table laid out its rows when it last rendered, and
    /// the row that reported its parts then ([`Tips::row_under`]).
    rows: Option<Rows>,
    near: Option<usize>,
    /// The row the pointer came onto since, which has not reported.
    waiting: Option<Waiting>,
    /// The pointer's way over the row that reports in this render, if the
    /// row was waited for ([`Waiting::trail`]).
    trail: Vec<(Instant, Point<Pixels>)>,
}

/// The rows of a list: `count` rows `height` apart from `top`, where the
/// scrolled list has its first one, seen through `viewport`.
#[derive(Clone, Copy)]
struct Rows {
    viewport: Bounds<Pixels>,
    top: Pixels,
    height: f32,
    count: usize,
}

impl Rows {
    fn at(&self, point: Point<Pixels>) -> Option<usize> {
        if !self.viewport.contains(&point) {
            return None;
        }
        let row = f32::from(point.y - self.top) / self.height;
        (row >= 0. && (row as usize) < self.count).then_some(row as usize)
    }
}

/// A row the pointer came onto after its list rendered.
struct Waiting {
    row: usize,
    /// Renders the list once the pointer has stayed on the row.
    _report: Task<()>,
    /// Where the pointer went on the row and when, oldest first: from it a
    /// part that reports late learns how long the pointer has been on it.
    trail: Vec<(Instant, Point<Pixels>)>,
}

struct Target {
    id: u64,
    bounds: Bounds<Pixels>,
    text: SharedString,
}

struct Shown {
    id: u64,
    view: AnyView,
    at: Point<Pixels>,
}

impl State {
    /// The part under `point`; the smallest when parts overlap.
    fn at(&self, point: Point<Pixels>) -> Option<&Target> {
        self.targets
            .iter()
            .filter(|target| target.bounds.contains(&point))
            .min_by(|a, b| {
                let area =
                    |t: &Target| f32::from(t.bounds.size.width) * f32::from(t.bounds.size.height);
                area(a).total_cmp(&area(b))
            })
    }

    /// How long the pointer has been on a part at `bounds` whose row was
    /// waited for: the part's hint is that much nearer, as if the row had
    /// reported when the pointer came onto it. Nothing for a row that reports
    /// in the render the pointer's move asked for.
    fn dwelt(&self, bounds: &Bounds<Pixels>) -> Duration {
        dwelt(&self.trail, bounds, Instant::now())
    }
}

/// The time up to `now` over which every move of `trail` (oldest first) lay
/// inside `bounds`.
fn dwelt(trail: &[(Instant, Point<Pixels>)], bounds: &Bounds<Pixels>, now: Instant) -> Duration {
    trail
        .iter()
        .rev()
        .take_while(|(_, position)| bounds.contains(position))
        .last()
        .map_or(Duration::ZERO, |(since, _)| {
            now.saturating_duration_since(*since)
        })
}

impl Tips {
    /// A table renders its rows anew, in the view being rendered. Whether the
    /// row under the pointer ([`Tips::row_under`]) reports its hinted parts
    /// ([`Tips::target`]): not while no hint can show (after a press or the
    /// wheel, until the pointer moves), so those renders, the frames of a wheel
    /// scroll among them, pay nothing.
    pub(super) fn begin(&self, window: &Window) -> bool {
        let mut state = self.0.borrow_mut();
        state.targets.clear();
        state.table = Some(window.current_view());
        state.rows = None;
        state.near = None;
        // A row that was waited for reports in this render, whatever asked
        // for it.
        state.trail = state
            .waiting
            .take()
            .map(|waiting| waiting.trail)
            .unwrap_or_default();
        state.armed
    }

    /// The row of a list under the pointer, which reports its hinted parts in
    /// this render: `rows` rows `row_height` apart from the top of `list`'s
    /// viewport, scrolled by it. Read where the rows are laid out, after the
    /// list applied this frame's scrolling.
    pub(super) fn row_under(
        &self,
        list: &ScrollHandle,
        row_height: f32,
        rows: usize,
        window: &Window,
    ) -> Option<usize> {
        let viewport = list.bounds();
        let rows = Rows {
            viewport,
            top: viewport.top() + list.offset().y,
            height: row_height,
            count: rows,
        };
        let near = rows.at(window.mouse_position());
        let mut state = self.0.borrow_mut();
        state.rows = Some(rows);
        state.near = near;
        near
    }

    /// `element`, reporting itself as a part hinted by `text` once laid out.
    /// Only for the row under the pointer, when [`Tips::begin`] allows.
    pub(super) fn target<E: ParentElement>(
        &self,
        element: E,
        id: u64,
        text: impl Into<SharedString>,
    ) -> E {
        let tips = self.clone();
        let text = text.into();
        // Pinned to the element's edges: an absolute child left at its static
        // position would sit after a text's line or at a right-aligned end.
        element.child(
            canvas(
                move |bounds, window, cx| tips.offer(id, bounds, text, window, cx),
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0(),
        )
    }

    fn offer(
        &self,
        id: u64,
        bounds: Bounds<Pixels>,
        text: SharedString,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Only what shows counts: a row scrolled under the header or a cell
        // scrolled under the fixed column is clipped there.
        let bounds = bounds.intersect(&window.content_mask().bounds);
        if bounds.is_empty() {
            return;
        }
        let pointer = window.mouse_position();
        let hit = {
            let mut state = self.0.borrow_mut();
            state.targets.push(Target { id, bounds, text });
            state.armed && bounds.contains(&pointer)
        };
        // A part laid out under a resting pointer: the row the pointer came
        // onto, or another row in its place.
        if hit {
            let (under, dwelt) = {
                let state = self.0.borrow();
                match state.at(pointer) {
                    Some(target) => (Some(target.id), state.dwelt(&target.bounds)),
                    None => (None, Duration::ZERO),
                }
            };
            self.hover(under, dwelt, window, cx);
        }
    }

    /// The pointer moved over a hinting table.
    pub(super) fn moved(&self, event: &MouseMoveEvent, window: &mut Window, cx: &mut App) {
        if event.pressed_button.is_some() {
            // No hints while a button is held, as with `.tooltip()`.
            self.dismiss(window, cx);
            return;
        }
        let (under, rearmed, table) = {
            let mut state = self.0.borrow_mut();
            let rearmed = !std::mem::replace(&mut state.armed, true);
            // A row the pointer comes onto has not reported its parts. It
            // does when the list renders, which this asks for once the
            // pointer has stayed on the row.
            let onto = state
                .rows
                .and_then(|rows| rows.at(event.position))
                .filter(|row| !rearmed && state.near != Some(*row));
            match (onto, &mut state.waiting) {
                (Some(row), Some(waiting)) if waiting.row == row => {
                    // Too many moves to keep: what is known starts over here.
                    if waiting.trail.len() >= TRAIL {
                        waiting.trail.clear();
                    }
                    waiting.trail.push((Instant::now(), event.position));
                }
                (Some(row), _) => {
                    state.waiting = Some(Waiting {
                        row,
                        _report: self.report_later(window, cx),
                        trail: vec![(Instant::now(), event.position)],
                    });
                }
                (None, _) => state.waiting = None,
            }
            (
                state.at(event.position).map(|target| target.id),
                rearmed,
                state.table,
            )
        };
        // The table stopped reporting its parts after the press or the wheel;
        // it reports them in its next render.
        if rearmed && let Some(table) = table {
            cx.notify(table);
        }
        self.hover(under, Duration::ZERO, window, cx);
    }

    /// Renders the hinting table after [`REPORT_AFTER`], unless dropped.
    fn report_later(&self, window: &mut Window, cx: &mut App) -> Task<()> {
        let weak = Rc::downgrade(&self.0);
        window.spawn(cx, async move |cx| {
            cx.background_executor().timer(REPORT_AFTER).await;
            let table = weak.upgrade().and_then(|state| state.borrow().table);
            if let Some(table) = table {
                cx.update(|_, cx| cx.notify(table)).ok();
            }
        })
    }

    /// The pointer left the table, or a press or the wheel hid the hint. What
    /// the table reported goes too: the page may change before the pointer
    /// moves over a table again, which has that table report anew.
    pub(super) fn dismiss(&self, window: &mut Window, cx: &mut App) {
        {
            let mut state = self.0.borrow_mut();
            state.armed = false;
            state.targets.clear();
            state.waiting = None;
            state.trail.clear();
        }
        self.hover(None, Duration::ZERO, window, cx);
    }

    /// The part under the pointer is `id`, and has been for `dwelt`.
    fn hover(&self, id: Option<u64>, dwelt: Duration, window: &mut Window, cx: &mut App) {
        let (hide, layer) = {
            let mut state = self.0.borrow_mut();
            if state.hovered == id {
                return;
            }
            state.hovered = id;
            state.pending = id.map(|id| {
                let weak = Rc::downgrade(&self.0);
                let delay = SHOW_DELAY.saturating_sub(dwelt);
                window.spawn(cx, async move |cx| {
                    cx.background_executor().timer(delay).await;
                    if let Some(state) = weak.upgrade() {
                        cx.update(|window, cx| Tips(state).show(id, window, cx))
                            .ok();
                    }
                })
            });
            (state.shown.take().is_some(), state.layer)
        };
        if hide && let Some(layer) = layer {
            cx.notify(layer);
        }
    }

    fn show(&self, id: u64, window: &mut Window, cx: &mut App) {
        let pointer = window.mouse_position();
        let (text, layer) = {
            let state = self.0.borrow();
            // The part must still be under the pointer: the table may have
            // moved or dropped it since.
            match state.at(pointer) {
                Some(target) if state.armed && state.hovered == Some(id) && target.id == id => {
                    (target.text.clone(), state.layer)
                }
                _ => return,
            }
        };
        let view = widgets::wrapping_tooltip(text, px(WIDTH), window, cx);
        self.0.borrow_mut().shown = Some(Shown {
            id,
            view,
            at: pointer,
        });
        if let Some(layer) = layer {
            cx.notify(layer);
        }
    }

    /// Puts the hint in the window for the frame being drawn.
    fn request(&self, window: &mut Window) {
        let state = self.0.borrow();
        let Some(shown) = &state.shown else {
            return;
        };
        let id = shown.id;
        let weak = Rc::downgrade(&self.0);
        window.set_tooltip(AnyTooltip {
            view: shown.view.clone(),
            mouse_position: shown.at,
            // Checked on each frame drawn (also when this layer is replayed),
            // after the tables reported their parts.
            check_visible_and_update: Rc::new(move |_, window, _| {
                let Some(state) = weak.upgrade() else {
                    return false;
                };
                let mut state = state.borrow_mut();
                let visible = state.shown.as_ref().is_some_and(|shown| shown.id == id)
                    && !window.last_input_was_keyboard()
                    && state
                        .at(window.mouse_position())
                        .is_some_and(|target| target.id == id);
                if !visible && state.shown.as_ref().is_some_and(|shown| shown.id == id) {
                    // Hidden by a key or a part that moved away: the next
                    // pointer move starts over, as with `.tooltip()`.
                    state.shown = None;
                    state.hovered = None;
                    state.pending = None;
                }
                visible
            }),
        });
    }
}

/// Draws the shown hint. A view of its own at the root, so that showing or
/// hiding a hint redraws only it: the desktop above replays its regions.
pub(super) struct TipLayer {
    tips: Tips,
}

impl TipLayer {
    pub(super) fn new(tips: Tips, cx: &mut Context<Self>) -> Self {
        tips.0.borrow_mut().layer = Some(cx.entity_id());
        Self { tips }
    }
}

impl Render for TipLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let request = self.tips.clone();
        let tips = self.tips.clone();
        canvas(
            move |_, window, _| request.request(window),
            move |_, _, window, _| {
                // Painted before the page, so these run before a list's own
                // wheel handling stops the event.
                let pressed = tips.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        pressed.dismiss(window, cx);
                    }
                });
                window.on_mouse_event(move |_: &ScrollEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        tips.dismiss(window, cx);
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }
}

#[cfg(test)]
mod tests {
    use super::dwelt;
    use gpui_kit::gpui::{Bounds, point, px, size};
    use std::time::{Duration, Instant};

    #[test]
    fn a_part_that_reports_late_counts_from_when_the_pointer_came_onto_it() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let part = Bounds::new(point(px(100.), px(0.)), size(px(50.), px(30.)));
        let on = |x: f32| point(px(x), px(10.));

        // Onto the row beside the part, then onto the part and about on it.
        let trail = [(at(0), on(20.)), (at(40), on(110.)), (at(90), on(120.))];
        assert_eq!(dwelt(&trail, &part, at(250)), Duration::from_millis(210));
        // Off the part and back: only the last stay counts.
        let trail = [(at(0), on(110.)), (at(40), on(20.)), (at(90), on(120.))];
        assert_eq!(dwelt(&trail, &part, at(250)), Duration::from_millis(160));
        // The pointer's last move was elsewhere, or its way is not known: the
        // hint waits its whole delay.
        let trail = [(at(0), on(110.)), (at(40), on(20.))];
        assert_eq!(dwelt(&trail, &part, at(250)), Duration::ZERO);
        assert_eq!(dwelt(&[], &part, at(250)), Duration::ZERO);
    }
}
