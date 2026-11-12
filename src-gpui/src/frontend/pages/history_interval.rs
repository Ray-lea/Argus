//! Interval selection keeps its frame boundaries independent of the hovered bucket.
use super::*;
use argus_core::measurement;
use widgets::Tone;

fn resource_key(metric: &str) -> &str {
    if metric == "io" { "disk" } else { metric }
}
fn metric_name(metric: &str) -> String {
    label(match metric {
        "memory" => "common.resource.memory",
        "io" => "history.interval.diskIo",
        "disk" => "common.resource.disk",
        "gpu" => "common.resource.gpu",
        "network" => "common.resource.network",
        _ => "common.resource.cpu",
    })
}
fn resource_value(summary: &Value, field: &str, metric: &str) -> String {
    let Some(value) = summary[field].as_f64() else {
        return label("format.missing");
    };
    let value = match metric {
        "cpu" | "gpu" => format!("{}%", theme::cpu(value)),
        "memory" => theme::bytes(value, false),
        _ if value > 0. && value < 1024. => format!("{} B/s", theme::count(value)),
        _ => theme::bytes(value, true),
    };
    if number(summary, "partialFrames") > 0. {
        format!("≥{value}")
    } else {
        value
    }
}
fn resource_coverage(summary: &Value) -> String {
    let total = number(summary, "validFrames")
        + number(summary, "unavailableFrames")
        + number(summary, "legacyFrames");
    let mut text = tr!(
        "history.interval.coverageFrames",
        valid = number(summary, "validFrames") as usize,
        total = total as usize,
        partial = number(summary, "partialFrames") as usize,
        legacy = number(summary, "legacyFrames") as usize
    );
    if summary["sampleCoverage"].is_object() {
        text.push_str(&format!(
            " · {}",
            tr!(
                "history.interval.coverageSamples",
                valid = summary["sampleCoverage"]["valid"],
                total = summary["sampleCoverage"]["samples"]
            )
        ));
        let bits = number(&summary["sampleCoverage"], "missing") as u8;
        for (bit, key) in [
            (measurement::UNAVAILABLE, "unavailable"),
            (measurement::ACCESS_DENIED, "denied"),
            (measurement::STALE, "stale"),
            (measurement::BASELINE, "baseline"),
        ] {
            if bits & bit != 0 {
                text.push_str(&format!(
                    " · {}",
                    label(&format!("history.coverage.reasons.{key}"))
                ));
            }
        }
    }
    text
}

