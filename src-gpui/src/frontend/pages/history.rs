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
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(label(if boolean(&self.data["settings"], "history") {
                    "history.empty.none"
                } else {
                    "history.empty.off"
                }))
                .child(
                    div()
                        .text_sm()
                        .text_color(self.palette.muted)
                        .child(label("history.empty.hint")),
                )
                .into_any_element();
        }
        let mut controls = div().flex().gap_2().flex_wrap();
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
        for (key, label_key, _, _) in metrics {
            if key == "battery" && !records.iter().any(|p| p[key].is_number()) {
                continue;
            }
            controls = controls.child(self.button(
                format!("history-{key}"),
                label(label_key),
                None,
                Action::Preference("historyMetric".into(), json!(key)),
                self.history_metric == key,
                cx,
            ));
        }
        let (_, name, color, maximum) = metrics
            .iter()
            .find(|(key, _, _, _)| *key == self.history_metric)
            .copied()
            .unwrap_or(metrics[0]);
        let step = self
            .data
            .get("historyWindow")
            .map(|w| {
                (number(w, "to") - number(w, "from"))
                    / f64::from(f32::from(window.viewport_size().width).max(1.))
            })
            .unwrap_or(argus_core::history::FRAME_MS as f64);
        let points = model::history_points(records, &self.history_metric, step);
        let start = self
            .data
            .get("historyWindow")
            .and_then(|w| w["from"].as_f64())
            .unwrap_or_else(|| points.first().map(|p| p.0).unwrap_or_default());
        let end = self
            .data
            .get("historyWindow")
            .and_then(|w| w["to"].as_f64())
            .unwrap_or_else(|| points.last().map(|p| p.0).unwrap_or(start + 1.));
        let max = maximum.unwrap_or_else(|| points.iter().filter_map(|p| p.1).fold(1., f64::max));
        let plot = div()
            .id("history-chart")
            .h(window.rem_size() * 18.)
            .child(charts::TimePlot {
                id: "history".into(),
                series: vec![charts::Series {
                    name: label(name),
                    points: Arc::new(points),
                    color,
                    format: charts::series_format(&self.history_metric),
                }],
                start,
                end,
                max,
                min: 0.,
                interactive: true,
                fill: true,
                selection: Some(cx.entity().downgrade()),
                cursor: self.history_cursor.map(|at| at as f64),
                geometry_start: start,
                measured_width: Rc::new(Cell::new(1.)),
            });
        let moment = self
            .data
            .get("historyMoment")
            .cloned()
            .unwrap_or(Value::Null);
        let mut table = div().flex().flex_col().gap_1();
        let mut processes = values(&moment["processes"]).to_vec();
        let process_metric = match self.history_metric.as_str() {
            "memUsed" => "mem",
            "disk" => "io",
            other => other,
        };
        processes.sort_by(|a, b| number(b, process_metric).total_cmp(&number(a, process_metric)));
        for (index, process) in processes.iter().enumerate() {
            table = table.child(
                div()
                    .flex()
                    .gap_4()
                    .py_2()
                    .border_b_1()
                    .border_color(self.palette.stroke)
                    .child(
                        self.button(
                            format!("history-process-{index}"),
                            text(process, "name").to_owned(),
                            None,
                            Action::Copy(text(process, "name").into()),
                            false,
                            cx,
                        )
                        .flex_1()
                        .justify_start(),
                    )
                    .child(process["pid"].to_string())
                    .child(format!("{}%", theme::cpu(number(process, "cpu"))))
                    .child(theme::memory(number(process, "mem")))
                    .child(theme::bytes(number(process, "io"), true)),
            );
        }
        div()
            .id("history-content")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_6()
            .flex()
            .flex_col()
            .gap_4()
            .child(controls)
            .child(plot)
            .child(self.button(
                "history-latest",
                label("history.latest.label"),
                None,
                Action::Preference("historyLatest".into(), Value::Null),
                false,
                cx,
            ))
            .child(if let Some(time) = moment["system"]["ts"].as_i64() {
                tr!("history.busiestAt", time = theme::timestamp(time))
            } else {
                label("history.busiest")
            })
            .child(table)
            .child(
                div()
                    .text_sm()
                    .text_color(self.palette.muted)
                    .child(label("history.note")),
            )
            .into_any_element()
    }
}
