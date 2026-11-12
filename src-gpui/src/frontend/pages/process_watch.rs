//! Explicit instance or program observation, independent of historical top lists.
use super::*;
use widgets::Tone;

impl Desktop {
    pub(in crate::frontend) fn watch_anchor(&self, key: &'static str) -> AnyElement {
        let anchors = self.ui.anchors.clone();
        canvas(
            move |bounds, _, _| {
                anchors.borrow_mut().insert(key.into(), bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }
    pub(super) fn load_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let active = !self.data.contains_key("processWatch")
            || self.data.contains_key("watchError")
            || self
                .data
                .get("processWatch")
                .is_some_and(|watch| matches!(text(watch, "state"), "recording" | "gap"));
        let last = self.data.get("watchQueryAt").and_then(Value::as_i64);
        if last.is_none()
            || (active && last.is_some_and(|at| now - at >= argus_core::watch::SAMPLE_MS))
            || last.is_some_and(|at| now < at || now - at >= 60_000)
        {
            self.data.insert("watchQueryAt".into(), json!(now));
            self.request(
                "watch_process",
                json!({"action":{"op":"status"},"generation":self.data.get("watchGeneration").and_then(Value::as_u64).unwrap_or(0)}),
                Some("processWatch"),
                window,
                cx,
            );
        }
    }

    pub(super) fn watch_card(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let watch = self.data.get("processWatch").unwrap_or(&Value::Null);
        let mut card = div()
            .flex_none()
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.layer);
        if !watch.is_object() {
            if let Some(message) = self.data.get("watchError").and_then(Value::as_str) {
                return card
                    .child(div().child(message.to_owned()))
                    .child(
                        self.btn_small(
                            "watch-delete-invalid",
                            label("history.watch.delete"),
                            None,
                            Tone::Plain,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.preference("watchProcess", json!({"op":"delete"}), window, cx)
                        })),
                    )
                    .into_any_element();
            }
            return card
                .child(
                    div()
                        .font_weight(theme::LABEL_WEIGHT)
                        .child(label("history.watch.title")),
                )
                .child(
                    div()
                        .text_size(theme::font(12.))
                        .text_color(p.muted)
                        .child(label("history.watch.empty")),
                )
                .into_any_element();
        }
        let state = text(watch, "state");
        let program_mode = text(watch, "mode") == "program";
        let points = values(&watch["points"]);
        let start = points
            .first()
            .map(|point| number(point, "ts"))
            .unwrap_or(0.);
        let end = points
            .last()
            .map(|point| number(point, "ts"))
            .unwrap_or(start)
            + argus_core::watch::SAMPLE_MS as f64;
        let latest = points.last();
        let names = [
            label("history.watch.metric.memory"),
            label("history.watch.metric.commit"),
        ];
        let key = charts::hash_of((
            watch["pid"].as_u64(),
            watch["createTime"].as_i64(),
            points.len(),
            end.to_bits(),
            latest.and_then(|point| point["memory"].as_u64()),
            latest.and_then(|point| point["commit"].as_u64()),
            p.dark,
            &names,
        ));
        let cached = window.use_keyed_state("process-watch-series", cx, |_, _| {
            None::<(u64, Arc<Vec<charts::Series>>, f64)>
        });
        let plot = cached
            .read(cx)
            .as_ref()
            .filter(|(stored, _, _)| *stored == key)
            .cloned();
        let (_, series, maximum) = plot.unwrap_or_else(|| {
            let mut maximum = 1024. * 1024.;
            let mut series = Vec::new();
            for ((field, color), name) in [("memory", p.memory), ("commit", p.cpu)]
                .into_iter()
                .zip(&names)
            {
                let data = points
                    .iter()
                    .map(|point| (number(point, "ts"), point[field].as_f64()))
                    .collect::<Vec<_>>();
                maximum = data
                    .iter()
                    .filter_map(|(_, value)| *value)
                    .fold(maximum, f64::max);
                series.push(
                    charts::Series::new(name.clone(), data, color, charts::MetricFormat::Gigabytes)
                        .source(charts::hash_of((key, field))),
                );
            }
            let plot = (key, Arc::new(series), maximum);
            cached.update(cx, |cache, _| *cache = Some(plot.clone()));
            plot
        });
        let mut legend = div().flex().flex_wrap().gap_x_4().gap_y_1();
        for (field, color) in [("memory", p.memory), ("commit", p.cpu)] {
            let value = latest
                .and_then(|point| point[field].as_f64())
                .map(theme::memory)
                .unwrap_or_else(|| label("format.missing"));
            legend = legend.child(div().text_size(theme::font(12.)).text_color(color).child(
                format!(
                    "{}: {value}",
                    label(&format!("history.watch.metric.{field}"))
                ),
            ));
        }
        let active = matches!(state, "recording" | "gap");
        let header = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(div().font_weight(theme::LABEL_WEIGHT).child(tr!(
                "history.watch.target",
                name = text(watch, "name"),
                pid = watch["pid"]
            )))
            .child(
                div()
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(label(
                        &if program_mode && active && watch["liveRuns"] == 0 {
                            "history.watch.waiting".into()
                        } else {
                            format!("history.watch.state.{state}")
                        },
                    )),
            )
            .child(
                self.btn_small("watch-stop", label("history.watch.stop"), None, Tone::Plain)
                    .child(self.watch_anchor("watch-stop"))
                    .disabled(!active)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preference("watchProcess", json!({"op":"stop"}), window, cx)
                    })),
            )
            .child(
                self.btn_small(
                    "watch-delete",
                    label("history.watch.delete"),
                    None,
                    Tone::Plain,
                )
                .child(self.watch_anchor("watch-delete"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.preference("watchProcess", json!({"op":"delete"}), window, cx)
                })),
            );
        card = card.child(header).child(legend);
        if program_mode {
            let runs = values(&watch["runs"]);
            let options = runs
                .iter()
                .map(|run| {
                    tr!(
                        "history.watch.run",
                        started = date_time(number(run, "createTime")),
                        pid = run["pid"],
                        state = label(&format!("history.watch.state.{}", text(run, "state")))
                    )
                })
                .collect();
            let selected = runs.iter().position(|run| {
                run["pid"] == watch["pid"] && run["createTime"] == watch["createTime"]
            });
            let keys: Vec<_> = runs
                .iter()
                .map(
                    |run| json!({"op":"selectRun","pid":run["pid"],"createTime":run["createTime"]}),
                )
                .collect();
            card = card.child(self.select(
                "watch-run",
                label("history.watch.chooseRun"),
                options,
                selected,
                keys.is_empty(),
                move |this, index, window, cx| {
                    if let Some(action) = keys.get(index) {
                        this.preference("watchProcess", action.clone(), window, cx);
                    }
                },
                cx,
            ));
            let offset = watch["runOffset"].as_u64().unwrap_or(0);
            let total = watch["totalRuns"].as_u64().unwrap_or(0);
            card = card.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        self.btn_small(
                            "watch-older-runs",
                            label("history.watch.earlierRuns"),
                            None,
                            Tone::Plain,
                        )
                        .child(self.watch_anchor("watch-older-runs"))
                        .disabled(offset + runs.len() as u64 >= total)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.preference(
                                    "watchProcess",
                                    json!({"op":"runs","offset":offset+256}),
                                    window,
                                    cx,
                                )
                            },
                        )),
                    )
                    .child(
                        self.btn_small(
                            "watch-latest-runs",
                            label("history.watch.latestRuns"),
                            None,
                            Tone::Plain,
                        )
                        .child(self.watch_anchor("watch-latest-runs"))
                        .disabled(offset == 0)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.preference(
                                "watchProcess",
                                json!({"op":"runs","offset":0}),
                                window,
                                cx,
                            )
                        })),
                    ),
            );
            card = card.child(
                div()
                    .min_w_0()
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(text(&watch["program"], "displayPath").to_owned()),
            );
            card = card.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(theme::font(12.))
                            .text_color(p.muted)
                            .child(tr!(
                                "history.watch.runSummary",
                                shown = runs.len(),
                                total = watch["totalRuns"],
                                points = watch["totalPoints"]
                            )),
                    )
                    .child(
                        self.btn_small(
                            "watch-earlier",
                            label("history.watch.earlier"),
                            None,
                            Tone::Plain,
                        )
                        .child(self.watch_anchor("watch-earlier"))
                        .disabled(watch["hasEarlier"] != true)
                        .on_click(cx.listener({
                            let before = watch["firstSequence"].clone();
                            move |this, _, window, cx| {
                                this.preference(
                                    "watchProcess",
                                    json!({"op":"page","before":before}),
                                    window,
                                    cx,
                                )
                            }
                        })),
                    )
                    .child(
                        self.btn_small(
                            "watch-latest",
                            label("history.watch.latest"),
                            None,
                            Tone::Plain,
                        )
                        .child(self.watch_anchor("watch-latest"))
                        .disabled(watch["hasLater"] != true)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.preference(
                                "watchProcess",
                                json!({"op":"page","before":null}),
                                window,
                                cx,
                            )
                        })),
                    ),
            );
            if watch["damaged"] == true {
                card = card.child(
                    div()
                        .text_size(theme::font(12.))
                        .text_color(p.muted)
                        .child(label("history.watch.damaged")),
                );
            }
            if number(watch, "unconfirmed") > 0. {
                card = card.child(div().text_size(theme::font(12.)).text_color(p.muted).child(
                    tr!("history.watch.unconfirmed", count = watch["unconfirmed"]),
                ));
            }
        }
        if let Some(message) = self.data.get("watchError").and_then(Value::as_str) {
            card = card.child(div().text_size(theme::font(12.)).child(message.to_owned()));
        }
        if !points.is_empty() {
            card = card.child(
                div()
                    .h(px(110.))
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(4.))
                    .bg(p.chart_surface)
                    .child(charts::TimePlot {
                        id: "process-watch-memory".into(),
                        series,
                        start,
                        end,
                        max: maximum * 1.1,
                        min: 0.,
                        interactive: true,
                        grid: true,
                        fill: false,
                        background_separate: false,
                        smooth: false,
                        ticks: None,
                        geometry_start: start,
                        measured_width: Rc::new(Cell::new(1.)),
                        live_clock: None,
                        image_surface: Default::default(),
                        dash_phases: Default::default(),
                        rescale: Default::default(),
                        readout: Default::default(),
                        captions: None,
                    }),
            );
        }
        card = card.child(
            div()
                .text_size(theme::font(12.))
                .text_color(p.muted)
                .child(label(if program_mode {
                    "history.watch.programNote"
                } else {
                    "history.watch.note"
                })),
        );
        for point in points
            .iter()
            .filter(|point| text(point, "state") != "recording")
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            card = card.child(div().text_size(theme::font(12.)).text_color(p.muted).child(
                format!(
                    "{} — {}",
                    date_time(number(point, "ts")),
                    label(&format!("history.watch.state.{}", text(point, "state")))
                ),
            ));
        }
        card.into_any_element()
    }
}
