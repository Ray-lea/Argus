//! The rows of a table, with their cells drawn by Argus itself.
//!
//! The component's table builds every visible cell out of two wrappers and
//! lays each scrolled cell out as a layout root of its own, a horizontal
//! virtual list in every row: some forty layout nodes a row, on every frame
//! that touches the list. Here a row is the same row element (its fill, its
//! hover, its marks and its listeners) around one element that draws the
//! cells: the name cell's elements, laid out once, and every other cell's
//! line of text placed and painted directly (`cell_text`). Cells with marks
//! or a switch keep their elements.
//!
//! The header stays the component's. The table tells it that it has no rows
//! and hands it this body as what an empty table shows (`rows_count`,
//! `render_empty`), with the list on the scroll handles the component's rows
//! use. Those rows remain as they were for the cases this body leaves to them
//! (`NativeTable::draws_rows`): the two draw the same pixels, so that a table
//! can go from one to the other between frames.
//!
//! The geometry is the component's, to the device pixel: where its layout
//! puts the name cell, the scrolled columns' viewport and each cell in it,
//! with the same sums in the same order (`gpui::taffy`, `VirtualList`).
use super::cell_text::{CellText, Placed, round_half_toward_zero};
use super::cells::Content;
use super::*;
use gpui_kit::base::StyledExt as _;
use std::ops::Range;

/// The rows the component's table draws itself, whatever else would have
/// Argus draw them: `ARGUS_TABLE_ROWS=component`, to compare the two.
fn component_rows() -> bool {
    static FORCED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FORCED.get_or_init(|| {
        std::env::var_os("ARGUS_TABLE_ROWS").is_some_and(|rows| rows == "component")
    })
}

impl NativeTable {
    /// Whether Argus draws this frame's rows. The component draws them for
    /// assistive technology, which reads its rows and cells, and while
    /// columns trade places, where each cell is drawn apart from its column
    /// (`gliding_cell`).
    pub(super) fn draws_rows(&self, now: Instant, window: &Window) -> bool {
        !component_rows()
            && !window.is_a11y_active()
            && self.header_drag.is_none()
            && self.lifted.is_none()
            && !self.flips.active(now)
    }

    /// The table's rows as its body, for a table that draws them itself:
    /// the component's list of rows, on the same scroll handle, and over it
    /// what follows how far it is scrolled (the process list's held headings
    /// and its groups beside the scrollbar, the other lists' scrollbar). The
    /// frame draws those for the component's rows (`TableFrame`); here they
    /// are drawn by `view`, the body's own, which a glide of the list draws
    /// alone.
    fn body(
        &mut self,
        view: EntityId,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Option<AnyElement> {
        let vertical = self.vertical.clone()?;
        let list = self.list.clone()?;
        let processes = self.page == "processes";
        // The rows render here, also in frames that draw them alone: the
        // first one settles which reports its hints (`render_tr`).
        self.near_due = self.tips.begin(window);
        self.near = None;
        // Planned before the rows render, from where the list is scrolled,
        // and laid out from the frame's top as the frame lays it out.
        let groups = processes
            .then(|| {
                self.plan_groups(window, cx);
                self.group_overlay(process_header_band(), window, cx)
            })
            .flatten()
            .map(|overlay| FromFrameTop {
                child: Some(overlay),
            });
        let dark = cx.theme().mode == ThemeMode::Dark;
        let rows = uniform_list(
            "table-uniform-list",
            self.rows.len(),
            cx.processor(
                move |state: &mut TableState<Self>, visible: Range<usize>, window, cx| {
                    let frame = Rc::new(Frame::new(state, cx.entity()));
                    // The table's row that stays selected under another
                    // selection (`TableFrame::step`).
                    state.delegate_mut().note_selection(frame.selection);
                    visible
                        .filter(|index| *index < frame.rows)
                        .map(|index| row(state, index, &frame, window, cx))
                        .collect::<Vec<_>>()
                },
            ),
        )
        .flex_grow_1()
        .size_full()
        .with_sizing_behavior(ListSizingBehavior::Auto)
        .track_scroll(&vertical);
        Some(
            div()
                .id("table-body")
                .relative()
                .h_flex()
                .flex_grow_1()
                .size_full()
                .child(gpui_kit::base::ScrollBounce::new(
                    "table-scroll-bounce",
                    &vertical,
                    rows,
                ))
                .children(groups)
                .when(!processes, |body| {
                    body.child(scrollbar::scrollbar("table-scrollbar", &list, dark).view(view))
                })
                .into_any_element(),
        )
    }
}

/// The rows of a table that draws them itself, as a view of their own: a
/// glide of the list draws it alone, and the table around it, header
/// included, replays (`Window::request_isolated_animation_frame`). It keeps
/// nothing: the rows are the table's, which tells it nothing of them, so it
/// renders whenever the table does (`ViewElement::following`).
pub(super) struct TableBody {
    table: WeakEntity<TableState<NativeTable>>,
}

impl TableBody {
    pub(super) fn new(table: WeakEntity<TableState<NativeTable>>) -> Self {
        Self { table }
    }

