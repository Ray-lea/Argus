//! The processes of the moment or the range the page describes, grouped by
//! name (a group opens to its instances), with the columns chosen for each
//! mode and a trend that follows the topmost lane's reading. A row's menu goes
//! to the live process or copies its name; a double click goes there too.
use super::state::TableMode;
use super::*;
use widgets::Tone;

/// Points in each process's trend.
pub(super) const TREND_POINTS: usize = 32;
const ROW: f32 = 28.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Percent,
    Bytes,
    /// Bytes per second.
    Rate,
    /// Bytes over the range.
    Total,
    Power,
    Count,
    Duration,
}

#[derive(Clone, Copy)]
struct Column {
    key: &'static str,
    /// The heat a cell takes, by `theme::heat_alpha`'s field names.
    heat: &'static str,
    kind: Kind,
    width: f32,
    shown: bool,
    /// Summed in a group's row (else its highest).
    additive: bool,
}

const fn column(
    key: &'static str,
    heat: &'static str,
    kind: Kind,
    width: f32,
    shown: bool,
    additive: bool,
) -> Column {
    Column {
        key,
        heat,
        kind,
        width,
        shown,
        additive,
    }
}

const MOMENT: [Column; 12] = [
    column("cpu", "cpu", Kind::Percent, 64., true, true),
    column("mem", "mem", Kind::Bytes, 84., true, true),
    column("disk", "diskRead", Kind::Rate, 84., true, true),
    column("diskRead", "diskRead", Kind::Rate, 84., false, true),
    column("diskWrite", "diskWrite", Kind::Rate, 84., false, true),
    column("io", "io", Kind::Rate, 80., false, true),
    column("net", "net", Kind::Rate, 84., true, true),
    column("gpu", "gpu", Kind::Percent, 60., true, true),
    column("gpuMem", "gpuDedicated", Kind::Bytes, 84., false, true),
    column("power", "power", Kind::Power, 100., true, false),
    column("threads", "", Kind::Count, 64., false, true),
    column("handles", "", Kind::Count, 72., false, true),
];

const RANGE: [Column; 14] = [
    column("cpu", "cpu", Kind::Percent, 82., true, true),
    column("cpuMax", "cpu", Kind::Percent, 82., true, false),
    column("mem", "mem", Kind::Bytes, 86., true, false),
    column("disk", "diskRead", Kind::Total, 94., true, true),
    column("diskRead", "diskRead", Kind::Total, 94., false, true),
    column("diskWrite", "diskWrite", Kind::Total, 94., false, true),
    column("io", "io", Kind::Total, 84., false, true),
    column("net", "net", Kind::Total, 94., true, true),
    column("gpu", "gpu", Kind::Percent, 80., true, true),
    column("gpuMem", "gpuDedicated", Kind::Bytes, 84., false, false),
    column("power", "power", Kind::Power, 100., true, false),
    column("seen", "", Kind::Duration, 96., true, false),
    column("threads", "", Kind::Count, 64., false, false),
    column("handles", "", Kind::Count, 72., false, false),
];

fn columns(mode: TableMode) -> &'static [Column] {
    match mode {
        TableMode::Moment => &MOMENT,
        TableMode::Range => &RANGE,
    }
}

/// The columns shown in `mode`: as saved, else the defaults.
pub(super) fn shown_columns(saved: &Value, mode: TableMode) -> Vec<&'static str> {
    let all = columns(mode);
    match saved.as_array() {
        Some(list) => {
            let shown = all
                .iter()
                .filter(|c| list.iter().any(|k| k.as_str() == Some(c.key)))
                .map(|c| c.key)
                .collect::<Vec<_>>();
            if shown.is_empty() {
                all.iter().filter(|c| c.shown).map(|c| c.key).collect()
            } else {
                shown
            }
        }
        None => all.iter().filter(|c| c.shown).map(|c| c.key).collect(),
    }
}

