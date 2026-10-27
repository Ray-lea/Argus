//! Native virtual tables share one column model and preserve process-instance identity.
use super::*;
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
}
/// A CSS grid track from the original tables: a fixed width or `minmax(min, fr)`.
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
/// Header and row geometry of the original tables.
const ROW_HEIGHT: f32 = 30.;
const ROW_INSET: f32 = 4.;
const CELL_PADDING: f32 = 10.;
const MONO_FONT: &str = "Cascadia Mono";
fn fields(page: &str, network: bool, available: f32) -> Vec<Field> {
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
        _ => {
            // The process table drops secondary columns as it narrows, like the
            // original container queries: first threads and handles, then PID and I/O.
            let (name, drop_opt, drop_opt2) = if network {
                (
                    if available <= 920. {
                        160.
                    } else if available <= 1080. {
                        200.
                    } else {
                        220.
                    },
                    available <= 1080.,
                    available <= 920.,
                )
            } else {
                (
                    if available <= 840. {
                        160.
                    } else if available <= 980. {
                        200.
                    } else {
                        220.
                    },
                    available <= 980.,
                    available <= 840.,
                )
            };
            vec![
                ("name", "processes.column.name.label", Name, Flex(name, 1.)),
                ("pid", "processes.column.pid.label", Pid, Fixed(72.)),
                ("cpu", "processes.column.cpu.label", Percent, Fixed(70.)),
                ("mem", "processes.column.mem.label", Memory, Fixed(96.)),
                (
                    "diskRead",
                    "processes.column.diskRead.label",
                    Rate,
                    Fixed(104.),
                ),
                (
                    "diskWrite",
                    "processes.column.diskWrite.label",
                    Rate,
                    Fixed(104.),
                ),
                ("io", "processes.column.io.label", Rate, Fixed(92.)),
                ("gpu", "processes.column.gpu.label", Percent, Fixed(70.)),
                ("net", "processes.column.net.label", Rate, Fixed(92.)),
                (
                    "threads",
                    "processes.column.threads.label",
                    Count,
                    Fixed(62.),
                ),
                (
                    "handles",
                    "processes.column.handles.label",
                    Count,
                    Fixed(76.),
                ),
            ]
            .into_iter()
            .filter(|(key, ..)| match *key {
                "net" => network,
                "threads" | "handles" => !drop_opt,
                "pid" | "io" => !drop_opt2,
                _ => true,
            })
            .collect()
        }
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
fn cell(row: &Row, field: &Field) -> String {
    let raw = &row.raw;
    let missing = || label("format.missing");
    match field.kind {
        CellKind::Name => row.title.clone(),
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
            .map(theme::memory)
            .unwrap_or_else(missing),
        CellKind::Rate => row
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
pub(super) struct NativeTable {
    owner: WeakEntity<Desktop>,
    page: String,
    rows: Arc<Vec<Row>>,
    fields: Vec<Field>,
    columns: Vec<Column>,
    icons: HashMap<u32, Arc<Image>>,
    available: Pixels,
}
pub(super) struct TableFrame {
    state: Entity<TableState<NativeTable>>,
}
impl Render for TableFrame {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Process headers carry a total above the label; the other tables use the
        // original 34 px header cell. Both sit in a 4 px padded header row.
        DataTable::new(&self.state)
            .small()
            .header_row_height(px(if self.state.read(cx).delegate().page == "processes" {
                44.
            } else {
                34.
            }))
            .bordered(false)
            .fixed_cols_border(false)
            .stripe(false)
    }
}
impl NativeTable {
    fn new(owner: WeakEntity<Desktop>) -> Self {
        Self {
            owner,
            page: String::new(),
            rows: Arc::new(vec![]),
            fields: vec![],
            columns: vec![],
            icons: HashMap::new(),
            available: px(0.),
        }
    }
    fn configure(
        &mut self,
        page: &str,
        rows: Arc<Vec<Row>>,
        network: bool,
        available: Pixels,
        sort: (&str, bool),
        _cx: &App,
    ) -> bool {
        self.rows = rows;
        let fields = fields(page, network, f32::from(available));
        if self.page == page
            && self.available == available
            && self
                .fields
                .iter()
                .map(|f| (&f.key, &f.label))
                .eq(fields.iter().map(|f| (&f.key, &f.label)))
        {
            return false;
        }
        self.available = available;
        self.page = page.into();
        self.fields = fields;
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
        let owner = self.owner.clone();
        window.defer(cx, move |_, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.sort = key;
                this.descending = sort == ColumnSort::Descending;
                if this.page == "processes" {
                    this.prefs
                        .set("sort", json!({"key":this.sort,"desc":this.descending}));
                }
                this.rebuild_rows(false, cx);
                cx.notify();
            });
        });
    }
    fn render_header(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div().id("native-table-header").px(px(ROW_INSET)).py(px(4.))
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
                                    this.filters.remove(&this.page);
                                    this.rebuild_rows(false, cx);
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
        let mut cell = div()
            .size_full()
            .px(px(CELL_PADDING))
            .py(px(4.))
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
            let total = app.column_total(field.key);
            cell = cell.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(15.))
                    .line_height(relative(1.25))
                    .font_family(theme::DISPLAY_FONT)
                    .text_color(p.ink)
                    .whitespace_nowrap()
                    .when(field.key == "name" && !app.query.trim().is_empty(), |d| {
                        d.child(icon("FilterIcon", p.accent).size(px(13.)))
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
            .text_size(px(12.))
            .whitespace_nowrap()
            .text_color(if sort.0 { p.ink } else { p.muted })
            .when(sort.0, |d| d.font_weight(FontWeight::SEMIBOLD))
            .child(field.label.clone())
            .when(sort.0, |d| {
                d.child(icon(if sort.1 { "ChevronDown" } else { "ChevronUp" }, p.ink).size(px(12.)))
            });
        let hover = p.hover;
        let press = theme::role(cx, "press");
        let header_bounds = owner
            .upgrade()
            .map(|owner| owner.read(cx).header_bounds.clone());
        let measured_key = key.clone();
        Button::new(("column-sort", index))
            .size_full()
            .min_w_0()
            .rounded(px(4.))
            .bg(gpui_kit::gpui::transparent_black())
            .on_prepaint(move |bounds, _, _| {
                if let Some(headers) = &header_bounds {
                    headers.borrow_mut().insert(measured_key.clone(), bounds);
                }
            })
            .hover(move |s| s.bg(hover))
            .active(move |s| s.bg(press))
            .accessibility_label(field.label.clone())
            .when(self.page == "processes", |button| {
                let title = theme::label(&format!("processes.column.{key}.title"));
                button.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window, cx)
                })
            })
            .child(cell.child(label))
            .on_click(move |_, window, cx| {
                let owner = owner.clone();
                let key = key.clone();
                window.defer(cx, move |window, cx| {
                    let _ = owner.update(cx, |this, cx| this.act(Action::Sort(key), window, cx));
                });
            })
    }
    fn render_td(
        &mut self,
        row_index: usize,
        column: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if column == 0
            && let Some(id) = self.rows[row_index].raw["icon"]
                .as_u64()
                .map(|id| id as u32)
            && !self.icons.contains_key(&id)
            && let Some(owner) = self.owner.upgrade()
            && let Some(png) = owner.read(cx).host.state::<AppState>().meta.icon_png(id)
        {
            self.icons.insert(
                id,
                Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec())),
            );
        }
        let row = &self.rows[row_index];
        let field = &self.fields[column];
        let p = Palette::current(cx);
        let faint = p.faint;
        let value = if field.key == "net" {
            let bits = self.owner.upgrade().is_some_and(|owner| {
                owner
                    .read(cx)
                    .prefs
                    .get::<String>("netUnit", "bytes".into())
                    == "bits"
            });
            row.metric("net")
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
        };
        let owner = self.owner.clone();
        let mut element = div()
            .text_size(cx.theme().font_size)
            .size_full()
            .px(px(CELL_PADDING))
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .when(field.end, |d| d.justify_end());
        let processes = self.page == "processes";
        if processes
            && row.section.is_none()
            && matches!(
                field.kind,
                CellKind::Percent | CellKind::Memory | CellKind::Rate
            )
            && let Some(value) = row.metric(field.key)
        {
            element = element.bg(p.heat.opacity(theme::heat_alpha(field.key, value)));
        }
        if column == 0 {
            return self.name_cell(row_index, element, cx).into_any_element();
        }
        if row.section.is_some() {
            // Folded headings show their totals a step brighter.
            let collapsed = !row.expanded;
            return element
                .text_size(px(12.))
                .text_color(if collapsed { p.muted } else { faint })
                .child(value)
                .into_any_element();
        }
        let dot = |color: Hsla| div().size(px(7.)).rounded_full().flex_shrink_0().bg(color);
        match field.kind {
            CellKind::Enabled => {
                let enabled = boolean(&row.raw, "enabled");
                let blocked = !row.raw["blocked"].is_null();
                let id = row.raw["id"].clone();
                element = element.gap_2().child(
                    gpui_kit::component::switch::Switch::new(("startup-toggle", row_index))
                        .checked(enabled)
                        .disabled(blocked)
                        .small()
                        .accessibility_label(if enabled {
                            tr!("startup.toggle.disable", name = text(&row.raw, "name"))
                        } else {
                            tr!("startup.toggle.enable", name = text(&row.raw, "name"))
                        })
                        .on_click(move |_, window, cx| {
                            let owner = owner.clone();
                            let id = id.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.act(
                                        Action::Invoke(
                                            "set_startup_enabled".into(),
                                            json!({"id": id, "enabled": !enabled}),
                                        ),
                                        window,
                                        cx,
                                    )
                                });
                            });
                        }),
                );
                element = element.child(div().when(!enabled, |d| d.text_color(faint)).child(value));
            }
            CellKind::ServiceState => {
                let state = text(&row.raw, "state");
                element = element
                    .gap(px(7.))
                    .child(dot(if state == "running" {
                        p.network
                    } else if state.ends_with("Pending") {
                        p.io
                    } else {
                        faint
                    }))
                    .child(value);
            }
            CellKind::ConnectionState => {
                let state = text(&row.raw, "state");
                element = element.gap(px(7.)).child(dot(match state {
                    "established" => p.network,
                    "listen" => p.cpu,
                    _ => faint,
                }));
                if !value.is_empty() {
                    element = element.child(value);
                }
            }
            CellKind::StartType => {
                element = element
                    .when(text(&row.raw, "startType") == "manual", |d| {
                        d.text_color(faint)
                    })
                    .when(text(&row.raw, "startType") == "disabled", |d| {
                        d.text_color(p.io)
                    })
                    .child(value);
            }
            CellKind::Protocol => {
                element = element
                    .gap(px(6.))
                    .child(value)
                    .when(boolean(&row.raw, "ipv6"), |d| {
                        d.child(tag("v6", p.stroke_strong, p.muted))
                    });
            }
            CellKind::Mono | CellKind::MonoDim | CellKind::Port | CellKind::PortDim => {
                element = element
                    .font_family(MONO_FONT)
                    .text_size(px(12.))
                    .when(
                        matches!(field.kind, CellKind::MonoDim | CellKind::PortDim),
                        |d| d.text_color(faint),
                    )
                    .child(div().truncate().child(value));
            }
            CellKind::Dim | CellKind::ServicePid | CellKind::StartupLocation => {
                element = element
                    .text_color(faint)
                    .child(div().truncate().child(value));
            }
            CellKind::Pid => {
                element = element.text_color(faint).child(value);
            }
            _ => {
                // Idle readings stay quiet so real load stands out.
                let reading = row.metric(field.key);
                let dim = match field.kind {
                    CellKind::Percent => reading.is_none_or(|v| v < 0.05),
                    CellKind::Rate => reading.is_none_or(|v| v < 1.),
                    _ => false,
                };
                element = element.when(dim, |d| d.text_color(faint)).child(value);
            }
        }
        element.into_any_element()
    }
    fn render_tr(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let p = Palette::current(cx);
        let row = &self.rows[index];
        let (selected, fresh) = self
            .owner
            .upgrade()
            .map(|owner| {
                let app = owner.read(cx);
                (
                    app.selected
                        .get(&self.page)
                        .is_some_and(|key| *key == row.key),
                    self.page == "processes" && app.is_fresh(row),
                )
            })
            .unwrap_or_default();
        // Pinned rows lead the list; a line under the last one ends them.
        let last_pinned = row.pinned && self.rows.get(index + 1).is_some_and(|next| !next.pinned);
        div()
            .id(("native-row", index))
            .relative()
            .rounded(px(4.))
            .when(fresh, |d| d.bg(theme::role(cx, "net").opacity(0.15)))
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
/// A small outlined label, like the original `.tag`.
fn tag(text: impl Into<SharedString>, border: Hsla, color: Hsla) -> Div {
    div()
        .flex_shrink_0()
        .px(px(6.))
        .border_1()
        .border_color(border)
        .rounded_full()
        .text_size(px(11.))
        .line_height(px(16.))
        .text_color(color)
        .child(text.into())
}
impl NativeTable {
    /// The first cell: fold control, icon, name and the marks that follow it.
    fn name_cell(&self, row_index: usize, element: Div, cx: &mut Context<TableState<Self>>) -> Div {
        let row = &self.rows[row_index];
        let p = Palette::current(cx);
        let owner = self.owner.clone();
        let fold = |expanded: bool, key: String, owner: WeakEntity<Desktop>| {
            let hover = p.hover;
            let ink = p.ink;
            Button::new(("fold", row_index))
                .size(px(18.))
                .flex_shrink_0()
                .rounded(px(3.))
                .flex()
                .items_center()
                .justify_center()
                .hover(move |s| s.bg(hover).text_color(ink))
                .child(
                    icon("ChevronRight", p.muted)
                        .size(px(12.))
                        .when(expanded, |i| {
                            i.rotate(gpui_kit::gpui::radians(std::f32::consts::FRAC_PI_2))
                        }),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    let owner = owner.clone();
                    let key = key.clone();
                    window.defer(cx, move |window, cx| {
                        let _ =
                            owner.update(cx, |this, cx| this.act(Action::Fold(key), window, cx));
                    });
                })
        };
        if let Some(section) = &row.section {
            let locked = self
                .owner
                .upgrade()
                .is_some_and(|owner| !owner.read(cx).query.trim().is_empty());
            let _ = section;
            return element
                .pl(px(6.))
                .gap_2()
                .child(div().when(locked, |d| d.opacity(0.4)).child(fold(
                    row.expanded,
                    row.key.clone(),
                    owner,
                )))
                .child(
                    div()
                        .flex_shrink(1.)
                        .min_w_0()
                        .truncate()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.ink)
                        .child(row.title.clone()),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .min_w(px(22.))
                        .px(px(7.))
                        .rounded_full()
                        .bg(p.accent.opacity(0.14))
                        .text_color(p.accent.blend(p.ink.opacity(0.22)))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(px(18.))
                        .flex()
                        .justify_center()
                        .child(row.raw["entryCount"].to_string()),
                )
                .child(div().flex_1().min_w_0().h(px(1.)).ml_1().bg(
                    gpui_kit::gpui::linear_gradient(
                        90.,
                        gpui_kit::gpui::linear_color_stop(p.stroke_strong, 0.),
                        gpui_kit::gpui::linear_color_stop(p.stroke_strong.opacity(0.), 1.),
                    ),
                ));
        }
        let processes = self.page == "processes";
        let mut element = element.gap_2();
        if processes {
            element = element.pl(px(6. + row.depth as f32 * 20.));
            element = if row.expandable {
                element.child(
                    div()
                        .mr(px(-4.))
                        .child(fold(row.expanded, row.key.clone(), owner)),
                )
            } else {
                element.child(div().w(px(14.)).flex_shrink_0())
            };
        }
        if self.page != "services" {
            let image = row.raw["icon"]
                .as_u64()
                .map(|id| id as u32)
                .and_then(|id| self.icons.get(&id).cloned());
            let kernel = processes && row.raw["path"].is_null() && row.members.len() <= 1;
            element = element.child(div().relative().size_4().flex_shrink_0().child(
                if let Some(image) = image {
                    img(image).size_4().into_any_element()
                } else {
                    let glyph = if kernel { "kernel" } else { "generic-app" };
                    div()
                        .size_4()
                        .child(div().absolute().inset_0().child(theme::glyph(
                            &format!("{glyph}-fill"),
                            theme::role(cx, "glyph-bg"),
                        )))
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .child(theme::glyph(glyph, theme::role(cx, "glyph-stroke"))),
                        )
                        .into_any_element()
                },
            ));
        }
        let group = processes && row.members.len() > 1;
        let off = self.page == "startup" && !boolean(&row.raw, "enabled");
        element = element.child(
            div()
                .min_w_0()
                .flex_shrink(1.)
                .truncate()
                .when(group, |d| d.font_weight(FontWeight::SEMIBOLD))
                .when(processes && boolean(&row.raw, "suspended"), |d| {
                    d.text_color(p.muted)
                })
                .when(off, |d| d.text_color(p.muted))
                .child(row.title.clone()),
        );
        if group {
            element = element.child(
                div()
                    .flex_shrink_0()
                    .min_w(px(20.))
                    .px(px(6.))
                    .rounded_full()
                    .bg(p.hover)
                    .text_color(p.muted)
                    .text_size(px(11.))
                    .line_height(px(17.))
                    .flex()
                    .justify_center()
                    .child(row.members.len().to_string()),
            );
        }
        if let Some(note) = &row.note {
            element = element.child(
                div()
                    .flex()
                    .flex_shrink(1.)
                    .items_center()
                    .gap_1()
                    .min_w(px(40.))
                    .h(px(20.))
                    .ml(px(-2.))
                    .pl(px(5.))
                    .pr(px(7.))
                    .rounded(px(4.))
                    .bg(p.accent.opacity(0.11))
                    .text_color(p.accent.blend(p.ink.opacity(0.28)))
                    .text_size(px(12.))
                    .child(
                        icon("NoteIcon", p.accent.blend(p.ink.opacity(0.28)))
                            .size(px(12.))
                            .flex_shrink_0(),
                    )
                    .child(div().min_w_0().truncate().child(note.clone())),
            );
        }
        if processes && !row.detail.is_empty() {
            element = element.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(p.faint)
                    .child(row.detail.clone()),
            );
        }
        if processes {
            let efficient = row
                .members
                .iter()
                .filter(|member| boolean(member, "efficiency"))
                .count();
            if efficient > 0 {
                element = element.child(icon("LeafIcon", p.muted).size(px(12.)).flex_shrink_0());
            }
            if row
                .members
                .iter()
                .all(|member| boolean(member, "suspended"))
            {
                element =
                    element.child(tag(label("processes.suspended"), p.stroke_strong, p.muted));
            }
        }
        if self.page == "startup" && !boolean(&row.raw, "exists") && !row.raw["path"].is_null() {
            element = element.child(tag(
                label("startup.missing.label"),
                p.io.opacity(0.55),
                p.io,
            ));
        }
        if row.pinned {
            element = element
                .child(div().flex_1())
                .child(icon("PinIcon", p.muted).flex_shrink_0());
        }
        element
    }
}
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
        self.rows_epoch = self.rows_epoch.wrapping_add(1);
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
            let data_key = match self.page.as_str() {
                "services" => "services",
                "startup" => "startup",
                "network" => "connections",
                _ => "",
            };
            let query = self.query.trim().to_lowercase();
            let source = self.data.get(data_key).map(values).unwrap_or_default();
            let filter = self
                .filters
                .get(&self.page)
                .map(String::as_str)
                .unwrap_or("all");
            let marks = self.marks();
            let processes: HashMap<u64, &Value> = values(&self.snapshot["processes"])
                .iter()
                .filter_map(|p| p["pid"].as_u64().map(|pid| (pid, p)))
                .collect();
            // A number is almost always a port or PID: match it exactly, so 80 does not find 8080.
            let number_query = query.parse::<u64>().ok();
            let contains = |value: &str| value.to_lowercase().contains(&query);
            let mut rows: Vec<_> = source
                .iter()
                .filter(|row| match self.page.as_str() {
                    "services" => filter == "all" || text(row, "state") == filter,
                    "network" => {
                        filter == "all"
                            || (filter == "listen" && text(row, "state") == "listen")
                            || (filter == "established" && text(row, "state") == "established")
                            || (filter == "udp" && text(row, "protocol") == "udp")
                    }
                    _ => true,
                })
                .cloned()
                .map(|raw| {
                    let mut row = Row::process(raw, 0);
                    row.detail.clear();
                    match self.page.as_str() {
                        "services" => {
                            row.key = text(&row.raw, "name").into();
                            row.title = text(&row.raw, "displayName").into();
                            // Service names are case-insensitive.
                            row.mark_key = Some(row.key.to_lowercase());
                        }
                        "startup" => {
                            row.key = text(&row.raw, "id").into();
                            row.title = text(&row.raw, "name").into();
                            row.mark_key = Some(row.key.clone());
                        }
                        "network" => {
                            row.key = format!(
                                "{}|{}|{}|{}|{}|{}|{}",
                                text(&row.raw, "protocol"),
                                boolean(&row.raw, "ipv6"),
                                row.raw["pid"],
                                text(&row.raw, "localAddress"),
                                row.raw["localPort"],
                                text(&row.raw, "remoteAddress"),
                                row.raw["remotePort"]
                            );
                            let process =
                                row.raw["pid"].as_u64().and_then(|pid| processes.get(&pid));
                            row.title = match process {
                                Some(p) => {
                                    let description = text(p, "description");
                                    if description.is_empty() {
                                        text(p, "name")
                                    } else {
                                        description
                                    }
                                    .to_owned()
                                }
                                None if row.raw["pid"].as_u64() == Some(0) => {
                                    "System Idle Process".into()
                                }
                                None => label("connections.exited"),
                            };
                            if let Some(process) = process {
                                row.raw["icon"] = process["icon"].clone();
                                let name = text(process, "name").to_lowercase();
                                let protocol = text(&row.raw, "protocol");
                                let local = row.raw["remoteAddress"].is_null()
                                    || source.iter().any(|c| {
                                        c["pid"] == row.raw["pid"]
                                            && c["localPort"] == row.raw["localPort"]
                                            && text(c, "state") == "listen"
                                    });
                                row.mark_key = Some(if local {
                                    format!("{protocol}|{name}|local|{}", row.raw["localPort"])
                                } else {
                                    format!(
                                        "{protocol}|{name}|remote|{}|{}",
                                        text(&row.raw, "remoteAddress"),
                                        row.raw["remotePort"]
                                            .as_u64()
                                            .map(|p| p.to_string())
                                            .unwrap_or_default()
                                    )
                                });
                            }
                        }
                        _ => {}
                    }
                    if let Some(key) = &row.mark_key {
                        row.note = marks[key]["note"].as_str().map(str::to_owned);
                        row.pinned = boolean(&marks[key], "pinned");
                    }
                    row
                })
                .filter(|row| {
                    if query.is_empty() {
                        return true;
                    }
                    let raw = &row.raw;
                    let note = row.note.as_deref().is_some_and(contains);
                    match self.page.as_str() {
                        "services" => {
                            number_query.is_some_and(|n| raw["pid"].as_u64() == Some(n))
                                || contains(text(raw, "name"))
                                || contains(text(raw, "displayName"))
                                || contains(text(raw, "description"))
                                || note
                        }
                        "startup" => {
                            contains(text(raw, "name"))
                                || contains(text(raw, "entry"))
                                || contains(text(raw, "publisher"))
                                || contains(text(raw, "command"))
                                || note
                        }
                        "network" => {
                            if let Some(n) = number_query {
                                return raw["localPort"].as_u64() == Some(n)
                                    || raw["remotePort"].as_u64() == Some(n)
                                    || raw["pid"].as_u64() == Some(n);
                            }
                            let process = raw["pid"].as_u64().and_then(|pid| processes.get(&pid));
                            text(raw, "localAddress").contains(&query)
                                || text(raw, "remoteAddress").contains(&query)
                                || process.is_some_and(|p| {
                                    contains(text(p, "name")) || contains(text(p, "description"))
                                })
                                || note
                        }
                        _ => true,
                    }
                })
                .collect();
            let first = fields(&self.page, false, f32::MAX)
                .first()
                .map(|field| field.key)
                .unwrap_or("name");
            rows.sort_by(|a, b| {
                let key = self.sort.as_str();
                let order = if key == first {
                    natural_cmp(&a.title, &b.title)
                } else if key == "protocol" {
                    natural_cmp(text(&a.raw, key), text(&b.raw, key))
                        .then_with(|| boolean(&a.raw, "ipv6").cmp(&boolean(&b.raw, "ipv6")))
                } else {
                    match (&a.raw[key], &b.raw[key]) {
                        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
                        (Value::Number(a), Value::Number(b)) => a
                            .as_f64()
                            .unwrap_or(0.)
                            .total_cmp(&b.as_f64().unwrap_or(0.)),
                        (a, b) => natural_cmp(a.as_str().unwrap_or(""), b.as_str().unwrap_or("")),
                    }
                };
                let order = if self.descending {
                    order.reverse()
                } else {
                    order
                };
                order.then_with(|| a.key.cmp(&b.key))
            });
            rows
        };
        let marks = self.marks();
        for row in &mut rows {
            if let Some(key) = &row.mark_key {
                row.note = marks[key]["note"].as_str().map(str::to_owned);
                row.pinned = boolean(&marks[key], "pinned");
            }
        }
        if self.page != "processes" {
            rows.sort_by_key(|row| !row.pinned);
        }
        if hold && self.page == "processes" {
            let order: HashMap<_, _> = self
                .rows
                .iter()
                .enumerate()
                .map(|(i, r)| (&r.key, i))
                .collect();
            rows.sort_by_key(|r| order.get(&r.key).copied().unwrap_or(usize::MAX));
            self.held_order = true;
        } else {
            self.held_order = false;
        }
        if rows.len() != self.rows.len() {
            self.list
                .reset_with_uniform_height(rows.len(), px(ROW_HEIGHT));
        }
        let previous_rows = std::mem::replace(&mut self.rows, Arc::new(rows));
        // Apply data before rendering. Updating TableState from table_view
        // invalidates its parent during paint and rebuilds Desktop twice per sample.
        if let Some(table) = &self.table {
            let rows = self.rows.clone();
            let page = self.page.clone();
            let network = boolean(&self.snapshot, "processNetwork");
            let sort = (self.sort.as_str(), self.descending);
            let selected = self
                .selected
                .get(&page)
                .and_then(|key| rows.iter().position(|row| &row.key == key));
            table.update(cx, |state, cx| {
                let available = state.delegate().available;
                if available <= px(0.) {
                    return;
                }
                if state
                    .delegate_mut()
                    .configure(&page, rows, network, available, sort, cx)
                {
                    state.refresh(cx);
                }
                if selected != state.selected_row() {
                    if let Some(index) = selected {
                        state.set_selected_row(index, cx);
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
        let elapsed = started.elapsed().as_micros() as u64;
        self.work.rows_us += elapsed;
        self.work.rows_max_us = self.work.rows_max_us.max(elapsed);
        self.work.row_builds += 1;
    }
    pub(in crate::frontend) fn table_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.table.is_none() {
            let owner = cx.entity().downgrade();
            let table = cx.new(|cx| {
                TableState::new(NativeTable::new(owner), window, cx)
                    .col_selectable(false)
                    .sortable(false)
                    .col_movable(false)
            });
            self.subscriptions.push(cx.subscribe_in(
                &table,
                window,
                |this, _, event, window, cx| match event {
                    TableEvent::SelectRow(index) => {
                        if let Some(row) = this.rows.get(*index) {
                            this.act(Action::Select(row.key.clone()), window, cx);
                        }
                    }
                    TableEvent::DoubleClickedRow(index) => {
                        if let Some(row) = this.rows.get(*index) {
                            let row = row.clone();
                            if this.page == "processes" {
                                if row.expandable {
                                    this.act(Action::Fold(row.key.clone()), window, cx);
                                }
                            } else {
                                this.activate_row(&row, window, cx);
                            }
                        }
                    }
                    TableEvent::RightClickedRow(Some(index)) => {
                        // A right press selects the row, as a left one does, then opens its menu.
                        if let Some(row) = this.rows.get(*index).cloned() {
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
                        this.selected.remove(&this.page);
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
        let available = window.viewport_size().width
            - px(24.)
            - px(if self.page == "processes" && self.details_open {
                self.details_width
            } else {
                0.
            })
            - px(if self.nav_top {
                0.
            } else if self.nav_expanded {
                184.
            } else {
                48.
            });
        let selected = self
            .selected
            .get(&page)
            .and_then(|key| rows.iter().position(|r| &r.key == key));
        let changed = table.update(cx, |state, cx| {
            let mut changed = false;
            if !Arc::ptr_eq(&state.delegate().rows, &rows)
                || state.delegate().available != available
            {
                if state
                    .delegate_mut()
                    .configure(&page, rows, network, available, sort, cx)
                {
                    state.refresh(cx);
                }
                changed = true;
            }
            if selected != state.selected_row() {
                changed = true;
                if let Some(index) = selected {
                    state.set_selected_row(index, cx);
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
        div()
            .id("table-region")
            .size_full()
            .on_hover(cx.listener(|this, hovered, _, cx| {
                this.pointer_in_table = *hovered;
                if !hovered {
                    this.rebuild_rows(false, cx);
                }
                cx.notify();
            }))
            .child(
                self.table_frame
                    .as_ref()
                    .expect("table frame")
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
            .into_any_element()
    }
    pub(in crate::frontend) fn marks(&self) -> Value {
        self.prefs.get(
            &format!(
                "rowMarks.{}",
                if self.page == "network" {
                    "connections"
                } else {
                    &self.page
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
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        chars.next();
                    }
                    digits
                };
                let (x, y) = (take(&mut a), take(&mut b));
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order.is_ne() {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order.is_ne() {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}
impl Desktop {
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
    /// The figure above a process column label, as in the original header.
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
            "network" if self.data.get("connections").is_none_or(Value::is_null) => {
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
/// The order a table opens in, as in the original pages.
pub(super) fn initial_sort(page: &str) -> (String, bool) {
    match page {
        "services" => ("displayName".into(), false),
        "network" => ("localPort".into(), false),
        _ => ("name".into(), false),
    }
}
/// Text columns start ascending and figures descending, the useful first view.
pub(super) fn descending_first(page: &str, key: &str) -> bool {
    match page {
        "processes" => !matches!(key, "name" | "pid"),
        "services" => key == "pid",
        "startup" => key == "enabled",
        "network" => matches!(key, "pid" | "localPort" | "remotePort"),
        _ => false,
    }
}
