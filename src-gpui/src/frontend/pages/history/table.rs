//! The processes of the moment or the range the page describes, grouped by
//! name (a group opens to its four busiest instances and the rest), with the
//! columns chosen for each mode, a search, and a trend over the window that
//! follows the topmost lane's reading. A row's menu lays its curve over the
//! lanes, goes to the live process or copies its name and PIDs.
use super::state::{Section, TableMode};
use super::*;
use widgets::Tone;

/// Points in each process's trend.
pub(super) const TREND_POINTS: usize = 32;
const ROW: f32 = 28.;
/// The trend column, beside the name.
const TREND_WIDTH: f32 = 96.;
/// The groups listed while the table follows the running processes.
const LIVE_GROUPS: usize = 40;

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

/// A process's reading for the trend or tooltip key: CPU and GPU in percent,
/// memory in bytes, disk and network in bytes per second (a range's totals
/// divided by `seconds`).
pub(super) fn process_value(process: &Value, key: &str, seconds: Option<f64>) -> Option<f64> {
    let value = match key {
        "disk" => value_of(process, "disk"),
        "net" => process["net"].as_f64(),
        "mem" => process["mem"].as_f64(),
        "gpu" => process["gpu"].as_f64(),
        _ => process["cpu"].as_f64(),
    }?;
    Some(match (key, seconds) {
        ("disk" | "net", Some(seconds)) => value / seconds.max(1.),
        _ => value,
    })
}

/// A value of the trend or tooltip key as the table shows it.
pub(super) fn format_process(key: &str, value: f64, bits: bool) -> String {
    match key {
        "mem" => theme::memory(value),
        "disk" => charts::byte_rate(value),
        "net" => charts::net_rate(value, bits),
        _ => format!("{}%", theme::cpu(value)),
    }
}

