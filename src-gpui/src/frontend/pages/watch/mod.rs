//! The process observation subpage under History. An observed program's
//! runs are drawn by the system history's own lanes, tabs, navigator and
//! tooltips (`history::Source::Watch`); this module adds what is the
//! observation's own: the page before anything is observed, and the card of
//! the program and its runs where the system's history lists processes.
use super::*;
use widgets::Tone;

/// A run's row in the runs card.
const RUN_ROW: f32 = 30.;

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
    pub(super) fn watch_anchor(&self, key: impl Into<SharedString>) -> AnyElement {
        layout::anchor_probe(key, self.ui.anchors.clone()).into_any_element()
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
            // Drawn by the history page's lanes once it can be read.
            div().into_any_element()
        };
        div()
            .id("watch-content")
            .track_scroll(&scroll)
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            .pl(px(12.))
            .pr(px(12. + scrollbar::SIZE))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .line_height(relative(1.4))
            .child(content)
            .child(scrollbar::scrollbar("watch-scrollbar", &scroll, p.dark))
            .into_any_element()
    }

    fn watch_surface(&self) -> Div {
        let p = self.palette;
        div()
            .flex_none()
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(10.))
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
                    .max_w(px(560.))
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

    /// Under the chart, where the system's history lists processes: the
    /// observed program (name, path, state, what can be done) and its runs,
    /// newest first. A run clicked is drawn on the lanes above.
    pub(in crate::frontend) fn watch_panel(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let Some(watch) = self.data.get("processWatch").filter(|w| w.is_object()) else {
            return div().into_any_element();
        };
        let state = text(watch, "state");
        let active = matches!(state, "recording" | "gap");
        let live_runs = watch["liveRuns"].as_u64().unwrap_or(0);
        let waiting = watch["program"].is_object() && active && live_runs == 0;
        let runs = values(&watch["runs"]);
        let total_runs = watch["totalRuns"].as_u64().unwrap_or(runs.len() as u64);
        let offset = watch["runOffset"].as_u64().unwrap_or(0);
        let path = text(&watch["program"], "displayPath").to_owned();

        // Stopping is offered while it records; deleting always.
        let actions = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .when(active, |d| {
                d.child(
                    self.btn_small(
                        "watch-stop",
                        label("history.watch.stop"),
                        Some("StopIcon"),
                        Tone::Plain,
                    )
                    .child(self.watch_anchor("watch-stop"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preference("watchProcess", json!({"op":"stop"}), window, cx)
                    })),
                )
            })
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
        // The program: its name and state on one line with what can be done,
        // its path under them (long paths end in an ellipsis).
        let heading = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .pt(px(8.))
            .px(px(14.))
            .pb(px(8.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(10.))
                    .gap_y_1()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(theme::font(14.))
                            .font_weight(theme::LABEL_WEIGHT)
                            .text_color(p.ink)
                            .child(text(watch, "name").to_owned()),
                    )
                    .child(self.watch_status_pill(state, waiting))
                    .when(live_runs > 1, |d| {
                        d.child(
                            div()
                                .text_size(theme::font(12.))
                                .font_features(theme::tabular())
                                .text_color(p.muted)
                                .child(tr!("history.watch.liveCount", count = live_runs)),
                        )
                    })
                    .child(div().flex_1())
                    .child(actions),
            )
            .when(!path.is_empty(), |d| {
                d.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(theme::mono_font(cx))
                        .text_size(theme::font(11.5))
                        .text_color(p.muted)
                        .child(path),
                )
            });

        let mut notices: Vec<String> = Vec::new();
        if watch["damaged"] == true {
            notices.push(label("history.watch.damaged"));
        }
        if number(watch, "unconfirmed") > 0. {
            notices.push(tr!(
                "history.watch.unconfirmed",
                count = watch["unconfirmed"]
            ));
        }
        if let Some(message) = self.data.get("watchError").and_then(Value::as_str) {
            notices.push(message.to_owned());
        }
        let notices = notices.into_iter().map(|notice| {
            div()
                .flex()
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
                .child(icon("WarningIcon", p.io).size(px(14.)).flex_shrink_0())
                .child(div().min_w_0().child(notice))
        });

        // The runs as a table: newest first, the one drawn selected.
        let cell = |width: Option<f32>, right: bool| {
            div()
                .flex_shrink_0()
                .when_some(width, |d, w| d.w(px(w)))
                .when(width.is_none(), |d| d.flex_1().min_w_0())
                .truncate()
                .when(right, |d| d.text_right())
        };
        let columns: [(&str, Option<f32>, bool); 6] = [
            ("pid", Some(96.), false),
            ("started", None, false),
            ("last", None, false),
            ("duration", Some(112.), true),
            ("points", Some(84.), true),
            ("state", Some(150.), false),
        ];
        let head = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(30.))
            .pl(px(32.))
            .pr(px(14.))
            .border_t_1()
            .border_b_1()
            .border_color(p.stroke)
            .text_size(theme::font(12.))
            .text_color(p.muted)
            .children(columns.iter().map(|(key, width, right)| {
                cell(*width, *right).child(label(&format!("history.watch.column.{key}")))
            }));
        let now = self.snapshot["timestamp"].as_i64().unwrap_or(0);
        let rows = runs
            .iter()
            .rev()
            .enumerate()
            .map(|(index, run)| {
                let selected =
                    run["pid"] == watch["pid"] && run["createTime"] == watch["createTime"];
                let run_state = text(run, "state");
                let recording = run_state == "recording";
                let first = run["firstTs"].as_i64().filter(|ts| *ts > 0);
                let last = run["lastTs"].as_i64().filter(|ts| *ts > 0);
                let end = if recording {
                    now.max(last.unwrap_or(0))
                } else {
                    last.unwrap_or(0)
                };
                let duration = first
                    .map(|first| super::history::duration_label((end - first).max(0)))
                    .unwrap_or_else(|| label("format.missing"));
                let stamp = |ts: Option<i64>| {
                    ts.map(|ts| date_time(ts as f64))
                        .unwrap_or_else(|| label("format.missing"))
                };
                let dot = status_color(run_state, false, p);
                let action =
                    json!({"op":"selectRun","pid":run["pid"],"createTime":run["createTime"]});
                div()
                    .id(SharedString::from(format!("watch-run-row-{index}")))
                    .relative()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(12.))
                    .h(px(RUN_ROW))
                    .pl(px(14.))
                    .pr(px(14.))
                    .text_size(theme::font(13.))
                    .font_features(theme::tabular())
                    .text_color(p.ink)
                    .when(selected, |d| d.bg(p.accent.opacity(0.12)))
                    .when(!selected, |d| {
                        d.cursor_pointer().hover(move |s| s.bg(p.hover))
                    })
                    // The run drawn: an accent bar at the row's start.
                    .when(selected, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left(px(4.))
                                .top(px(7.))
                                .bottom(px(7.))
                                .w(px(3.))
                                .rounded_full()
                                .bg(p.accent),
                        )
                    })
                    .child(
                        div()
                            .w(px(6.))
                            .flex_shrink_0()
                            .flex()
                            .justify_center()
                            .child(div().size(px(7.)).rounded_full().bg(dot)),
                    )
                    .child(self.watch_anchor(format!("watch-run-{index}")))
                    .child(cell(Some(96.), false).child(run["pid"].to_string()))
                    .child(cell(None, false).child(date_time(number(run, "createTime"))))
                    .child(cell(None, false).text_color(p.muted).child(stamp(last)))
                    .child(cell(Some(112.), true).child(duration))
                    .child(
                        cell(Some(84.), true)
                            .text_color(p.muted)
                            .child(crate::regional::count(number(run, "points"))),
                    )
                    .child(
                        cell(Some(150.), false)
                            .text_color(if recording { p.ink } else { p.muted })
                            .child(label(&format!("history.watch.state.{run_state}"))),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.preference("watchProcess", action.clone(), window, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let rows_scroll = window
            .use_keyed_state("watch-runs-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let body = div()
            .id("watch-runs")
            .relative()
            .flex_1()
            .min_h_0()
            .track_scroll(&rows_scroll)
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .children(rows)
            .child(scrollbar::scrollbar(
                "watch-runs-scrollbar",
                &rows_scroll,
                p.dark,
            ));

        // What is shown of how many, and how long records are kept.
        let paging = total_runs > runs.len() as u64;
        let foot = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_x_4()
            .gap_y_1()
            .px(px(14.))
            .py(px(8.))
            .border_t_1()
            .border_color(p.stroke)
            .text_size(theme::font(12.))
            .text_color(p.muted)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(tr!(
                        "history.watch.runSummary",
                        shown = runs.len(),
                        total = total_runs,
                        points = watch["totalPoints"]
                    ))
                    .when(paging, |d| {
                        d.child(
                            self.btn_small(
                                "watch-older-runs",
                                label("history.watch.earlierRuns"),
                                Some("ChevronLeft"),
                                Tone::Plain,
                            )
                            .child(self.watch_anchor("watch-older-runs"))
                            .disabled(offset + runs.len() as u64 >= total_runs)
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.preference(
                                        "watchProcess",
                                        json!({"op":"runs","offset":offset + 256}),
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
                                Some("ArrowRightIcon"),
                                Tone::Plain,
                            )
                            .child(self.watch_anchor("watch-latest-runs"))
                            .disabled(offset == 0)
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.preference(
                                        "watchProcess",
                                        json!({"op":"runs","offset":0}),
                                        window,
                                        cx,
                                    )
                                },
                            )),
                        )
                    }),
            )
            .child(div().child(label("history.watch.note")));

        div()
            .id("watch-panel")
            .flex_1()
            .min_h(px(RUN_ROW * 3. + 160.))
            .flex()
            .flex_col()
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.layer)
            .overflow_hidden()
            .child(heading)
            .children(notices)
            .child(head)
            .child(body)
            .child(foot)
            .into_any_element()
    }
}
