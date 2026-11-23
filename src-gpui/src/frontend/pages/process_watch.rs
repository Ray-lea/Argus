//! Dedicated process observation subpage under History.
use super::*;
use widgets::Tone;

/// Maximum launch rows shown directly in the launches card; older launches remain
/// reachable through the dropdown selector and pagination controls.
const VISIBLE_RUN_ROWS: usize = 8;

fn status_color(state: &str, waiting: bool, p: Palette) -> Hsla {
    if waiting {
        return p.io;
    }
    match state {
        "recording" => p.network,
        "gap" => p.io,
        "interrupted" | "limit" => p.danger,
        _ => p.muted,
    }
}

fn status_label(state: &str, waiting: bool) -> String {
    if waiting {
        label("history.watch.waiting")
    } else {
        label(&format!("history.watch.state.{state}"))
    }
}

impl Desktop {
    pub(super) fn watch_anchor(&self, key: &'static str) -> AnyElement {
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

    pub(in crate::frontend) fn watch_view(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let scroll = window
            .use_keyed_state("watch-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let watch = self.data.get("processWatch").unwrap_or(&Value::Null);
        let content = if !watch.is_object() {
            if let Some(message) = self.data.get("watchError").and_then(Value::as_str) {
                self.watch_invalid(message, cx)
            } else {
                self.watch_empty(cx)
            }
        } else {
            self.watch_body(watch, window, cx)
        };
        div()
            .id("watch-content")
            .track_scroll(&scroll)
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            .pl(px(12.))
            .pr(px(12. + scrollbar::SIZE))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .line_height(relative(1.4))
            .child(content)
            .child(scrollbar::scrollbar("watch-scrollbar", &scroll, p.dark))
            .into_any_element()
    }

    fn watch_surface(&self) -> Div {
        let p = self.palette;
        div()
            .flex_none()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.layer)
    }

    fn watch_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        self.watch_surface()
            .items_center()
            .py(px(40.))
            .px(px(24.))
            .gap(px(12.))
            .child(
                div()
                    .size(px(48.))
                    .rounded_full()
                    .bg(p.accent.opacity(0.12))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("EyeIcon", p.accent).size(px(22.))),
            )
            .child(
                div()
                    .font_family(theme::display_font(cx))
                    .text_size(theme::font(16.))
                    .font_weight(theme::LABEL_WEIGHT)
                    .text_color(p.ink)
                    .child(label("history.watch.title")),
            )
            .child(
                div()
                    .max_w(px(480.))
                    .text_center()
                    .text_size(theme::font(13.))
                    .text_color(p.muted)
                    .child(label("history.watch.empty")),
            )
            .child(
                div().pt(px(4.)).child(
                    self.btn(
                        "watch-open-processes",
                        label("history.watch.openProcesses"),
                        Some("ProcessesIcon"),
                        Tone::Plain,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.act(Action::Page("processes".into()), window, cx);
                    })),
                ),
            )
            .child(
                div()
                    .pt(px(8.))
                    .max_w(px(560.))
                    .text_center()
                    .text_size(theme::font(12.))
                    .text_color(p.faint)
                    .child(label("history.watch.note")),
            )
            .into_any_element()
    }

    fn watch_invalid(&self, message: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        self.watch_surface()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(icon("WarningIcon", p.danger).size(px(18.)))
                    .child(
                        div()
                            .font_family(theme::display_font(cx))
                            .text_size(theme::font(15.))
                            .font_weight(theme::LABEL_WEIGHT)
                            .text_color(p.ink)
                            .child(label("history.watch.title")),
                    ),
            )
            .child(
                div()
                    .text_size(theme::font(13.))
                    .text_color(p.muted)
                    .child(message.to_owned()),
            )
            .child(
                div().flex().child(
                    self.btn_small(
                        "watch-delete-invalid",
                        label("history.watch.delete"),
                        Some("TrashIcon"),
                        Tone::Danger,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preference("watchProcess", json!({"op":"delete"}), window, cx)
                    })),
                ),
            )
            .into_any_element()
    }

    fn watch_status_pill(&self, state: &str, waiting: bool) -> AnyElement {
        let p = self.palette;
        let dot = status_color(state, waiting, p);
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .h(px(22.))
            .rounded_full()
            .border_1()
            .border_color(dot.opacity(0.35))
            .bg(dot.opacity(0.1))
            .text_size(theme::font(12.))
            .text_color(p.ink)
            .child(div().size(px(7.)).rounded_full().bg(dot).flex_shrink_0())
            .child(status_label(state, waiting))
            .into_any_element()
    }

    fn watch_metric_tile(
        &self,
        title: String,
        swatch: Option<Hsla>,
        value: String,
        sub: String,
        cx: &App,
    ) -> AnyElement {
        let p = self.palette;
        div()
            .flex_1()
            .min_w(px(148.))
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .when_some(swatch, |d, color| {
                        d.child(div().size(px(8.)).rounded(px(2.)).bg(color).flex_shrink_0())
                    })
                    .child(title),
            )
            .child(
                div()
                    .font_family(theme::display_font(cx))
                    .text_size(theme::font(20.))
                    .font_weight(theme::LABEL_WEIGHT)
                    .font_features(theme::tabular())
                    .text_color(p.ink)
                    .child(value),
            )
            .child(
                div()
                    .text_size(theme::font(12.))
                    .font_features(theme::tabular())
                    .text_color(p.muted)
                    .child(sub),
            )
            .into_any_element()
    }

    fn watch_body(&self, watch: &Value, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let state = text(watch, "state");
        let has_program = watch["program"].is_object();
        let active = matches!(state, "recording" | "gap");
        let waiting = has_program && active && watch["liveRuns"] == 0;
        let points = values(&watch["points"]);
        let runs = values(&watch["runs"]);
        let start = points
            .first()
            .map(|point| number(point, "ts"))
            .unwrap_or(0.);
        let last_ts = points
            .last()
            .map(|point| number(point, "ts"))
            .unwrap_or(start);
        let end = last_ts + argus_core::watch::SAMPLE_MS as f64;
        let latest_reading = points
            .iter()
            .rev()
            .find(|point| point["memory"].is_number() || point["commit"].is_number());
        let peak_of = |field: &str| {
            points
                .iter()
                .filter_map(|point| point[field].as_f64())
                .reduce(f64::max)
        };
        let memory_peak = peak_of("memory");
        let commit_peak = peak_of("commit");
        let format_opt = |value: Option<f64>| {
            value
                .map(theme::memory)
                .unwrap_or_else(|| label("format.missing"))
        };
        let latest_memory = format_opt(latest_reading.and_then(|point| point["memory"].as_f64()));
        let latest_commit = format_opt(latest_reading.and_then(|point| point["commit"].as_f64()));

        // Card 1: target identity, status pill, actions, key metrics, and notices.
        let mut head_meta = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .font_family(theme::display_font(cx))
                            .text_size(theme::font(18.))
                            .font_weight(theme::LABEL_WEIGHT)
                            .text_color(p.ink)
                            .child(tr!(
                                "history.watch.target",
                                name = text(watch, "name"),
                                pid = watch["pid"]
                            )),
                    )
                    .child(self.watch_status_pill(state, waiting)),
            );
        if has_program {
            let path = text(&watch["program"], "displayPath");
            if !path.is_empty() {
                head_meta = head_meta.child(
                    div()
                        .min_w_0()
                        .font_family(theme::mono_font(cx))
                        .text_size(theme::font(12.))
                        .text_color(p.muted)
                        .child(path.to_owned()),
                );
            }
        }
        let actions = div()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(
                self.btn_small(
                    "watch-stop",
                    label("history.watch.stop"),
                    Some("StopIcon"),
                    Tone::Plain,
                )
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
                    Some("TrashIcon"),
                    Tone::Plain,
                )
                .child(self.watch_anchor("watch-delete"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.preference("watchProcess", json!({"op":"delete"}), window, cx)
                })),
            );
        let total_runs = watch["totalRuns"].as_u64().unwrap_or(runs.len() as u64);
        let live_runs = watch["liveRuns"].as_u64().unwrap_or(0);
        let total_points = watch["totalPoints"].as_u64().unwrap_or(points.len() as u64);
        let metrics = div()
            .flex()
            .flex_wrap()
            .gap(px(16.))
            .child(self.watch_metric_tile(
                label("history.watch.metric.memory"),
                Some(p.memory),
                latest_memory.clone(),
                tr!("history.watch.peak", value = format_opt(memory_peak)),
                cx,
            ))
            .child(self.watch_metric_tile(
                label("history.watch.metric.commit"),
                Some(p.cpu),
                latest_commit.clone(),
                tr!("history.watch.peak", value = format_opt(commit_peak)),
                cx,
            ))
            .child(self.watch_metric_tile(
                label("history.watch.metric.runs"),
                None,
                total_runs.to_string(),
                tr!("history.watch.liveCount", count = live_runs),
                cx,
            ))
            .child(self.watch_metric_tile(
                label("history.watch.metric.points"),
                None,
                total_points.to_string(),
                date_time(number(watch, "createTime")),
                cx,
            ));

        let mut overview = self
            .watch_surface()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .child(head_meta)
                    .child(actions),
            )
            .child(div().h(px(1.)).bg(p.stroke))
            .child(metrics);

        let mut banners: Vec<String> = Vec::new();
        if watch["damaged"] == true {
            banners.push(label("history.watch.damaged"));
        }
        if number(watch, "unconfirmed") > 0. {
            banners.push(tr!(
                "history.watch.unconfirmed",
                count = watch["unconfirmed"]
            ));
        }
        if let Some(message) = self.data.get("watchError").and_then(Value::as_str) {
            banners.push(message.to_owned());
        }
        for notice in banners {
            overview = overview.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.stroke)
                    .bg(p.control)
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(icon("WarningIcon", p.io).size(px(14.)).flex_shrink_0())
                    .child(div().min_w_0().child(notice)),
            );
        }

        // Card 2: memory & commit curve with pagination controls.
        let names = [
            label("history.watch.metric.memory"),
            label("history.watch.metric.commit"),
        ];
        let latest = points.last();
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
            for ((field, color, fill), name) in
                [("memory", p.memory, 0.18_f32), ("commit", p.cpu, 0.0_f32)]
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
                        .fill(fill)
                        .source(charts::hash_of((key, field))),
                );
            }
            let plot = (key, Arc::new(series), maximum);
            cached.update(cx, |cache, _| *cache = Some(plot.clone()));
            plot
        });
        let chart_top = maximum * 1.1;
        let legend = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(16.))
            .gap_y(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(theme::font(13.))
                    .child(div().size(px(8.)).rounded(px(2.)).bg(p.memory))
                    .child(div().text_color(p.muted).child(names[0].clone()))
                    .child(
                        div()
                            .font_weight(theme::LABEL_WEIGHT)
                            .font_features(theme::tabular())
                            .text_color(p.ink)
                            .child(latest_memory),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(theme::font(13.))
                    .child(div().size(px(8.)).rounded(px(2.)).bg(p.cpu))
                    .child(div().text_color(p.muted).child(names[1].clone()))
                    .child(
                        div()
                            .font_weight(theme::LABEL_WEIGHT)
                            .font_features(theme::tabular())
                            .text_color(p.ink)
                            .child(latest_commit),
                    ),
            );
        let point_paging = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(
                self.btn_small(
                    "watch-earlier",
                    label("history.watch.earlier"),
                    Some("ChevronLeft"),
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
                    Some("ArrowRightIcon"),
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
            );
        let mut chart_card = self.watch_surface().gap(px(8.)).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(legend)
                .child(point_paging),
        );
        if !points.is_empty() {
            chart_card = chart_card
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .text_size(theme::font(11.))
                        .font_features(theme::tabular())
                        .text_color(p.faint)
                        .child(theme::memory(chart_top)),
                )
                .child(
                    div()
                        .h(px(260.))
                        .border_1()
                        .border_color(p.stroke)
                        .rounded(px(6.))
                        .bg(p.chart_surface)
                        .child(charts::TimePlot {
                            id: "process-watch-memory".into(),
                            series,
                            start,
                            end,
                            max: chart_top,
                            min: 0.,
                            interactive: true,
                            grid: true,
                            fill: true,
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
                            redraw_alone: false,
                            raster_span: None,
                            corner: 0.,
                        }),
                )
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .gap(px(8.))
                        .text_size(theme::font(11.))
                        .font_features(theme::tabular())
                        .text_color(p.faint)
                        .child(date_time(start))
                        .child(date_time(last_ts)),
                );
        } else {
            chart_card = chart_card.child(
                div()
                    .h(px(180.))
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(6.))
                    .bg(p.chart_surface)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(theme::font(13.))
                    .text_color(p.muted)
                    .child(label("history.watch.waitingFirst")),
            );
        }

        // Card 3: launch selector, pagination, and clickable launch list.
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
        let selected = runs
            .iter()
            .position(|run| run["pid"] == watch["pid"] && run["createTime"] == watch["createTime"]);
        let keys: Vec<_> = runs
            .iter()
            .map(|run| json!({"op":"selectRun","pid":run["pid"],"createTime":run["createTime"]}))
            .collect();
        let offset = watch["runOffset"].as_u64().unwrap_or(0);
        let run_controls = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(div().w(px(280.)).max_w_full().child(self.select(
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
            )))
            .child(
                self.btn_small(
                    "watch-older-runs",
                    label("history.watch.earlierRuns"),
                    Some("ChevronLeft"),
                    Tone::Plain,
                )
                .child(self.watch_anchor("watch-older-runs"))
                .disabled(offset + runs.len() as u64 >= total_runs)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.preference(
                        "watchProcess",
                        json!({"op":"runs","offset":offset+256}),
                        window,
                        cx,
                    )
                })),
            )
            .child(
                self.btn_small(
                    "watch-latest-runs",
                    label("history.watch.latestRuns"),
                    Some("ArrowRightIcon"),
                    Tone::Plain,
                )
                .child(self.watch_anchor("watch-latest-runs"))
                .disabled(offset == 0)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.preference("watchProcess", json!({"op":"runs","offset":0}), window, cx)
                })),
            );
        let mut launches_card =
            self.watch_surface().gap(px(10.)).child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap(px(8.))
                            .child(
                                div()
                                    .font_weight(theme::LABEL_WEIGHT)
                                    .text_size(theme::font(14.))
                                    .text_color(p.ink)
                                    .child(label("history.watch.launches")),
                            )
                            .child(div().text_size(theme::font(12.)).text_color(p.muted).child(
                                tr!(
                                    "history.watch.runSummary",
                                    shown = runs.len(),
                                    total = watch["totalRuns"],
                                    points = watch["totalPoints"]
                                ),
                            )),
                    )
                    .child(run_controls),
            );
        if !runs.is_empty() {
            // Show newest launches first, capped so the page stays compact; ensure
            // the currently selected launch is always present in the list.
            let mut visible_indices: Vec<usize> =
                (0..runs.len()).rev().take(VISIBLE_RUN_ROWS).collect();
            if let Some(sel) = selected
                && !visible_indices.contains(&sel)
            {
                visible_indices.push(sel);
            }
            let mut list = div().flex().flex_col().gap(px(4.));
            for index in visible_indices {
                let run = &runs[index];
                let is_selected = selected == Some(index);
                let run_state = text(run, "state");
                let dot = status_color(run_state, false, p);
                let action =
                    json!({"op":"selectRun","pid":run["pid"],"createTime":run["createTime"]});
                let row = div()
                    .id(SharedString::from(format!("watch-run-row-{index}")))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(if is_selected {
                        p.accent.opacity(0.45)
                    } else {
                        gpui_kit::gpui::transparent_black()
                    })
                    .bg(if is_selected {
                        p.accent.opacity(0.1)
                    } else {
                        p.control.opacity(0.5)
                    })
                    .cursor_pointer()
                    .when(!is_selected, |d| d.hover(move |s| s.bg(p.hover)))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .child(div().size(px(7.)).rounded_full().bg(dot).flex_shrink_0())
                            .child(
                                div()
                                    .font_weight(if is_selected {
                                        theme::LABEL_WEIGHT
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .font_features(theme::tabular())
                                    .text_size(theme::font(13.))
                                    .text_color(p.ink)
                                    .child(format!("PID {}", run["pid"])),
                            )
                            .child(
                                div()
                                    .text_size(theme::font(12.))
                                    .font_features(theme::tabular())
                                    .text_color(p.muted)
                                    .child(date_time(number(run, "createTime"))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .text_size(theme::font(12.))
                            .child(
                                div()
                                    .font_features(theme::tabular())
                                    .text_color(p.muted)
                                    .child(tr!("history.watch.pointCount", count = run["points"])),
                            )
                            .child(
                                div()
                                    .text_color(if run_state == "recording" {
                                        p.ink
                                    } else {
                                        p.muted
                                    })
                                    .child(label(&format!("history.watch.state.{run_state}"))),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.preference("watchProcess", action.clone(), window, cx);
                    }));
                list = list.child(row);
            }
            launches_card = launches_card.child(list);
        }

        let mut stack = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(overview)
            .child(chart_card)
            .child(launches_card);

        // Card 4: recent status events (gaps, exit, stop, interruption, limit).
        let events: Vec<_> = points
            .iter()
            .filter(|point| text(point, "state") != "recording")
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if !events.is_empty() {
            let mut event_list = div().flex().flex_col().gap(px(6.));
            for point in events {
                let point_state = text(point, "state");
                let dot = status_color(point_state, false, p);
                event_list = event_list.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .text_size(theme::font(12.))
                        .child(div().size(px(6.)).rounded_full().bg(dot).flex_shrink_0())
                        .child(
                            div()
                                .font_features(theme::tabular())
                                .text_color(p.muted)
                                .child(date_time(number(point, "ts"))),
                        )
                        .child(
                            div()
                                .text_color(p.ink)
                                .child(label(&format!("history.watch.state.{point_state}"))),
                        ),
                );
            }
            stack = stack.child(
                self.watch_surface()
                    .gap(px(8.))
                    .child(
                        div()
                            .font_weight(theme::LABEL_WEIGHT)
                            .text_size(theme::font(13.))
                            .text_color(p.ink)
                            .child(label("history.watch.events")),
                    )
                    .child(event_list),
            );
        }

        stack
            .child(
                div()
                    .px(px(4.))
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(label("history.watch.note")),
            )
            .into_any_element()
    }
}