/// A process's value in a column; a disk total is its read and write together.
fn value_of(process: &Value, key: &str) -> Option<f64> {
    if key == "disk" {
        let read = process["diskRead"].as_f64();
        let write = process["diskWrite"].as_f64();
        return match (read, write) {
            (None, None) => process["disk"].as_f64(),
            (a, b) => Some(a.unwrap_or(0.) + b.unwrap_or(0.)),
        };
    }
    process[key].as_f64()
}

/// A group of instances with one name, and its values in the shown columns.
struct Group<'a> {
    name: String,
    instances: Vec<&'a Value>,
    values: Vec<Option<f64>>,
    trend: Option<Vec<Option<f64>>>,
}

impl Desktop {
    /// The title over the table: whose processes these are.
    fn history_table_title(&self) -> String {
        let now = self.history_now();
        match self.history.mode {
            TableMode::Moment => {
                let at = self
                    .history
                    .frame
                    .as_ref()
                    .and_then(|frame| {
                        frame["ts"]
                            .as_i64()
                            .or_else(|| frame["system"]["ts"].as_i64())
                    })
                    .unwrap_or_else(|| self.history_moment());
                let time = with_offset(
                    moment_label(at, now, true),
                    at,
                    self.history_offsets_differ(),
                );
                if self.history.pin.is_none() && self.history.follow {
                    tr!("history.table.latest", time = time)
                } else {
                    tr!("history.table.moment", time = time)
                }
            }
            TableMode::Range => {
                let (from, to) = self.history_table_range();
                tr!(
                    "history.table.range",
                    range = range_label(from, to, now, to - from < series::HOUR)
                )
            }
        }
    }

