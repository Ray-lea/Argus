//! Native virtual tables share one column model and preserve process-instance identity.
use super::*;
use gpui_kit::base::ElementExt;
use gpui_kit::component::{
    ActiveTheme, Sizable,
    menu::{PopupMenu, PopupMenuItem},
    table::{Column, ColumnFixed, ColumnSort, DataTable, TableDelegate, TableEvent, TableState},
};

#[derive(Clone, Copy)]
enum CellKind {
    Text,
    Count,
    Pid,
    Port,
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
#[derive(Clone)]
struct Field {
    key: &'static str,
    label: String,
    kind: CellKind,
}
fn fields(page: &str, network: bool) -> Vec<Field> {
    let definitions: Vec<(&str, &str, CellKind)> = match page {
        "services" => vec![
            ("displayName", "services.column.display", CellKind::Name),
            ("name", "common.name", CellKind::Text),
            ("pid", "processes.column.pid.label", CellKind::Pid),
            ("state", "common.status", CellKind::ServiceState),
            (
                "startType",
                "services.column.startType",
                CellKind::StartType,
            ),
            ("description", "services.column.description", CellKind::Text),
        ],
        "startup" => vec![
            ("name", "common.name", CellKind::Name),
            ("publisher", "startup.column.publisher", CellKind::Text),
            ("enabled", "common.status", CellKind::Enabled),
            (
                "location",
                "startup.column.location",
                CellKind::StartupLocation,
            ),
            ("command", "common.command", CellKind::Text),
        ],
        "network" => vec![
            ("name", "connections.column.process", CellKind::Name),
            ("pid", "processes.column.pid.label", CellKind::Pid),
            (
                "protocol",
                "connections.column.protocol",
                CellKind::Protocol,
            ),
            (
                "localAddress",
                "connections.column.localAddress",
                CellKind::Text,
            ),
            ("localPort", "connections.column.localPort", CellKind::Port),
            (
                "remoteAddress",
                "connections.column.remoteAddress",
                CellKind::Text,
            ),
            (
                "remotePort",
                "connections.column.remotePort",
                CellKind::Port,
            ),
            ("state", "common.status", CellKind::ConnectionState),
        ],
        _ => vec![
            ("name", "processes.column.name.label", CellKind::Name),
            ("pid", "processes.column.pid.label", CellKind::Pid),
            ("cpu", "processes.column.cpu.label", CellKind::Percent),
            ("mem", "processes.column.mem.label", CellKind::Memory),
            (
                "diskRead",
                "processes.column.diskRead.label",
                CellKind::Rate,
            ),
            (
                "diskWrite",
                "processes.column.diskWrite.label",
                CellKind::Rate,
            ),
            ("io", "processes.column.io.label", CellKind::Rate),
            ("gpu", "processes.column.gpu.label", CellKind::Percent),
            ("net", "processes.column.net.label", CellKind::Rate),
            ("threads", "processes.column.threads.label", CellKind::Count),
            ("handles", "processes.column.handles.label", CellKind::Count),
        ],
    };
    definitions
        .into_iter()
        .filter(|(key, _, _)| *key != "net" || network)
        .map(|(key, title, kind)| Field {
            key,
            label: label(title),
            kind,
        })
        .collect()
}
fn cell(row: &Row, field: &Field) -> String {
    let raw = &row.raw;
    let missing = || label("format.missing");
    match field.kind {
        CellKind::Name => row.title.clone(),
        CellKind::Text => {
            let value = &raw[field.key];
            value.as_str().map(str::to_owned).unwrap_or_else(|| {
                if value.is_null() {
                    missing()
                } else {
                    value.to_string()
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
        CellKind::Enabled => label(if boolean(raw, "enabled") {
            "common.enabled"
        } else {
            "common.disabled"
        }),
        CellKind::StartupLocation => label(&format!("startup.location.{}", text(raw, "location"))),
        CellKind::Protocol => {
            let proto = text(raw, "protocol").to_uppercase();
            if boolean(raw, "ipv6") {
                format!("{proto} v6")
            } else {
                proto
            }
        }
        CellKind::Port => raw[field.key]
            .as_u64()
            .map(|v| v.to_string())
            .unwrap_or_else(missing),
        CellKind::ConnectionState => {
            let state = text(raw, "state");
            if state.is_empty() {
                missing()
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
        DataTable::new(&self.state)
            .small()
            .header_row_height(
                cx.theme().font_size
                    * if self.state.read(cx).delegate().page == "processes" {
                        4.
                    } else {
                        2.5
                    },
            )
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
        cx: &App,
    ) -> bool {
        self.rows = rows;
        let em = f32::from(cx.theme().font_size);
        let mut fields = fields(page, network);
        if page == "processes" {
            for optional in ["handles", "threads", "io", "pid"] {
                let needed = em * (24. + 8. * (fields.len().saturating_sub(1)) as f32);
                if needed <= f32::from(available) {
                    break;
                }
                fields.retain(|field| field.key != optional);
            }
        }
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
        let em = f32::from(cx.theme().font_size);
        self.columns = self
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let text_width = |value: &str| {
                    value
                        .chars()
                        .map(|c| if c.is_ascii() { 0.6 } else { 1. })
                        .sum::<f32>()
                        * em
                };
                let largest = self
                    .rows
                    .iter()
                    .take(80)
                    .map(|r| text_width(&cell(r, field)))
                    .fold(text_width(&field.label), f32::max);
                let width = (largest + em * 2.)
                    .min(f32::from(available) / if index == 0 { 3. } else { 5. })
                    .max(text_width(&field.label) + em * 2.);
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
                } else if matches!(
                    field.kind,
                    CellKind::Count
                        | CellKind::Pid
                        | CellKind::Port
                        | CellKind::Percent
                        | CellKind::Memory
                        | CellKind::Rate
                ) {
                    column.text_right()
                } else {
                    column
                }
            })
            .collect();
        let used = self
            .columns
            .iter()
            .map(|column| f32::from(column.width))
            .sum::<f32>();
        let spare = (f32::from(available) - used - em * 2.).max(0.);
        let remaining = self.columns.len().saturating_sub(1).max(1) as f32;
        for (index, column) in self.columns.iter_mut().enumerate() {
            column.width += px(if index == 0 {
                spare / 2.
            } else {
                spare / (2. * remaining)
            });
        }
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
    fn render_th(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let field = &self.fields[index];
        let mut cell = div()
            .size_full()
            .px_2()
            .flex()
            .flex_col()
            .justify_center()
            .text_size(cx.theme().font_size)
            .when(index > 0, |d| d.items_end());
        if self.page == "processes"
            && let Some(owner) = self.owner.upgrade()
        {
            let app = owner.read(cx);
            let sys = &app.snapshot["system"];
            let total = match field.key {
                "name" => {
                    let count = tr!(
                        "processes.total",
                        count = number(sys, "processCount") as usize
                    );
                    if app.query.is_empty() {
                        count
                    } else {
                        format!("{count} · {}", label("processes.filtered"))
                    }
                }
                "cpu" => format!("{}%", theme::cpu(number(sys, "cpu"))),
                "mem" => format!(
                    "{}%",
                    theme::cpu(number(sys, "memUsed") / number(sys, "memTotal").max(1.) * 100.)
                ),
                "gpu" => format!(
                    "{}%",
                    theme::cpu(
                        values(&app.snapshot["hardware"]["gpus"])
                            .iter()
                            .map(|g| number(g, "util"))
                            .fold(0., f64::max)
                    )
                ),
                key if matches!(key, "diskRead" | "diskWrite" | "io" | "net") => {
                    let hardware = &app.snapshot["hardware"];
                    match key {
                        "diskRead" => values(&hardware["disks"])
                            .iter()
                            .filter_map(|d| d["read"].as_f64())
                            .reduce(|a, b| a + b)
                            .map(|v| theme::bytes(v, true))
                            .unwrap_or_else(|| label("format.missing")),
                        "diskWrite" => values(&hardware["disks"])
                            .iter()
                            .filter_map(|d| d["write"].as_f64())
                            .reduce(|a, b| a + b)
                            .map(|v| theme::bytes(v, true))
                            .unwrap_or_else(|| label("format.missing")),
                        "io" => values(&hardware["disks"])
                            .iter()
                            .map(|d| {
                                let r = d["read"].as_f64().unwrap_or(0.);
                                let w = d["write"].as_f64().unwrap_or(0.);
                                r + w
                            })
                            .reduce(|a, b| a + b)
                            .map(|v| theme::bytes(v, true))
                            .unwrap_or_else(|| label("format.missing")),
                        "net" => {
                            let rx = values(&hardware["nics"])
                                .iter()
                                .map(|n| number(n, "rx"))
                                .sum::<f64>();
                            let tx = values(&hardware["nics"])
                                .iter()
                                .map(|n| number(n, "tx"))
                                .sum::<f64>();
                            let bits = app.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
                            theme::network(rx + tx, bits)
                        }
                        _ => String::new(),
                    }
                }
                _ => String::new(),
            };
            cell = cell.child(div().text_color(cx.theme().foreground).child(total));
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
            .gap_1()
            .text_xs()
            .when(sort.0, |d| {
                d.font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().foreground)
            })
            .child(field.label.clone())
            .when(sort.0, |d| {
                d.child(
                    icon(
                        if sort.1 { "ChevronDown" } else { "ChevronUp" },
                        cx.theme().foreground,
                    )
                    .size_3(),
                )
            });
        let hover = cx.theme().table_hover;
        let header_bounds = owner
            .upgrade()
            .map(|owner| owner.read(cx).header_bounds.clone());
        let measured_key = key.clone();
        Button::new(("column-sort", index))
            .size_full()
            .min_w_0()
            .rounded_none()
            .cursor_pointer()
            .bg(gpui_kit::gpui::transparent_black())
            .on_prepaint(move |bounds, _, _| {
                if let Some(headers) = &header_bounds {
                    headers.borrow_mut().insert(measured_key.clone(), bounds);
                }
            })
            .hover(move |s| s.bg(hover))
            .accessibility_label(field.label.clone())
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
                    if value == 0. {
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
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .py_0()
            .overflow_hidden();
        if self.page == "processes"
            && row.section.is_none()
            && matches!(
                field.kind,
                CellKind::Percent | CellKind::Memory | CellKind::Rate
            )
            && let Some(value) = row.metric(field.key)
        {
            element = element.bg(Palette::current(cx)
                .heat
                .opacity(theme::heat_alpha(field.key, value)));
        }
        if column == 0 {
            let process_icon = row.raw["icon"]
                .as_u64()
                .map(|id| id as u32)
                .and_then(|id| self.icons.get(&id).cloned());
            if row.expandable {
                let key = row.key.clone();
                element = element.child(
                    Button::new(("fold", row_index))
                        .child(icon(
                            if row.expanded {
                                "ChevronDown"
                            } else {
                                "ChevronRight"
                            },
                            cx.theme().muted_foreground,
                        ))
                        .on_click(move |_, window, cx| {
                            let owner = owner.clone();
                            let key = key.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = owner
                                    .update(cx, |this, cx| this.act(Action::Fold(key), window, cx));
                            });
                        }),
                );
            } else if self.page == "processes" {
                element = element.child(div().w_4().flex_shrink_0());
            }
            if row.section.is_none() {
                element = element.child(div().w_4().h_4().flex_shrink_0().child(
                    if let Some(image) = process_icon {
                        img(image).size_4().into_any_element()
                    } else {
                        icon(
                            if self.page == "processes" {
                                "AppIcon"
                            } else if self.page == "services" {
                                "ServicesIcon"
                            } else {
                                "PowerIcon"
                            },
                            cx.theme().muted_foreground,
                        )
                        .into_any_element()
                    },
                ));
            }
            element = element
                .pl(px(8.) + px(row.depth as f32 * f32::from(cx.theme().font_size)))
                .when(row.section.is_some() || row.members.len() > 1, |d| {
                    d.font_weight(FontWeight::SEMIBOLD)
                })
                .child(
                    div()
                        .min_w_0()
                        .when(!row.detail.is_empty(), |d| d.max_w(relative(0.7)))
                        .truncate()
                        .child(value),
                );
            if row.members.len() > 1 && row.section.is_none() {
                element = element.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .bg(cx.theme().secondary)
                        .rounded_full()
                        .px_2()
                        .text_xs()
                        .flex_shrink_0()
                        .child(row.members.len().to_string()),
                );
            }
            if row.section.is_some() {
                let p = Palette::current(cx);
                element =
                    element
                        .text_sm()
                        .child(
                            div()
                                .px_2()
                                .rounded_full()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .bg(p.accent.opacity(0.14))
                                .text_color(p.accent)
                                .child(row.raw["entryCount"].to_string()),
                        )
                        .child(div().flex_1().h(px(1.)).ml_1().bg(
                            gpui_kit::gpui::linear_gradient(
                                90.,
                                gpui_kit::gpui::linear_color_stop(p.stroke, 0.),
                                gpui_kit::gpui::linear_color_stop(p.stroke.opacity(0.), 1.),
                            ),
                        ));
            }
            if !row.detail.is_empty() {
                element = element.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .font_weight(FontWeight::NORMAL)
                        .child(row.detail.clone()),
                );
            }
            if self.page == "startup" && boolean(&row.raw, "missing") {
                let p = Palette::current(cx);
                element = element.child(
                    div()
                        .px_2()
                        .rounded_full()
                        .text_xs()
                        .bg(p.danger.opacity(0.14))
                        .text_color(p.danger)
                        .flex_shrink_0()
                        .child(label("startup.missing.label")),
                );
            }
        } else if matches!(field.kind, CellKind::Enabled) {
            let enabled = boolean(&row.raw, "enabled");
            let id = row.raw["id"].clone();
            let p = Palette::current(cx);
            element = element.child(
                Button::new(("startup-toggle", row_index))
                    .px_2()
                    .py_0()
                    .h_5()
                    .rounded_full()
                    .text_xs()
                    .bg(if enabled {
                        p.accent.opacity(0.16)
                    } else {
                        p.control
                    })
                    .text_color(if enabled { p.accent } else { p.muted })
                    .child(value)
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
        } else {
            element = element
                .when(row.section.is_some(), |d| {
                    d.text_xs().text_color(Palette::current(cx).faint)
                })
                .when(row.metric(field.key) == Some(0.), |d| {
                    d.text_color(Palette::current(cx).faint)
                })
                .when(
                    matches!(
                        field.kind,
                        CellKind::Count
                            | CellKind::Pid
                            | CellKind::Port
                            | CellKind::Percent
                            | CellKind::Memory
                            | CellKind::Rate
                    ),
                    |d| d.justify_end(),
                )
                .child(value);
        }
        element
    }
    fn render_tr(
        &mut self,
        index: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div().id(("native-row", index))
    }
    fn context_menu(
        &mut self,
        index: usize,
        mut menu: PopupMenu,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        if let Some(owner) = self.owner.upgrade() {
            let items = owner.read(cx).row_menu(&self.rows[index]);
            for item in items {
                let owner = owner.downgrade();
                let action = item.action.clone();
                menu = menu.item(
                    PopupMenuItem::new(item.label)
                        .disabled(item.disabled)
                        .checked(item.checked)
                        .on_click(move |_, window, cx| {
                            let owner = owner.clone();
                            let action = action.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = owner.update(cx, |this, cx| this.act(action, window, cx));
                            });
                        }),
                );
            }
        }
        menu
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
                },
            )
        } else {
            let data_key = match self.page.as_str() {
                "services" => "services",
                "startup" => "startup",
                "network" => "connections",
                _ => "",
            };
            let query = self.query.to_lowercase();
            let source = self.data.get(data_key).map(values).unwrap_or_default();
            let filter = self
                .filters
                .get(&self.page)
                .map(String::as_str)
                .unwrap_or("all");
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
                .filter(|row| {
                    if query.is_empty() {
                        return true;
                    }
                    if self.page == "network" {
                        if let Ok(num) = query.parse::<u64>()
                            && (row["localPort"].as_u64() == Some(num)
                                || row["remotePort"].as_u64() == Some(num)
                                || row["pid"].as_u64() == Some(num))
                        {
                            return true;
                        }
                        let name = values(&self.snapshot["processes"])
                            .iter()
                            .find(|p| p["pid"] == row["pid"])
                            .map(model::title)
                            .unwrap_or_default();
                        return name.to_lowercase().contains(&query)
                            || row.to_string().to_lowercase().contains(&query);
                    }
                    row.to_string().to_lowercase().contains(&query)
                })
                .cloned()
                .map(|raw| {
                    let mut row = Row::process(raw, 0);
                    match self.page.as_str() {
                        "services" => {
                            row.key = text(&row.raw, "name").into();
                            row.title = text(&row.raw, "displayName").into();
                            row.mark_key = Some(row.key.clone());
                        }
                        "startup" => {
                            row.key = text(&row.raw, "id").into();
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
                            row.title = if row.raw["pid"].as_u64() == Some(0) {
                                "System Idle Process".into()
                            } else {
                                values(&self.snapshot["processes"])
                                    .iter()
                                    .find(|p| p["pid"] == row.raw["pid"])
                                    .map(model::title)
                                    .unwrap_or_else(|| {
                                        format!(
                                            "PID {} ({})",
                                            row.raw["pid"],
                                            label("connections.exited")
                                        )
                                    })
                            };
                            if let Some(process) = values(&self.snapshot["processes"])
                                .iter()
                                .find(|p| p["pid"] == row.raw["pid"])
                            {
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
                    row
                })
                .collect();
            rows.sort_by(|a, b| {
                let key = if self.sort == "name" {
                    "name"
                } else {
                    &self.sort
                };
                let order = if key == "name" {
                    a.title.to_lowercase().cmp(&b.title.to_lowercase())
                } else if matches!(key, "local" | "remote") {
                    let address = format!("{key}Address");
                    let port = format!("{key}Port");
                    text(&a.raw, &address)
                        .cmp(text(&b.raw, &address))
                        .then_with(|| number(&a.raw, &port).total_cmp(&number(&b.raw, &port)))
                } else {
                    match (a.raw[key].as_f64(), b.raw[key].as_f64()) {
                        (Some(a), Some(b)) => a.total_cmp(&b),
                        _ => text(&a.raw, key)
                            .to_lowercase()
                            .cmp(&text(&b.raw, key).to_lowercase()),
                    }
                };
                if self.descending {
                    order.reverse()
                } else {
                    order
                }
            });
            rows
        };
        let marks = self.marks();
        if self.page == "processes" {
            let mut hosted: HashMap<u64, Vec<String>> = HashMap::new();
            for service in self.data.get("services").map(values).unwrap_or_default() {
                if let Some(pid) = service["pid"].as_u64().filter(|pid| *pid != 0) {
                    hosted
                        .entry(pid)
                        .or_default()
                        .push(text(service, "displayName").to_owned());
                }
            }
            for row in &mut rows {
                if text(&row.raw, "name").eq_ignore_ascii_case("svchost.exe")
                    && let Some(names) = row.raw["pid"].as_u64().and_then(|pid| hosted.get(&pid))
                {
                    row.title = tr!(
                        "processes.serviceHost",
                        names = names.join(&label("common.listSeparator"))
                    );
                }
            }
        }
        for row in &mut rows {
            if let Some(key) = &row.mark_key
                && let Some(note) = marks[key]["note"].as_str()
            {
                row.detail = note.into();
            }
        }
        if self.page != "processes" {
            rows.sort_by_key(|row| {
                !row.mark_key
                    .as_ref()
                    .is_some_and(|key| boolean(&marks[key], "pinned"))
            });
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
            self.list.reset_with_uniform_height(rows.len(), px(30.));
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
                            let action = if this.page == "processes" {
                                Action::Fold(row.key.clone())
                            } else if this.page == "startup" {
                                Action::Invoke(
                                    "set_startup_enabled".into(),
                                    json!({
                                        "id": row.raw["id"],
                                        "enabled": !boolean(&row.raw, "enabled")
                                    }),
                                )
                            } else {
                                Action::Preference("revealProcess".into(), row.raw.clone())
                            };
                            this.act(action, window, cx);
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
                192.
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
