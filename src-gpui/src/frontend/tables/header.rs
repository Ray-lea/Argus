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
impl NativeTable {
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
        self.gliding_cell(index, true, button.into_any_element())
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
