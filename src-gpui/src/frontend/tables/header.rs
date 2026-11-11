//! Process headers dragged to another place: the column follows the pointer
//! as one raised strip, the others trade places with it and glide (`flip`).
use super::*;

/// What a dragged process header carries; the table keeps the drag itself.
pub(super) struct HeaderPayload;
/// Nothing follows the pointer but the column itself.
struct HeaderGhost;
impl Render for HeaderGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
/// A process header being dragged.
pub(super) struct HeaderDrag {
    key: &'static str,
    /// The pointer's x when it took the header, and now.
    grab: f32,
    pointer: f32,
    /// Where the column started, and where each column is laid out now
    /// (`NativeTable::column_places`).
    start: f32,
    places: HashMap<&'static str, f32>,
    /// How far left and right the column can go: within the scrolled
    /// columns, after the fixed name.
    lo: f32,
    hi: f32,
}
impl HeaderDrag {
    /// Where the pointer has taken the column: its start moved as far as the
    /// pointer has. It decides the column's place, even past either end.
    fn reach(&self) -> f32 {
        self.start + self.pointer - self.grab
    }
    /// Where the column is drawn: where it reaches, within the scrolled columns.
    fn at(&self) -> f32 {
        self.reach().clamp(self.lo, self.hi.max(self.lo))
    }
}
/// How near a column's edge the pointer finds it, either side.
const EDGE_REACH: f32 = 5.;
/// How long the pointer rests on a column's edge before its hint shows.
const EDGE_HINT_DELAY: Duration = Duration::from_millis(700);
/// How long a column set back to its automatic width stays washed in the accent.
const RESTORE_WASH: Duration = Duration::from_millis(700);

impl NativeTable {
    /// Where each column's right edge is drawn, in window coordinates, at
    /// the table's widths and scroll; `None` for an edge scrolled out of view.
    fn column_edges(state: &TableState<Self>) -> Vec<Option<f32>> {
        let delegate = state.delegate();
        let Some(handle) = delegate.horizontal.as_ref() else {
            return Vec::new();
        };
        let lane = handle.bounds();
        let (left, right) = (f32::from(lane.left()), f32::from(lane.right()));
        let mut edge = left + f32::from(handle.offset().x);
        (0..delegate.fields.len())
            .map(|index| {
                if index == 0 {
                    // The name is fixed: its edge is where the scrolled lane begins.
                    return Some(left);
                }
                edge += state.col_width(index).map(f32::from).unwrap_or(0.);
                (edge > left && edge <= right + 1.).then_some(edge)
            })
            .collect()
    }

    /// Follows the pointer at `at` over the process header `band`: the
    /// column edge within reach is marked. Returns whether the mark changed.
    fn track_edge(state: &mut TableState<Self>, at: Point<Pixels>, band: Bounds<Pixels>) -> bool {
        if state.delegate().page != "processes" {
            return false;
        }
        // While an edge is pressed, it stays the marked one.
        if state.delegate().resizing.is_some() {
            return false;
        }
        let edges = Self::column_edges(state);
        let delegate = state.delegate_mut();
        let hover = if band.contains(&at) && delegate.header_drag.is_none() {
            let pointer = f32::from(at.x);
            edges
                .iter()
                .enumerate()
                .filter_map(|(index, edge)| Some((index, (*edge)?)))
                .filter(|(_, edge)| (edge - pointer).abs() <= EDGE_REACH)
                .min_by(|(_, a), (_, b)| (a - pointer).abs().total_cmp(&(b - pointer).abs()))
                .map(|(index, _)| index)
        } else {
            None
        };
        if delegate.edge_hover == hover {
            return false;
        }
        delegate.edge_hover = hover;
        delegate.edge_since = hover.map(|_| Instant::now());
        true
    }

