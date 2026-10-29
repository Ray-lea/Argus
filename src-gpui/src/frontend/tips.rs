//! Hover hints on table cells: the original's native `title` tooltips.
//!
//! Tables lay out their visible rows again on every scroll frame, and a
//! `.tooltip()` on each cell would add a hitbox and four mouse listeners per
//! cell to every one of those frames. Instead only the row under the pointer
//! reports where its hinted parts were laid out ([`Tips::target`]), a table
//! forwards its pointer moves, and one tracker picks the part under the
//! pointer. Its hint shows after GPUI's tooltip delay in the window's tooltip
//! slot, where `.tooltip()` puts its own, and hides on the same occasions: the
//! pointer leaving the part, a press, the wheel or the keyboard.
use super::*;
use std::hash::{DefaultHasher, Hash, Hasher};

/// GPUI's `DEFAULT_TOOLTIP_SHOW_DELAY`, the delay of every other tooltip.
const SHOW_DELAY: Duration = Duration::from_millis(500);
/// Widest a hint runs before it wraps, as the performance page's hints do.
const WIDTH: f32 = 420.;

/// A hinted part's identity: the same part of the same row keeps it across
/// renders, another row in its place gets another one.
pub(super) fn id(parts: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    parts.hash(&mut hasher);
    hasher.finish()
}

/// The row of a list under the pointer: `rows` rows `row_height` apart from
/// the top of `list`'s viewport, scrolled by it. Read where the rows are laid
/// out, after the list applied this frame's scrolling.
pub(super) fn row_under(
    list: &ScrollHandle,
    row_height: f32,
    rows: usize,
    window: &Window,
) -> Option<usize> {
    let viewport = list.bounds();
    let pointer = window.mouse_position();
    if !viewport.contains(&pointer) {
        return None;
    }
    let row = f32::from(pointer.y - viewport.top() - list.offset().y) / row_height;
    (row >= 0. && (row as usize) < rows).then_some(row as usize)
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
}

impl Tips {
    /// A table renders its rows anew, in the view being rendered. Whether the
    /// row under the pointer ([`row_under`]) reports its hinted parts
    /// ([`Tips::target`]): not while no hint can show (after a press or the
    /// wheel, until the pointer moves), so those renders, the frames of a wheel
    /// scroll among them, pay nothing.
    pub(super) fn begin(&self, window: &Window) -> bool {
        let mut state = self.0.borrow_mut();
        state.targets.clear();
        state.table = Some(window.current_view());
        state.armed
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
        // A part laid out under a resting pointer: the row the pointer just
        // entered, or another row in its place.
        if hit {
            let under = self.0.borrow().at(pointer).map(|target| target.id);
            self.hover(under, window, cx);
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
            (
                state.at(event.position).map(|target| target.id),
                rearmed,
                state.table,
            )
        };
        // The table stopped reporting its parts after the press or the wheel;
        // it reports them in its next render. (The first move after a scroll
        // usually changes the hovered row, which renders it anyway.)
        if rearmed && let Some(table) = table {
            cx.notify(table);
        }
        self.hover(under, window, cx);
    }

    /// The pointer left the table, or a press or the wheel hid the hint. What
    /// the table reported goes too: the page may change before the pointer
    /// moves over a table again, which has that table report anew.
    pub(super) fn dismiss(&self, window: &mut Window, cx: &mut App) {
        {
            let mut state = self.0.borrow_mut();
            state.armed = false;
            state.targets.clear();
        }
        self.hover(None, window, cx);
    }

    fn hover(&self, id: Option<u64>, window: &mut Window, cx: &mut App) {
        let (hide, layer) = {
            let mut state = self.0.borrow_mut();
            if state.hovered == id {
                return;
            }
            state.hovered = id;
            state.pending = id.map(|id| {
                let weak = Rc::downgrade(&self.0);
                window.spawn(cx, async move |cx| {
                    cx.background_executor().timer(SHOW_DELAY).await;
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
                window.on_mouse_event(move |_: &ScrollWheelEvent, phase, window, cx| {
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
