//! History: what the machine and its busiest processes did, up to a week back. The
//! charts are drawn once per query; pointing at them only moves the cursor marks.
use super::*;
use charts::{GIB, KIB, MIB};

pub(super) const RANGES: [(&str, i64); 4] = [
    ("1h", 3_600_000),
    ("6h", 21_600_000),
    ("24h", 86_400_000),
    ("7d", 604_800_000),
];
const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
/// Axis tick spacing per range, on local clock boundaries.
const TICKS: [i64; 4] = [10 * MINUTE, HOUR, 3 * HOUR, DAY];
/// One recorded frame (`FRAME_MS` in history.rs).
const FRAME: f64 = argus_core::history::FRAME_MS as f64;
/// Label column, chart gap and time axis (the original's `.history-graphs`).
const LABELS: f32 = 150.;
const COLUMN_GAP: f32 = 14.;
const ROW_GAP: f32 = 8.;
const AXIS: f32 = 22.;
const BORDER: f32 = 1.;
const TABLE_ROW: f32 = 28.;

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
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
fn local_offset() -> time::UtcOffset {
    time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC)
}
fn local(ts: i64) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::from_unix_timestamp(ts.div_euclid(1000))
        .ok()
        .map(|t| t.to_offset(local_offset()))
}

// ---- Gaps (lib/history-gaps.ts) ------------------------------------------------------

/// The widest spacing of frames recorded while running: the slowest background
/// refresh plus how late a sample can land.
fn max_cadence() -> f64 {
    argus_app::settings::BACKGROUND_INTERVALS
        .iter()
        .copied()
        .max()
        .unwrap_or(0) as f64
        + FRAME
}
/// Whether the line joins each point to the next: always within 2.5 steps, and past
/// that within 2.5 of the wider neighbouring spacing (up to the slowest cadence).
fn joins(ts: &[f64], step: f64) -> Vec<bool> {
    (0..ts.len().saturating_sub(1))
        .map(|i| {
            let before = if i > 0 { ts[i] - ts[i - 1] } else { 0. };
            let after = if i + 2 < ts.len() {
                ts[i + 2] - ts[i + 1]
            } else {
                0.
            };
            let cadence = before.max(after).min(max_cadence());
            ts[i + 1] - ts[i] <= step.max(cadence) * 2.5
        })
        .collect()
}
/// Index of the point closest to `at`.
fn nearest(ts: &[f64], at: f64) -> Option<usize> {
    if ts.is_empty() {
        return None;
    }
    let index = ts.partition_point(|t| *t < at).min(ts.len() - 1);
    Some(if index > 0 && at - ts[index - 1] < ts[index] - at {
        index - 1
    } else {
        index
    })
}
/// The point a reading at `at` shows: the nearest one on the line, none in a gap.
fn point_at(ts: &[f64], joined: &[bool], step: f64, at: f64) -> Option<usize> {
    let index = nearest(ts, at)?;
    if (ts[index] - at).abs() <= step * 1.5 {
        return Some(index);
    }
    let pair = if at < ts[index] {
        index.checked_sub(1)?
    } else {
        index
    };
    joined.get(pair).copied().unwrap_or(false).then_some(index)
}

// ---- Window and selection -------------------------------------------------------------
//
// Every range shows its newest window, ending at the present, and follows it whether
// or not a moment is picked; a picked moment lives only as long as the window on
// screen contains it. (The original instead held the window still once a moment was
// picked and centred a new range on it.)

fn span_of(range: &str) -> i64 {
    RANGES
        .iter()
        .find(|(key, _)| *key == range)
        .unwrap_or(&RANGES[0])
        .1
}
/// How often a range moves on to the present: 10 s to 5 min, coarser views less often.
fn refresh_every(span: i64) -> i64 {
    (span / 360).clamp(10_000, 5 * MINUTE)
}
/// A picked moment stays picked only while the window contains it.
fn kept(cursor: Option<i64>, from: i64, to: i64) -> Option<i64> {
    cursor.filter(|at| (from..=to).contains(at))
}
/// What loading the page does at `now`, given the last query sent: the window to query,
/// if the one on screen is not current (another range or resolution, or its refresh is
/// due), and the moment still picked afterwards. A new window is always the newest of
/// its range; picking a moment neither moves it nor holds it still.
fn plan_load(
    query: &Value,
    range: &str,
    buckets: i64,
    cursor: Option<i64>,
    now: i64,
) -> (Option<(i64, i64)>, Option<i64>) {
    let span = span_of(range);
    let current = !query.is_null()
        && query["range"] == json!(range)
        && query["buckets"] == json!(buckets)
        && now - query["at"].as_i64().unwrap_or(0) < refresh_every(span);
    if current {
        return (None, cursor);
    }
    let (from, to) = (now - span, now);
    (Some((from, to)), kept(cursor, from, to))
}
/// Where a key moves the picked moment among the points on screen: arrows step one
/// point (Shift and the Page keys a tenth of them), Home and End go to either end. With
/// nothing picked the steps start from the newest point.
fn stepped(ts: &[f64], cursor: Option<i64>, key: &str, shift: bool) -> Option<i64> {
    let last = ts.len().checked_sub(1)?;
    let far = (ts.len() as f64 / 10.).round().max(1.) as usize;
    let current = cursor.and_then(|at| nearest(ts, at as f64)).unwrap_or(last);
    let index = match key {
        "left" => current.saturating_sub(if shift { far } else { 1 }),
        "right" => (current + if shift { far } else { 1 }).min(last),
        "pageup" => current.saturating_sub(far),
        "pagedown" => (current + far).min(last),
        "home" => 0,
        "end" => last,
        _ => return cursor,
    };
    Some(ts[index] as i64)
}

