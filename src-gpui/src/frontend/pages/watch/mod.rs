//! Dedicated process observation subpage under History, modeled after the
//! system history page's reading tabs, multi-lane dual-axis charts, and lower
//! launches panel.
use super::*;
use widgets::Tone;

mod lanes;
mod metrics;

/// Axis label gutter width either side of the plots, matching `history/lanes.rs`.
pub(super) const GUTTER: f32 = 48.;
/// Maximum concurrent chart lanes on the observation page.
pub(super) const MAX_LANES: usize = 4;
/// Maximum launch rows shown directly in the launches card.
const VISIBLE_RUN_ROWS: usize = 8;

#[derive(Clone, Debug)]
pub(in crate::frontend) enum Command {
    /// Show this metric in the active lane.
    Show(&'static str),
    /// Add a new lane showing this metric (or activate it if already shown).
    AddLane(&'static str),
    /// Change the primary metric of a specific lane.
    SetReading(u64, &'static str),
    /// Set or clear the overlay metric on a specific lane.
    SetOverlay(u64, Option<&'static str>),
    /// Toggle this metric as the active lane's overlay (right-click on a reading tab).
    ToggleOverlay(&'static str),
    /// Remove a lane when more than one exists.
    RemoveLane(u64),
}

#[derive(Clone, Debug)]
pub(in crate::frontend) struct WatchLane {
    pub(in crate::frontend) id: u64,
    pub(in crate::frontend) reading: &'static str,
    pub(in crate::frontend) overlay: Option<&'static str>,
}

pub(in crate::frontend) struct WatchUiState {
    pub(in crate::frontend) lanes: Vec<WatchLane>,
    pub(in crate::frontend) active: u64,
    next_id: u64,
    pub(super) hover: Option<i64>,
    pub(super) pin: Option<i64>,
    pub(super) plot_bounds: Rc<RefCell<Bounds<Pixels>>>,
}

impl Default for WatchUiState {
    fn default() -> Self {
        Self {
            lanes: vec![WatchLane {
                id: 1,
                reading: "memory",
                overlay: Some("commit"),
            }],
            active: 1,
            next_id: 2,
            hover: None,
            pin: None,
            plot_bounds: Rc::new(RefCell::new(Bounds::default())),
        }
    }
}

impl WatchUiState {
    pub(in crate::frontend) fn clear_cursor(&mut self) {
        self.hover = None;
        self.pin = None;
    }

    pub(super) fn active_lane(&self) -> &WatchLane {
        self.lanes
            .iter()
            .find(|lane| lane.id == self.active)
            .unwrap_or(&self.lanes[0])
    }

    fn active_lane_mut(&mut self) -> &mut WatchLane {
        let active = self.active;
        if let Some(pos) = self.lanes.iter().position(|lane| lane.id == active) {
            &mut self.lanes[pos]
        } else {
            &mut self.lanes[0]
        }
    }
}

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

    pub(in crate::frontend) fn watch_command(
        &mut self,
        command: Command,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::Show(id) => {
                let lane = self.watch_ui.active_lane_mut();
                lane.reading = id;
                if lane.overlay == Some(id) {
                    lane.overlay = None;
                }
            }
            Command::AddLane(id) => {
                if let Some(existing) = self.watch_ui.lanes.iter().find(|l| l.reading == id) {
                    self.watch_ui.active = existing.id;
                } else if self.watch_ui.lanes.len() < MAX_LANES {
                    let lane_id = self.watch_ui.next_id;
                    self.watch_ui.next_id += 1;
                    self.watch_ui.lanes.push(WatchLane {
                        id: lane_id,
                        reading: id,
                        overlay: None,
                    });
                    self.watch_ui.active = lane_id;
                }
            }
            Command::SetReading(lane_id, id) => {
                if let Some(lane) = self.watch_ui.lanes.iter_mut().find(|l| l.id == lane_id) {
                    lane.reading = id;
                    if lane.overlay == Some(id) {
                        lane.overlay = None;
                    }
                    self.watch_ui.active = lane_id;
                }
            }
            Command::SetOverlay(lane_id, id) => {
                if let Some(lane) = self.watch_ui.lanes.iter_mut().find(|l| l.id == lane_id) {
                    let valid = id.filter(|&oid| {
                        oid != lane.reading
                            && metrics::metric_by_id(oid).is_some_and(|m| !m.is_pair())
                    });
                    lane.overlay = valid;
                    self.watch_ui.active = lane_id;
                }
            }
            Command::ToggleOverlay(id) => {
                if metrics::metric_by_id(id).is_some_and(|m| !m.is_pair()) {
                    let lane = self.watch_ui.active_lane_mut();
                    if lane.reading != id {
                        lane.overlay = if lane.overlay == Some(id) {
                            None
                        } else {
                            Some(id)
                        };
                    }
                }
            }
            Command::RemoveLane(lane_id) => {
                if self.watch_ui.lanes.len() > 1 {
                    self.watch_ui.lanes.retain(|l| l.id != lane_id);
                    if !self
                        .watch_ui
                        .lanes
                        .iter()
                        .any(|l| l.id == self.watch_ui.active)
                    {
                        self.watch_ui.active = self.watch_ui.lanes[0].id;
                    }
                }
            }
        }
        cx.notify();
    }

    pub(in crate::frontend) fn watch_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if key == "escape" && self.watch_ui.pin.is_some() {
            self.watch_ui.pin = None;
            cx.notify();
            return true;
        }
        false
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

    fn watch_body(&self, watch: &Value, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let state = text(watch, "state");
        let has_program = watch["program"].is_object();
        let active = matches!(state, "recording" | "gap");
        let waiting = has_program && active && watch["liveRuns"] == 0;
        let points = values(&watch["points"]);
        let runs = values(&watch["runs"]);
        let total_runs = watch["totalRuns"].as_u64().unwrap_or(runs.len() as u64);
        let live_runs = watch["liveRuns"].as_u64().unwrap_or(0);

        // 1. Compact target header bar (target identity, status pill, path, stop/delete).
        let mut head_left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .font_family(theme::display_font(cx))
                            .text_size(theme::font(16.))
                            .font_weight(theme::LABEL_WEIGHT)
                            .text_color(p.ink)
                            .child(tr!(
                                "history.watch.target",
                                name = text(watch, "name"),
                                pid = watch["pid"]
                            )),
                    )
                    .child(self.watch_status_pill(state, waiting))
                    .child(
                        div()
                            .text_size(theme::font(12.))
                            .font_features(theme::tabular())
                            .text_color(p.muted)
                            .child(tr!("history.watch.liveCount", count = live_runs)),
                    ),
            );
        if has_program {
            let path = text(&watch["program"], "displayPath");
            if !path.is_empty() {
                head_left = head_left.child(
                    div()
                        .min_w_0()
                        .font_family(theme::mono_font(cx))
                        .text_size(theme::font(11.5))
                        .text_color(p.muted)
                        .child(path.to_owned()),
                );
            }
        }
        let head_actions = div()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
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

        let mut header_card = self
            .watch_surface()
            .py(px(10.))
            .px(px(14.))
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(head_left)
                    .child(head_actions),
            );
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
        for notice in notices {
            header_card = header_card.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(6.))
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

        // 2. History-style reading tabs bar + 3. Multi-lane dual-axis chart card.
        let reading_bar = self.watch_reading_bar(points, window, cx);
        let chart_card = self.watch_chart_card(watch, points, window, cx);

        // 4. Lower panel: launches list & selector (styled like history's lower table card).
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
            .gap(px(6.))
            .child(div().w(px(260.)).max_w_full().child(self.select(
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
            self.watch_surface().gap(px(8.)).child(
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
                                    .text_size(theme::font(13.))
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
            let mut visible_indices: Vec<usize> =
                (0..runs.len()).rev().take(VISIBLE_RUN_ROWS).collect();
            if let Some(sel) = selected
                && !visible_indices.contains(&sel)
            {
                visible_indices.push(sel);
            }
            let mut list = div().flex().flex_col().gap(px(3.));
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
                    .py(px(6.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(if is_selected {
                        p.accent.opacity(0.45)
                    } else {
                        gpui_kit::gpui::transparent_black()
                    })
                    .bg(if is_selected {
                        p.accent.opacity(0.1)
                    } else {
                        p.control.opacity(0.45)
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
                                    .text_size(theme::font(12.5))
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
            .gap(px(10.))
            .child(header_card)
            .child(reading_bar)
            .child(chart_card)
            .child(launches_card);

        // Status events card (gaps, exit, stop, interruption, limit).
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
            let mut event_list = div().flex().flex_col().gap(px(5.));
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
                    .gap(px(6.))
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