    /// Listens to the pointer over the process header, whatever is drawn
    /// above it (the table's resize handles take the hover there).
    pub(super) fn edge_probe(&self, cx: &mut Context<TableState<Self>>) -> impl IntoElement {
        let table = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |band, _, window, _| {
                let moved = table.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        let _ = moved.update(cx, |state, cx| {
                            if Self::track_edge(state, event.position, band) {
                                cx.notify();
                                // The hint shows once the pointer has rested there.
                                if state.delegate().edge_hover.is_some() {
                                    cx.spawn(async move |table, cx| {
                                        cx.background_executor().timer(EDGE_HINT_DELAY).await;
                                        let _ = table.update(cx, |_, cx| cx.notify());
                                    })
                                    .detach();
                                }
                            }
                        });
                    }
                });
                let pressed = table.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        let _ = pressed.update(cx, |state, cx| {
                            let delegate = state.delegate_mut();
                            delegate.resizing = delegate.edge_hover;
                            if delegate.resizing.is_some() {
                                cx.notify();
                            }
                        });
                    }
                });
                let released = table.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                        let _ = released.update(cx, |state, cx| {
                            if state.delegate_mut().resizing.take().is_some() {
                                // A double-click restoring the column moves its
                                // edge away from the pointer: nothing to mark.
                                if state.delegate().restored.is_none() {
                                    Self::track_edge(state, event.position, band);
                                }
                                cx.notify();
                            }
                        });
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    /// Column `key` set back to its automatic width (or fitted to its
    /// contents) by a double-click on its edge or from the menu: it shows
    /// a wash of the accent that fades as it settles (`restore_wash`).
    pub(super) fn mark_restored(&mut self, key: &'static str) {
        self.restored = Some((key, Instant::now()));
        self.edge_hover = None;
        self.edge_since = None;
    }

    /// What marks column `index`'s right edge while the pointer is on it,
    /// laid out with the header cell so that it is wherever the edge is
    /// drawn: a short accent line across the header, which runs down
    /// through the rows while the edge is dragged; and, once the pointer
    /// has rested there, what dragging and double-clicking do.
    pub(super) fn edge_marks(
        &self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Option<AnyElement> {
        if self.edge_hover != Some(index) || self.header_drag.is_some() {
            return None;
        }
        let key = self.fields.get(index)?.key;
        let now = Instant::now();
        if self
            .restored
            .is_some_and(|(restored, at)| restored == key && now < at + RESTORE_WASH)
        {
            return None;
        }
        let band = self.header_band.get();
        let list = self.list.as_ref()?.bounds();
        let dragging = self.resizing == Some(index) && cx.has_active_drag();
        // The header cell's content ends short of the column's edge by the
        // table's inset; the line is centred on the edge.
        let right = px(-(widths::HEADER_INSET + 1.));
        let top = px(2.);
        let height = if dragging {
            list.bottom() - band.top() - px(8.)
        } else {
            band.size.height - px(12.)
        };
        let p = Palette::current(cx);
        let motion = !cx.reduce_motion();
        let fade = |element: Div, id: (&'static str, usize), ms: u64| {
            if motion {
                element
                    .with_animation(
                        id,
                        Animation::new(Duration::from_millis(ms)).with_easing(theme::ease_out),
                        |element, t| element.opacity(t),
                    )
                    .into_any_element()
            } else {
                element.into_any_element()
            }
        };
        let line = div()
            .absolute()
            .top(top)
            .right(right)
            .w(px(2.))
            .h(height)
            .rounded(px(1.))
            .bg(if dragging {
                p.accent
            } else {
                p.accent.opacity(0.8)
            });
        let rested = self
            .edge_since
            .is_some_and(|since| now >= since + EDGE_HINT_DELAY);
        // Centred under the line, in a band twice as wide as it needs.
        const HINT_SPAN: f32 = 480.;
        let hint = (rested && self.resizing.is_none()).then(|| {
            div()
                .absolute()
                .top(band.size.height - px(2.))
                .right(right - px(HINT_SPAN / 2. - 1.))
                .w(px(HINT_SPAN))
                .flex()
                .justify_center()
                .child(widgets::tooltip(
                    label("processes.columns.edgeHint"),
                    window,
                    cx,
                ))
        });
        // The line over the neighbouring cells and the rows, within the
        // scrolled columns (not over the fixed name); the hint anywhere in
        // the table.
        let table = Bounds::from_corners(
            point(band.left(), band.top()),
            point(band.right(), list.bottom()),
        );
        let lane = self
            .horizontal
            .as_ref()
            .map(|handle| handle.bounds())
            .filter(|_| index > 0)
            .map_or(table, |lane| {
                Bounds::from_corners(
                    point(lane.left(), band.top()),
                    point(lane.right() + px(2.), list.bottom()),
                )
            });
        let layer = |child: AnyElement, clip| {
            super::flip::escaping(div().absolute().inset_0().child(child), clip, 2)
                .into_any_element()
        };
        let marks = div()
            .absolute()
            .inset_0()
            .child(layer(fade(line, ("column-edge", index), 120), lane))
            .children(hint.map(|hint| layer(fade(hint, ("column-edge-hint", index), 160), table)));
        Some(marks.into_any_element())
    }

    /// The column just set back to its automatic width: an accent wash over
    /// its header and rows that fades out while it eases to the new width,
    /// apart from the line a drag shows.
    pub(super) fn restore_wash(&mut self, now: Instant, cx: &App) -> Option<AnyElement> {
        let (key, at) = self.restored?;
        let t = now.saturating_duration_since(at).as_secs_f32() / RESTORE_WASH.as_secs_f32();
        if t >= 1. || cx.reduce_motion() {
            self.restored = None;
            return None;
        }
        let index = self.fields.iter().position(|field| field.key == key)?;
        let width = |column: &Column| f32::from(column.width);
        let place = self.columns[..index].iter().map(width).sum::<f32>();
        let name = self.columns.first().map_or(0., width);
        let handle = self.horizontal.as_ref()?;
        let lane = handle.bounds();
        let band = self.header_band.get();
        let bottom = self.list.as_ref()?.bounds().bottom();
        let left = if index == 0 {
            lane.left() - px(name)
        } else {
            lane.left() + px(place - name) + handle.offset().x
        };
        // Quickly in, then out slowly as the width settles.
        let level = if t < 0.15 {
            theme::ease_out(t / 0.15)
        } else {
            1. - theme::ease_in((t - 0.15) / 0.85)
        };
        let p = Palette::current(cx);
        Some(
            deferred(
                anchored().position(point(left, band.top())).child(
                    div()
                        .w(px(width(&self.columns[index])))
                        .h(bottom - band.top())
                        .rounded(px(6.))
                        .border_1()
                        .border_color(p.accent.opacity(0.5 * level))
                        .bg(p.accent.opacity(0.12 * level)),
                ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// Where each column starts, from the left of the first, at the table's widths.
    pub(super) fn column_places(state: &TableState<Self>) -> HashMap<&'static str, f32> {
        let mut left = 0.;
        state
            .delegate()
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let place = left;
                left += state.col_width(index).map(f32::from).unwrap_or(0.);
                (field.key, place)
            })
            .collect()
    }
    /// How far from its layout a column is drawn: the dragged one under the
    /// pointer, the others where their glides have taken them.
    pub(super) fn column_shift(&self, key: &str, now: Instant) -> f32 {
        match &self.header_drag {
            Some(drag) if drag.key == key => {
                drag.at() - drag.places.get(key).copied().unwrap_or(drag.start)
            }
            _ => self.flips.offset(key, now),
        }
    }
    /// A process header, which can be dragged to another place.
    pub(super) fn header_cell(
        &mut self,
        index: usize,
        button: Button,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> AnyElement {
        let movable = self.page == "processes"
            && self.columns.get(index).is_some_and(|column| column.movable);
        let button = match self.fields.get(index).filter(|_| movable) {
            Some(field) => {
                let key = field.key;
                let table = cx.entity().downgrade();
                button.on_drag(HeaderPayload, move |_, _, window, cx| {
                    let pointer = f32::from(window.mouse_position().x);
                    let _ = table.update(cx, |state, cx| {
                        let places = Self::column_places(state);
                        let start = places.get(key).copied().unwrap_or(0.);
                        let fields = &state.delegate().fields;
                        let index = fields.iter().position(|field| field.key == key);
                        let width = index
                            .and_then(|index| state.col_width(index))
                            .map(f32::from)
                            .unwrap_or(0.);
                        let lo = fields
                            .get(1)
                            .and_then(|field| places.get(field.key))
                            .copied()
                            .unwrap_or(start);
                        let end = fields
                            .len()
                            .checked_sub(1)
                            .and_then(|last| {
                                let place = places.get(fields[last].key)?;
                                Some(place + f32::from(state.col_width(last)?))
                            })
                            .unwrap_or(start + width);
                        let delegate = state.delegate_mut();
                        delegate.header_drag = Some(HeaderDrag {
                            key,
                            grab: pointer,
                            pointer,
                            start,
                            places,
                            lo,
                            hi: end - width,
                        });
                        delegate.lifted = Some(key);
                        cx.notify();
                    });
                    cx.new(|_| HeaderGhost)
                })
            }
            None => button,
        };
        let content = match self.edge_marks(index, window, cx) {
            Some(marks) => div()
                .relative()
                .size_full()
                .child(button)
                .child(marks)
                .into_any_element(),
            None => button.into_any_element(),
        };
        self.gliding_cell(index, true, content)
    }
    /// A cell's content drawn where its column is on its way to, over the
    /// cells beside it; the dragged column raised above the others.
    pub(super) fn gliding_cell(
        &mut self,
        column: usize,
        header: bool,
        content: AnyElement,
    ) -> AnyElement {
        let Some(key) = self.fields.get(column).map(|field| field.key) else {
            return content;
        };
        // Raised from the press until it has glided into place after the
        // release: drawn over its strip all that time (`lifted_strip`).
        let dragged = self.lifted == Some(key);
        let dx = self.column_shift(key, Instant::now());
        if dx == 0. && !dragged {
            return content;
        }
        // The scrolled columns' lane, across the header band or the rows.
        let Some(lane) = self.horizontal.as_ref().map(|handle| handle.bounds()) else {
            return content;
        };
        let band = if header {
            self.header_band.get()
        } else {
            self.list
                .as_ref()
                .map(|list| list.bounds())
                .unwrap_or_default()
        };
        let clip = Bounds::from_corners(
            point(lane.left(), band.top()),
            point(lane.right(), band.bottom()),
        );
        // The dragged column's cells go over its raised strip (`lifted_strip`),
        // which goes over the columns gliding past it.
        flip::escaping(
            super::shift::shifted(content, point(px(dx), px(0.)), 1.),
            clip,
            if dragged { 3 } else { 1 },
        )
        .into_any_element()
    }
    /// Follows the dragged header: once its middle passes a neighbour's, the
    /// two trade places at once and the one passed glides to its new place.
    pub(super) fn follow_header(
        state: &mut TableState<Self>,
        event: &DragMoveEvent<HeaderPayload>,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let animate = !cx.reduce_motion();
        let Some(drag) = state.delegate_mut().header_drag.as_mut() else {
            return;
        };
        drag.pointer = f32::from(event.event.position.x);
        let key = drag.key;
        let now = Instant::now();
        loop {
            let step = {
                let delegate = state.delegate();
                let Some(drag) = &delegate.header_drag else {
                    break;
                };
                let fields = &delegate.fields;
                let Some(index) = fields.iter().position(|field| field.key == key) else {
                    break;
                };
                let width = |at: usize| state.col_width(at).map(f32::from).unwrap_or(0.);
                let middle_of = |at: usize| {
                    drag.places.get(fields[at].key).copied().unwrap_or(0.) + width(at) / 2.
                };
                let middle = drag.reach() + width(index) / 2.;
                if index + 1 < fields.len() && middle > middle_of(index + 1) {
                    Some((index, index + 1))
                } else if index > 1 && middle < middle_of(index - 1) {
                    Some((index, index - 1))
                } else {
                    None
                }
            };
            let Some((from, to)) = step else {
                break;
            };
            let before = Self::column_places(state);
            state.delegate_mut().move_column(from, to, window, cx);
            // A move the table refused would otherwise be tried forever.
            if state.delegate().fields.get(to).map(|field| field.key) != Some(key) {
                break;
            }
            state.refresh(cx);
            let after = Self::column_places(state);
            let delegate = state.delegate_mut();
            if animate {
                for (moved, place) in &after {
                    if *moved != key
                        && let Some(old) = before.get(moved)
                        && old != place
                    {
                        delegate.flips.moved(moved, place - old, now);
                    }
                }
            }
            if let Some(drag) = delegate.header_drag.as_mut() {
                drag.places = after;
            }
            // The desktop saves the same order (`TableEvent::MoveColumn`): the
            // table keeps its layout, widths included, once the drag ends.
            let next = delegate.fields.get(to + 1).map(|field| field.key);
            delegate.process_columns.move_before(key, next);
            cx.emit(TableEvent::MoveColumn(from, to));
        }
        cx.notify();
    }
    /// Ends a header drag: the column glides from under the pointer into its place.
    pub(super) fn settle_header_drag(&mut self, animate: bool, now: Instant) {
        let Some(drag) = self.header_drag.take() else {
            return;
        };
        if animate {
            let place = drag.places.get(drag.key).copied().unwrap_or(drag.start);
            self.flips.moved(drag.key, place - drag.at(), now);
        }
    }
    /// The raised column: one strip from the top of the header to the bottom
    /// of the rows, under the column's own cells and over the others, so its
    /// edges run straight through. Placed in window coordinates; it stays
    /// within the scrolled columns (`HeaderDrag::lo`), so both edges show.
    pub(super) fn lifted_strip(&mut self, now: Instant, cx: &App) -> Option<AnyElement> {
        let key = self.lifted?;
        let index = self.fields.iter().position(|field| field.key == key)?;
        let offset = self.column_shift(key, now);
        if self.header_drag.is_none() && offset == 0. {
            self.lifted = None;
            return None;
        }
        let place = self.columns[..index]
            .iter()
            .map(|column| f32::from(column.width))
            .sum::<f32>();
        let name = self
            .columns
            .first()
            .map_or(0., |column| f32::from(column.width));
        let width = f32::from(self.columns[index].width);
        let handle = self.horizontal.as_ref()?;
        let lane = handle.bounds();
        let band = self.header_band.get();
        let bottom = self.list.as_ref()?.bounds().bottom();
        let left = lane.left() + px(place + offset - name) + handle.offset().x;
        let p = Palette::current(cx);
        Some(
            deferred(
                anchored().position(point(left, band.top())).child(
                    div()
                        .w(px(width))
                        .h(bottom - band.top())
                        .rounded(px(6.))
                        .bg(p.raised)
                        .child(
                            div()
                                .size_full()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(p.accent.opacity(0.45))
                                .bg(p.accent.opacity(0.07)),
                        ),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}
