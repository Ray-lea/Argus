//! Native virtual tables share one column model and preserve process-instance identity.
use super::*;
mod cells;
mod header;
use header::{HeaderDrag, HeaderPayload};
mod widths;
use gpui_kit::base::ElementExt;
use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable,
    menu::PopupMenu,
    table::{Column, ColumnFixed, ColumnSort, DataTable, TableDelegate, TableEvent, TableState},
};

#[derive(Clone, Copy, PartialEq)]
enum CellKind {
    Dim,
    Mono,
    MonoDim,
    Count,
    Pid,
    ServicePid,
    Port,
    PortDim,
    Percent,
    Memory,
    Rate,
    Name,
    Protocol,
    ConnectionState,
    ServiceState,
    StartType,
    Enabled,
    StartupLocation,
    ProcessText,
    Identifier,
    NetworkRate,
    Duration,
    Timestamp,
    ProcessBoolean,
    AddressSpace,
}
/// A CSS grid track from the tables: a fixed width or `minmax(min, fr)`.
#[derive(Clone, Copy, PartialEq)]
enum Track {
    Fixed(f32),
    Flex(f32, f32),
}
#[derive(Clone)]
struct Field {
    key: &'static str,
    label: String,
    kind: CellKind,
    track: Track,
    end: bool,
}

struct TableLayout<'a> {
    available: Pixels,
    columns: &'a columns::Preferences,
    totals: HashMap<String, String>,
    bits: bool,
    scope: (&'a str, &'a str),
}
/// Header and row geometry of the tables.
const ROW_HEIGHT: f32 = 30.;
const ROW_INSET: f32 = 4.;
const CELL_PADDING: f32 = 10.;
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
fn fields(page: &str, network: bool, _available: f32) -> Vec<Field> {
    use CellKind::*;
    use Track::*;
    let definitions: Vec<(&str, &str, CellKind, Track)> = match page {
        "services" => vec![
            (
                "displayName",
                "services.column.display",
                Name,
                Flex(220., 1.3),
            ),
            ("name", "common.name", Dim, Flex(110., 0.6)),
            ("pid", "processes.column.pid.label", ServicePid, Fixed(72.)),
            ("state", "common.status", ServiceState, Fixed(104.)),
            (
                "startType",
                "services.column.startType",
                StartType,
                Fixed(160.),
            ),
            (
                "description",
                "services.column.description",
                Dim,
                Flex(200., 1.6),
            ),
        ],
        "startup" => vec![
            ("name", "common.name", Name, Flex(200., 1.2)),
            (
                "publisher",
                "startup.column.publisher",
                Dim,
                Flex(140., 0.8),
            ),
            ("enabled", "common.status", Enabled, Fixed(116.)),
            (
                "location",
                "startup.column.location",
                StartupLocation,
                Flex(150., 0.8),
            ),
            ("command", "common.command", MonoDim, Flex(220., 1.6)),
        ],
        "network" => vec![
            ("name", "connections.column.process", Name, Flex(180., 1.1)),
            ("pid", "processes.column.pid.label", Pid, Fixed(72.)),
            (
                "protocol",
                "connections.column.protocol",
                Protocol,
                Fixed(84.),
            ),
            (
                "localAddress",
                "connections.column.localAddress",
                Mono,
                Flex(130., 0.9),
            ),
            (
                "localPort",
                "connections.column.localPort",
                Port,
                Fixed(88.),
            ),
            (
                "remoteAddress",
                "connections.column.remoteAddress",
                MonoDim,
                Flex(130., 0.9),
            ),
            (
                "remotePort",
                "connections.column.remotePort",
                PortDim,
                Fixed(88.),
            ),
            ("state", "common.status", ConnectionState, Fixed(96.)),
        ],
        _ => return process_fields(&columns::Preferences::default(), network),
    };
    definitions
        .into_iter()
        .map(|(key, title, kind, track)| Field {
            key,
            label: label(title),
            kind,
            track,
            end: matches!(
                kind,
                Count | Pid | ServicePid | Port | PortDim | Percent | Memory | Rate
            ),
        })
        .collect()
}