impl Desktop {
    fn request_report(&mut self, format: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(report) = self
            .data
            .get("historyInterval")
            .filter(|report| report.is_object())
        else {
            return;
        };
        if self
            .pending
            .values()
            .any(|pending| pending.command == "history_report_preview")
        {
            return;
        }
        let args = json!({"from":report["from"],"to":report["to"],"metric":report["metric"],"format":format,
            "generation":self.data.get("intervalGeneration").and_then(Value::as_u64).unwrap_or(0)});
        self.data.remove("reportError");
        self.data.insert("reportRequest".into(), args.clone());
        self.request("history_report_preview", args, None, window, cx);
        cx.notify();
    }
    pub(in crate::frontend) fn interval_test_state(&self) -> Value {
        let metric = self
            .data
            .get("intervalMetric")
            .and_then(Value::as_str)
            .unwrap_or("cpu");
        let report = self.data.get("historyInterval").unwrap_or(&Value::Null);
        json!({"start":self.data.get("intervalStart"),"end":self.data.get("intervalEnd"),"query":self.data.get("intervalQuery"),
            "metric":metric,"mean":report["cpu"]["mean"],"value":report[resource_key(metric)]["mean"],
            "readout":resource_value(&report[resource_key(metric)],"mean",metric),"coverage":report[resource_key(metric)],
            "missing":report["missingFrames"],"leaders":values(&report["leaders"]).iter().map(|p|text(p,"name")).collect::<Vec<_>>(),
            "leaderPids":values(&report["leaders"]).iter().map(|p|&p["pid"]).collect::<Vec<_>>(),
            "leaderReadout":values(&report["leaders"]).first().map(|p|resource_value(&p[metric],"mean",metric)),
            "observations":report["observations"]})
    }
    pub(super) fn interval_peak(&mut self, at: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.data.get("historyInterval").and_then(|r| {
            r[resource_key(
                self.data
                    .get("intervalMetric")
                    .and_then(Value::as_str)
                    .unwrap_or("cpu"),
            )]["peakTs"]
                .as_i64()
        }) != Some(at)
        {
            return;
        }
        if self
            .data
            .get("historyWindow")
            .is_none_or(|w| at < number(w, "from") as i64 || at > number(w, "to") as i64)
        {
            self.history_range = "7d".into();
        }
        self.history_hover = None;
        self.history_cursor = Some(at);
        self.data.remove("historyPeakAt");
        self.data.insert("historyExactAt".into(), json!(at));
        self.load_history(window, cx);
        cx.notify();
    }
    fn interval_frame(&self) -> Option<i64> {
        let moment = self.data.get("historyMoment")?;
        let ts = moment["system"]["frameTs"]
            .as_i64()
            .or_else(|| moment["system"]["ts"].as_i64())?;
        (self.data.get("historyMomentAt").and_then(Value::as_i64) == Some(ts)).then_some(ts)
    }
    pub(in crate::frontend) fn interval_action(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == "historyIntervalClear" {
            self.data.remove("intervalStart");
            self.data.remove("intervalEnd");
        } else if action != "historyIntervalRetry" {
            let Some(ts) = self.interval_frame() else {
                return;
            };
            if action == "historyIntervalStart" {
                self.data.insert("intervalStart".into(), json!(ts));
                self.data.remove("intervalEnd");
            } else {
                self.data.insert("intervalEnd".into(), json!(ts));
            }
        }
        let generation = self
            .data
            .get("intervalGeneration")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            + 1;
        self.data
            .insert("intervalGeneration".into(), json!(generation));
        self.data.remove("intervalQuery");
        self.data.remove("historyInterval");
        self.data.remove("intervalError");
        self.load_cpu_interval(window, cx);
        cx.notify();
    }

