//! History-style reading tabs and multi-lane dual-axis charts for process observation.
use super::super::*;
use super::metrics::{METRICS, WatchMetric, metric_by_id};
use super::{Command, GUTTER, MAX_LANES, WatchLane};
use crate::frontend::layout::Side;
use widgets::Tone;

const AXIS_ROWS: u32 = 4;

fn format_clock_tick(ts_ms: i64, show_date: bool) -> String {
    let Ok(utc) = time::OffsetDateTime::from_unix_timestamp(ts_ms.div_euclid(1000)) else {
        return String::new();
    };
    let offset = time::UtcOffset::local_offset_at(utc).unwrap_or(time::UtcOffset::UTC);
    let local = utc.to_offset(offset);
    if show_date {
        format!(
            "{:02}-{:02} {:02}:{:02}",
            local.month() as u8,
            local.day(),
            local.hour(),
            local.minute()
        )
    } else {
        format!(
            "{:02}:{:02}:{:02}",
            local.hour(),
            local.minute(),
            local.second()
        )
    }
}

fn time_ticks(start: i64, end: i64, max_ticks: usize) -> Vec<i64> {
    let span = (end - start).max(1);
    let candidates: &[i64] = &[
        5_000,
        10_000,
        15_000,
        30_000,
        60_000,
        2 * 60_000,
        5 * 60_000,
        10 * 60_000,
        15 * 60_000,
        30 * 60_000,
        60 * 60_000,
        2 * 60 * 60_000,
        6 * 60 * 60_000,
        12 * 60 * 60_000,
        24 * 60 * 60_000,
    ];
    let target = (span / max_ticks.max(2) as i64).max(1);
    let step = candidates
        .iter()
        .copied()
        .find(|&s| s >= target)
        .unwrap_or(24 * 60 * 60_000);
    let first = start.div_euclid(step) * step + step;
    let mut out = Vec::new();
    let mut t = first;
    while t < end {
        out.push(t);
        t += step;
    }
    out
}

fn onto(value: f64, from_top: f64, to_top: f64) -> f64 {
    let from = from_top.max(f64::EPSILON);
    value / from * to_top
}

fn nearest_point_index(points: &[Value], target_ts: i64) -> Option<usize> {
    if points.is_empty() {
        return None;
    }
    let idx = points.partition_point(|p| (number(p, "ts") as i64) < target_ts);
    if idx == 0 {
        Some(0)
    } else if idx >= points.len() {
        Some(points.len() - 1)
    } else {
        let prev = (number(&points[idx - 1], "ts") as i64).abs_diff(target_ts);
        let next = (number(&points[idx], "ts") as i64).abs_diff(target_ts);
        Some(if prev <= next { idx - 1 } else { idx })
    }
}

/// The newest recorded point, else the newest of any kind; none before the
/// first point (a watch just started).
fn latest_recording_index(points: &[Value]) -> Option<usize> {
    points
        .iter()
        .rposition(|p| text(p, "state") == "recording")
        .or_else(|| points.len().checked_sub(1))
}

impl Desktop {
    pub(super) fn watch_read_index(&self, points: &[Value]) -> Option<usize> {
        if let Some(at) = self.watch_ui.hover.or(self.watch_ui.pin) {
            return nearest_point_index(points, at);
        }
        latest_recording_index(points)
    }

    pub(super) fn watch_bits(&self) -> bool {
        self.prefs.get("netUnit", String::from("bytes")) == "bits"
    }

