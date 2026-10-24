use super::*;

pub(super) const RANGES: [(&str, i64); 4] = [
    ("1h", 3_600_000),
    ("6h", 21_600_000),
    ("24h", 86_400_000),
    ("7d", 604_800_000),
];
pub(super) fn range_label(id: &str) -> String {
    let hours = RANGES
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, ms)| ms / 3_600_000)
        .unwrap_or(1);
    if hours >= 24 {
        tr!("history.range.days", count = hours / 24)
    } else {
        tr!("history.range.hours", count = hours)
    }
}
impl Desktop {
    pub(in crate::frontend) fn load_history(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let to = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let span = RANGES
            .iter()
            .find(|(key, _)| *key == self.history_range)
            .unwrap_or(&RANGES[0])
            .1;
        let buckets = f32::from(window.viewport_size().width).ceil() as usize;
        self.request(
            "history_series",
            json!({"from":to-span,"to":to,"buckets":buckets,"range":self.history_range}),
            Some("historySeries"),
            window,
            cx,
        );
        self.request(
            "history_moment",
            json!({"ts":self.history_cursor.unwrap_or(to),"cursor":self.history_cursor}),
            Some("historyMoment"),
            window,
            cx,
        );
    }
    pub(in crate::frontend) fn history_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let records = self
            .data
            .get("historySeries")
            .map(values)
            .unwrap_or_default();
        if records.is_empty() {
            let enabled = boolean(&self.data["settings"], "history");
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(label(if enabled {
                    "history.empty.none"
                } else {
                    "history.empty.off"
                }))
                .child(
                    div()
                        .text_sm()
                        .text_color(self.palette.muted)
                        .child(label(if enabled {
                            "history.empty.hint"
                        } else {
                            "history.empty.offHint"
                        })),
                )
                .when(!enabled, |d| {
                    d.child(self.button(
                        "history-open-settings",
                        label("shell.page.settings"),
                        None,
                        Action::Preference("settingsSection".into(), json!("data")),
                        false,
                        cx,
                    ))
                })
                .into_any_element();
        }
        let metrics = [
            ("cpu", "common.resource.cpu", self.palette.cpu, Some(100.)),
            (
                "memUsed",
                "common.resource.memory",
                self.palette.memory,
                None,
            ),
            ("disk", "common.resource.disk", self.palette.io, None),
            ("net", "common.resource.network", self.palette.network, None),
            ("gpu", "common.resource.gpu", self.palette.gpu, Some(100.)),
            (
                "battery",
                "common.resource.battery",
                self.palette.battery,
                Some(100.),
            ),
        ];
        let visible_metrics = metrics
            .iter()
            .copied()
            .filter(|(key, _, _, _)| {
                *key != "battery" || records.iter().any(|p| p[*key].is_number())
            })
            .collect::<Vec<_>>();
        let controls = self.segments(
            "history-metric",
            visible_metrics
                .iter()
                .map(|(key, label_key, _, _)| {
                    (
                        label(label_key),
                        None,
                        Action::Preference("historyMetric".into(), json!(key)),
                        self.history_metric == *key,
                    )
                })
                .collect(),
            cx,
        );
        let step = self
            .data
            .get("historyWindow")
            .map(|w| {
                (number(w, "to") - number(w, "from"))
                    / f64::from(f32::from(window.viewport_size().width).max(1.))
            })
            .unwrap_or(argus_core::history::FRAME_MS as f64);
        let cpu_points = model::history_points(records, "cpu", step);
        let start = self
            .data
            .get("historyWindow")
            .and_then(|w| w["from"].as_f64())
            .unwrap_or_else(|| cpu_points.first().map(|p| p.0).unwrap_or_default());
        let end = self
            .data
            .get("historyWindow")
            .and_then(|w| w["to"].as_f64())
            .unwrap_or_else(|| cpu_points.last().map(|p| p.0).unwrap_or(start + 1.));
        let active_cursor = self.history_hover.or(self.history_cursor);
        let inspect_ts = active_cursor.unwrap_or(end as i64) as f64;
        let nearest_record = records.iter().min_by(|a, b| {
            (number(a, "ts") - inspect_ts)
                .abs()
                .total_cmp(&(number(b, "ts") - inspect_ts).abs())
        });
        let mut chart_stack = div()
            .id("history-chart")
            .flex()
            .flex_col()
            .gap_2()
            .on_mouse_move(cx.listener(|_, _, _, _| {}));
        for &(key, name, color, maximum) in &visible_metrics {
            let points = model::history_points(records, key, step);
            let max = maximum
                .or_else(|| {
                    if key == "memUsed" {
                        nearest_record
                            .and_then(|r| r["memTotal"].as_f64())
                            .or_else(|| self.snapshot["system"]["memTotal"].as_f64())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| points.iter().filter_map(|p| p.1).fold(1., f64::max));
            let current_val = nearest_record
                .and_then(|r| r[key].as_f64())
                .map(|v| charts::series_format(key).value(v))
                .unwrap_or_else(|| label("history.noData"));
            let extra = match key {
                "cpu" => nearest_record
                    .and_then(|r| r["cpuPeak"].as_f64())
                    .map(|p| tr!("history.metric.peak", value = theme::cpu(p))),
                "memUsed" => nearest_record
                    .and_then(|r| r["memTotal"].as_f64())
                    .or_else(|| self.snapshot["system"]["memTotal"].as_f64())
                    .filter(|t| *t > 0.)
                    .map(|t| {
                        tr!(
                            "history.metric.total",
                            value = format!("{:.1}", t / 1073741824.)
                        )
                    }),
                _ => None,
            };
            let mut series = vec![charts::Series::new(
                label(name),
                points,
                color,
                charts::series_format(key),
            )];
            if key == "cpu" && records.iter().any(|r| r["cpuPeak"].is_number()) {
                series.push(charts::Series::new(
                    label(name),
                    model::history_points(records, "cpuPeak", step),
                    color.opacity(0.35),
                    charts::series_format("cpu"),
                ));
            }
            let selected = self.history_metric == key;
            let row_height = if selected {
                window.rem_size() * 6.5
            } else {
                window.rem_size() * 3.8
            };
            chart_stack = chart_stack.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .text_xs()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .font_weight(if selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(if selected {
                                        self.palette.ink
                                    } else {
                                        self.palette.muted
                                    })
                                    .child(label(name)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .text_color(self.palette.muted)
                                    .when_some(extra, |d, ex| d.child(ex))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(self.palette.ink)
                                            .child(current_val),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .h(row_height)
                            .flex_shrink_0()
                            .border_1()
                            .border_color(if selected {
                                color.opacity(0.65)
                            } else {
                                color.opacity(0.28)
                            })
                            .rounded_sm()
                            .overflow_hidden()
                            .child(charts::TimePlot {
                                id: format!("history-{key}").into(),
                                series,
                                start,
                                end,
                                max,
                                min: 0.,
                                interactive: true,
                                fill: true,
                                bars: false,
                                background_separate: false,
                                selection: Some(cx.entity().downgrade()),
                                cursor: active_cursor.map(|at| at as f64),
                                geometry_start: start,
                                measured_width: Rc::new(Cell::new(1.)),
                                live_clock: None,
                                image_surface: Default::default(),
                                bar_surfaces: Default::default(),
                            }),
                    ),
            );
        }
        let moment = self
            .data
            .get("historyMoment")
            .cloned()
            .unwrap_or(Value::Null);
        use gpui_kit::component::table::{
            Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
        };
        let header = TableHeader::new().child(
            TableRow::new().children(
                [
                    "processes.column.name.label",
                    "processes.column.pid.label",
                    "processes.column.cpu.label",
                    "processes.column.mem.label",
                    "processes.column.io.label",
                    "processes.column.gpu.label",
                    "processes.column.net.label",
                ]
                .into_iter()
                .enumerate()
                .map(|(index, key)| {
                    TableHead::new()
                        .when(index > 0, |h| h.text_right())
                        .child(label(key))
                }),
            ),
        );
        let mut table = TableBody::new();
        let mut processes = values(&moment["processes"]).to_vec();
        let process_metric = match self.history_metric.as_str() {
            "memUsed" => "mem",
            "disk" => "io",
            other => other,
        };
        processes.sort_by(|a, b| number(b, process_metric).total_cmp(&number(a, process_metric)));
        let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
        for (index, process) in processes.iter().enumerate() {
            let proc_name = text(process, "name").to_owned();
            let pid = process["pid"].clone();
            let create_time = process["createTime"].clone();
            table = table.child(
                TableRow::new()
                    .child(
                        TableCell::new().child(
                            self.button(
                                format!("history-process-{index}"),
                                proc_name.clone(),
                                None,
                                Action::Preference(
                                    "revealProcess".into(),
                                    json!({"pid": pid, "createTime": create_time}),
                                ),
                                false,
                                cx,
                            )
                            .flex_1()
                            .justify_start(),
                        ),
                    )
                    .child(
                        TableCell::new()
                            .text_right()
                            .child(process["pid"].to_string()),
                    )
                    .child(
                        TableCell::new()
                            .text_right()
                            .child(format!("{}%", theme::cpu(number(process, "cpu")))),
                    )
                    .child(
                        TableCell::new()
                            .text_right()
                            .child(theme::memory(number(process, "mem"))),
                    )
                    .child(
                        TableCell::new()
                            .text_right()
                            .child(theme::bytes(number(process, "io"), true)),
                    )
                    .child(
                        TableCell::new()
                            .text_right()
                            .child(format!("{}%", theme::cpu(number(process, "gpu")))),
                    )
                    .child(
                        TableCell::new()
                            .text_right()
                            .child(theme::network(number(process, "net"), bits)),
                    ),
            );
        }
        let earliest_ts = records.first().and_then(|r| r["ts"].as_i64());
        let history_enabled = boolean(&self.data["settings"], "history");
        div()
            .id("history-content")
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            .mx_3()
            .p_5()
            .border_1()
            .border_color(self.palette.stroke)
            .rounded_lg()
            .bg(self.palette.layer)
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .child(controls)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_xs()
                            .text_color(self.palette.muted)
                            .child(if let Some(at) = active_cursor {
                                format!("{}: {}", label("history.time"), theme::timestamp(at))
                            } else {
                                label("history.hint.pick")
                            })
                            .when(self.history_cursor.is_some(), |d| {
                                d.child(
                                    self.button(
                                        "history-latest",
                                        label("history.latest.label"),
                                        None,
                                        Action::Preference("historyLatest".into(), Value::Null),
                                        false,
                                        cx,
                                    )
                                    .min_h_0()
                                    .h_6()
                                    .border_1()
                                    .border_color(self.palette.stroke)
                                    .bg(self.palette.control),
                                )
                            }),
                    ),
            )
            .child(chart_stack)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(self.palette.muted)
                    .child(theme::timestamp(start as i64))
                    .child(label("history.hint.move"))
                    .child(theme::timestamp(end as i64)),
            )
            .child(if let Some(time) = moment["system"]["ts"].as_i64() {
                tr!("history.busiestAt", time = theme::timestamp(time))
            } else {
                label("history.busiest")
            })
            .child(
                div()
                    .rounded_lg()
                    .overflow_hidden()
                    .border_1()
                    .border_color(self.palette.stroke)
                    .child(Table::new().child(header).child(table)),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .text_sm()
                    .text_color(self.palette.muted)
                    .child(label("history.note"))
                    .when_some(earliest_ts, |d, ts| {
                        d.child(div().text_xs().child(format!(
                            "{}: {}",
                            label("history.earliest"),
                            theme::timestamp(ts)
                        )))
                    }),
            )
            .when(!history_enabled, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(self.palette.muted)
                        .child(label("history.empty.off"))
                        .child(
                            self.button(
                                "history-off-settings",
                                label("shell.page.settings"),
                                None,
                                Action::Preference("settingsSection".into(), json!("data")),
                                false,
                                cx,
                            )
                            .min_h_0()
                            .h_6(),
                        ),
                )
            })
            .into_any_element()
    }
}