    pub(in crate::frontend) fn load_cpu_interval(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(a), Some(b)) = (
            self.data.get("intervalStart").and_then(Value::as_i64),
            self.data.get("intervalEnd").and_then(Value::as_i64),
        ) else {
            return;
        };
        let Some(to) = a.max(b).checked_add(argus_core::history::FRAME_MS) else {
            return;
        };
        let query = json!({"from":a.min(b),"to":to,"metric":self.data.get("intervalMetric").and_then(Value::as_str).unwrap_or("cpu"),"generation":self.data.get("intervalGeneration").and_then(Value::as_u64).unwrap_or(0)});
        if self.data.get("intervalQuery") == Some(&query)
            || self
                .pending
                .values()
                .any(|p| p.command == "history_interval")
        {
            return;
        }
        self.data.insert("intervalQuery".into(), query.clone());
        self.request(
            "history_interval",
            query,
            Some("historyInterval"),
            window,
            cx,
        );
    }

    pub(super) fn interval_markers(&self, cx: &mut Context<Self>) -> AnyElement {
        let ready = self.interval_frame().is_some();
        let mut controls = div().flex().flex_wrap().gap(px(8.));
        for (id, key, label_key, disabled) in [
            (
                "interval-start",
                "historyIntervalStart",
                "history.interval.start",
                !ready,
            ),
            (
                "interval-end",
                "historyIntervalEnd",
                "history.interval.end",
                !ready || !self.data.contains_key("intervalStart"),
            ),
        ] {
            controls = controls.child(
                self.btn_small(id, label(label_key), None, Tone::Plain)
                    .child(self.watch_anchor(id))
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.preference(key, json!(null), window, cx)
                    })),
            );
        }
        controls.into_any_element()
    }

    pub(super) fn interval_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let metric = self
            .data
            .get("intervalMetric")
            .and_then(Value::as_str)
            .unwrap_or("cpu");
        let metrics = ["cpu", "memory", "io", "gpu", "network"];
        let mut card = div()
            .flex_none()
            .min_w_0()
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.layer);
        card = card.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .font_weight(theme::LABEL_WEIGHT)
                        .child(label("history.interval.title")),
                )
                .child(
                    self.btn_small(
                        "interval-clear",
                        label("history.interval.clear"),
                        None,
                        Tone::Plain,
                    )
                    .child(self.watch_anchor("interval-clear"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preference("historyIntervalClear", Value::Null, window, cx)
                    })),
                ),
        );
        card = card.child(self.select(
            "interval-metric",
            label("history.interval.selectMetric"),
            metrics.iter().map(|key| metric_name(key)).collect(),
            metrics.iter().position(|key| *key == metric),
            false,
            move |this, index, window, cx| {
                this.preference("historyIntervalMetric", json!(metrics[index]), window, cx)
            },
            cx,
        ));
        let Some(start) = self.data.get("intervalStart").and_then(Value::as_i64) else {
            return card.into_any_element();
        };
        card = card.child(
            div()
                .text_color(p.muted)
                .text_size(theme::font(12.))
                .child(tr!(
                    "history.interval.startAt",
                    time = date_time(start as f64)
                )),
        );
        if !self.data.contains_key("intervalEnd") {
            return card
                .child(div().child(label("history.interval.chooseEnd")))
                .into_any_element();
        }
        if let Some(error) = self.data.get("intervalError").and_then(Value::as_str) {
            return card
                .child(error.to_owned())
                .child(
                    self.btn_small(
                        "interval-retry",
                        label("history.interval.retry"),
                        None,
                        Tone::Plain,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preference("historyIntervalRetry", Value::Null, window, cx)
                    })),
                )
                .into_any_element();
        }
        let Some(report) = self.data.get("historyInterval") else {
            return card
                .child(label("history.interval.loading"))
                .into_any_element();
        };
        if !report.is_object() {
            return card
                .child(label("history.interval.invalid"))
                .into_any_element();
        }
        let mut export = div().flex().flex_wrap().gap(px(8.));
        for (id, format, name) in [
            ("interval-report-markdown", "markdown", "Markdown"),
            ("interval-report-csv", "csv", "CSV"),
        ] {
            export = export.child(
                self.btn_small(
                    id,
                    tr!("history.report.preview", format = name),
                    None,
                    Tone::Plain,
                )
                .disabled(
                    self.pending
                        .values()
                        .any(|pending| pending.command == "history_report_preview"),
                )
                .child(self.watch_anchor(id))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.request_report(format, window, cx)),
                ),
            );
        }
        card = card.child(export);
        if let Some(error) = self.data.get("reportError").and_then(Value::as_str) {
            card = card.child(div().text_size(theme::font(12.)).child(error.to_owned()));
        }
        let percent = |value: &Value| {
            value
                .as_f64()
                .map(theme::cpu)
                .unwrap_or_else(|| label("format.missing"))
        };
        card = card.child(div().text_size(theme::font(12.)).child(tr!(
            "history.interval.range",
            from = date_time(number(report, "from")),
            to = date_time(number(report, "to"))
        )));
        card = card.child(div().text_size(theme::font(12.)).child(tr!(
            "history.interval.coverage",
            recorded = report["recordedFrames"],
            expected = report["expectedFrames"],
            missing = report["missingFrames"],
            step = number(report, "frameMs") / 1000.
        )));
        card = card.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(div().child(if metric == "cpu" {
                    tr!(
                        "history.interval.cpu",
                        average = percent(&report["cpu"]["mean"]),
                        peak = percent(&report["cpu"]["peak"])
                    )
                } else {
                    tr!(
                        "history.interval.resource",
                        resource = metric_name(resource_key(metric)),
                        average = resource_value(&report[resource_key(metric)], "mean", metric),
                        peak = resource_value(&report[resource_key(metric)], "peak", metric)
                    )
                }))
                .when_some(report[resource_key(metric)]["peakTs"].as_i64(), |d, ts| {
                    d.child(
                        self.btn_small(
                            "interval-peak",
                            label(if metric == "cpu" {
                                "history.showCpuPeak"
                            } else {
                                "history.interval.showResourcePeak"
                            }),
                            None,
                            Tone::Plain,
                        )
                        .child(self.watch_anchor("interval-peak"))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.preference("historyIntervalPeak", json!(ts), window, cx)
                            },
                        )),
                    )
                }),
        );
        if metric != "cpu" {
            card = card.child(
                div()
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(resource_coverage(&report[resource_key(metric)])),
            );
        }
        if matches!(metric, "memory" | "gpu") {
            card = card.child(
                div()
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(label(if metric == "gpu" {
                        "history.interval.gpuCaveat"
                    } else {
                        "history.interval.memoryCaveat"
                    })),
            );
        }
        let leaders = values(&report["leaders"]);
        card = card.child(div().text_size(theme::font(12.)).text_color(p.muted).child(
            if metric == "cpu" {
                tr!(
                    "history.interval.leaders",
                    shown = leaders.len(),
                    total = report["identitiesSeen"]
                )
            } else {
                tr!(
                    "history.interval.leadersMetric",
                    resource = if metric == "io" {
                        label("processes.column.io.label")
                    } else {
                        metric_name(metric)
                    },
                    shown = leaders.len(),
                    total = report["identitiesSeen"]
                )
            },
        ));
        for (index, leader) in leaders.iter().enumerate() {
            let identity = leader["createTime"]
                .as_i64()
                .map(|ts| date_time(ts as f64))
                .unwrap_or_else(|| label("history.interval.unknownInstance"));
            card = card.child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .pt(px(6.))
                    .border_t_1()
                    .border_color(p.stroke)
                    .child(div().font_weight(theme::LABEL_WEIGHT).child(tr!(
                        "history.interval.instance",
                        position = index + 1,
                        name = text(leader, "name"),
                        pid = leader["pid"],
                        started = identity
                    )))
                    .child(div().text_size(theme::font(12.)).child(tr!(
                        "history.interval.process",
                        average = percent(&leader["cpu"]["mean"]),
                        peak = percent(&leader["cpu"]["peak"]),
                        observed = leader["observedFrames"],
                        recorded = report["recordedFrames"],
                        memory = theme::bytes(number(leader, "privateWsPeak"), false)
                    )))
                    .when(matches!(metric, "io" | "gpu" | "network"), |d| {
                        d.child(div().text_size(theme::font(12.)).child(tr!(
                            "history.interval.processResource",
                            resource = if metric == "io" {
                                label("processes.column.io.label")
                            } else {
                                metric_name(metric)
                            },
                            average = resource_value(&leader[metric], "mean", metric),
                            peak = resource_value(&leader[metric], "peak", metric),
                            coverage = resource_coverage(&leader[metric])
                        )))
                    }),
            );
        }
        for gap in values(&report["gaps"]).iter().take(6) {
            card = card.child(
                div()
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(tr!(
                        "history.interval.gap",
                        from = date_time(number(gap, "from")),
                        to = date_time(number(gap, "to"))
                    )),
            );
        }
        let observations = &report["observations"];
        if number(observations, "sourceFiles") + number(observations, "unreadableFiles") > 0. {
            card = card.child(
                div()
                    .pt(px(8.))
                    .border_t_1()
                    .border_color(p.stroke)
                    .child(self.watch_anchor("interval-observations"))
                    .child(
                        div()
                            .font_weight(theme::LABEL_WEIGHT)
                            .child(label("history.interval.observationTitle")),
                    )
                    .child(div().text_size(theme::font(12.)).child(tr!(
                        "history.interval.observationCoverage",
                        shown = values(&observations["runs"]).len(),
                        total = observations["runsSeen"],
                        selected = observations["selectedPoints"],
                        saved = observations["savedPoints"],
                        step = number(observations, "sampleMs") / 1000.
                    ))),
            );
            if observations["firstSavedTs"].as_i64().is_some() {
                card = card.child(div().text_size(theme::font(12.)).child(tr!(
                    "history.interval.observationSavedRange",
                    from = date_time(number(observations, "firstSavedTs")),
                    to = date_time(number(observations, "lastSavedTs"))
                )));
            }
            for (index, run) in values(&observations["runs"]).iter().enumerate() {
                let mut row = div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .pt(px(6.))
                    .child(div().font_weight(theme::LABEL_WEIGHT).child(tr!(
                        "history.interval.instance",
                        position = index + 1,
                        name = text(run, "name"),
                        pid = run["pid"],
                        started = date_time(number(run, "createTime"))
                    )));
                for key in ["memory", "commit"] {
                    let summary = &run[key];
                    let bytes = |field: &str| {
                        summary[field]
                            .as_f64()
                            .map(|value| theme::bytes(value, false))
                            .unwrap_or_else(|| label("format.missing"))
                    };
                    row = row.child(div().text_size(theme::font(12.)).child(tr!(
                        "history.interval.observationMemory",
                        metric = label(&format!("history.watch.metric.{key}")),
                        average = bytes("mean"),
                        peak = bytes("peak"),
                        valid = summary["samples"],
                        missing = summary["missing"]
                    )));
                }
                card =
                    card.child(
                        row.child(div().text_size(theme::font(12.)).text_color(p.muted).child(
                            tr!(
                                "history.interval.observationMarkers",
                                from = date_time(number(run, "firstTs")),
                                to = date_time(number(run, "lastTs")),
                                gaps = run["gapEvents"],
                                missing = run["missingSampleSlots"],
                                terminal = run["terminalEvents"]
                            ),
                        )),
                    );
            }
            if values(&observations["runs"]).is_empty() {
                card = card.child(
                    div()
                        .text_size(theme::font(12.))
                        .child(label("history.interval.observationEmpty")),
                );
            }
            if [
                "unreadableFiles",
                "damagedFiles",
                "omittedPoints",
                "omittedRuns",
                "expiredPoints",
            ]
            .iter()
            .any(|key| number(observations, key) > 0.)
            {
                card = card.child(div().text_size(theme::font(12.)).child(tr!(
                    "history.interval.observationIncomplete",
                    unreadable = observations["unreadableFiles"],
                    damaged = observations["damagedFiles"],
                    expired = observations["expiredPoints"],
                    omittedPoints = observations["omittedPoints"],
                    omittedRuns = observations["omittedRuns"]
                )));
            }
            card = card.child(
                div()
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(label("history.interval.observationNote")),
            );
        }
        if number(report, "incompleteFiles")
            + number(report, "unreadableFiles")
            + number(report, "invalidFrames")
            + number(&report["cpu"], "invalidFrames")
            + number(report, "omittedEntries")
            + number(&report[resource_key(metric)], "invalidFrames")
            + leaders
                .iter()
                .map(|leader| number(&leader[metric], "invalidFrames"))
                .sum::<f64>()
            > 0.
        {
            card = card.child(
                div()
                    .text_size(theme::font(12.))
                    .child(label("history.interval.incomplete")),
            );
        }
        card.child(
            div()
                .text_size(theme::font(12.))
                .text_color(p.muted)
                .child(label(if metric == "cpu" {
                    "history.interval.note"
                } else if metric == "memory" {
                    "history.interval.memoryNote"
                } else {
                    "history.interval.resourceNote"
                })),
        )
        .into_any_element()
    }
}