// ---- Times ----------------------------------------------------------------------------

/// Axis ticks on local clock boundaries: so many minutes past midnight, or midnights.
fn ticks(from: f64, to: f64, step: i64) -> Vec<f64> {
    let offset = i64::from(local_offset().whole_seconds()) * 1000;
    let midnight = (from as i64 + offset).div_euclid(DAY) * DAY - offset;
    (0..)
        .map(|k| (midnight + k * step) as f64)
        .take_while(|at| *at <= to)
        .filter(|at| *at >= from)
        .collect()
}
fn month_day(ts: i64) -> String {
    let Some(t) = local(ts) else {
        return label("format.missing");
    };
    // The month as the locale names it: a number in Chinese, an abbreviation in English.
    let names = label("format.monthNames");
    let month = names
        .split_whitespace()
        .nth(usize::from(u8::from(t.month())) - 1)
        .map(String::from)
        .unwrap_or_else(|| u8::from(t.month()).to_string());
    tr!("format.monthDay", month = month, day = t.day())
}
fn clock(ts: i64, seconds: bool) -> String {
    let Some(t) = local(ts) else {
        return label("format.missing");
    };
    if seconds {
        format!("{:02}:{:02}:{:02}", t.hour(), t.minute(), t.second())
    } else {
        format!("{:02}:{:02}", t.hour(), t.minute())
    }
}
/// A moment, with its date unless it is today.
fn moment_label(ts: i64) -> String {
    let today = local(now_ms()).map(|t| t.date());
    if local(ts).map(|t| t.date()) == today {
        clock(ts, true)
    } else {
        format!("{} {}", month_day(ts), clock(ts, true))
    }
}
/// The cursor's time, as precise as the points under it.
fn cursor_label(ts: i64, span: f64) -> String {
    if span > DAY as f64 {
        format!("{} {}", month_day(ts), clock(ts, false))
    } else if span > 6. * HOUR as f64 {
        clock(ts, false)
    } else {
        clock(ts, true)
    }
}
fn tick_label(ts: i64) -> String {
    match local(ts) {
        Some(t) if t.hour() == 0 && t.minute() == 0 => month_day(ts),
        _ => clock(ts, false),
    }
}
/// Rough width of a short label at 11 px with its padding, for keeping chips inside the plot.
fn chip_width(text: &str) -> f32 {
    12. + text
        .chars()
        .map(|c| if c.is_ascii() { 6.4 } else { 11. })
        .sum::<f32>()
}

// ---- Formats --------------------------------------------------------------------------

/// A byte rate; idle rows read a bare "0".
fn quiet_rate(bytes: f64) -> String {
    if bytes < 1. {
        "0".into()
    } else {
        charts::byte_rate(bytes)
    }
}
fn quiet_net(bytes: f64, bits: bool) -> String {
    if !bits {
        quiet_rate(bytes)
    } else if bytes * 8. < 1. {
        "0".into()
    } else {
        charts::bit_rate(bytes)
    }
}

/// One chart: its record field, resource name and color.
struct Metric {
    field: &'static str,
    resource: &'static str,
    color: Hsla,
}