    /// The view's element in the table, which renders it when it renders.
    pub(super) fn element(view: &Entity<Self>) -> AnyElement {
        view.clone()
            .cached(StyleRefinement::default().size_full())
            .following()
            .into_any_element()
    }
}

impl Render for TableBody {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity_id();
        self.table
            .upgrade()
            .and_then(|table| {
                table.update(cx, |state, cx| state.delegate_mut().body(view, window, cx))
            })
            .unwrap_or_else(|| div().into_any_element())
    }
}

/// What the frame draws over the rows, drawn by the body instead: laid out
/// from where the frame starts, the header's height above the body, as the
/// frame lays it out among its own children.
///
/// Where a layout rounds an edge to the device pixels depends on where the
/// edge lies from the top of the layout, as it rounds halves toward zero: a
/// centred heading half a pixel above the body's top would round down where
/// the frame, starting further up, rounds it up. Laid out from the frame's
/// top, every edge lies where it lies in the frame's layout.
struct FromFrameTop {
    child: Option<AnyElement>,
}

impl IntoElement for FromFrameTop {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FromFrameTop {
    type RequestLayoutState = ();
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // Over the whole body, taking no room in it.
        let mut style = Style {
            position: Position::Absolute,
            ..Default::default()
        };
        style.inset.top = px(0.).into();
        style.inset.left = px(0.).into();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let child = self.child.take()?;
        // The header's height as its layout has it: its padding and its
        // band, each rounded to the device pixels on its own (`to_taffy`).
        let header = window.pixel_snap(px(4.)) + window.pixel_snap(process_header_band());
        // The frame's box, which the overlay takes its place in.
        let mut frame = div()
            .relative()
            .w(bounds.size.width)
            .h(header + bounds.size.height)
            .child(child)
            .into_any_element();
        frame.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(header + bounds.size.height),
            ),
            window,
            cx,
        );
        frame.prepaint_at(bounds.origin - point(px(0.), header), window, cx);
        Some(frame)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        frame: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(frame) = frame {
            frame.paint(window, cx);
        }
    }
}

/// The component's box around a cell's content (`render_cell`): as wide as
/// its column, the table's text size, and figures aligned by their column.
fn cell_box(width: Pixels, end: bool, cx: &App) -> Div {
    div()
        .w(width)
        .flex_shrink_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_sm()
        .when(text_right(end, cx), |cell| cell.text_right())
        .relative()
}

/// Whether a column's cells align their text to the right: its figures, in
/// a layout that reads from the left.
fn text_right(end: bool, cx: &App) -> bool {
    end != gpui_kit::base::LayoutDirection::global(cx).is_rtl()
}

/// What the rows of one frame share: the table's columns as it has them now.
pub(super) struct Frame {
    table: Entity<TableState<NativeTable>>,
    horizontal: ScrollHandle,
    rows: usize,
    selection: gpui_kit::component::table::TableSelection,
    right_clicked: Option<usize>,
    /// Whether the rows fill the list's height (the component's `is_filled`).
    filled: bool,
    /// The name column's width, which stays in place.
    name: Pixels,
    /// The scrolled columns: their widths, where each starts and what they
    /// add up to, summed as the component's virtual list sums them.
    widths: Vec<Pixels>,
    origins: Vec<Pixels>,
    total: Pixels,
    /// Whether each scrolled column ends its text at the right.
    ends: Vec<bool>,
    /// Where the rows have their scrolled columns, worked out by the first
    /// row of the frame.
    lane: Cell<Option<Lane>>,
}