    pub(super) fn history_table(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let mode = self.history.mode;
        let bits = self.history_bits();
        let pref = if mode == TableMode::Range {
            "history.columns.range"
        } else {
            "history.columns.moment"
        };
        let shown_keys = shown_columns(&self.prefs.get::<Value>(pref, Value::Null), mode);
        let shown = columns(mode)
            .iter()
            .filter(|c| shown_keys.contains(&c.key))
            .copied()
            .collect::<Vec<_>>();
        let answer = match mode {
            TableMode::Moment => self.history.frame.clone(),
            TableMode::Range => self.history.table.clone(),
        };
        let processes = answer
            .as_ref()
            .map(|a| values(&a["processes"]).to_vec())
            .unwrap_or_default();
        let frame = answer
            .as_ref()
            .and_then(|a| a["ts"].as_i64().or_else(|| a["system"]["ts"].as_i64()));
        let seconds = match mode {
            TableMode::Moment => 1.,
            TableMode::Range => {
                let (from, to) = self.history_table_range();
                answer
                    .as_ref()
                    .and_then(|a| a["recorded"].as_f64())
                    .unwrap_or((to - from) as f64)
                    .max(1000.)
                    / 1000.
            }
        };
        let trend_key = self.history_trend_key();
        let trend_color = match trend_key {
            "mem" => p.memory,
            "disk" => p.io,
            "net" => p.network,
            "gpu" => p.gpu,
            _ => p.cpu,
        };
        let has_trend = mode == TableMode::Range;
        // Groups by name, their values summed or their highest.
        let mut groups: Vec<Group> = Vec::new();
        for process in &processes {
            let name = text(process, "name").to_owned();
            match groups
                .iter_mut()
                .find(|g| g.name.eq_ignore_ascii_case(&name))
            {
                Some(group) => group.instances.push(process),
                None => groups.push(Group {
                    name,
                    instances: vec![process],
                    values: Vec::new(),
                    trend: None,
                }),
            }
        }
        for group in &mut groups {
            group.values = shown
                .iter()
                .map(|column| {
                    let values = group
                        .instances
                        .iter()
                        .filter_map(|p| value_of(p, column.key))
                        .collect::<Vec<_>>();
                    if values.is_empty() {
                        None
                    } else if column.additive {
                        Some(values.iter().sum())
                    } else {
                        values.into_iter().reduce(f64::max)
                    }
                })
                .collect();
            if has_trend {
                let mut sum: Vec<Option<f64>> = Vec::new();
                for instance in &group.instances {
                    for (i, v) in values(&instance["trend"]).iter().enumerate() {
                        if sum.len() <= i {
                            sum.resize(i + 1, None);
                        }
                        if let Some(v) = v.as_f64() {
                            sum[i] = Some(sum[i].unwrap_or(0.) + v);
                        }
                    }
                }
                group.trend = (!sum.is_empty()).then_some(sum);
            }
        }
        // The order survives restarts (`tableSort.history`).
        let (sort_key, descending) = {
            let saved = tables::saved_sort("history", &self.prefs);
            if saved.0 == "name" || shown.iter().any(|c| c.key == saved.0) {
                saved
            } else {
                ("cpu".to_owned(), true)
            }
        };
        let sort_index = shown.iter().position(|c| c.key == sort_key);
        groups.sort_by(|a, b| {
            let order = match sort_index {
                Some(i) => a.values[i]
                    .unwrap_or(f64::NEG_INFINITY)
                    .total_cmp(&b.values[i].unwrap_or(f64::NEG_INFINITY)),
                None => collation::compare(&a.name, &b.name),
            };
            if descending { order.reverse() } else { order }
        });
        let format = |column: &Column, value: Option<f64>| -> String {
            let Some(v) = value else {
                return label("format.missing");
            };
            match column.kind {
                Kind::Percent => theme::cpu(v),
                Kind::Bytes => theme::memory(v),
                Kind::Rate => {
                    if column.key == "net" {
                        if v * if bits { 8. } else { 1. } < 1. {
                            "0".into()
                        } else {
                            charts::net_rate(v, bits)
                        }
                    } else if v < 1. {
                        "0".into()
                    } else {
                        charts::byte_rate(v)
                    }
                }
                Kind::Total => {
                    if v < 512. {
                        "0".into()
                    } else {
                        theme::memory(v)
                    }
                }
                Kind::Power => columns::power_label(v),
                Kind::Count => crate::regional::count(v),
                Kind::Duration => duration_label(v as i64),
            }
        };
        let heat = |column: &Column, value: Option<f64>| -> f32 {
            let Some(v) = value else { return 0. };
            if column.heat.is_empty() {
                return 0.;
            }
            let v = if column.kind == Kind::Total {
                v / seconds
            } else {
                v
            };
            theme::heat_alpha(column.heat, v)
        };
        let live = values(&self.snapshot["processes"]);
        let mut by_name: HashMap<String, &Value> = HashMap::new();
        for process in live {
            by_name
                .entry(text(process, "name").to_lowercase())
                .or_insert(process);
        }
        let cell = |width: f32| {
            div()
                .px(px(8.))
                .w(px(width))
                .flex_shrink_0()
                .text_right()
                .truncate()
        };
        // The head: name, the shown columns, the trend.
        let mut head = div()
            .flex()
            .flex_shrink_0()
            .h(px(30.))
            .items_center()
            .pl(px(6.))
            .pr(px(6. + scrollbar::SIZE))
            .border_b_1()
            .border_color(p.stroke)
            .text_size(theme::font(12.))
            .text_color(p.muted)
            .child(self.history_head_cell(
                "name",
                label("common.name"),
                None,
                &sort_key,
                descending,
                cx,
            ));
        for column in &shown {
            head = head.child(self.history_head_cell(
                column.key,
                label(&format!(
                    "history.column.{}.{}",
                    if mode == TableMode::Range {
                        "range"
                    } else {
                        "moment"
                    },
                    column.key
                )),
                Some(column.width),
                &sort_key,
                descending,
                cx,
            ));
        }
        if has_trend {
            head = head.child(
                div()
                    .w(px(96.))
                    .flex_shrink_0()
                    .px(px(8.))
                    .truncate()
                    .child(tr!(
                        "history.column.trend",
                        name = label(&format!("history.trend.{trend_key}"))
                    )),
            );
        }
        let rows_scroll = window
            .use_keyed_state("history-processes-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let mut body = div()
            .id("history-processes")
            .track_scroll(&rows_scroll)
            .flex_1()
            .min_h(px(ROW * 3.))
            .overflow_y_scroll()
            .pr(px(scrollbar::SIZE))
            .flex()
            .flex_col();
        if groups.is_empty() {
            body = body.child(
                div()
                    .p_4()
                    .text_color(p.faint)
                    .child(label(if answer.is_none() {
                        "history.table.loading"
                    } else {
                        "history.noRecordAt"
                    })),
            );
        }
        for group in &groups {
            let expandable = group.instances.len() > 1;
            let expanded = expandable && self.history.expanded.contains(&group.name);
            let icon_process = by_name.get(&group.name.to_lowercase()).copied();
            let key = group.name.clone();
            let selected = self.history.selected.as_deref() == Some(key.as_str());
            let single = (group.instances.len() == 1).then(|| group.instances[0]);
            let mut line = div()
                .id(SharedString::from(format!("history-row-{key}")))
                .flex()
                .flex_shrink_0()
                .h(px(ROW))
                .items_center()
                .px(px(6.))
                .when(selected, |d| d.bg(p.control))
                .when(!selected, |d| d.hover(move |s| s.bg(p.hover)))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(200.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(12.)).flex_shrink_0().when(expandable, |d| {
                            d.child(
                                icon(
                                    if expanded {
                                        "ChevronDown"
                                    } else {
                                        "ChevronRight"
                                    },
                                    p.muted,
                                )
                                .size(px(12.)),
                            )
                        }))
                        .child(self.process_icon(icon_process, 16., cx))
                        .child(div().min_w_0().truncate().child(group.name.clone()))
                        .when(expandable, |d| {
                            d.child(
                                div()
                                    .text_color(p.faint)
                                    .flex_shrink_0()
                                    .child(format!("({})", group.instances.len())),
                            )
                        })
                        .when_some(single, |d, process| {
                            d.child(
                                div()
                                    .text_size(theme::font(11.))
                                    .text_color(p.faint)
                                    .flex_shrink_0()
                                    .child(tr!(
                                        "history.table.pid",
                                        pid = process["pid"].to_string()
                                    )),
                            )
                        }),
                );
            for (column, value) in shown.iter().zip(&group.values) {
                let alpha = heat(column, *value);
                line = line.child(
                    cell(column.width)
                        .when(alpha > 0., |d| d.bg(p.heat.opacity(alpha)))
                        .child(format(column, *value)),
                );
            }
            if has_trend {
                line = line.child(sparkline(group.trend.clone(), trend_color));
            }
            let target = single.and_then(|process| recorded_target(process, frame, live));
            let running = target.is_some();
            let reveal = Action::Preference("revealProcess".into(), target.unwrap_or(Value::Null));
            let menu = vec![
                MenuItem::new(label("common.goToProcess"), reveal.clone()).disabled(!running),
                MenuItem::new(
                    label("common.copyName"),
                    Action::Copy(group.name.clone(), "history.copied.name"),
                ),
            ];
            let exited = tr!(
                "history.instanceUnavailable",
                name = group.name.clone(),
                pid = single.map(|s| s["pid"].to_string()).unwrap_or_default()
            );
            let toggle_key = key.clone();
            let select_key = key.clone();
            body = body.child(
                line.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    this.history.selected = Some(select_key.clone());
                    if expandable && !this.history.expanded.remove(&toggle_key) {
                        this.history.expanded.insert(toggle_key.clone());
                    }
                    if event.click_count() >= 2 && !expandable {
                        if running {
                            this.act(reveal.clone(), window, cx);
                        } else {
                            this.notify_toast(widgets::ToastKind::Info, exited.clone());
                        }
                    }
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        this.act(Action::Popup(menu.clone()), window, cx);
                    }),
                ),
            );
            if expanded {
                let mut instances = group.instances.clone();
                if let Some(i) = sort_index {
                    let key = shown[i].key;
                    instances.sort_by(|a, b| {
                        value_of(b, key)
                            .unwrap_or(f64::NEG_INFINITY)
                            .total_cmp(&value_of(a, key).unwrap_or(f64::NEG_INFINITY))
                    });
                }
                for process in instances {
                    let mut line = div()
                        .flex()
                        .flex_shrink_0()
                        .h(px(ROW))
                        .items_center()
                        .px(px(6.))
                        .text_color(p.muted)
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(200.))
                                .pl(px(48.))
                                .pr(px(8.))
                                .truncate()
                                .child(tr!("history.table.pid", pid = process["pid"].to_string())),
                        );
                    for column in &shown {
                        let value = value_of(process, column.key);
                        line = line.child(cell(column.width).child(format(column, value)));
                    }
                    if has_trend {
                        let trend = process["trend"]
                            .as_array()
                            .map(|list| list.iter().map(Value::as_f64).collect::<Vec<_>>());
                        line = line.child(sparkline(trend, trend_color));
                    }
                    let target = recorded_target(process, frame, live);
                    let running = target.is_some();
                    let reveal =
                        Action::Preference("revealProcess".into(), target.unwrap_or(Value::Null));
                    let menu = vec![
                        MenuItem::new(label("common.goToProcess"), reveal).disabled(!running),
                        MenuItem::new(
                            label("common.copyName"),
                            Action::Copy(text(process, "name").to_owned(), "history.copied.name"),
                        ),
                    ];
                    body = body.child(line.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, window, cx| {
                            this.act(Action::Popup(menu.clone()), window, cx);
                        }),
                    ));
                }
            }
        }
        // The card's head: whose processes, the mode, the columns.
        let clearing = self.history.pin.is_some() || self.history.selection.is_some();
        let mode_choice = self.segments(
            "history-table-mode",
            vec![
                (
                    label("history.table.modeMoment"),
                    None,
                    Action::History(Command::Mode(false)),
                    mode == TableMode::Moment,
                ),
                (
                    label("history.table.modeRange"),
                    None,
                    Action::History(Command::Mode(true)),
                    mode == TableMode::Range,
                ),
            ],
            cx,
        );
        let chooser_items = columns(mode)
            .iter()
            .map(|column| {
                MenuItem::new(
                    label(&format!(
                        "history.column.{}.{}",
                        if mode == TableMode::Range {
                            "range"
                        } else {
                            "moment"
                        },
                        column.key
                    )),
                    Action::History(Command::Column(column.key)),
                )
                .checked(shown_keys.contains(&column.key))
                .stay()
            })
            .collect::<Vec<_>>();
        let heading = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(10.))
            .gap_y_1()
            .pt(px(8.))
            .px(px(14.))
            .pb(px(6.))
            .child(
                div()
                    .text_size(theme::font(14.))
                    .font_weight(theme::LABEL_WEIGHT)
                    .child(self.history_table_title()),
            )
            .when(clearing, |d| {
                d.child(
                    div()
                        .id("history-clear")
                        .size(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(p.hover))
                        .child(icon("CloseIcon", p.muted).size(px(13.)))
                        .tooltip(|window, cx| {
                            widgets::tooltip(label("history.clearCursor"), window, cx)
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.history_command(Command::ClearCursor, window, cx)
                        })),
                )
            })
            .child(mode_choice)
            .child(div().flex_1())
            .child(
                div()
                    .relative()
                    .child(
                        self.btn_small(
                            "history-columns",
                            label("history.table.columns"),
                            Some("ColumnsIcon"),
                            Tone::Plain,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.open_anchored(
                                    chooser_items.clone(),
                                    "history-columns",
                                    layout::Side::Below,
                                    window,
                                    cx,
                                );
                            },
                        )),
                    )
                    .child(layout::anchor_probe(
                        "history-columns",
                        self.ui.anchors.clone(),
                    )),
            );
        let recording_off = self
            .data
            .get("settings")
            .and_then(|s| s["history"].as_bool())
            == Some(false);
        let foot = div()
            .flex()
            .flex_wrap()
            .justify_between()
            .gap_x_4()
            .gap_y_1()
            .px(px(14.))
            .py(px(8.))
            .border_t_1()
            .border_color(p.stroke)
            .text_size(theme::font(12.))
            .text_color(p.muted)
            .when_some(self.history.first, |d, ts| {
                d.child(
                    div().flex().gap_1().child(label("history.earliest")).child(
                        div()
                            .font_weight(theme::LABEL_WEIGHT)
                            .text_color(p.ink)
                            .child(moment_label(ts, self.history_now(), false)),
                    ),
                )
            })
            .child(if recording_off {
                let text = label("history.off");
                let (before, rest) = text.split_once("<link>").unwrap_or((&text, ""));
                let (link, after) = rest.split_once("</link>").unwrap_or((rest, ""));
                div()
                    .flex()
                    .items_center()
                    .child(before.to_owned())
                    .child(
                        div()
                            .id("history-open-settings")
                            .text_color(p.accent)
                            .cursor_pointer()
                            .child(link.to_owned())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.history_command(Command::Settings, window, cx)
                            })),
                    )
                    .child(after.to_owned())
                    .into_any_element()
            } else {
                div().child(label("history.note")).into_any_element()
            });
        div()
            .id("history-table-card")
            .flex_1()
            .min_h(px(36. + 30. + ROW * 3. + 34.))
            .flex()
            .flex_col()
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.layer)
            .overflow_hidden()
            .child(heading)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(head)
                    .child(body.child(scrollbar::scrollbar(
                        "history-processes-scrollbar",
                        &rows_scroll,
                        p.dark,
                    ))),
            )
            .child(foot)
            .into_any_element()
    }

    fn history_head_cell(
        &self,
        key: &'static str,
        name: String,
        width: Option<f32>,
        sort_key: &str,
        descending: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let active = key == sort_key;
        let arrow = if !active {
            ""
        } else if descending {
            " ↓"
        } else {
            " ↑"
        };
        let next = if active { !descending } else { key != "name" };
        div()
            .id(SharedString::from(format!("history-head-{key}")))
            .px(px(8.))
            .truncate()
            .cursor_pointer()
            .when_some(width, |d, w| d.w(px(w)).flex_shrink_0().text_right())
            .when(width.is_none(), |d| d.flex_1().min_w(px(200.)).pl(px(28.)))
            .when(active, |d| d.text_color(p.ink))
            .child(format!("{name}{arrow}"))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some((key, value)) = tables::sort_preference("history", key, next) {
                    this.prefs.set(&key, value);
                }
                cx.notify();
            }))
            .into_any_element()
    }
}