impl Desktop {
    fn history_span(&self) -> i64 {
        span_of(&self.history_range)
    }
    /// The plot column's width: the page less margins, card padding, the label column and gap.
    fn history_plot_width(&self, window: &Window) -> f32 {
        let navigation = if self.nav_top {
            0.
        } else if self.nav_expanded {
            self.nav_width + 8.
        } else {
            48.
        };
        (f32::from(window.viewport_size().width)
            - navigation
            - 24.
            - 2.
            - 32.
            - LABELS
            - COLUMN_GAP
            - 10.)
            .max(120.)
    }
    /// Queries the newest window of the range and the processes of the shown moment (the
    /// picked one, else the present). The window follows the present, every 10 s to 5 min,
    /// whether or not a moment is picked; a new range starts from its newest window at
    /// once. A picked moment the new window no longer contains is let go.
    pub(in crate::frontend) fn load_history(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = now_ms();
        let span = self.history_span();
        let buckets = ((span as f64 / FRAME).ceil() as i64)
            .min((self.history_plot_width(window) / 2.).floor() as i64)
            .max(30);
        let (query, cursor) = plan_load(
            self.data.get("historyQuery").unwrap_or(&Value::Null),
            &self.history_range,
            buckets,
            self.history_cursor,
            now,
        );
        if cursor != self.history_cursor {
            self.history_cursor = cursor;
            cx.notify();
        }
        if let Some((from, to)) = query {
            self.data.insert(
                "historyQuery".into(),
                json!({"range": self.history_range, "buckets": buckets, "to": to, "at": now}),
            );
            self.request(
                "history_series",
                json!({"from": from, "to": to, "buckets": buckets, "range": self.history_range}),
                Some("historySeries"),
                window,
                cx,
            );
            self.request("history_range", json!({}), Some("historyRange"), window, cx);
        }
        // A kept moment keeps its processes: they are read again only for another moment.
        let moment = self.history_cursor.unwrap_or(now);
        if self.data.get("historyMomentAt") != Some(&json!(moment)) {
            self.data.insert("historyMomentAt".into(), json!(moment));
            self.request(
                "history_moment",
                json!({"ts": moment, "cursor": self.history_cursor}),
                Some("historyMoment"),
                window,
                cx,
            );
        }
    }
    /// A series answer is on screen. A moment picked on the previous window while it was
    /// queried (pointer or keys) stays picked only if the new window contains it.
    pub(in crate::frontend) fn history_answered(
        &mut self,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(from), Some(to)) = (args["from"].as_i64(), args["to"].as_i64()) else {
            return;
        };
        let cursor = kept(self.history_cursor, from, to);
        if cursor != self.history_cursor {
            self.history_cursor = cursor;
            self.load_history(window, cx);
        }
    }
    /// Keyboard on the history page: arrows step between points (Shift and Page keys by a
    /// tenth), Home and End jump to either end, Escape returns to the newest data. Steps
    /// stay among the points on screen and never move the window.
    pub(in crate::frontend) fn history_key(
        &mut self,
        key: &str,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if key == "escape" {
            if self.history_cursor.is_none() {
                return false;
            }
            self.history_cursor = None;
            self.load_history(window, cx);
            cx.notify();
            return true;
        }
        if !matches!(
            key,
            "left" | "right" | "home" | "end" | "pageup" | "pagedown"
        ) {
            return false;
        }
        let ts = self
            .data
            .get("historySeries")
            .map(values)
            .unwrap_or_default()
            .iter()
            .filter_map(|p| p["ts"].as_f64())
            .collect::<Vec<_>>();
        let Some(at) = stepped(&ts, self.history_cursor, key, shift) else {
            return true;
        };
        self.history_cursor = Some(at);
        self.load_history(window, cx);
        cx.notify();
        true
    }
    pub(in crate::frontend) fn history_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let records = self
            .data
            .get("historySeries")
            .map(values)
            .unwrap_or_default()
            .to_vec();
        let span_ms = self.history_span();
        let (from, to) = self
            .data
            .get("historyWindow")
            .and_then(|w| Some((w["from"].as_f64()?, w["to"].as_f64()?)))
            .unwrap_or_else(|| {
                let now = now_ms() as f64;
                (now - span_ms as f64, now)
            });
        let span = (to - from).max(1.);
        let buckets = self
            .data
            .get("historyQuery")
            .and_then(|query| query["buckets"].as_f64())
            .unwrap_or(records.len().max(1) as f64);
        let step = (span / buckets.max(1.)).max(FRAME);
        let ts = records.iter().map(|r| number(r, "ts")).collect::<Vec<_>>();
        // The query answer on screen: its window and points.
        let answer = (from.to_bits(), to.to_bits(), records.len());
        let joined = joins(&ts, step);
        let tick_step = TICKS[RANGES
            .iter()
            .position(|(key, _)| *key == self.history_range)
            .unwrap_or(0)];
        let tick_list = Arc::new(ticks(from, to, tick_step));
        let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
        let has_battery = records.iter().any(|r| r["battery"].is_number());
        let metrics = [
            ("cpu", "cpu", p.cpu),
            ("memUsed", "memory", p.memory),
            ("disk", "disk", p.io),
            ("net", "network", p.network),
            ("gpu", "gpu", p.gpu),
            ("battery", "battery", p.battery),
        ]
        .into_iter()
        .filter(|(field, _, _)| *field != "battery" || has_battery)
        .map(|(field, resource, color)| Metric {
            field,
            resource,
            color,
        })
        .collect::<Vec<_>>();
        let shown_at =
            |at: f64| point_at(&ts, &joined, step, at).and_then(|index| records.get(index));
        let read_ts = self.history_hover.or(self.history_cursor);
        let readout = match read_ts {
            Some(at) => shown_at(at as f64),
            None => records.last(),
        };
        let cursor_point = read_ts.and_then(|at| shown_at(at as f64));
        let row = px((f32::from(window.viewport_size().height) * 0.075).clamp(36., 96.));
        let compact = f32::from(window.viewport_size().height) <= 720.;

        // Labels: color square and name, the value at the readout, a detail line.
        let mut labels = div()
            .w(px(LABELS))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(ROW_GAP));
        let mut charts_column = Vec::new();
        for metric in &metrics {
            let value_of = |r: &Value| r[metric.field].as_f64();
            let reading = readout.and_then(|r| value_of(r).map(|v| (r, v)));
            let value_text = reading.map(|(_, v)| match metric.field {
                "cpu" | "gpu" => format!("{}%", theme::cpu(v)),
                "memUsed" => format!("{:.1} GB", v / GIB),
                "disk" => charts::byte_rate(v),
                "net" => charts::net_rate(v, bits),
                _ => format!("{}%", v.round()),
            });
            let detail = reading.and_then(|(r, _)| match metric.field {
                "cpu" => r["cpuPeak"]
                    .as_f64()
                    .map(|peak| tr!("history.metric.peak", value = theme::cpu(peak))),
                "memUsed" => r["memTotal"].as_f64().map(|total| {
                    tr!(
                        "history.metric.total",
                        value = format!("{:.1}", total / GIB)
                    )
                }),
                "battery" => r["batteryRate"]
                    .as_f64()
                    .filter(|rate| *rate != 0.)
                    .map(|rate| {
                        let value = format!("{} W", charts::watts_tenths(rate.abs() / 1000.));
                        if rate > 0. {
                            tr!("history.metric.charging", value = value)
                        } else {
                            tr!("history.metric.discharging", value = value)
                        }
                    }),
                _ => None,
            });
            labels = labels.child(
                div()
                    .h(row)
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(theme::font(12.))
                            .text_color(p.muted)
                            .child(div().size(px(8.)).rounded(px(2.)).bg(metric.color))
                            .child(label(&format!("common.resource.{}", metric.resource))),
                    )
                    .child(match value_text {
                        Some(text) => div()
                            .truncate()
                            .text_size(theme::font(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(text),
                        None => div().text_size(theme::font(15.)).text_color(p.faint).child(
                            if read_ts.is_none() && readout.is_none() {
                                label("format.missing")
                            } else {
                                label("history.noData")
                            },
                        ),
                    })
                    .when_some(detail.filter(|_| !compact), |d, detail| {
                        d.child(
                            div()
                                .text_size(theme::font(12.))
                                .text_color(p.faint)
                                .child(detail),
                        )
                    }),
            );

            // The chart: drawn once per query, the readout's dot above it.
            let mut points = Vec::with_capacity(records.len() + 8);
            let mut peaks = Vec::new();
            for (i, record) in records.iter().enumerate() {
                points.push((ts[i], value_of(record)));
                if metric.field == "cpu" {
                    peaks.push((ts[i], record["cpuPeak"].as_f64()));
                }
                if joined.get(i) == Some(&false) {
                    let gap = (ts[i] + ts[i + 1]) / 2.;
                    points.push((gap, None));
                    if metric.field == "cpu" {
                        peaks.push((gap, None));
                    }
                }
            }
            let highest = |field: &str| {
                records
                    .iter()
                    .filter_map(|r| r[field].as_f64())
                    .fold(0., f64::max)
            };
            let (max, scale) = match metric.field {
                "memUsed" => (highest("memTotal").max(1.), None),
                "disk" => {
                    let top = charts::rate_ceiling(highest("disk").max(MIB), false);
                    (top, Some(charts::rate_ceiling_label(top, false)))
                }
                "net" => {
                    let floor = if bits { 125_000. } else { 100. * KIB };
                    let top = charts::rate_ceiling(highest("net").max(floor), bits);
                    (top, Some(charts::rate_ceiling_label(top, bits)))
                }
                _ => (100., None),
            };
            let format = match metric.field {
                "memUsed" => charts::MetricFormat::Gigabytes,
                "disk" => charts::MetricFormat::ByteRate,
                "net" => charts::MetricFormat::Network(bits),
                _ => charts::MetricFormat::Percent,
            };
            // Each answer is other data: a curve drawn for another window (the
            // previous range, or the one before a refresh) is never stretched
            // onto this one while its own is drawn. The charts wait for their
            // curves together, as on a first display, and change in one frame.
            let source = |part: &str| charts::hash_of((answer, metric.field, part));
            let mut series = Vec::with_capacity(2);
            if metric.field == "cpu" && peaks.iter().any(|(_, v)| v.is_some()) {
                series.push(
                    charts::Series::new(String::new(), peaks, metric.color.opacity(0.45), format)
                        .fill(0.)
                        .source(source("peak")),
                );
            }
            series.push(
                charts::Series::new(String::new(), points, metric.color, format)
                    .fill(0.28)
                    .source(source("value")),
            );
            let dot = cursor_point.and_then(|r| {
                let value = value_of(r)?;
                let x = ((number(r, "ts") - from) / span) as f32;
                let y = 1. - (value / max).clamp(0., 1.) as f32;
                Some((x, y))
            });
            let inner = row - px(2. * BORDER);
            charts_column.push(
                div()
                    .relative()
                    .h(row)
                    .flex_shrink_0()
                    .border_1()
                    .border_color(metric.color.opacity(0.45))
                    .rounded(px(4.))
                    .child(charts::TimePlot {
                        id: format!("history-{}", metric.field).into(),
                        series,
                        start: from,
                        end: to,
                        max,
                        min: 0.,
                        interactive: false,
                        grid: true,
                        fill: true,
                        bars: false,
                        background_separate: false,
                        smooth: false,
                        ticks: Some(tick_list.clone()),
                        geometry_start: from,
                        measured_width: Rc::new(Cell::new(1.)),
                        live_clock: None,
                        image_surface: Default::default(),
                        bar_surfaces: Default::default(),
                        bar_glides: Default::default(),
                        dash_phases: Default::default(),
                        rescale: Default::default(),
                        readout: Default::default(),
                        captions: None,
                    })
                    .when_some(scale, |d, scale| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(3.))
                                .right(px(6.))
                                .text_size(theme::font(11.))
                                .text_color(p.faint)
                                .child(scale),
                        )
                    })
                    .when_some(dot, |d, (x, y)| {
                        d.child(
                            div()
                                .absolute()
                                .left(relative(x))
                                .top(inner * y)
                                .ml(px(-4.5))
                                .mt(px(-4.5))
                                .size(px(9.))
                                .rounded_full()
                                .border_2()
                                .border_color(p.tip)
                                .bg(metric.color),
                        )
                    })
                    .into_any_element(),
            );
        }
        labels = labels.child(div().h(px(AXIS)).flex_shrink_0());

        // Time axis, the empty state and the pointer overlay over the plot column.
        let axis = div()
            .relative()
            .h(px(AXIS))
            .flex_shrink_0()
            .text_size(theme::font(11.))
            .text_color(p.faint)
            .children(tick_list.iter().filter_map(|tick| {
                let f = ((tick - from) / span) as f32;
                (f > 0.03 && f < 0.97).then(|| {
                    let text = tick_label(*tick as i64);
                    let width = chip_width(&text) - 12.;
                    div()
                        .absolute()
                        .top(px(3.))
                        .left(relative(f))
                        .ml(px(-width / 2.))
                        .whitespace_nowrap()
                        .child(text)
                })
            }));
        let bounds_state = window.use_keyed_state("history-plot-bounds", cx, |_, _| {
            Bounds::<Pixels>::default()
        });
        let plot_bounds = *bounds_state.read(cx);
        let plot_width = f32::from(plot_bounds.size.width).max(1.);
        // Pointer position to time, snapped to the point drawn there.
        let time_at = {
            let ts = ts.clone();
            let joined = joined.clone();
            move |position: Point<Pixels>| {
                let f = (f32::from(position.x - plot_bounds.left()) - BORDER)
                    / (f32::from(plot_bounds.size.width) - 2. * BORDER).max(1.);
                if !(0. ..=1.).contains(&f) {
                    return None;
                }
                let at = from + f as f64 * span;
                Some(
                    point_at(&ts, &joined, step, at)
                        .map(|index| ts[index] as i64)
                        .unwrap_or(at.round() as i64),
                )
            }
        };
        let mark = |at: Option<i64>| {
            let at = at?;
            let f = (at as f64 - from) / span;
            (0. ..=1.)
                .contains(&f)
                .then_some((at, BORDER + f as f32 * (plot_width - 2. * BORDER)))
        };
        let on_accent = cx.theme().primary_foreground;
        let chip = |at: i64, x: f32, pinned: bool| {
            let text = cursor_label(at, span);
            let width = chip_width(&text);
            div()
                .absolute()
                .bottom_0()
                .left(px(x - width * (x / plot_width)))
                .px(px(6.))
                .border_1()
                .border_color(if pinned {
                    gpui_kit::gpui::transparent_black()
                } else {
                    p.stroke_strong
                })
                .rounded(px(4.))
                .bg(if pinned { p.accent } else { p.tip })
                .text_color(if pinned { on_accent } else { p.ink })
                .text_size(theme::font(11.))
                .line_height(px(18.))
                .whitespace_nowrap()
                .child(text)
                .into_any_element()
        };
        let line = |x: f32, color: Hsla| {
            div()
                .absolute()
                .top_0()
                .bottom(px(AXIS))
                .left(px(x.round()))
                .w(px(1.))
                .bg(color)
                .into_any_element()
        };
        let mut marks = Vec::new();
        // While hovering, the picked moment keeps its line but gives up its label.
        if let Some((at, x)) = mark(self.history_cursor) {
            marks.push(line(x, p.accent));
            if self.history_hover.is_none() {
                marks.push(chip(at, x, true));
            }
        }
        if let Some((at, x)) = mark(self.history_hover) {
            marks.push(line(x, p.ink.opacity(0.35)));
            marks.push(chip(at, x, false));
        }
        let recording_off = self
            .data
            .get("settings")
            .and_then(|s| s["history"].as_bool())
            == Some(false);
        let empty = self.data.contains_key("historySeries") && records.is_empty();
        let move_time = time_at.clone();
        let click_time = time_at;
        let overlay = div()
            .id("history-overlay")
            .absolute()
            .inset_0()
            .cursor_crosshair()
            .child(
                canvas(
                    move |bounds, _, cx| {
                        bounds_state.update(cx, |state, _| *state = bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .children(marks)
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let hovered = move_time(event.position);
                if this.history_hover != hovered {
                    this.history_hover = hovered;
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.history_hover.is_some() {
                    this.history_hover = None;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if let Some(at) = click_time(event.position) {
                        this.history_cursor = Some(at);
                        this.load_history(window, cx);
                        cx.notify();
                    }
                }),
            );
        let plot =
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(ROW_GAP))
                .children(charts_column)
                .child(axis)
                .when(empty, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom(px(AXIS))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_1()
                            .text_color(p.muted)
                            .child(label(if recording_off {
                                "history.empty.off"
                            } else {
                                "history.empty.none"
                            }))
                            .child(div().text_size(theme::font(12.)).text_color(p.faint).child(
                                label(if recording_off {
                                    "history.empty.offHint"
                                } else {
                                    "history.empty.hint"
                                }),
                            )),
                    )
                })
                .child(overlay);
        let graphs = div()
            .flex_none()
            .flex()
            .gap(px(COLUMN_GAP))
            .pt(px(14.))
            .px(px(16.))
            .pb(px(6.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.layer)
            .child(labels)
            .child(plot);

        // The processes of the shown moment.
        let moment = self
            .data
            .get("historyMoment")
            .cloned()
            .unwrap_or(Value::Null);
        let title = match moment["system"]["ts"].as_i64() {
            Some(time) => tr!("history.busiestAt", time = moment_label(time)),
            None => label("history.busiest"),
        };
        let earliest = self
            .data
            .get("historyRange")
            .and_then(|range| range[0].as_i64());
        let table = self.history_table(values(&moment["processes"]), bits, window, cx);
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
            .when_some(earliest, |d, ts| {
                d.child(
                    div().flex().gap_1().child(label("history.earliest")).child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.ink)
                            .child(moment_label(ts)),
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
                                this.act(
                                    Action::Preference("settingsSection".into(), json!("data")),
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(after.to_owned())
                    .into_any_element()
            } else {
                div().child(label("history.note")).into_any_element()
            });
        let moment_card = div()
            .flex_1()
            .min_h(px(36. + 30. + TABLE_ROW * 3. + 34.))
            .flex()
            .flex_col()
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.layer)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_baseline()
                    .gap_x(px(14.))
                    .gap_y_1()
                    .pt(px(10.))
                    .px(px(14.))
                    .pb(px(4.))
                    .child(
                        div()
                            .text_size(theme::font(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(theme::font(12.))
                            .text_color(p.faint)
                            .child(label(if self.history_cursor.is_none() {
                                "history.hint.pick"
                            } else {
                                "history.hint.move"
                            })),
                    ),
            )
            .child(table)
            .child(foot);
        let page = window
            .use_keyed_state("history-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        div()
            .id("history-content")
            .track_scroll(&page)
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            // The gutter is kept (`scrollbar-gutter: stable`).
            .pl(px(12.))
            .pr(px(12. + scrollbar::SIZE))
            .flex()
            .flex_col()
            .gap(px(10.))
            .line_height(relative(1.4))
            .child(graphs)
            .child(moment_card)
            .child(scrollbar::scrollbar("history-scrollbar", &page, p.dark))
            .into_any_element()
    }
    /// The moment's processes: sortable, with a menu and double-click to the live process.
    fn history_table(
        &self,
        processes: &[Value],
        bits: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        // The order survives restarts (`tableSort.history`, as the original's DataTable).
        let sort = window.use_keyed_state("history-sort", cx, |_, _| {
            tables::saved_sort("history", &self.prefs)
        });
        let selected = window.use_keyed_state("history-selected", cx, |_, _| None::<String>);
        let stored = sort.read(cx).clone();
        let selected_key = selected.read(cx).clone();
        // Per-process network needs administrator rights; without it the column is all zeros.
        let has_net = processes.iter().any(|row| number(row, "net") > 0.);
        let mut columns: Vec<(&str, String, f32, Option<&str>)> = vec![
            ("name", label("common.name"), 0., None),
            ("pid", label("processes.column.pid.label"), 72., None),
            (
                "cpu",
                label("common.resource.cpu"),
                70.,
                Some("history.column.cpuTitle"),
            ),
            (
                "mem",
                label("common.resource.memory"),
                96.,
                Some("history.column.memTitle"),
            ),
            ("io", label("processes.column.io.label"), 92., None),
        ];
        if has_net {
            columns.push(("net", label("common.resource.network"), 92., None));
        }
        columns.push(("gpu", label("common.resource.gpu"), 70., None));
        // A saved order by a column not shown (network without its readings)
        // gives way to the default.
        let (sort_key, descending) = if columns.iter().any(|column| column.0 == stored.0) {
            stored
        } else {
            tables::initial_sort("history")
        };
        let mut rows = processes.to_vec();
        rows.sort_by(|a, b| {
            let order = if sort_key == "name" {
                collation::compare(text(a, "name"), text(b, "name"))
            } else {
                number(a, &sort_key).total_cmp(&number(b, &sort_key))
            };
            if descending { order.reverse() } else { order }
        });
        let cell = |width: f32, end: bool| {
            div()
                .px(px(8.))
                .min_w_0()
                .truncate()
                .when(width > 0., |d| d.w(px(width)).flex_shrink_0())
                .when(width <= 0., |d| d.flex_1().min_w(px(200.)))
                .when(end, |d| d.text_right())
        };
        let header = div()
            .flex()
            .flex_shrink_0()
            .h(px(30.))
            .items_center()
            .pl(px(6.))
            // Over the rows' scrollbar gutter, like `.thead`.
            .pr(px(6. + scrollbar::SIZE))
            .border_b_1()
            .border_color(p.stroke)
            .text_size(theme::font(12.))
            .text_color(p.muted)
            .children(columns.iter().map(|(id, name, width, title)| {
                let id = (*id).to_owned();
                let active = id == sort_key;
                let arrow = if !active {
                    ""
                } else if descending {
                    " ↓"
                } else {
                    " ↑"
                };
                let sort = sort.clone();
                let title = title.map(label);
                let next = if active {
                    (id.clone(), !descending)
                } else {
                    (id.clone(), id != "name")
                };
                cell(*width, id != "name")
                    .id(SharedString::from(format!("history-head-{id}")))
                    .cursor_pointer()
                    .when(active, |d| d.text_color(p.ink))
                    .child(format!("{name}{arrow}"))
                    .when_some(title, |d, title| {
                        d.tooltip(move |window, cx| widgets::tooltip(title.clone(), window, cx))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let next = next.clone();
                        if let Some((key, value)) =
                            tables::sort_preference("history", &next.0, next.1)
                        {
                            this.prefs.set(&key, value);
                        }
                        sort.update(cx, |state, _| *state = next);
                        cx.notify();
                    }))
            }));
        let live = values(&self.snapshot["processes"]);
        // A recorded process shows the icon of a running one with the same name.
        let mut by_name: HashMap<String, &Value> = HashMap::new();
        for process in live {
            by_name
                .entry(text(process, "name").to_lowercase())
                .or_insert(process);
        }
        let rows_scroll = window
            .use_keyed_state("history-processes-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        // The name under the pointer reports where it lies, for its hint (`tips`).
        let near = self
            .tips
            .begin(window)
            .then(|| tips::row_under(&rows_scroll, TABLE_ROW, rows.len(), window))
            .flatten();
        let moved = self.tips.clone();
        let mut body = div()
            .id("history-processes")
            .track_scroll(&rows_scroll)
            .on_mouse_move(move |event, window, cx| moved.moved(event, window, cx))
            .on_hover(cx.listener(|this, hovered, window, cx| {
                if !hovered {
                    this.tips.dismiss(window, cx);
                }
            }))
            .flex_1()
            .min_h(px(TABLE_ROW * 3.))
            .overflow_y_scroll()
            .pr(px(scrollbar::SIZE))
            .flex()
            .flex_col();
        if rows.is_empty() {
            body = body.child(
                div()
                    .p_4()
                    .text_color(p.faint)
                    .child(label("history.noRecordAt")),
            );
        }
        for (index, row) in rows.iter().enumerate() {
            let name = text(row, "name").to_owned();
            let pid = row["pid"].clone();
            let key = format!("{name}:{pid}");
            let current = selected_key.as_deref() == Some(key.as_str());
            // A reused PID is not the recorded process: the name must match too.
            let running = live
                .iter()
                .find(|process| process["pid"] == pid)
                .is_some_and(|process| text(process, "name") == name);
            let reveal = Action::Preference("revealProcess".into(), json!({"pid": pid}));
            let exited = tr!("history.exited", name = name, pid = pid);
            let menu = vec![
                MenuItem::new(label("common.goToProcess"), reveal.clone()).disabled(!running),
                MenuItem::new(
                    label("common.copyName"),
                    Action::Copy(name.clone(), "history.copied.name"),
                ),
            ];
            let mut line = div()
                .id(("history-row", index))
                .flex()
                .flex_shrink_0()
                .h(px(TABLE_ROW))
                .items_center()
                .px(px(6.))
                .when(current, |d| d.bg(p.control))
                .when(!current, |d| d.hover(move |s| s.bg(p.hover)))
                .child(
                    cell(0., false)
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(self.process_icon(
                            by_name.get(&name.to_lowercase()).copied(),
                            16.,
                            cx,
                        ))
                        .child({
                            let label = div().min_w_0().truncate().child(name.clone());
                            // The description of a running program of that name.
                            let hint = (near == Some(index))
                                .then(|| by_name.get(&name.to_lowercase()))
                                .flatten()
                                .map(|process| text(process, "description"))
                                .filter(|description| !description.is_empty());
                            match hint {
                                Some(hint) => self.tips.target(
                                    label,
                                    tips::id(("history", &key, "name")),
                                    hint.to_owned(),
                                ),
                                None => label,
                            }
                        }),
                )
                .child(
                    cell(72., true)
                        .text_color(p.muted)
                        .child(row["pid"].to_string()),
                )
                .child(cell(70., true).child(theme::cpu(number(row, "cpu"))))
                .child(cell(96., true).child(theme::memory(number(row, "mem"))))
                .child(cell(92., true).child(quiet_rate(number(row, "io"))));
            if has_net {
                line = line.child(cell(92., true).child(quiet_net(number(row, "net"), bits)));
            }
            line = line.child(cell(70., true).child(theme::cpu(number(row, "gpu"))));
            let select = selected.clone();
            let select_menu = selected.clone();
            let row_key = key.clone();
            let menu_key = key;
            body = body.child(
                line.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    select.update(cx, |state, _| *state = Some(row_key.clone()));
                    if event.click_count() >= 2 {
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
                        select_menu.update(cx, |state, _| *state = Some(menu_key.clone()));
                        this.act(Action::Popup(menu.clone()), window, cx);
                    }),
                ),
            );
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(header)
            .child(body.child(scrollbar::scrollbar(
                "history-processes-scrollbar",
                &rows_scroll,
                p.dark,
            )))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DAY, HOUR, MINUTE, joins, kept, plan_load, point_at, refresh_every, span_of, stepped,
    };
    use serde_json::{Value, json};

    const NOW: i64 = 1_800_000_000_000;

    /// The last query sent, as `load_history` stores it.
    fn sent(range: &str, at: i64) -> Value {
        json!({"range": range, "buckets": 600, "to": at, "at": at})
    }

    #[test]
    fn a_new_range_shows_its_newest_window() {
        // Seven days on screen, a moment three days back picked, then one hour: the
        // newest hour, and the moment is let go.
        let week = sent("7d", NOW - 2_000);
        assert_eq!(
            plan_load(&week, "1h", 600, Some(NOW - 3 * DAY), NOW),
            (Some((NOW - HOUR, NOW)), None)
        );
        // A moment within the newest hour stays picked.
        let recent = Some(NOW - 30 * MINUTE);
        assert_eq!(
            plan_load(&week, "1h", 600, recent, NOW),
            (Some((NOW - HOUR, NOW)), recent)
        );
        // Every range ends at the present, widening as well.
        for (range, span) in [("6h", 6 * HOUR), ("24h", DAY), ("7d", 7 * DAY)] {
            assert_eq!(
                plan_load(&sent("1h", NOW - 2_000), range, 600, recent, NOW),
                (Some((NOW - span, NOW)), recent)
            );
        }
        // The first load, with nothing sent yet.
        assert_eq!(
            plan_load(&Value::Null, "24h", 600, None, NOW),
            (Some((NOW - DAY, NOW)), None)
        );
    }

    #[test]
    fn picking_neither_moves_nor_holds_the_window() {
        let hour = sent("1h", NOW);
        let picked = Some(NOW - 20 * MINUTE);
        // Picking, stepping or choosing the range on screen again queries nothing.
        assert_eq!(
            plan_load(&hour, "1h", 600, picked, NOW + 1_000),
            (None, picked)
        );
        assert_eq!(plan_load(&hour, "1h", 600, None, NOW + 1_000), (None, None));
        // The window still moves on to the present when its refresh is due ...
        let due = NOW + refresh_every(HOUR);
        assert_eq!(
            plan_load(&hour, "1h", 600, picked, due),
            (Some((due - HOUR, due)), picked)
        );
        // ... and lets go of the moment once it has moved past it.
        let past = NOW + 45 * MINUTE;
        assert_eq!(
            plan_load(&hour, "1h", 600, picked, past),
            (Some((past - HOUR, past)), None)
        );
        // Another resolution (the window was resized) is the newest window again.
        assert_eq!(
            plan_load(&hour, "1h", 500, picked, NOW + 1_000),
            (Some((NOW + 1_000 - HOUR, NOW + 1_000)), picked)
        );
    }

    #[test]
    fn coarser_ranges_refresh_less_often() {
        let every = ["1h", "6h", "24h", "7d"].map(|range| refresh_every(span_of(range)));
        assert_eq!(every, [10_000, MINUTE, 4 * MINUTE, 5 * MINUTE]);
        // An unknown range reads as the first.
        assert_eq!(span_of("2h"), HOUR);
    }

    #[test]
    fn a_moment_stays_picked_only_inside_the_window() {
        // Used again when an answer arrives, for a moment picked on the previous
        // window while it was queried.
        assert_eq!(kept(Some(50), 0, 100), Some(50));
        assert_eq!(kept(Some(0), 0, 100), Some(0));
        assert_eq!(kept(Some(100), 0, 100), Some(100));
        assert_eq!(kept(Some(-1), 0, 100), None);
        assert_eq!(kept(Some(101), 0, 100), None);
        assert_eq!(kept(None, 0, 100), None);
    }

    #[test]
    fn keys_step_among_the_points_on_screen() {
        let ts = (0..20).map(|i| f64::from(i) * 1_000.).collect::<Vec<_>>();
        // With nothing picked the steps start from the newest point.
        assert_eq!(stepped(&ts, None, "left", false), Some(18_000));
        assert_eq!(stepped(&ts, None, "right", false), Some(19_000));
        assert_eq!(stepped(&ts, Some(5_000), "right", false), Some(6_000));
        // Shift and the Page keys move a tenth of the points.
        assert_eq!(stepped(&ts, Some(5_000), "left", true), Some(3_000));
        assert_eq!(stepped(&ts, Some(5_000), "pagedown", false), Some(7_000));
        assert_eq!(stepped(&ts, Some(1_000), "pageup", false), Some(0));
        assert_eq!(stepped(&ts, Some(19_000), "right", true), Some(19_000));
        assert_eq!(stepped(&ts, Some(7_000), "home", false), Some(0));
        assert_eq!(stepped(&ts, Some(7_000), "end", false), Some(19_000));
        // A moment picked at another resolution steps from the point nearest to it.
        assert_eq!(stepped(&ts, Some(7_400), "right", false), Some(8_000));
        assert_eq!(stepped(&[], Some(7_000), "left", false), None);
    }

    #[test]
    fn gaps_follow_the_recording_cadence() {
        // Five-second frames, then a stop, then frames again.
        let ts = [0., 5_000., 10_000., 200_000., 205_000.];
        let joined = joins(&ts, 5_000.);
        assert_eq!(joined, vec![true, true, false, true]);
        assert_eq!(point_at(&ts, &joined, 5_000., 6_000.), Some(1));
        assert_eq!(point_at(&ts, &joined, 5_000., 100_000.), None);
        // A slower background cadence stays joined.
        let slow = [0., 30_000., 60_000., 600_000., 630_000.];
        assert_eq!(joins(&slow, 7_200.), vec![true, true, false, true]);
    }
}