/// The scrolled columns across a row at `left` that is `width` wide.
#[derive(Clone, Copy)]
struct Lane {
    left: Pixels,
    width: Pixels,
    /// How far the columns are scrolled, within what there is to scroll.
    offset: Point<Pixels>,
    /// The columns in view, counted from the first scrolled one.
    first: usize,
    last: usize,
}

impl Frame {
    fn new(state: &TableState<NativeTable>, table: Entity<TableState<NativeTable>>) -> Self {
        let delegate = state.delegate();
        let columns = delegate.fields.len().min(delegate.columns.len());
        let width = |column: usize| state.col_width(column).unwrap_or_default();
        let widths: Vec<Pixels> = (1..columns).map(width).collect();
        let mut total = px(0.);
        let origins = widths
            .iter()
            .map(|width| {
                let origin = total;
                total += *width;
                origin
            })
            .collect();
        let rows = delegate.rows.len();
        let height = delegate
            .list
            .as_ref()
            .map_or(px(0.), |list| list.bounds().size.height);
        Self {
            table,
            horizontal: state.horizontal_scroll_handle.as_ref().clone(),
            rows,
            selection: state.selection(),
            right_clicked: state.right_clicked_row(),
            filled: height > px(0.) && height <= px(ROW_HEIGHT) * rows as f32,
            name: if columns > 0 { width(0) } else { px(0.) },
            widths,
            origins,
            total,
            ends: delegate
                .fields
                .iter()
                .take(columns)
                .skip(1)
                .map(|field| field.end)
                .collect(),
            lane: Cell::new(None),
        }
    }

    /// The scrolled columns in a row whose virtual list would have `content`
    /// for its viewport: the offset within what there is to scroll, kept on
    /// the handle as each of the component's rows keeps it, and the columns
    /// in view.
    fn lane(&self, row: Bounds<Pixels>, content: Bounds<Pixels>) -> Lane {
        if let Some(lane) = self
            .lane
            .get()
            .filter(|lane| lane.left == row.origin.x && lane.width == row.size.width)
        {
            return lane;
        }
        // As each row's virtual list clamps it: its cells have no height of
        // their own, which leaves the list nothing to scroll down.
        let offset = self
            .horizontal
            .offset()
            .max(&point(content.size.width - self.total, content.size.height))
            .min(&point(px(0.), px(0.)));
        if offset != self.horizontal.offset() {
            self.horizontal.set_offset(offset);
        }
        let viewport = -offset.x..-offset.x + content.size.width;
        // The first column that ends past either edge of the viewport: the
        // columns' ends grow with their place.
        let ends_past = |edge: Pixels| {
            self.origins
                .iter()
                .zip(&self.widths)
                .position(|(origin, width)| *origin + *width > edge)
                .unwrap_or(self.widths.len())
        };
        // Up to the column the viewport ends in; the component draws one
        // more past it, which its clip hides.
        let first = ends_past(viewport.start);
        let last = (ends_past(viewport.end) + 1)
            .min(self.widths.len())
            .max(first);
        let lane = Lane {
            left: row.origin.x,
            width: row.size.width,
            offset,
            first,
            last,
        };
        self.lane.set(Some(lane));
        lane
    }

    /// The field at `x` across the rows as this frame laid them out: the
    /// name column, or the scrolled column in view there; None past the
    /// last column or before any row was laid out.
    fn column_at(&self, x: Pixels) -> Option<usize> {
        let lane = self.lane.get()?;
        column_at(
            &self.origins,
            &self.widths,
            self.name,
            x - lane.left,
            lane.offset.x,
        )
    }
}