/// A process's trend: a small area, scaled to its own highest value.
fn sparkline(trend: Option<Vec<Option<f64>>>, color: Hsla) -> AnyElement {
    let cell = div()
        .w(px(96.))
        .flex_shrink_0()
        .h(px(ROW))
        .px(px(8.))
        .py(px(6.));
    let Some(trend) = trend.filter(|t| t.iter().any(Option::is_some)) else {
        return cell.into_any_element();
    };
    let top = trend.iter().flatten().copied().fold(0., f64::max).max(1e-9);
    cell.child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let n = trend.len().max(2) - 1;
                let at = |i: usize, v: f64| {
                    point(
                        bounds.left() + bounds.size.width * (i as f32 / n as f32),
                        bounds.bottom() - bounds.size.height * (v / top).clamp(0., 1.) as f32,
                    )
                };
                let mut area = PathBuilder::fill();
                let mut line = PathBuilder::stroke(px(1.25));
                let mut started = false;
                for (i, value) in trend.iter().enumerate() {
                    let v = value.unwrap_or(0.);
                    if !started {
                        area.move_to(point(at(i, 0.).x, bounds.bottom()));
                        line.move_to(at(i, v));
                        started = true;
                    } else {
                        line.line_to(at(i, v));
                    }
                    area.line_to(at(i, v));
                }
                area.line_to(point(bounds.right(), bounds.bottom()));
                area.close();
                if let Ok(path) = area.build() {
                    window.paint_path(path, color.opacity(0.18));
                }
                if let Ok(path) = line.build() {
                    window.paint_path(path, color.opacity(0.9));
                }
            },
        )
        .size_full(),
    )
    .into_any_element()
}
