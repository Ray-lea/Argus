//! Native virtual tables share one column model and preserve process-instance identity.
//!
//! The columns are defined in `schema` and their cells' text in `display`.
//! `delegate` is the component table's state, `tracks` settles its column
//! widths and `view` draws it for the desktop, which builds its rows in
//! `rows` (and in `width_rows` the list the widths measure). `sorting` and
//! `filters` keep the lists' orders and column filters, `totals` the figures
//! above the process columns.
use super::*;
mod body;
mod cell_text;
mod cells;
mod export;
mod groups;
/// A category heading's tint and its count's, for lists drawn like the
/// process list's groups (the history page's table).
pub(in crate::frontend) use groups::{COUNT_TINT, HEADING_TINT};
mod header;
pub(in crate::frontend) mod touch;
use header::{HeaderDrag, HeaderPayload};
mod widths;
use super::units::ByteUnit;
use gpui_kit::base::ElementExt;
use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable,
    menu::PopupMenu,
    table::{Column, ColumnFixed, ColumnSort, DataTable, TableDelegate, TableEvent, TableState},
};
use std::collections::BTreeMap;
mod delegate;
mod display;
mod filters;
mod rows;
mod schema;
mod sorting;
mod totals;
mod tracks;
mod view;
mod width_rows;
pub(super) use delegate::NativeTable;
pub(in crate::frontend) use display::{base_priority, base_priority_hint};
use display::{copied_cell, display_cell, entry_cell, pending_start_type};
use filters::FILTER_BUTTON;
pub(super) use filters::{ColumnFilter, column_filter, column_filters, saved_filters};
pub(in crate::frontend) use rows::HOLD_ORDER;
use rows::row_mark;
pub(super) use schema::fills;
use schema::{CellKind, Field, Track, cell_labels, fields, process_fields};
pub(super) use sorting::{descending_first, first_column, saved_sort, sort_preference, sortable};
pub(super) use tracks::list_widths_preference;
use tracks::{TableLayout, WidthGlide};
pub(super) use view::TableFrame;
pub(super) use width_rows::WIDTH_ROWS_EVERY;
pub(in crate::frontend) use width_rows::WidthRows;
pub(in crate::frontend) use widths::WidthRow;
// Still at the paths they had when this file held them, though nothing names
// them there.
#[allow(unused_imports)]
pub(super) use {filters::COLUMN_FILTER, sorting::initial_sort, width_rows::WidthSource};
/// Header and row geometry of the tables.
const ROW_HEIGHT: f32 = 30.;
const ROW_INSET: f32 = 4.;
const CELL_PADDING: f32 = 10.;
/// How far the rows of a user's group stand in from its heading.
const USER_INSET: f32 = 20.;
/// The process table's header band: a total above each label, the interface's
/// cell padding and the band's own separator row.
const PROCESS_HEADER_BAND: f32 = 49.;
/// The process header band at the chosen font size: its total (15 px, line
/// height 1.25) and label (12 px, 1.4) grow with the text.
fn process_header_band() -> Pixels {
    px(PROCESS_HEADER_BAND) + theme::text_growth(15., 1.25) + theme::text_growth(12., 1.4)
}
/// The process table header: 4 px above the band, its separator the last row.
pub(super) fn process_header_height() -> Pixels {
    px(4.) + process_header_band()
}
/// The table's fixed budget for `rows` process lines, header, rail and border.
pub(super) fn process_table_height(rows: usize) -> Pixels {
    process_header_height() + px(ROW_HEIGHT * rows as f32 + scrollbar::SIZE + 2.)
}