/// The field at `x` from a row's left edge: 0 for the name column `name`
/// wide, else one past the scrolled column (starting at `origins`,
/// `widths` wide, scrolled by `offset`) that holds it.
fn column_at(
    origins: &[Pixels],
    widths: &[Pixels],
    name: Pixels,
    x: Pixels,
    offset: Pixels,
) -> Option<usize> {
    if x < px(0.) {
        return None;
    }
    if x < name {
        return Some(0);
    }
    let along = x - name - offset;
    origins
        .iter()
        .zip(widths)
        .position(|(origin, width)| along >= *origin && along < *origin + *width)
        .map(|scrolled| scrolled + 1)
}

/// A row of the table: the row the component builds around the delegate's
/// (`render_table_row`), with the cells Argus draws in place of its own.
fn row(
    state: &mut TableState<NativeTable>,
    index: usize,
    frame: &Rc<Frame>,
    window: &mut Window,
    cx: &mut Context<TableState<NativeTable>>,
) -> AnyElement {
    use gpui_kit::component::table::TableSelection;
    let selected = frame.selection == TableSelection::Row(index);
    let right_clicked = frame.right_clicked == Some(index);
    let bordered = selected || index + 1 != frame.rows || !frame.filled;
    let mut tr = state.delegate_mut().render_tr(index, window, cx);
    let style = tr.style().clone();
    let theme = cx.theme();
    let (border, hover, active, outline) = (
        theme.table_row_border,
        theme.tokens.table_hover,
        if theme.list.active_highlight {
            theme.tokens.table_active
        } else {
            theme.tokens.accent
        },
        theme.selection,
    );
    tr.h_flex()
        .w_full()
        .h(px(ROW_HEIGHT))
        .when(bordered, |row| row.border_b_1().border_color(border))
        .refine_style(&style)
        .hover(move |row| {
            if selected || right_clicked {
                row
            } else {
                row.bg(hover)
            }
        })
        .child(Cells {
            frame: frame.clone(),
            row: index,
        })
        .when(selected, |row| row.bg(active))
        // The row a menu was opened on, until the menu shows its own mark.
        .when(right_clicked, |row| {
            row.border_color(gpui_kit::gpui::transparent_white()).child(
                div()
                    .top(if index == 0 { px(0.) } else { px(-1.) })
                    .left(px(0.))
                    .right(px(0.))
                    .bottom(px(-1.))
                    .absolute()
                    .border_1()
                    .border_color(outline),
            )
        })
        .on_mouse_down(MouseButton::Right, {
            let frame = frame.clone();
            cx.listener(move |state, event: &MouseDownEvent, _, cx| {
                // The menu offers to copy the cell under the pointer.
                let column = frame.column_at(event.position.x);
                let delegate = state.delegate_mut();
                delegate.context_column = column
                    .and_then(|column| delegate.fields.get(column))
                    .map(|field| field.key);
                state.set_right_clicked_row(Some(index), cx);
                cx.emit(TableEvent::RightClickedRow(Some(index)));
            })
        })
        .on_click(cx.listener(move |state, event: &ClickEvent, _, cx| {
            cx.stop_propagation();
            state.set_selected_row(index, cx);
            if event.click_count() == 2 {
                cx.emit(TableEvent::DoubleClickedRow(index));
            }
        }))
        .into_any_element()
}

/// The cells of a row.
struct Cells {
    frame: Rc<Frame>,
    row: usize,
}

/// A row's cells, placed.
#[derive(Default)]
pub(super) struct Drawn {
    name: Option<AnyElement>,
    /// What the scrolled cells show through as they paint: the row after the
    /// name column.
    lane: Bounds<Pixels>,
    cells: Vec<DrawnCell>,
}

// As with `Content`: most cells are text.
#[allow(clippy::large_enum_variant)]
enum DrawnCell {
    Text {
        cell: CellText,
        bounds: Bounds<Pixels>,
        placed: Option<Placed>,
    },
    Element(AnyElement),
}