fn process_fields(preferences: &columns::Preferences, network: bool) -> Vec<Field> {
    use columns::Format;
    preferences
        .visible(network)
        .into_iter()
        .map(|column| {
            let kind = match column.format {
                Format::Name => CellKind::Name,
                Format::Text => CellKind::ProcessText,
                Format::Identifier if column.key == "pid" => CellKind::Pid,
                Format::Identifier => CellKind::Identifier,
                Format::Count => CellKind::Count,
                Format::Percent => CellKind::Percent,
                Format::Memory => CellKind::Memory,
                Format::AddressSpace => CellKind::AddressSpace,
                Format::Rate => CellKind::Rate,
                Format::Network => CellKind::NetworkRate,
                Format::Duration => CellKind::Duration,
                Format::Timestamp => CellKind::Timestamp,
                Format::Boolean => CellKind::ProcessBoolean,
            };
            Field {
                key: column.key,
                label: label(&format!("processes.column.{}.label", column.key)),
                kind,
                track: if !preferences.auto_width {
                    Track::Fixed(
                        preferences
                            .widths
                            .get(column.key)
                            .copied()
                            .unwrap_or(column.width),
                    )
                } else if column.key == "name" {
                    Track::Flex(column.width, 1.)
                } else {
                    Track::Fixed(column.width)
                },
                end: !matches!(
                    column.format,
                    Format::Name | Format::Text | Format::Timestamp | Format::Boolean
                ),
            }
        })
        .collect()
}
fn cell(row: &Row, field: &Field) -> String {
    let raw = &row.raw;
    let missing = || label("format.missing");
    match field.kind {
        CellKind::Name => row.title.clone(),
        CellKind::ProcessText => row
            .column_text(field.key)
            .map(str::to_owned)
            .unwrap_or_else(missing),
        CellKind::Identifier => row
            .metric(field.key)
            .map(|value| format!("{value:.0}"))
            .unwrap_or_else(missing),
        CellKind::Duration => row
            .metric(field.key)
            .map(pages::cpu_time)
            .unwrap_or_else(missing),
        CellKind::Timestamp => row
            .metric(field.key)
            .map(pages::date_time)
            .unwrap_or_else(missing),
        CellKind::AddressSpace => row
            .metric(field.key)
            .map(|value| theme::bytes(value, false))
            .unwrap_or_else(missing),
        CellKind::ProcessBoolean => row
            .metric(field.key)
            .map(|value| {
                label(match (field.key, value > 0.) {
                    ("suspended", true) => "processes.columns.yes",
                    ("suspended", false) => "processes.columns.no",
                    (_, true) => "common.on",
                    (_, false) => "common.off",
                })
            })
            .unwrap_or_else(missing),
        CellKind::Dim | CellKind::Mono | CellKind::MonoDim => {
            let value = &raw[field.key];
            value.as_str().map(str::to_owned).unwrap_or_else(|| {
                if !value.is_null() {
                    value.to_string()
                } else if field.key == "publisher" {
                    missing()
                } else {
                    String::new()
                }
            })
        }
        CellKind::Pid => {
            if row.members.len() > 1 || row.section.is_some() {
                String::new()
            } else {
                raw["pid"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            }
        }
        // A stopped service has no process.
        CellKind::ServicePid => raw["pid"]
            .as_u64()
            .filter(|pid| *pid != 0)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        CellKind::Count => row
            .metric(field.key)
            .map(theme::count)
            .unwrap_or_else(missing),
        CellKind::Percent => row
            .metric(field.key)
            .map(theme::cpu)
            .unwrap_or_else(missing),
        CellKind::Memory => row
            .metric(field.key)
            .map(|value| {
                if field.key.starts_with("io") && field.key.ends_with("Total") {
                    theme::bytes(value, false)
                } else {
                    theme::memory(value)
                }
            })
            .unwrap_or_else(missing),
        CellKind::Rate | CellKind::NetworkRate => row
            .metric(field.key)
            .map(|v| theme::bytes(v, true))
            .unwrap_or_else(missing),
        CellKind::ServiceState => label(&format!("services.state.{}", text(raw, "state"))),
        CellKind::StartType => {
            if raw["startType"].is_null() {
                missing()
            } else {
                label(&format!("services.startType.{}", text(raw, "startType")))
            }
        }
        CellKind::Enabled => label(if text(raw, "blocked") == "unavailable" {
            "startup.unavailable"
        } else if boolean(raw, "enabled") {
            "common.enabled"
        } else {
            "common.disabled"
        }),
        CellKind::StartupLocation => label(&format!("startup.location.{}", text(raw, "location"))),
        CellKind::Protocol => text(raw, "protocol").to_uppercase(),
        CellKind::Port | CellKind::PortDim => raw[field.key]
            .as_u64()
            .map(|v| v.to_string())
            .unwrap_or_default(),
        CellKind::ConnectionState => {
            let state = text(raw, "state");
            if state.is_empty() {
                String::new()
            } else {
                label(&format!("connections.state.{state}"))
            }
        }
    }
}

fn display_cell(row: &Row, field: &Field, bits: bool) -> String {
    if field.kind == CellKind::NetworkRate {
        row.metric(field.key)
            .map(|value| {
                if value < 1. {
                    "0".into()
                } else {
                    theme::network(value, bits)
                }
            })
            .unwrap_or_else(|| label("format.missing"))
    } else {
        cell(row, field)
    }
}
pub(super) struct NativeTable {
    owner: WeakEntity<Desktop>,
    page: String,
    rows: Arc<Vec<Row>>,
    fields: Vec<Field>,
    columns: Vec<Column>,
    icons: HashMap<u32, Arc<Image>>,
    available: Pixels,
    process_columns: columns::Preferences,
    font_size: Pixels,
    locale: argus_core::i18n::Locale,
    bits: bool,
    scope: (String, String),
    totals: HashMap<String, String>,
    auto_widths: widths::AutoWidths,
    cell_values: widths::CellValues,
    horizontal: Option<ScrollHandle>,
    /// A row the desktop selected itself; the table's select event for it is ignored.
    echo: Option<usize>,
    /// Cell hints (`tips`): the row under the pointer reports its hinted
    /// parts, read from the list's scroll handle once this render lays out rows.
    tips: tips::Tips,
    list: Option<ScrollHandle>,
    near: Option<usize>,
    near_due: bool,
    /// Why disk readings are missing, as of a sample.
    disk_hint: RefCell<Option<(u64, String)>>,
    /// A process header being dragged: its column follows the pointer while
    /// the others make room, gliding to their new places (`flip`).
    header_drag: Option<HeaderDrag>,
    /// Columns gliding to new places after the dragged one passed them.
    flips: flip::Flips,
    /// The header band, which clips gliding header cells.
    header_band: Rc<Cell<Bounds<Pixels>>>,
    /// The column raised under the pointer, until it has glided into place.
    lifted: Option<&'static str>,
}
pub(super) struct TableFrame {
    state: Entity<TableState<NativeTable>>,
}
impl Render for TableFrame {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Process headers carry a total above the label; the other tables use the
        // 34 px header cell. Both sit in a 4 px padded header row.
        let state = self.state.read(cx);
        let rows = state.vertical_scroll_handle.0.borrow().base_handle.clone();
        let table = self.state.entity_id();
        let dark = cx.theme().mode == ThemeMode::Dark;
        let processes = state.delegate().page == "processes";
        let columns = state.horizontal_scroll_handle.as_ref().clone();
        div()
            .relative()
            .size_full()
            // The process table keeps a rail for sideways scrolling below its
            // rows, even when the columns fit (`.hscroll`).
            .when(processes, |d| d.pb(px(scrollbar::SIZE)))
            .child(scrollbar::wheel_glide(&rows, &columns, table))
            .child(
                DataTable::new(&self.state)
                    .small()
                    .header_row_height(if processes {
                        process_header_band()
                    } else {
                        px(34.) + theme::text_growth(12., 1.4)
                    })
                    .bordered(false)
                    .fixed_cols_border(false)
                    .stripe(false)
                    // The interface's scrollbars are drawn below.
                    .scrollbar_visible(false, false),
            )
            // The process page draws the list's bar itself (`Desktop::table_scrollbar`).
            .when(!processes, |d| {
                d.child(scrollbar::scrollbar("table-scrollbar", &rows, dark).view(table))
            })
            .when(processes, |d| {
                d.child(
                    scrollbar::scrollbar("table-columns-scrollbar", &columns, dark)
                        .rail_below(&rows)
                        .view(table),
                )
            })
    }
}
impl NativeTable {
    fn new(owner: WeakEntity<Desktop>, tips: tips::Tips) -> Self {
        Self {
            owner,
            tips,
            list: None,
            near: None,
            near_due: false,
            disk_hint: RefCell::new(None),
            header_drag: None,
            flips: Default::default(),
            header_band: Rc::new(Cell::new(Bounds::default())),
            lifted: None,
            page: String::new(),
            rows: Arc::new(vec![]),
            fields: vec![],
            columns: vec![],
            icons: HashMap::new(),
            available: px(0.),
            process_columns: columns::Preferences::default(),
            font_size: px(0.),
            locale: argus_core::i18n::locale(),
            bits: false,
            scope: (String::new(), String::new()),
            totals: HashMap::new(),
            auto_widths: Default::default(),
            cell_values: HashMap::new(),
            horizontal: None,
            echo: None,
        }
    }
    fn configure(
        &mut self,
        page: &str,
        rows: Arc<Vec<Row>>,
        network: bool,
        layout: TableLayout<'_>,
        sort: (&str, bool),
        cx: &App,
    ) -> bool {
        let TableLayout {
            available,
            columns: preferences,
            totals,
            bits,
            scope,
        } = layout;
        let rows_changed = !Arc::ptr_eq(&self.rows, &rows);
        let totals_changed = self.totals != totals;
        let locale = argus_core::i18n::locale();
        if rows_changed || bits != self.bits || self.locale != locale {
            self.cell_values.clear();
        }
        self.rows = rows;
        self.totals = totals;
        self.bits = bits;
        if self.page == page && !self.fields.is_empty() && cx.has_active_drag() {
            return false;
        }
        let mut fields = if page == "processes" {
            process_fields(preferences, network)
        } else {
            fields(page, network, f32::from(available))
        };
        let font_size = theme::font(12.);
        let same_layout = self.page == page
            && self.available == available
            && self.font_size == font_size
            && self.locale == locale
            && self.scope.0 == scope.0
            && self.scope.1 == scope.1
            && (page != "processes" || &self.process_columns == preferences)
            && self
                .fields
                .iter()
                .map(|f| (&f.key, &f.label))
                .eq(fields.iter().map(|f| (&f.key, &f.label)));
        let gliding = self
            .horizontal
            .as_ref()
            .is_some_and(|h| h.wheel_target() != h.offset());
        if same_layout && (!preferences.auto_width || (!rows_changed && !totals_changed) || gliding)
        {
            return false;
        }
        // Widths last through window sizes, views, pages and column order:
        // only another text size or language measures them anew.
        if self.font_size != font_size || self.locale != locale {
            self.auto_widths.reset();
        }
        self.locale = locale;
        self.scope = (scope.0.into(), scope.1.into());
        self.available = available;
        self.font_size = font_size;
        self.process_columns = preferences.clone();
        self.page = page.into();
        if page == "processes" && preferences.auto_width {
            let (first, count) = self
                .list
                .as_ref()
                .map(|list| {
                    let height = f32::from(list.bounds().size.height);
                    (
                        (f32::from(-list.offset().y).max(0.) / ROW_HEIGHT) as usize,
                        if height > 0. {
                            (height / ROW_HEIGHT).ceil() as usize + 4
                        } else {
                            64
                        },
                    )
                })
                .unwrap_or((0, 64));
            self.auto_widths.apply(
                &mut fields,
                widths::Contents {
                    rows: &self.rows,
                    range: first..(first + count.min(96)).min(self.rows.len()),
                    totals: &self.totals,
                    values: &mut self.cell_values,
                    bits,
                },
                cx,
            );
        }
        let changed = !same_layout
            || !self
                .fields
                .iter()
                .map(|field| field.track)
                .eq(fields.iter().map(|field| field.track));
        self.fields = fields;
        if !changed {
            return false;
        }
        // Resolve the grid tracks like CSS: fixed tracks first, then the flexible
        // ones share what is left by their fractions without going below their minimum.
        // The rows sit 4 px in from each side and the scrollbar keeps its gutter.
        let room = f32::from(available) - ROW_INSET * 2. - CELL_PADDING;
        let fixed: f32 = self
            .fields
            .iter()
            .map(|f| match f.track {
                Track::Fixed(width) => width,
                Track::Flex(..) => 0.,
            })
            .sum();
        let mut flexible: Vec<usize> = (0..self.fields.len())
            .filter(|&i| matches!(self.fields[i].track, Track::Flex(..)))
            .collect();
        let mut widths: Vec<f32> = self
            .fields
            .iter()
            .map(|f| match f.track {
                Track::Fixed(width) => width,
                Track::Flex(minimum, _) => minimum,
            })
            .collect();
        let mut free = (room - fixed).max(0.);
        loop {
            let fractions: f32 = flexible
                .iter()
                .map(|&i| match self.fields[i].track {
                    Track::Flex(_, fr) => fr,
                    Track::Fixed(_) => 0.,
                })
                .sum();
            if fractions <= 0. {
                break;
            }
            let unit = free / fractions;
            // A track whose share falls under its minimum keeps the minimum and
            // leaves the distribution, like `minmax()` in a grid.
            let clamped: Vec<usize> = flexible
                .iter()
                .copied()
                .filter(|&i| match self.fields[i].track {
                    Track::Flex(minimum, fr) => unit * fr < minimum,
                    Track::Fixed(_) => false,
                })
                .collect();
            if clamped.is_empty() {
                for &i in &flexible {
                    if let Track::Flex(_, fr) = self.fields[i].track {
                        widths[i] = unit * fr;
                    }
                }
                break;
            }
            for i in clamped {
                free -= widths[i];
                flexible.retain(|&f| f != i);
            }
            free = free.max(0.);
        }
        self.columns = self
            .fields
            .iter()
            .zip(widths)
            .enumerate()
            .map(|(index, (field, width))| {
                let column = Column::new(field.key, field.label.clone())
                    .width(width)
                    .min_width(48.)
                    .resizable(page != "processes" || !preferences.auto_width)
                    .movable(page == "processes" && field.key != "name")
                    .paddings(Edges::all(px(0.)))
                    .sort(if field.key == sort.0 {
                        if sort.1 {
                            ColumnSort::Descending
                        } else {
                            ColumnSort::Ascending
                        }
                    } else {
                        ColumnSort::Default
                    });
                if index == 0 {
                    column.fixed(ColumnFixed::Left)
                } else if field.end {
                    column.text_right()
                } else {
                    column
                }
            })
            .collect();
        true
    }
}
impl TableDelegate for NativeTable {
    fn move_column(
        &mut self,
        from: usize,
        to: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        if self.page != "processes"
            || from == 0
            || to == 0
            || from >= self.fields.len()
            || to >= self.fields.len()
        {
            return;
        }
        let field = self.fields.remove(from);
        self.fields.insert(to, field);
        let column = self.columns.remove(from);
        self.columns.insert(to, column);
    }
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }
    fn rows_count(&self, _cx: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, index: usize, _cx: &App) -> Column {
        self.columns[index].clone()
    }
    fn perform_sort(
        &mut self,
        index: usize,
        sort: ColumnSort,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let key = self.fields[index].key.to_owned();
        if !sortable(&self.page, &key) {
            return;
        }
        let owner = self.owner.clone();
        window.defer(cx, move |_, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.sort = key;
                this.descending = sort == ColumnSort::Descending;
                if let Some((key, value)) = sort_preference(&this.page, &this.sort, this.descending)
                {
                    this.prefs.set(&key, value);
                }
                this.rebuild_rows(false, cx);
                cx.notify();
            });
        });
    }
    fn render_header(
        &mut self,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        // The rows render after the header, laid out where the list scrolled
        // to this frame: the first one settles which reports its hints.
        self.near_due = self.tips.begin(window);
        self.near = None;
        // A search narrows the process list: an accent header with the card's
        // accent ring (`.workspace.filtered .thead`), rounded into the card's
        // corner, and the right one too while the details are closed.
        let filtered = (self.page == "processes")
            .then(|| self.owner.upgrade())
            .flatten()
            .map(|owner| {
                let app = owner.read(cx);
                (!app.query.trim().is_empty(), app.details_shown())
            });
        let p = Palette::current(cx);
        let now = Instant::now();
        // A drag released away from the header (no drop here) settles too.
        if self.header_drag.is_some() && !cx.has_active_drag() {
            self.settle_header_drag(!cx.reduce_motion(), now);
        }
        if self.header_drag.is_some() || self.flips.active(now) {
            window.request_animation_frame();
        }
        let band = self.header_band.clone();
        let strip = self.lifted_strip(now, cx);
        // The process header band ends in its own separator row, so the 4 px
        // below the labels are inside it (see `render_th`): no bottom padding.
        // While filtering, the workspace draws the accent separator over that
        // row (`process_header_height`).
        div()
            .id("native-table-header")
            .on_prepaint(move |bounds, _, _| band.set(bounds))
            .children(strip)
            .on_drag_move(
                cx.listener(|state, event: &DragMoveEvent<HeaderPayload>, window, cx| {
                    Self::follow_header(state, event, window, cx)
                }),
            )
            .on_drop(cx.listener(|state, _: &HeaderPayload, _, cx| {
                let animate = !cx.reduce_motion();
                state
                    .delegate_mut()
                    .settle_header_drag(animate, Instant::now());
                cx.notify();
            }))
            .when(self.page == "processes", |d| {
                let owner = self.owner.clone();
                d.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                    cx.stop_propagation();
                    let owner = owner.clone();
                    let at = event.position;
                    window.defer(cx, move |window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            // The column under the pointer, for its own items.
                            let column = this
                                .header_bounds
                                .borrow()
                                .iter()
                                .find(|(_, bounds)| bounds.contains(&at))
                                .map(|(key, _)| key.clone());
                            this.process_header_menu(column, None, window, cx)
                        });
                    });
                })
            })
            .px(px(ROW_INSET))
            .pt(px(4.))
            .when(filtered.is_none(), |d| d.pb(px(4.)))
            .when_some(filtered.filter(|(on, _)| *on), |d, (_, details)| {
                d.bg(p.accent.opacity(0.08))
                    .rounded_tl(px(7.))
                    .when(!details, |d| d.rounded_tr(px(7.)))
            })
    }
    /// The space after the last column: the columns leave room for the
    /// header's insets and the scrollbar gutter (see `configure`), and with the
    /// library's 12 px default here the header would scroll sideways by 4 px
    /// even when the columns fit, showing a horizontal thumb.
    fn render_last_empty_col(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div().w(px(ROW_INSET * 2.)).h_full().flex_shrink_0()
    }
    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let p = Palette::current(cx);
        let owner = self.owner.clone();
        let (message, clear) = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).empty_message())
            .unwrap_or_default();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .text_color(p.muted)
            .child(message)
            .when_some(clear, |d, text| {
                d.child(
                    Button::new("table-clear-filter")
                        .px_3()
                        .h(px(32.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(p.stroke)
                        .bg(p.control)
                        .text_color(p.ink)
                        .hover(move |s| s.bg(p.hover))
                        .child(text)
                        .on_click(move |_, window, cx| {
                            let owner = owner.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.preference("clearSearch", Value::Null, window, cx);
                                    let page = this.page.clone();
                                    this.set_filter(&page, "all", cx);
                                });
                            });
                        }),
                )
            })
    }
    fn render_th(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let field = &self.fields[index];
        let p = Palette::current(cx);
        let end = field.end;
        // Process headers: the interface's 4 px cell padding inside its 4 px
        // header padding below the label, which the header band leaves out.
        let processes = self.page == "processes";
        let mut cell = div()
            .size_full()
            .px(px(CELL_PADDING))
            .pt(px(4.))
            .pb(px(if processes { 8. } else { 4. }))
            .flex()
            .flex_col()
            .justify_end()
            .when(end, |d| d.items_end())
            .when(!end, |d| d.items_start());
        if self.page != "processes" {
            cell = cell.justify_center();
        }
        if self.page == "processes"
            && let Some(owner) = self.owner.upgrade()
        {
            let app = owner.read(cx);
            let total = self.totals.get(field.key).cloned().unwrap_or_default();
            cell = cell.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(theme::font(15.))
                    .line_height(relative(1.25))
                    .font_family(theme::display_font(cx))
                    .text_color(p.ink)
                    .whitespace_nowrap()
                    .max_w_full()
                    .overflow_hidden()
                    .when(field.key == "name" && !app.query.trim().is_empty(), |d| {
                        let tip = label("processes.filtered");
                        d.child(
                            div()
                                .id("filter-mark")
                                .flex()
                                .flex_shrink_0()
                                .child(icon("FilterIcon", p.accent).size(px(13.)))
                                .tooltip(move |window, cx| {
                                    widgets::tooltip(tip.clone(), window, cx)
                                }),
                        )
                    })
                    .child(total),
            );
        }
        let owner = self.owner.clone();
        let key = field.key.to_owned();
        let sort = owner
            .upgrade()
            .map(|owner| {
                let app = owner.read(cx);
                (app.sort == key, app.descending)
            })
            .unwrap_or_default();
        let label = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .min_w_0()
            .max_w_full()
            .text_size(theme::font(12.))
            .line_height(relative(1.4))
            .when(processes, |d| d.mt(px(1.)))
            .whitespace_nowrap()
            .text_color(if sort.0 { p.ink } else { p.muted })
            .when(sort.0, |d| d.font_weight(theme::LABEL_WEIGHT))
            .child(div().min_w_0().truncate().child(field.label.clone()))
            .when(sort.0, |d| {
                d.child(
                    icon(if sort.1 { "ChevronDown" } else { "ChevronUp" }, p.ink)
                        .size(px(12.))
                        .flex_shrink_0(),
                )
            });
        let hover = p.hover;
        let press = p.press;
        let header_bounds = owner
            .upgrade()
            .map(|owner| owner.read(cx).header_bounds.clone());
        let measured_key = key.clone();
        // A column the table cannot sort by has a disabled header: no
        // hover or press fill, and a click does nothing (`.th:disabled`).
        let sortable = sortable(&self.page, &key);
        let button = Button::new(("column-sort", index))
            .size_full()
            .min_w_0()
            .rounded(px(4.))
            .bg(gpui_kit::gpui::transparent_black())
            .on_prepaint(move |bounds, _, _| {
                if let Some(headers) = &header_bounds {
                    headers.borrow_mut().insert(measured_key.clone(), bounds);
                }
            })
            .when(sortable, |button| {
                button
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(press))
            })
            .disabled(!sortable)
            .accessibility_label(field.label.clone())
            .when(self.page == "processes", |button| {
                let title = theme::label(&format!("processes.column.{key}.title"));
                button.tooltip(move |window, cx| widgets::tooltip(title.clone(), window, cx))
            })
            .child(cell.child(label))
            .on_click(move |_, window, cx| {
                let owner = owner.clone();
                let key = key.clone();
                window.defer(cx, move |window, cx| {
                    let _ = owner.update(cx, |this, cx| this.act(Action::Sort(key), window, cx));
                });
            });
        self.header_cell(index, button, cx)
    }
    fn render_td(
        &mut self,
        row_index: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let content = self.td_content(row_index, column, window, cx);
        self.gliding_cell(column, false, content)
    }
    fn render_tr(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        if std::mem::take(&mut self.near_due) {
            self.near = self
                .list
                .as_ref()
                .and_then(|list| tips::row_under(list, ROW_HEIGHT, self.rows.len(), window));
        }
        let p = Palette::current(cx);
        let row = &self.rows[index];
        let (selected, fresh, searching) = self
            .owner
            .upgrade()
            .map(|owner| {
                let app = owner.read(cx);
                (
                    app.selected
                        .get(&self.page)
                        .is_some_and(|key| *key == row.key),
                    self.page == "processes" && app.is_fresh(row),
                    !app.query.trim().is_empty(),
                )
            })
            .unwrap_or_default();
        // A heading cannot fold while a search shows every match; its row says so.
        let locked = (row.section.is_some() && searching && self.is_near(index))
            .then(|| label("processes.sectionSearching"));
        // Pinned rows lead the list; a line under the last one ends them.
        let last_pinned = row.pinned && self.rows.get(index + 1).is_some_and(|next| !next.pinned);
        // A double-click folds a group (the table reports the first, click
        // count 2). Another double-click right after it arrives as clicks 3 and
        // 4 within the system's double-click time and would be lost until that
        // time passed: every completed pair folds again. A category heading
        // folds on every press instead (its select event), as the interface's
        // `onClick`: no pair folds it again.
        let refold =
            (self.page == "processes" && row.expandable && row.section.is_none()).then(|| {
                let owner = self.owner.clone();
                let key = row.key.clone();
                move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let count = event.click_count();
                    if count >= 4 && count.is_multiple_of(2) {
                        let owner = owner.clone();
                        let key = key.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = owner
                                .update(cx, |this, cx| this.act(Action::Fold(key), window, cx));
                        });
                    }
                }
            });
        let tr = div()
            .id(("native-row", index))
            .relative()
            .border_b_0()
            .rounded(px(4.));
        let tr = match locked {
            Some(hint) => self
                .tips
                .target(tr, tips::id((&self.page, &row.key, "row")), hint),
            None => tr,
        };
        tr.when_some(refold, |d, refold| d.on_click(refold))
            .when(fresh, |d| d.bg(p.network.opacity(0.15)))
            .when(selected, |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(8.))
                        .bottom(px(8.))
                        .w(px(3.))
                        .rounded(px(2.))
                        .bg(p.accent),
                )
            })
            .when(last_pinned, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .right(px(8.))
                        .bottom(px(-1.))
                        .h(px(1.))
                        .bg(p.stroke_strong),
                )
            })
    }
    // Row menus open from the desktop (see `RightClickedRow`), where submenus can nest.
    fn context_menu(
        &mut self,
        _index: usize,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        menu
    }
}
/// Shows the desktop's selection in the table without the side effects of a
/// click: no select event back to the desktop (which would toggle it off) and no
/// scroll to the row, which would pull the list back whenever rows reorder.
fn mirror_selection(
    state: &mut TableState<NativeTable>,
    index: usize,
    cx: &mut Context<TableState<NativeTable>>,
) {
    state.delegate_mut().echo = Some(index);
    state.set_selected_row(index, cx);
    state
        .vertical_scroll_handle
        .0
        .borrow_mut()
        .deferred_scroll_to_item = None;
}
/// The icon a row shows: a WebView's host application's, else its own.
impl Desktop {
    pub(in crate::frontend) fn rebuild_rows(&mut self, hold: bool, cx: &mut Context<Self>) {
        self.rebuild_rows_prepared(hold, None, cx);
    }
    pub(in crate::frontend) fn rebuild_rows_prepared(
        &mut self,
        hold: bool,
        prepared: Option<Vec<Row>>,
        cx: &mut Context<Self>,
    ) {
        let started = Instant::now();
        let mut rows = if let Some(rows) = prepared {
            rows
        } else if self.page == "processes" {
            model::process_rows(
                values(&self.snapshot["processes"]),
                model::ProcessOptions {
                    mode: &self.view,
                    query: &self.query,
                    sort: &self.sort,
                    descending: self.descending,
                    expanded: &self.expanded,
                    collapsed: &self.collapsed,
                    sections: &self.sections,
                    hosted: &self.hosted_services(),
                },
            )
        } else {
            let page = self.page.clone();
            let spec = self.list_spec(&page, first_column(&page));
            if page == "network" {
                let (source, owners) = self.connection_source();
                lists::list_rows(
                    &spec,
                    source.as_deref().map(values).unwrap_or_default(),
                    &owners,
                )
            } else {
                let source = match page.as_str() {
                    "services" | "startup" => self.data.get(&page).map(values),
                    _ => None,
                };
                lists::list_rows(&spec, source.unwrap_or_default(), &HashMap::new())
            }
        };
        // The other lists come with their marks, pinned rows first.
        if self.page == "processes" {
            let marks = self.marks();
            for row in &mut rows {
                if let Some(key) = &row.mark_key {
                    row.note = marks[key]["note"].as_str().map(str::to_owned);
                    row.pinned = boolean(&marks[key], "pinned");
                }
            }
        }
        if hold && self.page == "processes" {
            let order: HashMap<_, _> = self
                .rows
                .iter()
                .enumerate()
                .map(|(i, r)| (&r.key, i))
                .collect();
            // Rows keep their places; a new row follows the row it comes after in
            // the fresh order, so it stays in its category instead of dropping to the end.
            let fresh: Vec<String> = rows.iter().map(|r| r.key.clone()).collect();
            let mut place = -1.;
            let mut keyed: Vec<(f64, Row)> = rows
                .into_iter()
                .map(|row| {
                    place = match order.get(&row.key) {
                        Some(&index) => index as f64,
                        None => place + 1e-6,
                    };
                    (place, row)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows = keyed.into_iter().map(|(_, row)| row).collect();
            self.held_order = !rows.iter().map(|r| &r.key).eq(fresh.iter());
        } else {
            self.held_order = false;
        }
        let rows = Arc::new(rows);
        self.install_rows(rows.clone(), cx);
        if self.page == "network" {
            self.keep_connection_rows(rows);
        }
        super::trace_work(
            "rows",
            Some(&format!("{} {}", self.page, self.rows.len())),
            started,
        );
        let elapsed = started.elapsed().as_micros() as u64;
        self.work.rows_us += elapsed;
        self.work.rows_max_us = self.work.rows_max_us.max(elapsed);
        self.work.row_builds += 1;
    }
    /// Shows `rows` on the current page's table.
    pub(in crate::frontend) fn install_rows(
        &mut self,
        rows: Arc<Vec<Row>>,
        cx: &mut Context<Self>,
    ) {
        self.rows_epoch = self.rows_epoch.wrapping_add(1);
        if rows.len() != self.rows.len() {
            self.list
                .reset_with_uniform_height(rows.len(), px(ROW_HEIGHT));
        }
        let previous_rows = std::mem::replace(&mut self.rows, rows);
        // Apply data before rendering. Updating TableState from table_view
        // invalidates its parent during paint and rebuilds Desktop twice per sample.
        if let Some(table) = &self.table {
            let rows = self.rows.clone();
            let page = self.page.clone();
            let network = boolean(&self.snapshot, "processNetwork");
            let sort = (self.sort.as_str(), self.descending);
            let column_preferences = columns::Preferences::load(&self.prefs);
            let totals = self.process_column_totals(&column_preferences);
            let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
            let scope = (self.view.as_str(), self.query.as_str());
            let selected = self
                .selected
                .get(&page)
                .and_then(|key| rows.iter().position(|row| &row.key == key));
            table.update(cx, |state, cx| {
                let available = state.delegate().available;
                if available <= px(0.) {
                    return;
                }
                if state.delegate_mut().configure(
                    &page,
                    rows,
                    network,
                    TableLayout {
                        available,
                        columns: &column_preferences,
                        totals,
                        bits,
                        scope,
                    },
                    sort,
                    cx,
                ) {
                    state.refresh(cx);
                }
                if selected != state.selected_row() {
                    if let Some(index) = selected {
                        mirror_selection(state, index, cx);
                    } else {
                        state.clear_selection(cx);
                    }
                }
                cx.notify();
            });
        }
        if let Some(frame) = &self.table_frame {
            frame.update(cx, |_, cx| cx.notify());
        }
        // Retain old rows until delegates release them, then reclaim the value trees off-thread.
        if !previous_rows.is_empty() {
            cx.background_executor()
                .spawn(async move { drop(previous_rows) })
                .detach();
        }
    }
    /// The list's vertical scrollbar for a page that draws it beside the table:
    /// the process page, whose list is drawn narrower than it is laid out while
    /// the details pane opens or closes.
    pub(in crate::frontend) fn table_scrollbar(&self, cx: &App) -> Option<scrollbar::Scrollbar> {
        let table = self.table.as_ref()?;
        let rows = table
            .read(cx)
            .vertical_scroll_handle
            .0
            .borrow()
            .base_handle
            .clone();
        Some(
            scrollbar::scrollbar("table-scrollbar", &rows, self.palette.dark)
                .view(table.entity_id()),
        )
    }
    pub(in crate::frontend) fn table_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.table.is_none() {
            let owner = cx.entity().downgrade();
            let tips = self.tips.clone();
            let table = cx.new(|cx| {
                let mut state = TableState::new(NativeTable::new(owner, tips), window, cx)
                    .col_selectable(false)
                    .sortable(false)
                    // Process headers are dragged by the table itself (`HeaderDrag`).
                    .col_movable(false);
                let list = state.vertical_scroll_handle.0.borrow().base_handle.clone();
                let horizontal = state.horizontal_scroll_handle.as_ref().clone();
                state.delegate_mut().list = Some(list);
                state.delegate_mut().horizontal = Some(horizontal);
                state
            });
            self.subscriptions.push(cx.subscribe_in(
                &table,
                window,
                |this, source, event, window, cx| match event {
                    TableEvent::MoveColumn(_, to) => {
                        let state = source.read(cx);
                        if state.delegate().page != "processes" {
                            return;
                        }
                        let fields = &state.delegate().fields;
                        let Some(moved) = fields.get(*to) else {
                            return;
                        };
                        let mut preferences = columns::Preferences::load(&this.prefs);
                        if preferences
                            .move_before(moved.key, fields.get(to + 1).map(|field| field.key))
                        {
                            this.set_process_columns(preferences, cx);
                        }
                    }
                    TableEvent::ColumnWidthsChanged(widths) => {
                        let source = source.read(cx);
                        if source.delegate().page != "processes" {
                            return;
                        }
                        let mut preferences = columns::Preferences::load(&this.prefs);
                        if preferences.auto_width {
                            return;
                        }
                        for (field, width) in source.delegate().fields.iter().zip(widths) {
                            preferences
                                .widths
                                .insert(field.key.into(), f32::from(*width).clamp(48., 1200.));
                        }
                        this.prefs.set(columns::PREFERENCE, preferences);
                        cx.notify();
                    }
                    TableEvent::SelectRow(index) => {
                        let echo = this.table.as_ref().is_some_and(|table| {
                            table.update(cx, |state, _| state.delegate_mut().echo.take())
                                == Some(*index)
                        });
                        if echo {
                            return;
                        }
                        if let Some(row) = this.rows.get(*index).cloned() {
                            if row.section.is_some() {
                                this.act(Action::Fold(row.key.clone()), window, cx);
                                this.sync_table_selection(cx);
                            } else {
                                this.act(Action::Select(row.key.clone()), window, cx);
                            }
                        }
                    }
                    TableEvent::DoubleClickedRow(index) => {
                        if let Some(row) = this.rows.get(*index) {
                            let row = row.clone();
                            if this.page == "processes" {
                                // A heading already folded on this press (`SelectRow`).
                                if row.expandable && row.section.is_none() {
                                    this.act(Action::Fold(row.key.clone()), window, cx);
                                }
                            } else {
                                this.activate_row(&row, window, cx);
                            }
                        }
                    }
                    TableEvent::RightClickedRow(Some(index)) => {
                        // A right press selects the row, as a left one does, then opens its menu.
                        if let Some(row) = this.rows.get(*index).cloned()
                            && row.section.is_none()
                        {
                            this.selected.insert(this.page.clone(), row.key.clone());
                            let items = this.row_menu(&row);
                            this.open_popup(items, window, cx);
                            if let Some(table) = &this.table {
                                table.update(cx, |state, cx| state.set_right_clicked_row(None, cx));
                            }
                            cx.notify();
                        }
                    }
                    TableEvent::ClearSelection => {
                        // A selection the current filter or folding hides stays
                        // selected; only clearing a visible row deselects it.
                        if this
                            .selected
                            .get(&this.page)
                            .is_some_and(|key| this.rows.iter().any(|row| &row.key == key))
                        {
                            this.selected.remove(&this.page);
                        }
                        cx.notify();
                    }
                    _ => {}
                },
            ));
            self.table = Some(table);
            let state = self.table.as_ref().expect("table created").clone();
            self.table_frame = Some(cx.new(|_| TableFrame { state }));
            self.tables.insert(
                self.page.clone(),
                (
                    self.table.as_ref().unwrap().clone(),
                    self.table_frame.as_ref().unwrap().clone(),
                ),
            );
        }
        let table = self.table.as_ref().expect("table initialized").clone();
        let rows = self.rows.clone();
        let page = self.page.clone();
        let network = boolean(&self.snapshot, "processNetwork");
        let sort = (self.sort.as_str(), self.descending);
        let column_preferences = columns::Preferences::load(&self.prefs);
        let totals = self.process_column_totals(&column_preferences);
        let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
        let scope = (self.view.as_str(), self.query.as_str());
        let available = window.viewport_size().width
            - px(24.)
            - px(if self.page == "processes" && self.details_shown() {
                self.details_display_width(window)
            } else {
                0.
            })
            - px(if self.nav_top {
                0.
            } else {
                self.side_nav_width()
            });
        let selected = self
            .selected
            .get(&page)
            .and_then(|key| rows.iter().position(|r| &r.key == key));
        let changed = table.update(cx, |state, cx| {
            let mut changed = false;
            if !Arc::ptr_eq(&state.delegate().rows, &rows)
                || state.delegate().available != available
                || (page == "processes" && state.delegate().process_columns != column_preferences)
                || state.delegate().font_size != theme::font(12.)
                || state.delegate().bits != bits
                || state.delegate().locale != argus_core::i18n::locale()
            {
                if state.delegate_mut().configure(
                    &page,
                    rows,
                    network,
                    TableLayout {
                        available,
                        columns: &column_preferences,
                        totals,
                        bits,
                        scope,
                    },
                    sort,
                    cx,
                ) {
                    state.refresh(cx);
                }
                changed = true;
            }
            if selected != state.selected_row() {
                changed = true;
                if let Some(index) = selected {
                    mirror_selection(state, index, cx);
                } else {
                    state.clear_selection(cx);
                }
            }
            if changed {
                cx.notify();
            }
            changed
        });
        if changed && let Some(frame) = &self.table_frame {
            frame.update(cx, |_, cx| cx.notify());
        }
        let tips = self.tips.clone();
        div()
            .id("table-region")
            .size_full()
            .on_hover(cx.listener(|this, hovered, window, cx| {
                this.pointer_in_table = *hovered;
                if !hovered {
                    this.tips.dismiss(window, cx);
                    // Only the process list holds its order under the pointer.
                    if this.page == "processes" {
                        this.rebuild_rows(false, cx);
                    }
                }
                cx.notify();
            }))
            // Cell hints follow the pointer here, one listener for the table.
            .on_mouse_move(move |event, window, cx| tips.moved(event, window, cx))
            .child(
                self.table_frame
                    .as_ref()
                    .expect("table frame")
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
            .into_any_element()
    }
    /// Narrows a list to `choice` and keeps it for the next start, under the
    /// interface's preference (`services.filter`, `connections.filter`).
    pub(in crate::frontend) fn set_filter(
        &mut self,
        page: &str,
        choice: &str,
        cx: &mut Context<Self>,
    ) {
        let Some((key, choices)) = filter_choices(page) else {
            return;
        };
        let choice = if choices.contains(&choice) {
            choice
        } else {
            "all"
        };
        self.filters.insert(page.into(), choice.into());
        self.prefs.set(key, choice);
        self.rebuild_rows(false, cx);
    }
    pub(in crate::frontend) fn marks(&self) -> Value {
        self.marks_of(&self.page)
    }
    /// A page's row marks, under the interface's preference.
    pub(in crate::frontend) fn marks_of(&self, page: &str) -> Value {
        self.prefs.get(
            &format!(
                "rowMarks.{}",
                if page == "network" {
                    "connections"
                } else {
                    page
                }
            ),
            json!({}),
        )
    }
    pub(in crate::frontend) fn set_mark(
        &mut self,
        key: &str,
        field: &str,
        value: Value,
        cx: &mut Context<Self>,
    ) {
        let mut marks = self.marks();
        if !marks[key].is_object() {
            marks[key] = json!({});
        }
        marks[key][field] = value;
        self.prefs.set(
            &format!(
                "rowMarks.{}",
                if self.page == "network" {
                    "connections"
                } else {
                    &self.page
                }
            ),
            marks,
        );
        self.rebuild_rows(false, cx);
    }
}
/// Case-insensitive comparison that orders digit runs by value, like `Intl.Collator`
/// with `numeric: true`.
impl Desktop {
    fn process_column_totals(&self, preferences: &columns::Preferences) -> HashMap<String, String> {
        if self.page != "processes" {
            return HashMap::new();
        }
        preferences
            .selected
            .iter()
            .map(|key| (key.clone(), self.column_total(key)))
            .collect()
    }
    pub(in crate::frontend) fn process_columns_diagnostics(&self, cx: &App) -> Value {
        let table = self.tables.get("processes").map(|pair| &pair.0);
        let visible=table.map(|table| {
            let state=table.read(cx);
            state.delegate().fields.iter().enumerate().map(|(index,field)|json!({
                "key":field.key,"width":state.col_width(index).map(f32::from),
                "resizable":state.delegate().columns[index].resizable,
                "movable":state.delegate().columns[index].movable,
                "autoWidth":state.delegate().auto_widths.widths.get(field.key),
                "bounds":self.header_bounds.borrow().get(field.key).map(|b|[f32::from(b.left()),f32::from(b.top()),f32::from(b.size.width),f32::from(b.size.height)])
            })).collect::<Vec<_>>()
        }).unwrap_or_default();
        let anchors = self
            .ui
            .anchors
            .borrow()
            .iter()
            .filter(|(key, _)| key.starts_with("column-"))
            .map(|(key, b)| {
                (
                    key.to_string(),
                    json!([
                        f32::from(b.left()),
                        f32::from(b.top()),
                        f32::from(b.size.width),
                        f32::from(b.size.height)
                    ]),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let scroll = table.map(|table| {
            let state = table.read(cx);
            let handle = state.horizontal_scroll_handle.as_ref();
            json!({"x":f32::from(handle.offset().x),"maxX":f32::from(handle.max_offset().x),
                "y":f32::from(state.vertical_scroll_handle.0.borrow().base_handle.offset().y)})
        });
        json!({"preferences":columns::Preferences::load(&self.prefs),"visible":visible,"anchors":anchors,
            "scroll":scroll,"sort":{"key":self.sort,"desc":self.descending},
            "popup":self.popup.is_some(),"modal":self.modal.as_ref().filter(|modal|modal.kind=="columns").map(|modal|&modal.args)})
    }

    pub(in crate::frontend) fn capture_process_widths(
        &self,
        preferences: &mut columns::Preferences,
        cx: &App,
    ) {
        let table = self
            .tables
            .get("processes")
            .map(|pair| &pair.0)
            .or_else(|| {
                (self.page == "processes")
                    .then_some(self.table.as_ref())
                    .flatten()
            });
        if let Some(table) = table {
            let state = table.read(cx);
            for (index, field) in state.delegate().fields.iter().enumerate() {
                if let Some(width) = state.col_width(index) {
                    preferences
                        .widths
                        .insert(field.key.into(), f32::from(width).clamp(48., 1200.));
                }
            }
        }
    }

    /// Puts the table's own selection back on the selected row (or none), after
    /// a click the desktop did not turn into a selection.
    pub(in crate::frontend) fn sync_table_selection(&self, cx: &mut Context<Self>) {
        let selected = self
            .selected
            .get(&self.page)
            .and_then(|key| self.rows.iter().position(|row| &row.key == key));
        if let Some(table) = &self.table {
            table.update(cx, |state, cx| {
                match selected {
                    Some(index) => mirror_selection(state, index, cx),
                    None => state.clear_selection(cx),
                }
                state.set_right_clicked_row(None, cx);
            });
        }
    }
    /// The running process `pid` in the latest snapshot.
    fn process(&self, pid: u64) -> Option<&Value> {
        values(&self.snapshot["processes"])
            .iter()
            .find(|process| process["pid"].as_u64() == Some(pid))
    }
    /// Display names of the services running in each process, by PID.
    pub(in crate::frontend) fn hosted_services(&self) -> HashMap<u64, Vec<String>> {
        let mut hosted: HashMap<u64, Vec<String>> = HashMap::new();
        for service in self.data.get("services").map(values).unwrap_or_default() {
            if let Some(pid) = service["pid"].as_u64().filter(|pid| *pid != 0) {
                hosted
                    .entry(pid)
                    .or_default()
                    .push(text(service, "displayName").to_owned());
            }
        }
        hosted
    }
    /// The figure above a process column label.
    pub(in crate::frontend) fn column_total(&self, key: &str) -> String {
        let sys = &self.snapshot["system"];
        if sys.is_null() {
            return String::new();
        }
        let processes = values(&self.snapshot["processes"]);
        match key {
            "name" => tr!(
                "processes.total",
                count = number(sys, "processCount") as usize
            ),
            "cpu" => format!("{}%", number(sys, "cpu").round()),
            "mem" => format!(
                "{}%",
                (number(sys, "memUsed") / number(sys, "memTotal").max(1.) * 100.).round()
            ),
            "gpu" => {
                let gpus = values(&self.snapshot["hardware"]["gpus"]);
                if gpus.is_empty() {
                    String::new()
                } else {
                    format!(
                        "{}%",
                        gpus.iter()
                            .map(|g| number(g, "util"))
                            .fold(0., f64::max)
                            .round()
                    )
                }
            }
            "io" => theme::bytes(
                number(sys, "ioRead") + number(sys, "ioWrite") + number(sys, "ioOther"),
                true,
            ),
            "diskRead" | "diskWrite" => processes
                .iter()
                .filter_map(|p| p[key].as_f64())
                .reduce(|a, b| a + b)
                .map(|v| {
                    if v < 1. {
                        "0 KB/s".into()
                    } else {
                        theme::bytes(v, true)
                    }
                })
                .unwrap_or_else(|| label("format.missing")),
            "net" => {
                let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
                let total = processes
                    .iter()
                    .map(|p| number(p, "netSent") + number(p, "netReceived"))
                    .sum::<f64>();
                if total < 1. {
                    "0".into()
                } else {
                    theme::network(total, bits)
                }
            }
            "threads" => theme::count(number(sys, "threadCount")),
            "handles" => theme::count(number(sys, "handleCount")),
            _ => String::new(),
        }
    }
    /// What an empty table says, and the label of the button that clears its filter.
    pub(in crate::frontend) fn empty_message(&self) -> (String, Option<String>) {
        let query = self.query.trim();
        match self.page.as_str() {
            "processes" => (
                tr!("processes.noMatch", query = query),
                Some(label("shell.search.clear")),
            ),
            "services"
                if self
                    .data
                    .get("services")
                    .is_none_or(|s| values(s).is_empty()) =>
            {
                (label("services.loading"), None)
            }
            "services" => (
                label("services.noMatch"),
                Some(label("services.clearFilter")),
            ),
            "startup" if self.data.get("startup").is_none_or(Value::is_null) => {
                (label("startup.loading"), None)
            }
            "startup" => (label("startup.noMatch"), None),
            "network" if self.connections.as_deref().is_none_or(Value::is_null) => {
                (label("connections.loading"), None)
            }
            "network" => (
                if !query.is_empty() && query.bytes().all(|b| b.is_ascii_digit()) {
                    tr!("connections.noMatchPort", port = query)
                } else {
                    label("connections.noMatch")
                },
                Some(label("connections.clearFilter")),
            ),
            _ => (String::new(), None),
        }
    }
}
/// A table's first column, which orders by name.
pub(super) fn first_column(page: &str) -> &'static str {
    fields(page, false, f32::MAX)
        .first()
        .map(|field| field.key)
        .unwrap_or("name")
}
/// The order a table opens in.
pub(super) fn initial_sort(page: &str) -> (String, bool) {
    match page {
        "services" => ("displayName".into(), false),
        "network" => ("localPort".into(), false),
        "history" => ("cpu".into(), true),
        _ => ("name".into(), false),
    }
}
/// The interface's table behind each list and its sortable columns, as (field
/// here, column id there). `tableSort.<table>` keeps the saved preferences's
/// `{"id": column, "desc": bool}`, so both interfaces read the
/// same saved order. Columns the table does not sort by are left out.
type SortColumns = (&'static str, &'static [(&'static str, &'static str)]);
fn sort_columns(page: &str) -> Option<SortColumns> {
    Some(match page {
        "services" => (
            "services",
            &[
                ("displayName", "display"),
                ("name", "name"),
                ("pid", "pid"),
                ("state", "state"),
                ("startType", "start"),
            ],
        ),
        "startup" => (
            "startup",
            &[
                ("name", "name"),
                ("publisher", "publisher"),
                ("enabled", "state"),
                ("location", "location"),
            ],
        ),
        "network" => (
            "connections",
            &[
                ("name", "process"),
                ("pid", "pid"),
                ("protocol", "proto"),
                ("localAddress", "local"),
                ("localPort", "lport"),
                ("remoteAddress", "remote"),
                ("remotePort", "rport"),
                ("state", "state"),
            ],
        ),
        "history" => (
            "history",
            &[
                ("name", "name"),
                ("pid", "pid"),
                ("cpu", "cpu"),
                ("mem", "mem"),
                ("io", "io"),
                ("net", "net"),
                ("gpu", "gpu"),
            ],
        ),
        _ => return None,
    })
}
/// Whether `page`'s table sorts by the column `key`; the table disables the
/// headers of the others (a column without `sortValue`).
pub(super) fn sortable(page: &str, key: &str) -> bool {
    if page == "processes" {
        return columns::definition(key).is_some();
    }
    sort_columns(page).is_none_or(|(_, columns)| columns.iter().any(|(field, _)| *field == key))
}
/// A saved `tableSort.<table>` value as a sort of `page`, if it names one of
/// the table's sortable columns (the interface's validator).
fn restore_sort(page: &str, saved: &Value) -> Option<(String, bool)> {
    let (_, columns) = sort_columns(page)?;
    let id = saved["id"].as_str()?;
    let descending = saved["desc"].as_bool()?;
    columns
        .iter()
        .find(|(_, column)| *column == id)
        .map(|(field, _)| ((*field).to_owned(), descending))
}
/// The order a list opens in after a restart: the saved one, else the default.
pub(super) fn saved_sort(page: &str, prefs: &prefs::Prefs) -> (String, bool) {
    sort_columns(page)
        .and_then(|(table, _)| {
            restore_sort(page, &prefs.get(&format!("tableSort.{table}"), Value::Null))
        })
        .unwrap_or_else(|| initial_sort(page))
}
/// The preference that keeps a sort of `page`, and its value.
pub(super) fn sort_preference(page: &str, key: &str, descending: bool) -> Option<(String, Value)> {
    if page == "processes" {
        return Some(("sort".into(), json!({"key":key,"desc":descending})));
    }
    let (table, columns) = sort_columns(page)?;
    let (_, column) = columns.iter().find(|(field, _)| *field == key)?;
    Some((
        format!("tableSort.{table}"),
        json!({"id":column,"desc":descending}),
    ))
}
/// A list's filter choices and the preference that keeps the choice, named as
/// under `services.filter` and `connections.filter`.
pub(super) fn filter_choices(page: &str) -> Option<(&'static str, &'static [&'static str])> {
    match page {
        "services" => Some(("services.filter", &["all", "running", "stopped"])),
        "network" => Some((
            "connections.filter",
            &["all", "listen", "established", "udp"],
        )),
        _ => None,
    }
}
/// The saved filters of the lists that have one; an unknown value is "all".
pub(super) fn saved_filters(prefs: &prefs::Prefs) -> HashMap<String, String> {
    ["services", "network"]
        .into_iter()
        .filter_map(|page| {
            let (key, choices) = filter_choices(page)?;
            let saved = prefs.get::<String>(key, "all".into());
            (saved != "all" && choices.contains(&saved.as_str())).then(|| (page.to_owned(), saved))
        })
        .collect()
}
/// Text columns start ascending and figures descending, the useful first view.
pub(super) fn descending_first(page: &str, key: &str) -> bool {
    match page {
        "processes" => columns::definition(key).is_some_and(|column| {
            !matches!(
                column.format,
                columns::Format::Name
                    | columns::Format::Text
                    | columns::Format::Identifier
                    | columns::Format::Timestamp
            )
        }),
        "services" => key == "pid",
        "startup" => key == "enabled",
        "network" => matches!(key, "pid" | "localPort" | "remotePort"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    // Named imports keep the glob-imported GPUI `test` macro out of scope.
    use super::{
        Value, json, prefs, restore_sort, saved_filters, sort_columns, sort_preference, sortable,
    };

    #[test]
    fn saved_orders_keep_persistent_column_ids() {
        // Values a preferences.json from the saved preferences holds.
        assert_eq!(
            restore_sort("services", &json!({"id":"start","desc":true})),
            Some(("startType".into(), true))
        );
        assert_eq!(
            restore_sort("network", &json!({"id":"lport","desc":false})),
            Some(("localPort".into(), false))
        );
        assert_eq!(
            restore_sort("startup", &json!({"id":"state","desc":true})),
            Some(("enabled".into(), true))
        );
        for page in ["services", "startup", "network", "history"] {
            let (table, columns) = sort_columns(page).unwrap();
            for (field, _) in columns {
                let (key, value) = sort_preference(page, field, true).unwrap();
                assert_eq!(key, format!("tableSort.{table}"));
                assert_eq!(restore_sort(page, &value), Some(((*field).into(), true)));
            }
        }
        assert_eq!(
            sort_preference("processes", "cpu", true),
            Some(("sort".into(), json!({"key":"cpu","desc":true})))
        );
    }

    #[test]
    fn only_supported_columns_sort() {
        assert!(!sortable("services", "description"));
        assert!(!sortable("startup", "command"));
        assert!(sortable("services", "startType") && sortable("processes", "threads"));
        assert_eq!(sort_preference("services", "description", false), None);
        // The interface's validator: unsortable or unknown columns and malformed
        // values give way to the default order.
        for saved in [
            json!({"id":"desc","desc":false}),
            json!({"id":"displayName","desc":false}),
            json!({"id":"display"}),
            json!({"id":"display","desc":"true"}),
            Value::Null,
        ] {
            assert_eq!(restore_sort("services", &saved), None);
        }
    }

    #[test]
    fn filters_use_persistent_preference_keys() {
        let file =
            std::env::temp_dir().join(format!("argus-native-filters-{}.json", std::process::id()));
        std::fs::write(
            &file,
            br#"{"values":{"services.filter":"stopped","connections.filter":"bogus"}}"#,
        )
        .unwrap();
        let prefs = prefs::Prefs::load(file.clone());
        std::fs::remove_file(&file).unwrap();
        let filters = saved_filters(&prefs);
        assert_eq!(filters.get("services").map(String::as_str), Some("stopped"));
        assert!(!filters.contains_key("network"));
    }
}