    /// History-style reading tabs above the chart card.
    pub(super) fn watch_reading_bar(
        &self,
        points: &[Value],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let bits = self.watch_bits();
        let read_index = self.watch_read_index(points);
        let point = read_index.and_then(|i| points.get(i));
        let lanes = &self.watch_ui.lanes;
        let multi = lanes.len() > 1;
        let active_reading = self.watch_ui.active_lane().reading;
        let value_size = theme::font(13.);
        let tabs_scroll = window
            .use_keyed_state("watch-tabs-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();

        let mut row = div().flex().flex_none().items_center().gap(px(4.));
        for metric in METRICS {
            let id = metric.id;
            let color = metric.color(p);
            let shown = lanes.iter().any(|lane| lane.reading == id);
            let overlaid = lanes.iter().any(|lane| lane.overlay == Some(id));
            let current = multi && active_reading == id;
            let has_data = points.iter().any(|pt| metric.headline_value(pt).is_some());
            let value_text = point
                .and_then(|pt| metric.headline_value(pt))
                .map(|v| metric.format(v, bits))
                .unwrap_or_else(|| label("format.missing"));
            let width = metric
                .widest_samples(bits)
                .iter()
                .map(|s| theme::text_width(s, value_size, window, cx))
                .fold(px(0.), Pixels::max)
                .ceil();
            let group = SharedString::from(format!("watch-tab-{id}"));
            let border = if current {
                color.opacity(0.6)
            } else if overlaid {
                color.opacity(0.7)
            } else {
                gpui_kit::gpui::transparent_black()
            };
            let can_add = lanes.len() < MAX_LANES;
            row = row.child(
                div()
                    .id(group.clone())
                    .group(group.clone())
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .h(px(30.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(border)
                    .when(overlaid && !current, |d| d.border_dashed())
                    .text_color(if shown { p.ink } else { p.muted })
                    .when(shown, |d| d.bg(color.opacity(0.15)))
                    .when(!shown, |d| d.hover(move |s| s.bg(p.hover_strong)))
                    .cursor_pointer()
                    .child(layout::anchor_probe(
                        format!("watch-tab-{id}"),
                        self.ui.anchors.clone(),
                    ))
                    .child(div().size(px(8.)).rounded(px(2.)).bg(color).flex_shrink_0())
                    .child(
                        div()
                            .text_size(theme::font(13.))
                            .whitespace_nowrap()
                            .child(metric.short_name()),
                    )
                    .child(
                        div()
                            .min_w(width)
                            .text_right()
                            .text_size(value_size)
                            .when(has_data, |d| {
                                d.text_color(p.ink).font_weight(FontWeight::SEMIBOLD)
                            })
                            .when(!has_data, |d| d.text_color(p.faint))
                            .font_features(theme::tabular())
                            .whitespace_nowrap()
                            .child(value_text),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("watch-tab-add-{id}")))
                            .relative()
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .opacity(0.)
                            .when(can_add, |d| {
                                d.group_hover(group, |s| s.opacity(1.))
                                    .hover(move |s| s.bg(p.hover_strong))
                            })
                            .child(icon("PlusIcon", p.muted).size(px(12.)))
                            .tooltip(|window, cx| {
                                widgets::tooltip(label("history.tab.add"), window, cx)
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.watch_command(Command::AddLane(id), window, cx);
                            })),
                    )
                    .tooltip(|window, cx| {
                        widgets::tooltip(label("history.watch.tabTitle"), window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.watch_command(Command::Show(id), window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.watch_command(Command::ToggleOverlay(id), window, cx);
                        }),
                    ),
            );
        }

        let scroll_for_wheel = tabs_scroll.clone();
        let scroller = div()
            .id("watch-tabs")
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&tabs_scroll)
            .on_scroll(move |event: &ScrollEvent, _, _| {
                let delta = event.delta.pixel_delta(px(24.));
                let shift = if delta.x != px(0.) { delta.x } else { delta.y };
                if shift != px(0.) {
                    let mut offset = scroll_for_wheel.offset();
                    offset.x += shift;
                    scroll_for_wheel.set_offset(offset);
                }
            })
            .child(row);

        let add_lane = (lanes.len() < MAX_LANES).then(|| {
            div()
                .id("watch-add-lane")
                .relative()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(10.))
                .h(px(30.))
                .rounded(px(6.))
                .text_size(theme::font(13.))
                .text_color(p.muted)
                .whitespace_nowrap()
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover_strong).text_color(p.ink))
                .child(icon("PlusIcon", p.muted).size(px(13.)))
                .child(label("history.lane.add"))
                .child(layout::anchor_probe(
                    "watch-add-lane",
                    self.ui.anchors.clone(),
                ))
                .tooltip(|window, cx| widgets::tooltip(label("history.lane.addTitle"), window, cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.watch_open_add_lane_menu(window, cx);
                }))
        });

        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(scroller)
            .children(add_lane)
            .into_any_element()
    }

    fn watch_open_add_lane_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items: Vec<MenuItem> = METRICS
            .iter()
            .map(|metric| {
                let shown = self.watch_ui.lanes.iter().any(|l| l.reading == metric.id);
                MenuItem::new(
                    metric.long_name(),
                    Action::Watch(Command::AddLane(metric.id)),
                )
                .checked(shown)
            })
            .collect();
        self.open_anchored(items, "watch-add-lane", Side::Below, window, cx);
    }

    fn watch_open_metric_menu(
        &mut self,
        lane_id: u64,
        current: &str,
        anchor: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items: Vec<MenuItem> = METRICS
            .iter()
            .map(|metric| {
                MenuItem::new(
                    metric.long_name(),
                    Action::Watch(Command::SetReading(lane_id, metric.id)),
                )
                .checked(metric.id == current)
            })
            .collect();
        self.open_anchored(items, anchor, Side::Below, window, cx);
    }

    fn watch_open_overlay_menu(
        &mut self,
        lane_id: u64,
        primary: &str,
        current: Option<&str>,
        anchor: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut items = vec![
            MenuItem::new(
                label("history.menu.none"),
                Action::Watch(Command::SetOverlay(lane_id, None)),
            )
            .checked(current.is_none()),
            MenuItem::separator(),
        ];
        // Single-line metrics overlay cleanly on a lane without crowding two pair curves.
        for metric in METRICS.iter().filter(|m| m.id != primary && !m.is_pair()) {
            items.push(
                MenuItem::new(
                    metric.long_name(),
                    Action::Watch(Command::SetOverlay(lane_id, Some(metric.id))),
                )
                .checked(current == Some(metric.id)),
            );
        }
        self.open_anchored(items, anchor, Side::Below, window, cx);
    }

    /// Multi-lane chart card styled after `history/lanes.rs`.
    pub(super) fn watch_chart_card(
        &self,
        watch: &Value,
        points: &[Value],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let bits = self.watch_bits();
        let read_index = self.watch_read_index(points);
        let start = points.first().map(|pt| number(pt, "ts")).unwrap_or(0.);
        let last_ts = points.last().map(|pt| number(pt, "ts")).unwrap_or(start);
        let end = (last_ts + argus_core::watch::SAMPLE_MS as f64).max(start + 1000.);
        let span = (end - start).max(1.);
        let show_date = span >= 24. * 3600. * 1000.;
        let ticks = time_ticks(start as i64, end as i64, 7);

        let lanes = self.watch_ui.lanes.clone();
        let multi = lanes.len() > 1;
        let lane_height = if multi { 164. } else { 256. };
        let cursor_ts = self.watch_ui.hover.or(self.watch_ui.pin);
        let pinned_only = self.watch_ui.hover.is_none() && self.watch_ui.pin.is_some();

        let mut card = div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(10.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            // Opaque surface so charts stay on the compositor path.
            .bg(p.chart_surface);

        if points.is_empty() {
            card = card.child(
                div()
                    .h(px(220.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(theme::font(13.))
                    .text_color(p.muted)
                    .child(label("history.watch.waitingFirst")),
            );
        } else {
            let mut lanes_col = div().flex().flex_col().gap(px(12.));
            for lane in &lanes {
                lanes_col = lanes_col.child(self.watch_lane(
                    lane,
                    multi,
                    lane_height,
                    watch,
                    points,
                    (start, end),
                    &ticks,
                    read_index,
                    cursor_ts,
                    pinned_only,
                    bits,
                    window,
                    cx,
                ));
            }
            card = card.child(lanes_col);

            // Unified time axis directly beneath the plots, aligned with both gutters.
            let mut time_axis = div().relative().flex_1().h(px(20.)).overflow_hidden();
            for &tick in &ticks {
                let frac = ((tick as f64 - start) / span) as f32;
                if frac > 0.04 && frac < 0.96 {
                    time_axis = time_axis.child(
                        div()
                            .absolute()
                            .left(relative(frac))
                            .ml(px(-32.))
                            .w(px(64.))
                            .text_center()
                            .text_size(theme::font(11.))
                            .font_features(theme::tabular())
                            .text_color(p.faint)
                            .whitespace_nowrap()
                            .child(format_clock_tick(tick, show_date)),
                    );
                }
            }
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .child(div().w(px(GUTTER)).flex_shrink_0())
                    .child(time_axis)
                    .child(div().w(px(GUTTER)).flex_shrink_0()),
            );
        }

        // Footer bar: hovered/pinned moment info (or page time range) + earlier/latest paging.
        let left_footer = if let Some(at) = cursor_ts
            && let Some(idx) = nearest_point_index(points, at)
            && let Some(pt) = points.get(idx)
        {
            let pt_state = text(pt, "state");
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.))
                .text_size(theme::font(12.))
                .child(
                    div()
                        .font_weight(theme::LABEL_WEIGHT)
                        .font_features(theme::tabular())
                        .text_color(if pinned_only { p.accent } else { p.ink })
                        .child(date_time(number(pt, "ts"))),
                )
                .when(pt_state != "recording", |d| {
                    d.child(
                        div()
                            .text_color(p.io)
                            .child(label(&format!("history.watch.state.{pt_state}"))),
                    )
                })
                .child(div().text_color(p.faint).child(label("history.tip.hint")))
        } else {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.))
                .text_size(theme::font(12.))
                .font_features(theme::tabular())
                .text_color(p.muted)
                .when(!points.is_empty(), |d| {
                    d.child(format!("{} — {}", date_time(start), date_time(last_ts)))
                })
                .child(div().text_color(p.faint).child(label("history.tip.frame")))
        };

        let paging = div()
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

        card.child(div().h(px(1.)).bg(p.stroke))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .px(px(4.))
                    .child(left_footer)
                    .child(paging),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn watch_lane(
        &self,
        lane: &WatchLane,
        multi: bool,
        plot_height: f32,
        watch: &Value,
        points: &[Value],
        (start, end): (f64, f64),
        ticks: &[i64],
        read_index: Option<usize>,
        cursor_ts: Option<i64>,
        pinned_only: bool,
        bits: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let lane_id = lane.id;
        let active = multi && self.watch_ui.active == lane_id;
        let metric = metric_by_id(lane.reading).unwrap_or(&METRICS[0]);
        let overlay = lane.overlay.and_then(metric_by_id);
        let shared = overlay.is_some_and(|o| metric.shares_axis(*o));

        let primary_max = points
            .iter()
            .filter_map(|pt| metric.series_max_at(pt))
            .fold(0.0_f64, f64::max);
        let overlay_max = overlay.map(|o| {
            points
                .iter()
                .filter_map(|pt| o.series_max_at(pt))
                .fold(0.0_f64, f64::max)
        });
        let left_top = if shared {
            metric.axis_top(primary_max.max(overlay_max.unwrap_or(0.)), bits)
        } else {
            metric.axis_top(primary_max, bits)
        };
        let right_top = if let Some(o) = overlay
            && !shared
        {
            Some(o.axis_top(overlay_max.unwrap_or(0.), bits))
        } else {
            None
        };

        // Header row: main metric selector + live/hover value + peak + overlay control + remove.
        let point = read_index.and_then(|i| points.get(i));
        let primary_color = metric.color(p);
        let metric_anchor = format!("watch-lane-metric-{lane_id}");
        let overlay_anchor = format!("watch-lane-overlay-{lane_id}");

        let primary_readout = if let Some((part_a, part_b)) = metric.part_names() {
            let (a, b) = point.map(|pt| metric.read_pair(pt)).unwrap_or((None, None));
            let fmt = |v: Option<f64>| {
                v.map(|v| metric.format(v, bits))
                    .unwrap_or_else(|| label("format.missing"))
            };
            format!("{part_a} {} · {part_b} {}", fmt(a), fmt(b))
        } else {
            point
                .and_then(|pt| metric.headline_value(pt))
                .map(|v| metric.format(v, bits))
                .unwrap_or_else(|| label("format.missing"))
        };
        let peak_text = points
            .iter()
            .filter_map(|pt| metric.headline_value(pt))
            .reduce(f64::max)
            .map(|peak| tr!("history.watch.peak", value = metric.format(peak, bits)));

        let metric_btn = div()
            .id(SharedString::from(format!(
                "watch-lane-metric-btn-{lane_id}"
            )))
            .relative()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(6.))
            .h(px(24.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(move |s| s.bg(p.hover))
            .child(layout::anchor_probe(
                metric_anchor.clone(),
                self.ui.anchors.clone(),
            ))
            .child(div().size(px(8.)).rounded(px(2.)).bg(primary_color))
            .child(
                div()
                    .font_weight(theme::LABEL_WEIGHT)
                    .text_size(theme::font(13.))
                    .text_color(p.ink)
                    .child(metric.long_name()),
            )
            .child(icon("ChevronDown", p.muted).size(px(12.)))
            .on_click(cx.listener({
                let current = metric.id;
                let anchor = metric_anchor.clone();
                move |this, _, window, cx| {
                    this.watch_open_metric_menu(lane_id, current, &anchor, window, cx);
                }
            }));

        let mut header_left = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(metric_btn)
            .child(
                div()
                    .text_size(theme::font(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_features(theme::tabular())
                    .text_color(primary_color)
                    .child(primary_readout),
            );
        if let Some(peak) = peak_text {
            header_left = header_left.child(
                div()
                    .text_size(theme::font(12.))
                    .font_features(theme::tabular())
                    .text_color(p.muted)
                    .child(peak),
            );
        }

        let overlay_control = if let Some(o) = overlay {
            let o_color = o.color(p);
            let o_val = point
                .and_then(|pt| o.headline_value(pt))
                .map(|v| o.format(v, bits))
                .unwrap_or_else(|| label("format.missing"));
            let axis_tag = label(if shared {
                "history.overlay.shared"
            } else {
                "history.overlay.right"
            });
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "watch-lane-overlay-btn-{lane_id}"
                        )))
                        .relative()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(6.))
                        .h(px(24.))
                        .rounded(px(4.))
                        .border_1()
                        .border_dashed()
                        .border_color(o_color.opacity(0.6))
                        .bg(o_color.opacity(0.08))
                        .cursor_pointer()
                        .hover(move |s| s.bg(o_color.opacity(0.16)))
                        .child(layout::anchor_probe(
                            overlay_anchor.clone(),
                            self.ui.anchors.clone(),
                        ))
                        .child(div().w(px(10.)).h(px(2.)).bg(o_color))
                        .child(
                            div()
                                .text_size(theme::font(12.))
                                .text_color(p.ink)
                                .child(o.short_name()),
                        )
                        .child(
                            div()
                                .text_size(theme::font(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .font_features(theme::tabular())
                                .text_color(o_color)
                                .child(o_val),
                        )
                        .child(
                            div()
                                .text_size(theme::font(11.))
                                .text_color(p.muted)
                                .child(axis_tag),
                        )
                        .tooltip(|window, cx| {
                            widgets::tooltip(label("history.overlay.change"), window, cx)
                        })
                        .on_click(cx.listener({
                            let primary_id = metric.id;
                            let current = Some(o.id);
                            let anchor = overlay_anchor.clone();
                            move |this, _, window, cx| {
                                this.watch_open_overlay_menu(
                                    lane_id, primary_id, current, &anchor, window, cx,
                                );
                            }
                        })),
                )
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "watch-lane-overlay-clear-{lane_id}"
                        )))
                        .size(px(22.))
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(move |s| s.bg(p.hover))
                        .child(icon("DismissIcon", p.muted).size(px(12.)))
                        .tooltip(|window, cx| {
                            widgets::tooltip(label("history.overlay.remove"), window, cx)
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.watch_command(Command::SetOverlay(lane_id, None), window, cx);
                        })),
                )
                .into_any_element()
        } else {
            div()
                .id(SharedString::from(format!(
                    "watch-lane-overlay-btn-{lane_id}"
                )))
                .relative()
                .flex()
                .items_center()
                .gap(px(5.))
                .px(px(8.))
                .h(px(24.))
                .rounded(px(4.))
                .text_size(theme::font(12.))
                .text_color(p.muted)
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover).text_color(p.ink))
                .child(layout::anchor_probe(
                    overlay_anchor.clone(),
                    self.ui.anchors.clone(),
                ))
                .child(icon("LayersIcon", p.muted).size(px(12.)))
                .child(label("history.overlay.add"))
                .tooltip(|window, cx| {
                    widgets::tooltip(label("history.overlay.addTitle"), window, cx)
                })
                .on_click(cx.listener({
                    let primary_id = metric.id;
                    let anchor = overlay_anchor.clone();
                    move |this, _, window, cx| {
                        this.watch_open_overlay_menu(
                            lane_id, primary_id, None, &anchor, window, cx,
                        );
                    }
                }))
                .into_any_element()
        };

        let mut header_right = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(overlay_control);
        if multi {
            header_right = header_right.child(
                div()
                    .id(SharedString::from(format!("watch-lane-remove-{lane_id}")))
                    .relative()
                    .size(px(24.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(p.hover))
                    .child(layout::anchor_probe(
                        format!("watch-lane-remove-{lane_id}"),
                        self.ui.anchors.clone(),
                    ))
                    .child(icon("DismissIcon", p.muted).size(px(12.)))
                    .tooltip(|window, cx| {
                        widgets::tooltip(label("history.lane.remove"), window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.watch_command(Command::RemoveLane(lane_id), window, cx);
                    })),
            );
        }

        let header = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .pl(px(GUTTER))
            .pr(px(4.))
            .min_h(px(28.))
            .child(header_left)
            .child(header_right);

        // Build cached series for this lane.
        let last_pt = points.last();
        let series_key = charts::hash_of((
            (lane_id, metric.id, overlay.map(|o| o.id)),
            (
                watch["pid"].as_u64(),
                watch["createTime"].as_i64(),
                points.len(),
                end.to_bits(),
            ),
            (
                left_top.to_bits(),
                right_top.map(f64::to_bits),
                last_pt.and_then(|pt| metric.headline_value(pt).map(f64::to_bits)),
                last_pt.and_then(|pt| overlay.and_then(|o| o.headline_value(pt).map(f64::to_bits))),
            ),
            (p.dark, bits),
        ));
        let cache_key = SharedString::from(format!("watch-lane-series-{lane_id}"));
        let cached =
            window.use_keyed_state(
                cache_key,
                cx,
                |_, _| None::<(u64, Arc<Vec<charts::Series>>)>,
            );
        let series = cached
            .read(cx)
            .as_ref()
            .filter(|(stored, _)| *stored == series_key)
            .map(|(_, s)| s.clone())
            .unwrap_or_else(|| {
                let mut list = Vec::new();
                if let Some((part_a, part_b)) = metric.part_names() {
                    let data_a: Vec<_> = points
                        .iter()
                        .map(|pt| (number(pt, "ts"), metric.read_pair(pt).0))
                        .collect();
                    let data_b: Vec<_> = points
                        .iter()
                        .map(|pt| (number(pt, "ts"), metric.read_pair(pt).1))
                        .collect();
                    list.push(
                        charts::Series::new(
                            part_a,
                            data_a,
                            primary_color,
                            metric.chart_format(bits),
                        )
                        .fill(0.15)
                        .source(charts::hash_of((series_key, "a"))),
                    );
                    list.push(
                        charts::Series::new(
                            part_b,
                            data_b,
                            primary_color.opacity(0.7),
                            metric.chart_format(bits),
                        )
                        .dashed(true)
                        .fill(0.)
                        .source(charts::hash_of((series_key, "b"))),
                    );
                } else {
                    let data: Vec<_> = points
                        .iter()
                        .map(|pt| (number(pt, "ts"), metric.headline_value(pt)))
                        .collect();
                    list.push(
                        charts::Series::new(
                            metric.short_name(),
                            data,
                            primary_color,
                            metric.chart_format(bits),
                        )
                        .fill(0.18)
                        .source(charts::hash_of((series_key, "primary"))),
                    );
                }
                if let Some(o) = overlay {
                    let o_color = o.color(p);
                    let data: Vec<_> = points
                        .iter()
                        .map(|pt| {
                            let value = o.headline_value(pt).map(|v| {
                                if let Some(rt) = right_top {
                                    onto(v, rt, left_top)
                                } else {
                                    v
                                }
                            });
                            (number(pt, "ts"), value)
                        })
                        .collect();
                    list.push(
                        charts::Series::new(o.short_name(), data, o_color, o.chart_format(bits))
                            .dashed(true)
                            .fill(0.)
                            .source(charts::hash_of((series_key, "overlay"))),
                    );
                }
                let arc = Arc::new(list);
                cached.update(cx, |slot, _| *slot = Some((series_key, arc.clone())));
                arc
            });

        // Plot box: horizontal grid, vertical clock ticks, hatched gap regions, curves, and crosshair.
        let span = (end - start).max(1.);
        let plot_bounds = self.watch_ui.plot_bounds.clone();
        let mut plot = div()
            .id(SharedString::from(format!("watch-plot-{lane_id}")))
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .cursor_crosshair()
            .child(
                canvas(
                    move |bounds, _, _| {
                        *plot_bounds.borrow_mut() = bounds;
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            );

        for row in 0..=AXIS_ROWS {
            let at = row as f32 / AXIS_ROWS as f32;
            plot = plot.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(at))
                    .when(row == AXIS_ROWS, |d| d.mt(px(-1.)))
                    .h(px(1.))
                    .bg(p.stroke),
            );
        }
        for &tick in ticks {
            let f = ((tick as f64 - start) / span) as f32;
            if f > 0.002 && f < 0.998 {
                plot = plot.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(f))
                        .w(px(1.))
                        .bg(p.stroke.opacity(0.6)),
                );
            }
        }

        // Hatched gap shading where samples are missing / non-recording.
        for window_pair in points.windows(2) {
            let a = &window_pair[0];
            let b = &window_pair[1];
            if text(a, "state") != "recording" {
                let fa = ((number(a, "ts") - start) / span).clamp(0., 1.) as f32;
                let fb = ((number(b, "ts") - start) / span).clamp(0., 1.) as f32;
                if fb > fa {
                    plot = plot.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(relative(fa))
                            .w(relative(fb - fa))
                            .bg(gpui_kit::gpui::pattern_slash(p.faint.opacity(0.22), 1., 6.)),
                    );
                }
            }
        }

        plot = plot.child(charts::TimePlot {
            id: SharedString::from(format!("watch-lane-plot-{lane_id}")),
            series,
            start,
            end,
            max: left_top,
            min: 0.,
            interactive: false,
            grid: false,
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
        });

        // Crosshair line + curve intersection dots when hovered or pinned.
        if let Some(at) = cursor_ts
            && let Some(idx) = nearest_point_index(points, at)
            && let Some(pt) = points.get(idx)
        {
            let pt_ts = number(pt, "ts");
            let frac = ((pt_ts - start) / span).clamp(0., 1.) as f32;
            plot = plot.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(frac))
                    .w(px(1.))
                    .bg(if pinned_only {
                        p.accent.opacity(0.8)
                    } else {
                        p.ink.opacity(0.4)
                    }),
            );
            let mut dots: Vec<(Option<f64>, Hsla, f64)> = Vec::new();
            if metric.is_pair() {
                let (a, b) = metric.read_pair(pt);
                dots.push((a, primary_color, left_top));
                dots.push((b, primary_color.opacity(0.7), left_top));
            } else {
                dots.push((metric.headline_value(pt), primary_color, left_top));
            }
            if let Some(o) = overlay {
                dots.push((
                    o.headline_value(pt),
                    o.color(p),
                    right_top.unwrap_or(left_top),
                ));
            }
            for (value, color, top) in dots {
                let Some(value) = value else { continue };
                let y = (1. - (value / top.max(f64::EPSILON)).clamp(0., 1.)) as f32;
                plot = plot.child(
                    div()
                        .absolute()
                        .left(relative(frac))
                        .ml(px(-4.5))
                        .top(relative(y))
                        .mt(px(-4.5))
                        .size(px(9.))
                        .rounded_full()
                        .border_2()
                        .border_color(p.tip)
                        .bg(color),
                );
            }
        }

        let plot = plot
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let bounds = *this.watch_ui.plot_bounds.borrow();
                let w = f32::from(bounds.size.width);
                if w > 1. {
                    let frac = ((f32::from(event.position.x - bounds.left())) / w).clamp(0., 1.);
                    let at = (start + f64::from(frac) * (end - start)).round() as i64;
                    if this.watch_ui.hover != Some(at) {
                        this.watch_ui.hover = Some(at);
                        cx.notify();
                    }
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.watch_ui.hover.is_some() {
                    this.watch_ui.hover = None;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.watch_ui.active = lane_id;
                    let bounds = *this.watch_ui.plot_bounds.borrow();
                    let w = f32::from(bounds.size.width);
                    if w > 1. {
                        let frac =
                            ((f32::from(event.position.x - bounds.left())) / w).clamp(0., 1.);
                        let at = (start + f64::from(frac) * (end - start)).round() as i64;
                        this.watch_ui.pin = Some(at);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    this.watch_ui.pin = None;
                    cx.notify();
                }),
            );

        // Left and right axis columns.
        let axis_column = |m: &WatchMetric, top: f64, color: Hsla, right: bool| {
            let mut col = div().relative().w(px(GUTTER)).flex_shrink_0().h_full();
            for row in 0..=AXIS_ROWS {
                let frac = row as f32 / AXIS_ROWS as f32;
                let value = top * (1. - f64::from(frac));
                let lift = -8. - 8. * ((frac - 0.94) / 0.06).clamp(0., 1.);
                col = col.child(
                    div()
                        .absolute()
                        .top(relative(frac))
                        .mt(px(lift))
                        .h(px(16.))
                        .when(!right, |d| d.right(px(6.)))
                        .when(right, |d| d.left(px(6.)))
                        .flex()
                        .items_center()
                        .text_size(theme::font(11.))
                        .font_features(theme::tabular())
                        .text_color(color)
                        .whitespace_nowrap()
                        .child(m.axis_label(value, top, bits)),
                );
            }
            col
        };

        let left_axis = axis_column(metric, left_top, p.faint, false);
        let right_axis = if let (Some(o), Some(rt)) = (overlay, right_top) {
            axis_column(o, rt, o.color(p), true)
        } else {
            axis_column(metric, left_top, p.faint, true)
        };

        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .rounded(px(6.))
            .when(active, |d| {
                d.border_1()
                    .border_color(primary_color.opacity(0.35))
                    .p(px(2.))
            })
            .child(header)
            .child(
                div()
                    .h(px(plot_height))
                    .flex()
                    .items_stretch()
                    .child(left_axis)
                    .child(plot)
                    .child(right_axis),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if this.watch_ui.active != lane_id {
                        this.watch_ui.active = lane_id;
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{latest_recording_index, nearest_point_index};
    use serde_json::json;

    #[test]
    fn a_watch_without_points_yet_reads_none() {
        // A watch just started answers before its first point is recorded.
        assert_eq!(latest_recording_index(&[]), None);
        assert_eq!(nearest_point_index(&[], 0), None);
        let points = [
            json!({"ts": 0, "state": "recording"}),
            json!({"ts": 5000, "state": "gap"}),
        ];
        assert_eq!(latest_recording_index(&points), Some(0));
        assert_eq!(latest_recording_index(&points[1..]), Some(0));
    }
}