impl IntoElement for Cells {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Cells {
    type RequestLayoutState = ();
    type PrepaintState = Drawn;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // The row's own box: the cells take their places inside it as it
        // prepaints.
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let frame = self.frame.clone();
        let row = self.row;
        let scale = window.scale_factor();
        // The row's box is on whole device pixels, as every box of a layout.
        let device = |value: Pixels| (f32::from(value) * scale).round();
        let snapped = |value: Pixels| round_half_toward_zero(f32::from(value) * scale);
        let (row_width, row_height) = (device(bounds.size.width), device(bounds.size.height));
        // The name cell, then the scrolled columns up to the space the
        // component leaves after the last one (`render_last_empty_col`).
        let name_width = snapped(frame.name).min(row_width);
        let rest = snapped(px(ROW_INSET * 2.));
        let list_right = (row_width - rest).max(name_width);
        let after_name = point(px(name_width / scale), px(0.)) + bounds.origin;
        // Where the row's virtual list of cells would be, and what its cells
        // would be clipped to as they prepaint.
        let list = Bounds::new(
            after_name,
            size(
                px((list_right - name_width) / scale),
                px(row_height / scale),
            ),
        );
        let content = Bounds::from_corners(list.origin, list.bottom_right());
        let lane = frame.lane(bounds, content);

        let table = frame.table.clone();
        let (mut name, cells) = table.update(cx, |state, cx| {
            let delegate = state.delegate_mut();
            if row >= delegate.rows.len() {
                return (None, Vec::new());
            }
            let name = (!delegate.fields.is_empty())
                .then(|| delegate.name_root(row, size(frame.name, bounds.size.height), cx));
            let columns = delegate.fields.len();
            let cells = (lane.first..lane.last)
                .filter(|scrolled| scrolled + 1 < columns)
                .map(|scrolled| (scrolled, delegate.cell(row, scrolled + 1, window, cx)))
                .collect::<Vec<_>>();
            (name, cells)
        });
        if let Some(name) = name.as_mut() {
            name.layout_as_root(
                size(
                    AvailableSpace::Definite(bounds.size.width),
                    AvailableSpace::Definite(bounds.size.height),
                ),
                window,
                cx,
            );
            name.prepaint_at(bounds.origin, window, cx);
        }

        // Each cell is a layout root of its own in the component: its place
        // snaps to the device pixels by itself, and it is as wide as its
        // column's width snaps to.
        let cell_height = round_half_toward_zero(f32::from(content.size.height) * scale);
        let cells = window.with_content_mask(Some(ContentMask { bounds: list }), |window| {
            cells
                .into_iter()
                .map(|(scrolled, cell)| {
                    let origin = content.origin
                        + point(frame.origins[scrolled] + lane.offset.x, lane.offset.y);
                    match cell {
                        Content::Text(mut cell) => {
                            let bounds = Bounds::new(
                                window.pixel_snap_point(origin),
                                size(
                                    px(snapped(frame.widths[scrolled]) / scale),
                                    px(cell_height / scale),
                                ),
                            );
                            // Of what the cell's box gives its content
                            // (`cell_box`), the text reads the alignment.
                            if text_right(frame.ends[scrolled], cx) {
                                cell.align(TextAlign::Right);
                            }
                            let placed = window
                                .with_content_mask(Some(ContentMask { bounds }), |window| {
                                    cell.settle(bounds, 0., window, cx)
                                });
                            DrawnCell::Text {
                                cell,
                                bounds,
                                placed,
                            }
                        }
                        Content::Element(content_element) => {
                            // The component's two boxes around the content.
                            let mut element = div()
                                .h_flex()
                                .h_full()
                                .child(
                                    cell_box(frame.widths[scrolled], frame.ends[scrolled], cx)
                                        .h_full()
                                        .child(content_element),
                                )
                                .into_any_element();
                            element.layout_as_root(
                                size(
                                    AvailableSpace::Definite(frame.widths[scrolled]),
                                    AvailableSpace::Definite(content.size.height),
                                ),
                                window,
                                cx,
                            );
                            element.prepaint_at(origin, window, cx);
                            DrawnCell::Element(element)
                        }
                    }
                })
                .collect()
        });
        Drawn {
            name,
            lane: Bounds::new(
                after_name,
                size(px((row_width - name_width) / scale), px(row_height / scale)),
            ),
            cells,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        drawn: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(name) = drawn.name.as_mut() {
            name.paint(window, cx);
        }
        window.with_content_mask(Some(ContentMask { bounds: drawn.lane }), |window| {
            for cell in &mut drawn.cells {
                match cell {
                    DrawnCell::Text {
                        cell,
                        bounds,
                        placed,
                    } => {
                        let bounds = *bounds;
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            cell.draw(bounds, placed.as_ref(), window, cx)
                        });
                    }
                    DrawnCell::Element(element) => element.paint(window, cx),
                }
            }
        });
    }
}