/// A line of the table: a category's heading (its groups, whether it is
/// open), or a group.
enum Entry {
    Heading(Section, Vec<usize>, bool),
    Row(usize),
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
        match self.history.mode {
            // Following the latest readings, the table lists the processes as
            // they are now, every second: no moment to name.
            TableMode::Moment if self.history_table_live() => label("history.table.live"),
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
                let time = with_offset(moment_label(at, true), at, self.history_offsets_differ());
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
                    range = range_label(from, to, to - from < series::HOUR)
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
        let mode_key = if mode == TableMode::Range {
            "range"
        } else {
            "moment"
        };
        let pref = format!("history.columns.{mode_key}");
        let shown_keys = shown_columns(&self.prefs.get::<Value>(&pref, Value::Null), mode);
        let shown = columns(mode)
            .iter()
            .filter(|c| shown_keys.contains(&c.key))
            .copied()
            .collect::<Vec<_>>();
        let live = mode == TableMode::Moment && self.history_table_live();
        let answer = match mode {
            TableMode::Moment if live => None,
            TableMode::Moment => self.history.frame.clone(),
            TableMode::Range => self.history.table.clone(),
        };
        let processes = if live {
            self.history_live_processes()
        } else {
            answer
                .as_ref()
                .map(|a| values(&a["processes"]).to_vec())
                .unwrap_or_default()
        };
        // Live processes are the running ones: any of them can be gone to.
        let frame = if live {
            self.snapshot["timestamp"].as_i64()
        } else {
            answer
                .as_ref()
                .and_then(|a| a["ts"].as_i64().or_else(|| a["system"]["ts"].as_i64()))
        };
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
        let has_trend = self.history_trend_shown();
        // Trends: a range's own; in moment mode, each process over the window.
        let trend_source = match mode {
            TableMode::Range => processes.clone(),
            TableMode::Moment => self
                .history
                .trends
                .as_ref()
                .or(if self.options.fixture.is_some() {
                    self.history.table.as_ref()
                } else {
                    None
                })
                .map(|t| values(&t["processes"]).to_vec())
                .unwrap_or_default(),
        };
        let trend_of = |process: &Value| -> Option<Vec<Option<f64>>> {
            process["trend"]
                .as_array()
                .map(|list| list.iter().map(Value::as_f64).collect())
        };
        let mut trend_by_name: HashMap<String, Vec<Option<f64>>> = HashMap::new();
        let mut trend_by_instance: HashMap<(String, String, String), Vec<Option<f64>>> =
            HashMap::new();
        for process in &trend_source {
            let Some(trend) = trend_of(process) else {
                continue;
            };
            let name = text(process, "name").to_lowercase();
            let sum = trend_by_name.entry(name.clone()).or_default();
            if sum.len() < trend.len() {
                sum.resize(trend.len(), None);
            }
            for (i, v) in trend.iter().enumerate() {
                if let Some(v) = v {
                    sum[i] = Some(sum[i].unwrap_or(0.) + v);
                }
            }
            trend_by_instance.insert(
                (
                    name,
                    process["pid"].to_string(),
                    process["createTime"].to_string(),
                ),
                trend,
            );
        }
        let instance_trend = |process: &Value| {
            trend_by_instance
                .get(&(
                    text(process, "name").to_lowercase(),
                    process["pid"].to_string(),
                    process["createTime"].to_string(),
                ))
                .cloned()
        };
        // Per-process network needs the privileged service under a standard user.
        let net_missing = self.snapshot["processNetwork"].as_bool() == Some(false)
            && !processes.iter().any(|p| number(p, "net") > 0.);
        let net_shown = shown.iter().any(|c| c.key == "net");
        // Groups by name, their values summed or their highest.
        let search = self
            .ui
            .inputs
            .get("historySearch")
            .map(|input| input.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        let aggregate = |instances: &[&Value]| -> Vec<Option<f64>> {
            shown
                .iter()
                .map(|column| {
                    let values = instances
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
                .collect()
        };
        let mut groups: Vec<Group> = Vec::new();
        for process in &processes {
            let name = text(process, "name").to_owned();
            if !search.is_empty() && !name.to_lowercase().contains(&search) {
                continue;
            }
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
            group.values = aggregate(&group.instances);
            group.trend = trend_by_name.get(&group.name.to_lowercase()).cloned();
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
        // Every running process is at hand live: the busiest are listed, by
        // the column sorted on (by CPU when sorted by name), in its order.
        if live && groups.len() > LIVE_GROUPS {
            let busy = |group: &Group| match sort_index {
                Some(i) => group.values[i].unwrap_or(f64::NEG_INFINITY),
                None => group
                    .instances
                    .iter()
                    .filter_map(|process| value_of(process, "cpu"))
                    .sum(),
            };
            groups.sort_by(|a, b| busy(b).total_cmp(&busy(a)));
            groups.truncate(LIVE_GROUPS);
        }
        groups.sort_by(|a, b| {
            let order = match sort_index {
                Some(i) => a.values[i]
                    .unwrap_or(f64::NEG_INFINITY)
                    .total_cmp(&b.values[i].unwrap_or(f64::NEG_INFINITY)),
                None => collation::compare(&a.name, &b.name),
            };
            if descending { order.reverse() } else { order }
        });
        // Sorted by name, as on the process page, the rows fall under its
        // categories, each under a heading that folds it (a search shows every
        // match). Running instances say what they are; records are sorted by
        // what their names were seen to be (`state::Kinds`).
        let section_of = |group: &Group| {
            if live {
                Section::of(
                    group
                        .instances
                        .iter()
                        .any(|process| boolean(process, "appWindow")),
                    group
                        .instances
                        .iter()
                        .all(|process| boolean(process, "windowsProcess")),
                )
            } else {
                self.history.kinds.section(&group.name)
            }
        };
        let searching = !search.is_empty();
        let mut entries = Vec::new();
        if sort_index.is_none() {
            for section in Section::ALL {
                let members = (0..groups.len())
                    .filter(|i| section_of(&groups[*i]) == section)
                    .collect::<Vec<_>>();
                if members.is_empty() {
                    continue;
                }
                let open = searching || !self.history.folded.contains(section.key());
                entries.push(Entry::Heading(section, members.clone(), open));
                if open {
                    entries.extend(members.into_iter().map(Entry::Row));
                }
            }
        } else {
            entries.extend((0..groups.len()).map(Entry::Row));
        }
        let format = |column: &Column, value: Option<f64>| -> String {
            if column.key == "net" && net_missing {
                return label("format.missing");
            }
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
            if column.heat.is_empty() || (column.key == "net" && net_missing) {
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
        // A value with its heat filling the whole cell, as on the process
        // page: busy cells of neighbouring rows and columns meet without seams.
        let cell = |width: f32, alpha: f32, quiet: bool, text: String| {
            div()
                .h_full()
                .w(px(width))
                .flex_shrink_0()
                .px(px(8.))
                .flex()
                .items_center()
                .justify_end()
                .when(alpha > 0., |d| d.bg(p.heat.opacity(alpha)))
                .when(quiet, |d| d.text_color(p.faint))
                .child(div().min_w_0().truncate().child(text))
        };
        let column_name = |key: &str| label(&format!("history.column.{mode_key}.{key}"));
        // The head: name, the trend, the shown columns.
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
                false,
                cx,
            ));
        if has_trend {
            head = head.child(
                div()
                    .id("history-head-trend")
                    .w(px(TREND_WIDTH))
                    .flex_shrink_0()
                    .px(px(8.))
                    .truncate()
                    .child(tr!(
                        "history.column.trend",
                        name = label(&format!("history.trend.{trend_key}"))
                    ))
                    .tooltip(|window, cx| {
                        widgets::tooltip(label("history.column.trendTitle"), window, cx)
                    }),
            );
        }
        for column in &shown {
            head = head.child(self.history_head_cell(
                column.key,
                column_name(column.key),
                Some(column.width),
                &sort_key,
                descending,
                column.key == "net" && net_missing,
                cx,
            ));
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
                    } else if mode == TableMode::Range {
                        "history.table.emptyRange"
                    } else {
                        "history.noRecordAt"
                    })),
            );
        }
        let curve = self.history.curve.clone();
        // The menu of a row: its curve on the lanes, the live process, a copy.
        let row_menu = |name: &str, pids: &[String], target: Option<Value>| -> Vec<MenuItem> {
            let drawn = curve
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(name));
            let copied = if pids.is_empty() {
                name.to_owned()
            } else {
                tr!(
                    "history.copy.text",
                    name = name.to_owned(),
                    pids = pids.join(", ")
                )
            };
            vec![
                MenuItem::new(
                    label(if drawn {
                        "history.menu.curveOff"
                    } else {
                        "history.menu.curve"
                    }),
                    Action::History(Command::Curve((!drawn).then(|| name.to_owned()))),
                )
                .checked(drawn),
                MenuItem::new(
                    label("common.goToProcess"),
                    Action::Preference(
                        "revealProcess".into(),
                        target.clone().unwrap_or(Value::Null),
                    ),
                )
                .disabled(target.is_none()),
                MenuItem::new(
                    label("history.menu.copy"),
                    Action::Copy(copied, "history.copied.namePid"),
                ),
            ]
        };
        for entry in entries {
            let group = match entry {
                Entry::Row(index) => &groups[index],
                Entry::Heading(section, members, open) => {
                    // The category's colour, name and count, and its totals.
                    let key = section.key();
                    let tone = match section {
                        Section::Apps => p.group_apps,
                        Section::Background => p.group_background,
                        Section::Windows => p.group_windows,
                    };
                    let instances = members
                        .iter()
                        .flat_map(|i| groups[*i].instances.iter().copied())
                        .collect::<Vec<_>>();
                    let mut line = div()
                        .id(SharedString::from(format!("history-section-{key}")))
                        .flex()
                        .flex_shrink_0()
                        .h(px(ROW))
                        .items_center()
                        .px(px(6.))
                        .bg(tone.opacity(tables::HEADING_TINT))
                        .when(!searching, |d| d.cursor_pointer())
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(200.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .w(px(16.))
                                        .h(px(16.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(searching, |d| d.opacity(0.4))
                                        .child(
                                            icon(
                                                if open { "ChevronDown" } else { "ChevronRight" },
                                                p.muted,
                                            )
                                            .size(px(13.)),
                                        ),
                                )
                                .child(div().size(px(8.)).flex_shrink_0().rounded(px(2.)).bg(tone))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .font_weight(theme::LABEL_WEIGHT)
                                        .text_color(p.ink)
                                        .child(label(&format!("processes.section.{key}"))),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .min_w(px(22.))
                                        .px(px(7.))
                                        .rounded_full()
                                        .bg(tone.opacity(tables::COUNT_TINT))
                                        .text_color(tone.blend(p.ink.opacity(0.22)))
                                        .text_size(theme::font(11.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .line_height(px(18.))
                                        .flex()
                                        .justify_center()
                                        .child(members.len().to_string()),
                                ),
                        );
                    if has_trend {
                        line = line.child(div().w(px(TREND_WIDTH)).flex_shrink_0());
                    }
                    for (column, value) in shown.iter().zip(aggregate(&instances)) {
                        line = line.child(cell(
                            column.width,
                            0.,
                            column.key == "net" && net_missing,
                            format(column, value),
                        ));
                    }
                    body = body.child(line.on_click(cx.listener(move |this, _, window, cx| {
                        // A search shows every match: nothing folds meanwhile.
                        if searching {
                            return;
                        }
                        if !this.history.folded.remove(key) {
                            this.history.folded.insert(key.to_owned());
                        }
                        let mut folded = this.history.folded.iter().cloned().collect::<Vec<_>>();
                        folded.sort();
                        this.prefs.set("history.foldedSections", folded);
                        this.history_redraw(window, cx);
                    })));
                    continue;
                }
            };
            let expandable = group.instances.len() > 1;
            let expanded = expandable && self.history.expanded.contains(&group.name);
            let icon_process = by_name.get(&group.name.to_lowercase()).copied();
            let publisher = icon_process
                .map(|process| text(process, "company").to_owned())
                .filter(|company| !company.is_empty());
            let key = group.name.clone();
            let selected = self.history.selected.as_deref() == Some(key.as_str());
            let single = (group.instances.len() == 1).then(|| group.instances[0]);
            let toggle_key = key.clone();
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
                        .child(
                            div()
                                .id(SharedString::from(format!("history-expand-{key}")))
                                .w(px(16.))
                                .h(px(16.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(3.))
                                .when(expandable, |d| {
                                    d.hover(move |s| s.bg(p.hover_strong))
                                        .child(
                                            icon(
                                                if expanded {
                                                    "ChevronDown"
                                                } else {
                                                    "ChevronRight"
                                                },
                                                p.muted,
                                            )
                                            .size(px(13.)),
                                        )
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            if !this.history.expanded.remove(&toggle_key) {
                                                this.history.expanded.insert(toggle_key.clone());
                                            }
                                            this.history_redraw(window, cx);
                                        }))
                                }),
                        )
                        .child(self.process_icon(icon_process, 16., cx))
                        .child(div().min_w_0().truncate().child(group.name.clone()))
                        .when(expandable, |d| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(theme::font(11.))
                                    .text_color(p.faint)
                                    .child(format!("×{}", group.instances.len())),
                            )
                        })
                        .when_some(single, |d, process| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(theme::font(11.))
                                    .text_color(p.faint)
                                    .child(tr!(
                                        "history.table.pid",
                                        pid = process["pid"].to_string()
                                    )),
                            )
                        })
                        .when_some(publisher, |d, publisher| {
                            d.child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(theme::font(11.))
                                    .text_color(p.faint)
                                    .child(publisher),
                            )
                        }),
                );
            if has_trend {
                line = line.child(sparkline(group.trend.clone(), trend_color));
            }
            for (column, value) in shown.iter().zip(&group.values) {
                line = line.child(cell(
                    column.width,
                    heat(column, *value),
                    column.key == "net" && net_missing,
                    format(column, *value),
                ));
            }
            let target = single.and_then(|process| recorded_target(process, frame, live));
            let running = target.is_some();
            let reveal = Action::Preference(
                "revealProcess".into(),
                target.clone().unwrap_or(Value::Null),
            );
            let pids = group
                .instances
                .iter()
                .map(|process| process["pid"].to_string())
                .collect::<Vec<_>>();
            let menu = row_menu(&group.name, &pids, target);
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
                    if event.click_count() >= 2 {
                        if expandable {
                            if !this.history.expanded.remove(&toggle_key) {
                                this.history.expanded.insert(toggle_key.clone());
                            }
                        } else if running {
                            this.act(reveal.clone(), window, cx);
                        } else {
                            this.notify_toast(widgets::ToastKind::Info, exited.clone());
                        }
                    }
                    this.history_redraw(window, cx);
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
                // The four busiest instances, and the rest together.
                let rest = instances.split_off(instances.len().min(4));
                for process in &instances {
                    let mut line = div()
                        .flex()
                        .flex_shrink_0()
                        .h(px(ROW))
                        .items_center()
                        .px(px(6.))
                        .text_color(p.muted)
                        .hover(move |s| s.bg(p.hover))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(200.))
                                .pl(px(56.))
                                .pr(px(8.))
                                .truncate()
                                .child(tr!("history.table.pid", pid = process["pid"].to_string())),
                        );
                    if has_trend {
                        line = line.child(sparkline(instance_trend(process), trend_color));
                    }
                    for column in &shown {
                        let value = value_of(process, column.key);
                        line = line.child(cell(
                            column.width,
                            heat(column, value),
                            column.key == "net" && net_missing,
                            format(column, value),
                        ));
                    }
                    let target = recorded_target(process, frame, live);
                    let menu = row_menu(&group.name, &[process["pid"].to_string()], target);
                    body = body.child(line.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, window, cx| {
                            this.act(Action::Popup(menu.clone()), window, cx);
                        }),
                    ));
                }
                if !rest.is_empty() {
                    let values = aggregate(&rest);
                    let mut line = div()
                        .flex()
                        .flex_shrink_0()
                        .h(px(ROW))
                        .items_center()
                        .px(px(6.))
                        .text_color(p.faint)
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(200.))
                                .pl(px(56.))
                                .pr(px(8.))
                                .truncate()
                                .child(tr!("history.table.rest", count = rest.len())),
                        );
                    if has_trend {
                        line = line.child(div().w(px(TREND_WIDTH)).flex_shrink_0());
                    }
                    for (column, value) in shown.iter().zip(&values) {
                        line = line.child(cell(
                            column.width,
                            heat(column, *value),
                            column.key == "net" && net_missing,
                            format(column, *value),
                        ));
                    }
                    body = body.child(line);
                }
            }
        }
        // The card's head: whose processes, the mode, a search, the columns.
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
        let mut chooser_items = vec![
            MenuItem::new(
                label(&format!("history.table.chooser.{mode_key}")),
                Action::CloseOverlay,
            )
            .disabled(true),
        ];
        chooser_items.extend(columns(mode).iter().map(|column| {
            let mut item = MenuItem::new(
                column_name(column.key),
                Action::History(Command::Column(column.key)),
            )
            .checked(shown_keys.contains(&column.key))
            .stay();
            if column.key == "net" && net_missing {
                item = item.detail(label("history.menu.needsService"));
            }
            item
        }));
        chooser_items.push(MenuItem::separator());
        chooser_items.push(
            MenuItem::new(
                label("history.table.trend"),
                Action::History(Command::Trend),
            )
            .checked(has_trend)
            .stay(),
        );
        chooser_items.push(MenuItem::new(
            label("history.table.resetColumns"),
            Action::History(Command::ResetColumns),
        ));
        let search_input = self.history_search_input(window, cx);
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
            .child(div().w(px(200.)).child(self.text_field(
                &search_input,
                widgets::input(&search_input),
                widgets::Field::Boxed,
                label("history.table.search").into(),
                true,
                window,
                cx,
            )))
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
        // Without the privileged service the network column has no records.
        let notice = (net_shown && net_missing).then(|| {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .mx(px(14.))
                .mb(px(6.))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(6.))
                .bg(p.io.opacity(0.08))
                .text_size(theme::font(12.))
                .text_color(p.muted)
                .child(icon("ShieldAlertIcon", p.io).size(px(14.)))
                .child(label("history.table.netService"))
                .child(
                    div()
                        .id("history-net-service")
                        .text_color(p.accent)
                        .cursor_pointer()
                        .child(label("history.unrecorded.install"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.act(
                                Action::Preference("settingsSection".into(), json!("permissions")),
                                window,
                                cx,
                            );
                            this.act(Action::Page("settings".into()), window, cx);
                        })),
                )
        });
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
                            .child(moment_label(ts, false)),
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
        // Room for three rows under the head and over the foot, as tall as
        // they wrapped last (a narrow window wraps both); the page scrolls
        // beyond. Measured, as the card clips and would let the foot cover
        // the rows.
        let (head_height, foot_height) = {
            let geometry = self.history.geometry.borrow();
            (
                f32::from(geometry.table_head).max(36.),
                f32::from(geometry.table_foot).max(34.),
            )
        };
        let probe = |foot: bool| {
            let geometry = self.history.geometry.clone();
            canvas(
                move |bounds, _, _| {
                    let mut geometry = geometry.borrow_mut();
                    if foot {
                        geometry.table_foot = bounds.size.height;
                    } else {
                        geometry.table_head = bounds.size.height;
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        };
        let foot = foot.relative().child(probe(true));
        div()
            .id("history-table-card")
            .flex_1()
            .min_h(px(head_height + 30. + ROW * 3. + foot_height + 2.))
            .flex()
            .flex_col()
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.layer)
            .overflow_hidden()
            .child(
                div()
                    .relative()
                    .child(probe(false))
                    .child(heading)
                    .when_some(notice, |d, notice| d.child(notice)),
            )
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

    /// The search box over the table: it keeps its text while the page is away.
    fn history_search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.ui.inputs.get("historySearch").cloned() {
            return input;
        }
        let input = cx.new(|cx| InputState::new(window, cx));
        self.subscriptions.push(
            cx.subscribe_in(&input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.history_redraw(window, cx);
                }
            }),
        );
        self.ui.inputs.insert("historySearch".into(), input.clone());
        input
    }

    /// Whether the table's search box has the keyboard (the page's keys leave it alone).
    pub(super) fn history_searching(&self, window: &Window, cx: &App) -> bool {
        self.ui
            .inputs
            .get("historySearch")
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window))
    }

    #[allow(clippy::too_many_arguments)]
    fn history_head_cell(
        &self,
        key: &'static str,
        name: String,
        width: Option<f32>,
        sort_key: &str,
        descending: bool,
        service: bool,
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
            .flex()
            .items_center()
            .gap(px(4.))
            .cursor_pointer()
            .when_some(width, |d, w| d.w(px(w)).flex_shrink_0().justify_end())
            .when(width.is_none(), |d| d.flex_1().min_w(px(200.)).pl(px(32.)))
            .when(active, |d| d.text_color(p.ink))
            .when(service, |d| {
                d.child(icon("ShieldAlertIcon", p.io).size(px(12.)))
            })
            .child(div().truncate().child(format!("{name}{arrow}")))
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some((key, value)) = tables::sort_preference("history", key, next) {
                    this.prefs.set(&key, value);
                }
                this.history_redraw(window, cx);
            }))
            .into_any_element()
    }
}

/// A process's trend: a small area, scaled to its own highest value, drawn
/// as an image at the window's density (`charts::sparkline_image`).
fn sparkline(trend: Option<Vec<Option<f64>>>, color: Hsla) -> AnyElement {
    let cell = div()
        .w(px(TREND_WIDTH))
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
                let scale = window.scale_factor();
                let size = (
                    (f32::from(bounds.size.width) * scale).round() as u32,
                    (f32::from(bounds.size.height) * scale).round() as u32,
                );
                if size.0 == 0 || size.1 == 0 {
                    return;
                }
                if let Some(image) =
                    charts::sparkline_image(&trend, top, size, scale, color, window)
                {
                    let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
                }
            },
        )
        .size_full(),
    )
    .into_any_element()
}