/// Which way the keyboard moves the selected row.
#[derive(Clone, Copy)]
pub(super) enum Step {
    Up,
    Down,
    First,
    Last,
    PageDown,
}

impl NativeTable {
    /// Keeps the row the table had selected last: it stays the table's row
    /// under a selected column, and the keys go on from it.
    pub(super) fn note_selection(&mut self, selection: gpui_kit::component::table::TableSelection) {
        use gpui_kit::component::table::TableSelection;
        match selection {
            TableSelection::Row(row) => self.selected_row = Some(row),
            TableSelection::None => self.selected_row = None,
            _ => {}
        }
    }

    /// Moves the selection of a table that draws its own rows, as the
    /// component's table moves it among the rows it has.
    pub(super) fn step(
        state: &mut TableState<Self>,
        step: Step,
        cx: &mut Context<TableState<Self>>,
    ) {
        use gpui_kit::component::table::TableSelection;
        let selection = state.selection();
        state.delegate_mut().note_selection(selection);
        let delegate = state.delegate();
        let (rows, columns) = (delegate.rows.len(), delegate.columns.len());
        let selected = delegate.selected_row;
        let row_mode = matches!(selection, TableSelection::Row(_) | TableSelection::None);
        match step {
            Step::Up | Step::Down if rows == 0 => {}
            Step::Up => state.set_selected_row(
                match selected {
                    Some(row) if row > 0 => row - 1,
                    _ => rows - 1,
                },
                cx,
            ),
            Step::Down => state.set_selected_row(
                match selected {
                    Some(row) if row + 1 < rows => row + 1,
                    _ => 0,
                },
                cx,
            ),
            Step::First if row_mode => {
                if rows > 0 {
                    state.set_selected_row(0, cx);
                }
            }
            Step::First => state.set_selected_col(0, cx),
            Step::Last if row_mode => {
                if rows > 0 {
                    state.set_selected_row(rows - 1, cx);
                }
            }
            Step::Last => state.set_selected_col(columns.saturating_sub(1), cx),
            Step::PageDown => {
                if rows == 0 {
                    return;
                }
                // The rows a page holds: the table's height below its header
                // row, as the component counts them.
                let delegate = state.delegate();
                let header = if delegate.page == "processes" {
                    process_header_band()
                } else {
                    px(34.) + theme::text_growth(12., 1.4)
                };
                let table = delegate.header_band.get().size.height
                    + delegate
                        .list
                        .as_ref()
                        .map_or(px(0.), |list| list.bounds().size.height);
                let page =
                    (((table - header).max(px(0.)) / px(ROW_HEIGHT)).floor() as usize).max(1);
                state.set_selected_row((selected.unwrap_or(0) + page).min(rows - 1), cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::column_at;
    use gpui_kit::gpui::{Pixels, px};

    fn at(x: f32, offset: f32) -> Option<usize> {
        let widths: Vec<Pixels> = vec![px(70.), px(100.), px(50.)];
        let origins: Vec<Pixels> = vec![px(0.), px(70.), px(170.)];
        column_at(&origins, &widths, px(200.), px(x), px(offset))
    }

    #[test]
    fn the_pointer_finds_the_name_or_the_scrolled_column_under_it() {
        assert_eq!(at(-1., 0.), None);
        assert_eq!(at(0., 0.), Some(0));
        assert_eq!(at(199., -40.), Some(0));
        assert_eq!(at(200., 0.), Some(1));
        assert_eq!(at(269., 0.), Some(1));
        assert_eq!(at(270., 0.), Some(2));
        assert_eq!(at(419., 0.), Some(3));
        assert_eq!(at(420., 0.), None);
        // Scrolled 80 px to the left: the second column shows first.
        assert_eq!(at(200., -80.), Some(2));
        assert_eq!(at(339., -80.), Some(3));
        assert_eq!(at(340., -80.), None);
    }
}
