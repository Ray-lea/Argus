//! History: what the machine and its busiest processes did. Lanes of readings
//! over a window that follows the newest readings or holds still, a navigator
//! over the whole range, and the processes of a moment or of a range.
//!
//! Points come in buckets aligned to absolute time (`series`): following the
//! clock, only the newest buckets are asked again, once a second up to six
//! hours on screen and once a minute beyond.
use super::*;

mod bar;
mod lanes;
mod navigator;
mod picker;
mod readings;
mod series;
mod settings;
mod state;
mod table;

use crate::frontend::motion;
use readings::Reading;
use series::{FRAME, HOUR, MINUTE, Points, SECOND};
pub(in crate::frontend) use state::HistoryState;
use state::{Range, TableMode};

/// Commands from the page's menus and buttons.
#[derive(Clone, Debug)]
pub(in crate::frontend) enum Command {
    /// Shows a reading in the active lane.
    Show(String),
    /// A new lane, last, showing a reading.
    AddLane(String),
    /// Another reading for a lane.
    Reading(u64, String),
    /// A second reading on a lane's right axis, or none.
    Overlay(u64, Option<String>),
    RemoveLane(u64),
    /// A range by its preference id (`5m` … `30d`, `all`).
    Range(&'static str),
    Latest,
    Pause,
    ClearCursor,
    ZoomSelection,
    /// Shows or hides the band from lowest to highest.
    Band,
    /// The table lists a moment (false) or a range (true).
    Mode(bool),
    /// Shows or hides a table column.
    Column(&'static str),
    /// Shows or hides the table's trend column.
    Trend,
    /// The table's columns as they first were.
    ResetColumns,
    /// Lays a process's own curve over the lanes, or takes it off.
    Curve(Option<String>),
    /// Opens the history settings.
    Settings,
    /// Opens the range picker.
    Picker,
}

/// Old frames have no creation time: a matching PID and name cannot establish
/// identity, even if the live process is older than the frame. A process may
/// start inside the five-second frame, but never after it.
pub(super) fn recorded_target(row: &Value, frame: Option<i64>, live: &[Value]) -> Option<Value> {
    let created = row["createTime"].as_i64().filter(|time| *time > 0)?;
    let end = frame?.checked_add(argus_core::history::FRAME_MS)?;
    if created >= end {
        return None;
    }
    live.iter()
        .find(|process| {
            process["pid"] == row["pid"]
                && process["createTime"].as_i64() == Some(created)
                && text(process, "name") == text(row, "name")
        })
        .map(|process| {
            json!({"pid": process["pid"], "createTime": created,
            "name": process["name"], "historyFrame": frame})
        })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
/// The time it is, to a fraction of a millisecond.
fn precise_now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.
}
fn local(ts: i64) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::from_unix_timestamp(ts.div_euclid(1000))
        .ok()
        .map(|t| t.to_offset(time::UtcOffset::local_offset_at(t).unwrap_or(time::UtcOffset::UTC)))
}
/// The local midnight starting the day of `ts`: buckets are counted from it, so
/// that six-hour buckets start at 0, 6, 12 and 18 o'clock.
fn local_midnight(ts: i64) -> i64 {
    local(ts)
        .map(|t| t.replace_time(time::Time::MIDNIGHT))
        .map(|t| t.unix_timestamp() * 1000)
        .unwrap_or(ts - ts.rem_euclid(series::DAY))
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
fn same_day(a: i64, b: i64) -> bool {
    local(a).map(|t| t.date()) == local(b).map(|t| t.date())
}
/// A time with its UTC offset, for a range across a change of the local
/// offset (daylight saving time), where an hour can repeat.
fn with_offset(text: String, ts: i64, needed: bool) -> String {
    match local(ts) {
        Some(time) if needed => {
            format!("{text} ({})", crate::regional::offset_label(time.offset()))
        }
        _ => text,
    }
}
/// A moment with its date: records span days, so even today's carry it.
fn moment_label(ts: i64, seconds: bool) -> String {
    format!("{} {}", month_day(ts), clock(ts, seconds))
}
/// A span of time between two moments, the date said once when they share it.
fn range_label(from: i64, to: i64, seconds: bool) -> String {
    if same_day(from, to) {
        format!("{}–{}", moment_label(from, seconds), clock(to, seconds))
    } else {
        format!(
            "{} – {}",
            moment_label(from, seconds),
            moment_label(to, seconds)
        )
    }
}
/// A clock tick: the date at midnight or for whole days, else the time.
fn tick_label(ts: i64, step: i64) -> String {
    match local(ts) {
        Some(t) if step >= series::DAY || (t.hour() == 0 && t.minute() == 0 && t.second() == 0) => {
            month_day(ts)
        }
        _ => clock(ts, step < MINUTE),
    }
}
/// A navigator end: the date for long ranges, the time today, both otherwise.
fn edge_label(ts: i64, span: i64, now: i64) -> String {
    if span > 2 * series::DAY {
        month_day(ts)
    } else if same_day(ts, now) {
        clock(ts, span <= 15 * MINUTE)
    } else {
        format!("{} {}", month_day(ts), clock(ts, false))
    }
}
/// A duration in words: "2 小时 5 分钟".
fn duration_label(ms: i64) -> String {
    let minutes = ms / MINUTE;
    if ms < MINUTE {
        tr!("history.duration.seconds", count = (ms / SECOND).max(1))
    } else if minutes < 60 {
        tr!("history.duration.minutes", count = minutes)
    } else if minutes < 24 * 60 {
        tr!(
            "history.duration.hoursMinutes",
            hours = minutes / 60,
            minutes = minutes % 60
        )
    } else {
        tr!(
            "history.duration.daysHours",
            days = minutes / (24 * 60),
            hours = minutes % (24 * 60) / 60
        )
    }
}

/// Points in a process's curve laid over the lanes.
const CURVE_POINTS: usize = 160;
/// The processes the lanes' tooltip lists; it keeps their rows while fewer
/// are known, so that it keeps its height.
const TIP_PROCESSES: usize = 5;
/// The widest window that glides with the clock: wider ones move less than
/// a pixel a second.
const FLOW_SPAN: i64 = 15 * MINUTE;

/// How often the newest readings are asked for while following: at every
/// sample up to six hours on screen (at most four times a second), every
/// minute beyond. A pace of exactly one sample skipped every sample that came
/// a little early, and the curve then waited two seconds for its readings.
fn refresh_every(span: i64) -> Duration {
    if span > 6 * HOUR {
        Duration::from_secs(60)
    } else {
        Duration::from_millis(250)
    }
}

impl Desktop {
    /// The time it is: wall time, or the sample's in a fixture run.
    fn history_now(&self) -> i64 {
        if self.options.fixture.is_some() {
            self.snapshot["timestamp"].as_i64().unwrap_or_else(now_ms)
        } else {
            now_ms()
        }
    }

    /// What the window follows: the newest reading, while readings come in;
    /// else the present, less a second.
    fn history_clock(&self) -> i64 {
        let now = self.history_now();
        match self.history.points.as_ref().and_then(Points::newest) {
            Some(newest) if newest <= now + SECOND && now - newest < 10 * SECOND => newest,
            _ => now - SECOND,
        }
    }

    /// How far behind the present a gliding window ends: as far as the live
    /// charts' (two sampling intervals and a quarter of a second). A reading
    /// is on screen a sample, a query and a raster after it was taken; a
    /// window a second behind reached the newest one before the next was in,
    /// and the curve's end left the edge until it came.
    fn history_lag(&self) -> i64 {
        let config = motion::config();
        (self.interval as f64 * config["chartLagIntervals"].as_f64().unwrap_or(2.)
            + config["chartLagSlackMs"].as_f64().unwrap_or(250.))
        .round() as i64
    }

    /// Whether the range on screen spans a change of the local offset: times
    /// then carry their offset.
    fn history_offsets_differ(&self) -> bool {
        let (from, to) = self.history.extent;
        local(from).map(|t| t.offset()) != local(to).map(|t| t.offset())
    }

    fn history_motion(&self, cx: &App) -> bool {
        !cx.reduce_motion()
    }

    /// Whether the window glides with the clock: while it follows a short
    /// window (the newest readings move across it a pixel at a time), with
    /// motion on. Longer windows move a second or a minute at a time.
    fn history_flowing(&self, cx: &App) -> bool {
        self.history.follow
            && self.history.span() <= FLOW_SPAN
            && self.history_motion(cx)
            && self.chart_clock.borrow().mode != crate::frontend::motion::Mode::Off
            && self.options.fixture.is_none()
    }

    /// The chart card, drawn on its own: while the window glides, this region
    /// is drawn alone, the page around it replaying, each time its labels and
    /// marks move a step (a quarter of a device pixel, more when the window
    /// moves fast), and in the fluid and balanced chart modes the curves'
    /// textures move with the clock on every frame in between, nothing drawn
    /// (`history_glide`); eased zooms and lanes draw the region every frame.
    pub(in crate::frontend) fn history_chart_region(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let motion = self.history_motion(cx);
        let now = Instant::now();
        let flowing = self.history_flowing(cx);
        // The window ends behind the present while it glides, so that the
        // newest reading is in before the edge reaches it (`history_lag`); the
        // curves keep the texture drawn for the readings' own window and move it.
        // The state keeps whole milliseconds; the window drawn ends at the
        // exact time, as the frames that only move the textures place them.
        let data = self.history_clock();
        let precise = flowing.then(|| precise_now_ms() - self.history_lag() as f64);
        let clock = precise.map_or(data, |end| end.floor() as i64);
        self.history.anchor(clock);
        self.history.flow_origin = flowing.then(|| {
            // The newest reading is usually ahead of the window on screen: the
            // texture keeps the readings' own window, a little ahead of it.
            let shift = clock - data;
            (
                (self.history.view.0 - shift) as f64,
                (self.history.extent.0 - shift) as f64,
            )
        });
        let (mut shown, easing) = self.history.shown(now, motion);
        if let Some(end) = precise.filter(|_| !easing) {
            shown = (end - (shown.1 - shown.0), end);
        }
        let lanes_moving = self.history.step_lanes(now, motion);
        let toast = self
            .history
            .toast
            .as_ref()
            .is_some_and(|(_, until)| *until > now);
        if !toast {
            self.history.toast = None;
        }
        self.history.glides.clear();
        let chart = self.history_chart(shown, window, cx);
        let glides = std::mem::take(&mut self.history.glides);
        self.history.gliding = None;
        if easing || lanes_moving || toast {
            window.request_isolated_animation_frame();
        } else if flowing {
            // Device pixels the window moves in a millisecond.
            let width = self.history_plot_width(window) * window.scale_factor();
            let speed = width.max(1.) / self.history.span() as f32;
            let mode = self.chart_clock.borrow().mode;
            if matches!(mode, motion::Mode::Fluid | motion::Mode::Balanced) {
                // Labels and marks move a quarter of a device pixel at a time
                // (glyphs are drawn at quarter pixels), in larger steps as the
                // window moves faster: about thirty a second, a pixel at most.
                let pixels = (speed * 1000. / 30.).clamp(0.25, 1.);
                let step = Duration::from_secs_f32((pixels / speed / 1000.).clamp(0.001, 1.));
                self.history.gliding = Some(state::Gliding {
                    plots: glides,
                    drawn: now,
                    step,
                });
                self.schedule_history_glide(window, cx);
            } else {
                let pixel = Duration::from_secs_f32((1. / speed / 1000.).clamp(0.016, 1.));
                self.history_flow_later(pixel, window, cx);
            }
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(chart)
            .into_any_element()
    }

    /// Moves the curves with the clock on the next frame while they glide,
    /// and asks for the frame after: as the live charts' tick, while the
    /// window is shown and no dialog covers the page.
    pub(in crate::frontend) fn schedule_history_glide(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = self.page == "history"
            && self.modal.is_none()
            && self.history.gliding.is_some()
            && self.host.get_window("main").is_some_and(|w| {
                w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)
            });
        if self.history.glide_queued || !shown {
            return;
        }
        self.history.glide_queued = true;
        let owner = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.history.glide_queued = false;
                this.history_glide(window, cx);
                this.schedule_history_glide(window, cx);
            });
        });
    }

    /// One frame of the glide (every frame in the fluid mode, about 60 a
    /// second in the balanced one): the curves' textures move to the clock,
    /// nothing drawn. The card is drawn again once the window has moved a
    /// step since it was drawn (its labels, gaps and marks move then), or
    /// when a texture cannot move (the window drew since, or one is still on
    /// its way).
    fn history_glide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != "history" || !self.history_flowing(cx) {
            self.history.gliding = None;
            return;
        }
        // While the page slides in, the curves move with it as drawn: every
        // frame of the slide draws the window, and moving the textures would
        // draw the card again in each.
        if self.page_enter_pending || self.page_entering(cx) {
            return;
        }
        let Some(gliding) = &self.history.gliding else {
            return;
        };
        let mode = self.chart_clock.borrow().mode;
        self.history.glide_frames = self.history.glide_frames.wrapping_add(1);
        let refresh = motion::frame_period(motion::Mode::Fluid, window.refresh_interval(cx));
        let every = (motion::frame_period(mode, Some(refresh)).as_secs_f64()
            / refresh.as_secs_f64())
        .round()
        .max(1.) as u64;
        if !self.history.glide_frames.is_multiple_of(every) {
            return;
        }
        let elapsed = gliding.drawn.elapsed();
        let moved = elapsed < gliding.step && {
            let ms = elapsed.as_secs_f64() * 1000.;
            gliding.plots.iter().all(|glide| {
                glide
                    .plot
                    .glide(glide.bounds.get(), glide.plot.end + ms, window)
            })
        };
        if !moved {
            self.redraw_region_alone(regions::RegionKind::HistoryChart, window);
        }
    }

    /// Draws the chart again once the window has moved a device pixel.
    fn history_flow_later(&mut self, delay: Duration, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.flow.is_some() {
            return;
        }
        self.history.flow = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update_in(cx, |this, window, _| {
                this.history.flow = None;
                if this.page == "history" {
                    this.redraw_region_alone(regions::RegionKind::HistoryChart, window);
                }
            });
        }));
    }

    /// Whether the window follows the newest readings (the live button says so).
    pub(in crate::frontend) fn history_follows(&self) -> bool {
        self.history.follow
    }

    /// The range chosen through the preference (the test controls' `historyRange`).
    pub(in crate::frontend) fn history_range_preference(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.history_restore();
        let clock = self.history_clock();
        self.history.set_range(Range::parse(id), clock);
        self.load_history(window, cx);
        cx.notify();
    }

    /// Whether the table lists the running processes: in moment mode, while
    /// the window follows the newest readings and no moment is pinned.
    fn history_table_live(&self) -> bool {
        self.history.mode == TableMode::Moment && self.history.follow && self.history.pin.is_none()
    }

    /// The running processes in the recorded frames' terms: private working
    /// set as memory, all I/O, sent and received together, dedicated GPU memory.
    fn history_live_processes(&self) -> Vec<Value> {
        let sum = |process: &Value, keys: &[&str]| -> Value {
            let parts = keys
                .iter()
                .filter_map(|key| process[*key].as_f64())
                .collect::<Vec<_>>();
            if parts.is_empty() {
                Value::Null
            } else {
                json!(parts.iter().sum::<f64>())
            }
        };
        values(&self.snapshot["processes"])
            .iter()
            .map(|process| {
                json!({
                    "name": process["name"],
                    "pid": process["pid"],
                    "createTime": process["createTime"],
                    "cpu": process["cpu"],
                    "mem": process["privateWs"],
                    "io": sum(process, &["ioRead", "ioWrite", "ioOther"]),
                    "gpu": process["gpu"],
                    "net": sum(process, &["netSent", "netReceived"]),
                    "diskRead": process["diskRead"],
                    "diskWrite": process["diskWrite"],
                    "gpuMem": process["gpuDedicated"],
                    "power": process["power"],
                    "threads": process["threads"],
                    "handles": process["handles"],
                })
            })
            .collect()
    }

    /// The network unit the readings use.
    fn history_bits(&self) -> bool {
        self.prefs.get::<String>("netUnit", "bytes".into()) == "bits"
    }

    /// Lanes, range and choices from the preferences, once.
    fn history_restore(&mut self) {
        if self.history.restored {
            return;
        }
        self.history.restored = true;
        let saved = self.prefs.get::<String>("historyRange", "5m".into());
        let clock = self.history_clock();
        self.history.set_range(Range::parse(&saved), clock);
        self.history.settle();
        self.history
            .restore_lanes(&self.prefs.get::<Value>("history.lanes", Value::Null));
        self.history.band = self.prefs.get::<bool>("history.band", true);
        self.history.split = self
            .prefs
            .get::<f64>("history.split", 0.6)
            .clamp(0.34, 0.82) as f32;
        self.history_catalogue();
    }

    /// The readings there are, from what was recorded.
    fn history_catalogue(&mut self) {
        self.history.readings = readings::catalogue(&self.history.channels, &self.palette);
        self.history.catalogue_key = (self.palette.dark, argus_core::i18n::locale() as u8);
    }

    fn history_reading(&self, id: &str) -> Option<&Reading> {
        self.history.readings.iter().find(|r| r.id == id)
    }

    fn history_save_lanes(&mut self) {
        let saved = self.history.saved_lanes();
        if self.prefs.get::<Value>("history.lanes", Value::Null) != saved {
            self.prefs.set("history.lanes", saved);
        }
    }

    /// The plot's width: measured, else the page less the gutters.
    fn history_plot_width(&self, window: &Window) -> f32 {
        if let Some((_, width)) = self.history.geometry.borrow().plot_x() {
            return width;
        }
        let navigation = if self.nav_top {
            0.
        } else if self.nav_expanded {
            self.nav_width + 8.
        } else {
            48.
        };
        (f32::from(window.viewport_size().width) - navigation - 24. - 2. * lanes::GUTTER - 40.)
            .max(120.)
    }

    /// The channels a points query needs: the lanes', their overlays' and the tabs'.
    fn history_keys(&self) -> Vec<String> {
        let mut ids: Vec<&str> = Vec::new();
        for lane in self.history.live_lanes() {
            ids.push(&lane.reading);
            if let Some(overlay) = &lane.overlay {
                ids.push(overlay);
            }
        }
        let tabs = readings::tab_ids(&self.history.readings);
        ids.extend(tabs.iter().map(String::as_str));
        let mut keys = ids
            .into_iter()
            .filter_map(|id| self.history_reading(id))
            .flat_map(readings::query_keys)
            .collect::<Vec<_>>();
        keys.sort();
        keys.dedup();
        keys
    }

    /// Asks for what the page shows and is not at hand: the channels, the
    /// window's points (only the newest while following), the navigator's
    /// overview and the table's processes.
    pub(in crate::frontend) fn load_history(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.load_watch(window, cx);
        self.history_restore();
        if self.options.fixture.is_some() {
            self.history_fixture();
            return;
        }
        let clock = self.history_clock();
        self.history.anchor(clock);
        let now = self.history_now();
        let asked = Instant::now();
        if self
            .history
            .channels_asked
            .is_none_or(|at| asked.duration_since(at) >= Duration::from_secs(60))
        {
            self.history.channels_asked = Some(asked);
            let from = self.history.extent.0;
            self.request(
                "history_channels",
                json!({"from": from.min(now - series::DAY), "to": now + MINUTE}),
                Some("historyChannels"),
                window,
                cx,
            );
        }
        // The earliest record, for the range "All" and the table's foot; again
        // once a minute, as pruning moves it.
        if self
            .history
            .range_asked
            .is_none_or(|at| asked.duration_since(at) >= Duration::from_secs(60))
        {
            self.history.range_asked = Some(asked);
            self.request("history_range", json!({}), Some("historyRange"), window, cx);
        }
        self.history_ask_points(window, cx);
        self.history_ask_overview(window, cx);
        self.history_ask_table(window, cx);
    }

    /// The pointer reached the page's tab: the page's readings are asked for
    /// now, so that it enters with its curves rather than drawing them in
    /// after its slide.
    pub(in crate::frontend) fn history_prefetch(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != "history" && self.options.fixture.is_none() {
            self.load_history(window, cx);
        }
    }

    /// The page comes into view: a following window ends at the present at
    /// once (`HistoryState::settle`), with no pointer or drag left over from
    /// when the page was last shown.
    pub(in crate::frontend) fn history_enter(&mut self) {
        self.history_restore();
        let clock = self.history_clock();
        self.history.anchor(clock);
        self.history.settle();
        self.history.hover = None;
        self.history.drag = None;
    }

    /// A sample arrived: while following, the newest readings are asked for.
    pub(in crate::frontend) fn history_sampled(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page == "history" && self.history.follow && self.options.fixture.is_none() {
            self.load_history(window, cx);
        }
    }

    fn history_ask_points(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // One query at a time: asked again as soon as the one on its way
        // lands (`history_complete`), not a sample later.
        if self
            .pending
            .values()
            .any(|p| p.cache.as_deref() == Some("historyPoints"))
        {
            self.history.points_again = true;
            return;
        }
        let width = self.history_plot_width(window);
        let (a, b) = self.history.view;
        let span = (b - a).max(SECOND);
        let bucket = series::bucket_for(span, width);
        let now = self.history_now();
        let origin = local_midnight(now);
        let keys = self.history_keys();
        let follow = self.history.follow;
        let fresh = self.history.points.as_ref().is_some_and(|points| {
            points.matches(bucket, origin, &keys) && points.from <= a && (follow || points.to >= b)
        });
        if fresh {
            if !follow
                || self
                    .history
                    .points_asked
                    .is_some_and(|at| at.elapsed() < refresh_every(span))
            {
                return;
            }
            // Only what is new since the last settled bucket.
            let settled = self.history.points.as_ref().map_or(b, |p| p.settled);
            let from = settled - (settled - origin).rem_euclid(bucket) - bucket;
            self.history.points_asked = Some(Instant::now());
            self.request(
                "history_points",
                json!({"from": from, "to": now + bucket, "bucket": bucket, "origin": origin,
                    "keys": keys, "part": "main", "incremental": true}),
                Some("historyPoints"),
                window,
                cx,
            );
            return;
        }
        let margin = span / 4 + bucket;
        let from = a - margin;
        let to = if follow { now + bucket } else { b + margin };
        self.history.points_asked = Some(Instant::now());
        self.request(
            "history_points",
            json!({"from": from, "to": to, "bucket": bucket, "origin": origin, "keys": keys,
                "part": "main", "incremental": false}),
            Some("historyPoints"),
            window,
            cx,
        );
    }

    /// The navigator draws the active lane's reading over the whole range: from
    /// the window's points when they cover it, else from an overview of its own.
    fn history_ask_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (a, b) = self.history.extent;
        let covered = self
            .history
            .points
            .as_ref()
            .is_some_and(|points| points.from <= a && points.to >= b);
        if covered {
            return;
        }
        let width = f32::from(self.history.geometry.borrow().track.size.width).max(300.);
        let bucket = series::bucket_for(b - a, width * 1.3);
        let now = self.history_now();
        let origin = local_midnight(now);
        let keys = self
            .history_reading(&self.history.active_lane().reading.clone())
            .map(readings::query_keys)
            .unwrap_or_else(|| vec!["cpu".into()]);
        let fresh = self.history.overview.as_ref().is_some_and(|points| {
            points.matches(bucket, origin, &keys) && points.from <= a + bucket
        });
        if fresh
            && (!self.history.follow
                || self.history.overview_asked.is_some_and(|at| {
                    at.elapsed() < refresh_every(b - a).max(Duration::from_secs(5))
                }))
        {
            return;
        }
        let incremental = fresh && self.history.follow;
        let from = if incremental {
            let settled = self.history.overview.as_ref().map_or(a, |p| p.settled);
            settled - (settled - origin).rem_euclid(bucket) - bucket
        } else {
            a - bucket
        };
        self.history.overview_asked = Some(Instant::now());
        self.request(
            "history_points",
            json!({"from": from, "to": now + bucket, "bucket": bucket, "origin": origin, "keys": keys,
                "part": "overview", "incremental": incremental}),
            Some("historyOverview"),
            window,
            cx,
        );
    }

    /// The moment the table lists: the pinned one, else the newest frame shown.
    fn history_moment(&self) -> i64 {
        let points = self.history.points.as_ref();
        if let Some(pin) = self.history.pin {
            return points
                .and_then(|p| p.index_at(pin).map(|i| p.frame[i]))
                .filter(|frame| *frame > 0)
                .unwrap_or(pin);
        }
        let end = if self.history.follow {
            self.history_clock()
        } else {
            self.history.view.1
        };
        points
            .and_then(|p| {
                (0..p.ts.len())
                    .rev()
                    .find(|i| p.frame[*i] > 0 && p.frame[*i] <= end)
                    .map(|i| p.frame[i])
            })
            .unwrap_or(end - FRAME)
    }

    /// The range the table lists in range mode: the selection, else the window.
    fn history_table_range(&self) -> (i64, i64) {
        self.history.selection.unwrap_or(self.history.view)
    }

    /// The trend column follows the topmost lane's reading.
    /// The process value that goes with a lane's reading (the tooltip ranks by it).
    fn history_trend_of(&self, lane: &state::Lane) -> &'static str {
        self.history_reading(&lane.reading)
            .and_then(|reading| reading.process)
            .unwrap_or("cpu")
    }

    /// The five processes busiest by `key` at point `index`, by name, once
    /// their answer for that point is in.
    /// The processes the tooltip lists, busiest by `key` first: the point's
    /// under the pointer, or while they are asked for, the last point's. The
    /// list keeps its rows as the pointer moves, rather than emptying for a
    /// moment at every point.
    fn history_tip_ranking(&self, key: &str) -> Vec<(String, f64)> {
        let Some((_, answer)) = &self.history.tip else {
            return Vec::new();
        };
        // A range answer holds totals over the time recorded.
        let seconds = answer["recorded"].as_f64().map(|ms| ms / 1000.);
        let mut by_name: Vec<(String, f64)> = Vec::new();
        for process in values(&answer["processes"]) {
            let Some(value) = table::process_value(process, key, seconds) else {
                continue;
            };
            let name = text(process, "name").to_owned();
            match by_name
                .iter_mut()
                .find(|(n, _)| n.eq_ignore_ascii_case(&name))
            {
                Some((_, sum)) => *sum += value,
                None => by_name.push((name, value)),
            }
        }
        by_name.retain(|(_, v)| *v > 0.);
        by_name.sort_by(|a, b| b.1.total_cmp(&a.1));
        by_name.truncate(TIP_PROCESSES);
        by_name
    }

    /// Asks for the processes of the point under the pointer: its frame, or for
    /// a bucket the range table over it.
    fn history_ask_tip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hover) = self.history.hover else {
            return;
        };
        let Some(points) = self.history.points.as_ref() else {
            return;
        };
        let Some(index) = points.index_at(hover.at) else {
            return;
        };
        let point = (points.ts[index], points.span[index]);
        if self.history.tip_for == Some(point) {
            return;
        }
        self.history.tip_for = Some(point);
        let frame = points.frame[index];
        if self.options.fixture.is_some() {
            // A fixture holds one frame and one range table.
            let answer = if point.1 <= FRAME {
                self.data.get("historyFrame")
            } else {
                self.data.get("historyTable")
            };
            self.history.tip = answer.map(|answer| (point, answer.clone()));
            return;
        }
        if point.1 <= FRAME {
            let ts = if frame > 0 { frame } else { point.0 };
            self.request(
                "history_frame",
                json!({"ts": ts, "tip": [point.0, point.1]}),
                Some("historyTip"),
                window,
                cx,
            );
        } else {
            let lane = self
                .history
                .lanes
                .iter()
                .find(|l| l.id == hover.lane)
                .cloned();
            let trend = lane.map_or("cpu", |lane| self.history_trend_of(&lane));
            self.request(
                "history_table",
                json!({"from": point.0, "to": point.0 + point.1, "trend": trend, "points": 1,
                    "tip": [point.0, point.1]}),
                Some("historyTip"),
                window,
                cx,
            );
        }
    }

    fn history_trend_key(&self) -> &'static str {
        self.history
            .live_lanes()
            .next()
            .and_then(|lane| self.history_reading(&lane.reading))
            .and_then(|reading| reading.process)
            .unwrap_or("cpu")
    }

    fn history_ask_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.history.mode {
            // The running processes need no query: the table reads the sample.
            TableMode::Moment if self.history_table_live() => {}
            TableMode::Moment => {
                let moment = self.history_moment();
                if self.history.frame_at == Some(moment) {
                    return;
                }
                self.history.frame_at = Some(moment);
                self.request(
                    "history_frame",
                    json!({"ts": moment}),
                    Some("historyFrame"),
                    window,
                    cx,
                );
            }
            TableMode::Range => {
                let range = self.history_table_range();
                let trend = self.history_trend_key();
                let moving = self.history.follow && self.history.selection.is_none();
                self.history_ask_range(
                    "historyTable",
                    range,
                    trend,
                    table::TREND_POINTS,
                    moving,
                    window,
                    cx,
                );
            }
        }
        // In moment mode the trend column still shows the window.
        if self.history.mode == TableMode::Moment && self.history_trend_shown() {
            let trend = self.history_trend_key();
            let follow = self.history.follow;
            let view = self.history.view;
            self.history_ask_range(
                "historyTrends",
                view,
                trend,
                table::TREND_POINTS,
                follow,
                window,
                cx,
            );
        }
        // The process laid over the lanes: its readings for each lane's key.
        if self.history.curve.is_some() {
            let mut keys = self
                .history
                .live_lanes()
                .filter_map(|lane| self.history_reading(&lane.reading).and_then(|r| r.process))
                .collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            let follow = self.history.follow;
            let view = self.history.view;
            for key in keys {
                let cache = format!("historyCurve.{key}");
                self.history_ask_range(&cache, view, key, CURVE_POINTS, follow, window, cx);
            }
        }
    }

    /// Whether the table shows its trend column (`history.trend`, on unless turned off).
    pub(super) fn history_trend_shown(&self) -> bool {
        self.prefs.get::<bool>("history.trend", true)
    }

    /// Asks for each process's aggregates over `range` once per range and trend,
    /// and again every ten seconds (a minute beyond six hours) while it moves.
    #[allow(clippy::too_many_arguments)]
    fn history_ask_range(
        &mut self,
        cache: &str,
        (from, to): (i64, i64),
        trend: &'static str,
        points: usize,
        moving: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.options.fixture.is_some() {
            return;
        }
        let key = (from, to, trend, points);
        if let Some((asked, at)) = self.history.ranges_asked.get(cache) {
            let same = asked.2 == trend
                && asked.3 == points
                && (moving || (asked.0, asked.1) == (from, to));
            let due = at.elapsed() >= refresh_every(to - from).max(Duration::from_secs(10));
            if same && !(moving && due) {
                return;
            }
        }
        self.history
            .ranges_asked
            .insert(cache.to_owned(), (key, Instant::now()));
        self.request(
            "history_table",
            json!({"from": from, "to": to, "trend": trend, "points": points}),
            Some(cache),
            window,
            cx,
        );
    }

    /// A fixture run reads the page's data from its fixture, and again after
    /// the test controls replace it (`data`).
    fn history_fixture(&mut self) {
        if let Some(answer) = self.data.get("historyPoints") {
            let keys = answer["series"]
                .as_object()
                .map(|series| series.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            let args = json!({"from": answer["from"], "to": answer["to"], "bucket": answer["bucket"],
                "origin": answer["origin"], "keys": keys});
            self.history.points = Some(Points::parse(answer, &args));
        }
        if let Some(answer) = self.data.get("historyChannels") {
            let channels = readings::parse_channels(answer);
            if channels.len() != self.history.channels.len() {
                self.history.channels = channels;
                self.history_catalogue();
                self.history_restore_default_overlay();
            }
        }
        self.history.frame = self.data.get("historyFrame").cloned();
        self.history.table = self.data.get("historyTable").cloned();
        self.history.first = self
            .data
            .get("historyRange")
            .and_then(|range| range[0].as_i64());
        let clock = self.history_clock();
        self.history.anchor(clock);
    }

    /// Takes in an answer to one of the page's queries; whether it was one.
    pub(in crate::frontend) fn history_complete(
        &mut self,
        command: &str,
        cache: Option<&str>,
        args: &Value,
        result: &Result<Value, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(
            command,
            "history_points"
                | "history_channels"
                | "history_frame"
                | "history_table"
                | "history_events"
        ) && !(command == "history_range" && cache == Some("historyRange"))
        {
            return false;
        }
        // The range picker's hours and events: kept for it as they come.
        if let Some(key @ ("historyDays" | "historyEvents")) = cache {
            if let Ok(answer) = result {
                self.data.insert(key.into(), answer.clone());
            }
            self.history_redraw(window, cx);
            return true;
        }
        if cache == Some("historyTip") {
            let point = (
                args["tip"][0].as_i64().unwrap_or(0),
                args["tip"][1].as_i64().unwrap_or(0),
            );
            if self.history.tip_for == Some(point) {
                if let Ok(answer) = result {
                    self.history.tip = Some((point, answer.clone()));
                }
            } else {
                // The pointer moved on while this was read.
                self.history.tip_for = None;
                self.history_ask_tip(window, cx);
            }
            self.history_redraw(window, cx);
            return true;
        }
        let Ok(answer) = result else {
            // Kept as it was; asked again on the next poll.
            return true;
        };
        match command {
            "history_points" => {
                let points = Points::parse(answer, args);
                let overview = args["part"] == "overview";
                let slot = if overview {
                    &mut self.history.overview
                } else {
                    &mut self.history.points
                };
                if args["incremental"] == true {
                    if let Some(existing) = slot
                        && existing.matches(points.bucket, points.origin, &points.keys)
                    {
                        existing.merge(points);
                    }
                } else {
                    *slot = Some(points);
                }
                // A window following the clock for hours keeps a bounded list.
                let keep = self.history.extent.0 - self.history.span();
                if let Some(points) = &mut self.history.points {
                    points.trim(keep);
                }
                if !overview {
                    let clock = self.history_clock();
                    self.history.anchor(clock);
                    // A newer frame may be what the table lists now.
                    self.history_ask_table(window, cx);
                    if std::mem::take(&mut self.history.points_again) {
                        self.history.points_asked = None;
                        self.history_ask_points(window, cx);
                    }
                }
            }
            "history_channels" => {
                let channels = readings::parse_channels(answer);
                if channels.len() != self.history.channels.len()
                    || channels
                        .iter()
                        .zip(&self.history.channels)
                        .any(|(a, b)| a.key != b.key || a.label != b.label)
                {
                    self.history.channels = channels;
                    self.history_catalogue();
                    self.history_restore_default_overlay();
                    // The tabs' readings may have changed what to ask for.
                    self.history_ask_points(window, cx);
                }
            }
            "history_frame" => {
                if args["ts"].as_i64() == self.history.frame_at {
                    self.history.frame = Some(answer.clone());
                }
            }
            "history_table" => match cache {
                Some("historyTrends") => self.history.trends = Some(answer.clone()),
                Some(curve) if curve.starts_with("historyCurve.") => {
                    let key = args["trend"].as_str().unwrap_or("cpu");
                    if let Some(key) = ["cpu", "mem", "disk", "net", "gpu"]
                        .into_iter()
                        .find(|k| *k == key)
                    {
                        // The answer holds no window of its own: kept with the one asked.
                        let mut answer = answer.clone();
                        answer["from"] = args["from"].clone();
                        answer["to"] = args["to"].clone();
                        self.history.curves.insert(key, answer);
                    }
                }
                _ => self.history.table = Some(answer.clone()),
            },
            "history_range" => {
                self.history.first = answer[0].as_i64();
            }
            _ => {}
        }
        self.history_redraw(window, cx);
        true
    }

    /// Draws the page again, not the window around it: what the page's data,
    /// the pointer and the page's own controls change shows only there. The
    /// live button in the page bar changes with commands, which draw everything.
    /// Not for answers that come while another page is shown (a prefetch), and
    /// not while the page enters: it draws once its slide is over, as the list
    /// pages do. Drawing the whole page mid-entrance held the slide still for
    /// as long as that took.
    fn history_redraw(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.page != "history" {
            return;
        }
        if self.page_enter_pending || self.page_entering(cx) {
            self.after_entrance(cx);
            return;
        }
        if self.has_region(regions::RegionKind::Page) {
            self.refresh_region(regions::RegionKind::Page, cx);
            window.request_view_redraw(cx.entity_id());
        } else {
            cx.notify();
        }
    }

    /// With no lanes saved, the first lane lays CPU temperature over CPU once
    /// it is recorded.
    fn history_restore_default_overlay(&mut self) {
        let saved = self.prefs.get::<Value>("history.lanes", Value::Null);
        if !saved.is_null() {
            return;
        }
        let recorded = self
            .history_reading("cpuTemp")
            .is_some_and(|reading| reading.recorded);
        if recorded
            && self.history.lanes.len() == 1
            && self.history.lanes[0].reading == "cpu"
            && self.history.lanes[0].overlay.is_none()
        {
            self.history.lanes[0].overlay = Some("cpuTemp".into());
        }
    }

    /// A lane added: the chart grows to give each lane room, up to 80 % of the
    /// page (it can be dragged smaller again).
    fn history_make_room(&mut self) {
        let content = f32::from(self.history.geometry.borrow().content.size.height);
        if content <= 0. {
            return;
        }
        let lanes = self.history.live_lanes().count() as f32;
        let wanted = lanes::comfortable_height(lanes) / content;
        if wanted > self.history.split {
            self.history.split = wanted.min(0.8);
            self.prefs
                .set("history.split", f64::from(self.history.split));
        }
    }

    /// Shows a short note over the lanes for a moment.
    fn history_toast(&mut self, key: &str) {
        self.history.toast = Some((label(key), Instant::now() + Duration::from_millis(1500)));
    }

    pub(in crate::frontend) fn history_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let clock = self.history_clock();
        let motion = self.history_motion(cx);
        match command {
            Command::Show(id) => self.history.show(&id),
            Command::AddLane(id) => {
                let index = self.history.lanes.len();
                self.history.insert_lane(id, index, motion);
                self.history_make_room();
            }
            Command::Reading(lane, id) => self.history.set_reading(lane, &id),
            Command::Overlay(lane, id) => self.history.set_overlay(lane, id),
            Command::RemoveLane(lane) => self.history.remove_lane(lane, motion),
            Command::Range(id) => {
                self.history.set_range(Range::parse(id), clock);
                self.prefs.set("historyRange", id);
            }
            Command::Latest => {
                self.history.go_latest(clock);
                self.history_toast("history.toast.latest");
            }
            Command::Pause => {
                self.history.pause();
                self.history_toast("history.toast.paused");
            }
            Command::ClearCursor => {
                self.history.clear_cursor();
            }
            Command::ZoomSelection => self.history.zoom_selection(),
            Command::Band => {
                self.history.band = !self.history.band;
                self.prefs.set("history.band", self.history.band);
            }
            Command::Mode(range) => {
                self.history.mode = if range {
                    TableMode::Range
                } else {
                    TableMode::Moment
                };
            }
            Command::Column(key) => self.history_toggle_column(key),
            Command::Trend => {
                let shown = self.history_trend_shown();
                self.prefs.set("history.trend", !shown);
            }
            Command::ResetColumns => {
                let mode = if self.history.mode == TableMode::Range {
                    "range"
                } else {
                    "moment"
                };
                self.prefs
                    .set(&format!("history.columns.{mode}"), Value::Null);
                self.prefs.set("history.trend", true);
            }
            Command::Curve(name) => {
                self.history.curve = name;
                self.history.curves.clear();
                self.history
                    .ranges_asked
                    .retain(|cache, _| !cache.starts_with("historyCurve."));
            }
            Command::Settings => {
                self.act(
                    Action::Preference("settingsSection".into(), json!("data")),
                    window,
                    cx,
                );
                self.act(Action::Page("settings".into()), window, cx);
                return;
            }
            Command::Picker => {
                self.history_open_picker(window, cx);
                return;
            }
        }
        self.history_save_lanes();
        self.load_history(window, cx);
        cx.notify();
    }

    /// Keyboard on the history page: arrows move the cursor (Shift ten at a
    /// time), Home goes to the earliest point, End back to the latest, + and −
    /// zoom, 0 shows the whole range, Esc clears the cursor and selection.
    pub(in crate::frontend) fn history_key(
        &mut self,
        key: &str,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // The range picker takes the keys while it is open: Esc closes it.
        if self.history.picker.is_some() {
            if key == "escape" {
                self.history_close_picker(cx);
                return true;
            }
            return false;
        }
        // Typing in the table's search box keeps its keys.
        if self.history_searching(window, cx) {
            return false;
        }
        let clock = self.history_clock();
        match key {
            "escape" => {
                if !self.history.clear_cursor() {
                    return false;
                }
            }
            "left" | "right" => {
                self.history_step(if key == "left" { -1 } else { 1 }, shift, window)
            }
            "pageup" | "pagedown" => {
                self.history_step(if key == "pageup" { -1 } else { 1 }, true, window)
            }
            "home" => {
                let points = self.history.points.as_ref();
                let first = points.and_then(|p| {
                    (0..p.ts.len())
                        .find(|i| p.ts[*i] >= self.history.view.0)
                        .map(|i| p.ts[i])
                });
                if let Some(first) = first {
                    self.history.pin = Some(first);
                    self.history.selection = None;
                    self.history.mode = TableMode::Moment;
                }
            }
            "end" => {
                self.history.go_latest(clock);
                self.history_toast("history.toast.latest");
            }
            "+" | "=" => {
                let at = self
                    .history
                    .pin
                    .unwrap_or((self.history.view.0 + self.history.view.1) / 2);
                self.history.zoom(0.7, at, state::SLOW);
            }
            "-" => {
                let at = self
                    .history
                    .pin
                    .unwrap_or((self.history.view.0 + self.history.view.1) / 2);
                self.history.zoom(1.4, at, state::SLOW);
            }
            "0" => self.history.reset_zoom(),
            _ => return false,
        }
        self.history_save_lanes();
        self.load_history(window, cx);
        cx.notify();
        true
    }

    /// Moves the cursor a point (or ten) along the lanes. Buckets wider than a
    /// frame select the bucket instead, as a click does.
    fn history_step(&mut self, direction: i64, far: bool, window: &Window) {
        let Some(points) = self.history.points.as_ref().filter(|p| !p.is_empty()) else {
            return;
        };
        let count = if far { 10 } else { 1 } as i64;
        let width = self.history_plot_width(window);
        let bucket = series::bucket_for(self.history.span(), width);
        let end = self.history_clock();
        let current = self
            .history
            .pin
            .or(self.history.selection.map(|(a, _)| a))
            .and_then(|at| points.index_at(at))
            .unwrap_or(points.ts.len() - 1) as i64;
        let index = (current + direction * count).clamp(0, points.ts.len() as i64 - 1) as usize;
        let at = points.ts[index];
        if bucket <= FRAME {
            self.history.pin = Some(at);
            self.history.selection = None;
            self.history.mode = TableMode::Moment;
        } else {
            self.history.selection = Some((at, at + points.span[index]));
            self.history.pin = None;
            self.history.mode = TableMode::Range;
        }
        // Off screen: the window moves to it.
        let shown = points.x_time(index, end);
        let (a, b) = self.history.view;
        if shown < a || shown > b {
            let span = b - a;
            self.history.pan(shown - span / 2 - a, state::FAST);
        }
    }

    /// The history page's controls in the page bar: following or not, and its menu.
    pub(in crate::frontend) fn history_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let follow = self.history.follow;
        let live = if follow {
            // Following: a quiet mark rather than a button; a click pauses.
            div()
                .id("history-live")
                .flex()
                .items_center()
                .gap(px(7.))
                .px(px(12.))
                .h(px(32.))
                .rounded(px(4.))
                .border_1()
                .border_color(p.stroke)
                .text_size(theme::font(13.))
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover))
                .child(div().size(px(8.)).rounded_full().bg(p.network))
                .child(label("history.live.on"))
                .tooltip(|window, cx| widgets::tooltip(label("history.live.onTitle"), window, cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.history_command(Command::Pause, window, cx)
                }))
                .into_any_element()
        } else {
            self.btn(
                "history-live",
                label("history.live.off"),
                Some("ArrowRightIcon"),
                widgets::Tone::Plain,
            )
            .tooltip(|window, cx| widgets::tooltip(label("history.live.offTitle"), window, cx))
            .on_click(
                cx.listener(|this, _, window, cx| {
                    this.history_command(Command::Latest, window, cx)
                }),
            )
            .into_any_element()
        };
        div().flex().items_center().child(live).into_any_element()
    }

    /// The history page's entries in the page bar's "More" menu: how the lanes
    /// draw, and the history settings.
    pub(in crate::frontend) fn history_more_items(&self) -> Vec<MenuItem> {
        vec![
            MenuItem::new(label("history.menu.band"), Action::History(Command::Band))
                .checked(self.history.band),
            MenuItem::new(
                label("history.menu.settings"),
                Action::History(Command::Settings),
            )
            .icon("HistoryIcon"),
        ]
    }

    /// The page's state for the test controls (`inspectLayout.history`).
    pub(in crate::frontend) fn history_inspect(&self) -> Value {
        let h = &self.history;
        json!({
            "follow": h.follow,
            "picker": h.picker.is_some(),
            "range": format!("{:?}", h.range),
            "view": [h.view.0, h.view.1],
            "extent": [h.extent.0, h.extent.1],
            "pin": h.pin,
            "selection": h.selection.map(|(a, b)| [a, b]),
            "mode": if h.mode == TableMode::Range { "range" } else { "moment" },
            "lanes": h.live_lanes().map(|l| json!({"id": l.id, "reading": l.reading, "overlay": l.overlay, "weight": l.weight})).collect::<Vec<_>>(),
            "active": h.active,
            "points": h.points.as_ref().map(|p| json!({"count": p.ts.len(), "bucket": p.bucket, "keys": p.keys})),
            "frameAt": h.frame_at,
            "hover": h.hover.map(|hover| hover.at),
        })
    }

    pub(in crate::frontend) fn history_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.history_restore();
        if self.history.catalogue_key != (self.palette.dark, argus_core::i18n::locale() as u8) {
            self.history_catalogue();
        }
        // The chart region draws the window as it moves; the page reads it
        // as the newest readings leave it.
        let clock = self.history_clock();
        self.history.anchor(clock);
        let p = self.palette;
        let page = window
            .use_keyed_state("history-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        // The page's room, as laid out last: the chart takes its share of it.
        // (A probe filling the scroller would count its padding as overflow.)
        let room = page.bounds();
        if room.size.height > px(0.) {
            self.history.geometry.borrow_mut().content = room;
        }
        let bar = self.history_bar(window, cx);
        let chart = self.region(
            regions::RegionKind::HistoryChart,
            StyleRefinement::default()
                .w_full()
                .h(px(self.history_chart_height(window)))
                .flex_shrink_0(),
            cx,
        );
        let table = self.history_table(window, cx);
        let dragging = self.history.drag.is_some();
        div()
            .id("history-content")
            .track_scroll(&page)
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
            .when(
                self.data.get("processWatch").is_some_and(Value::is_object)
                    || self.data.contains_key("watchError"),
                |d| d.child(self.watch_card(window, cx)),
            )
            .child(bar)
            .child(chart)
            .child(self.history_splitter(cx))
            .child(table)
            .child(scrollbar::scrollbar("history-scrollbar", &page, p.dark))
            .when(dragging, |d| d.child(self.history_drag_capture(cx)))
            .children(self.history_picker(window, cx))
            .into_any_element()
    }

    /// Follows the pointer while something on the page is dragged, wherever it
    /// goes, and ends the drag on release (the window's listeners, in the
    /// capture phase, as the splitters do).
    fn history_drag_capture(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let desktop = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moving = desktop.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = moving.update(cx, |this, cx| {
                            this.history_drag_move(event.position, window, cx)
                        });
                    }
                });
                let ending = desktop.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        let _ = ending.update(cx, |this, cx| {
                            this.history_drag_end(event.position, window, cx)
                        });
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    /// The bar between the chart and the table: dragged, it shares the height between them.
    fn history_splitter(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let active = matches!(self.history.drag, Some(state::Drag::Split { .. }));
        div()
            .id("history-splitter")
            .flex_shrink_0()
            .h(px(8.))
            .my(px(-5.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_row_resize()
            .child(div().w(px(36.)).h(px(3.)).rounded_full().bg(if active {
                p.accent
            } else {
                p.stroke_strong
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let height = f32::from(this.history.geometry.borrow().content.size.height);
                    this.history.drag = Some(state::Drag::Split {
                        y: f32::from(event.position.y),
                        split: this.history.split,
                        height,
                    });
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    fn history_drag_move(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.history.drag.clone() else {
            return;
        };
        match drag {
            state::Drag::Split { y, split, height } => {
                let dy = f32::from(at.y) - y;
                self.history.split = (split + dy / height.max(1.)).clamp(0.34, 0.82);
            }
            state::Drag::Divider {
                index,
                y,
                weights,
                height,
            } => {
                let dy = f32::from(at.y) - y;
                self.history.resize(index, weights, dy, height);
            }
            state::Drag::Select {
                start, moved, x, ..
            } => {
                let at_time = self.history_time_at(at.x, true);
                let moved = moved || (f32::from(at.x) - x).abs() > 4.;
                self.history.drag = Some(state::Drag::Select { start, moved, x });
                if moved && let Some(at_time) = at_time {
                    self.history.selection = Some((start.min(at_time), start.max(at_time)));
                    self.history.hover = None;
                }
            }
            state::Drag::Brush { mode, extent } => self.history_brush_move(mode, extent, at),
            state::Drag::Reading {
                id, start, moved, ..
            } => {
                let moved = moved || (at - start).magnitude() > 6.;
                let target = moved
                    .then(|| self.history_drop_target(at, false, &id))
                    .flatten();
                self.history.drag = Some(state::Drag::Reading {
                    id,
                    start,
                    at,
                    moved,
                    target,
                });
            }
            state::Drag::Lane {
                id, start, moved, ..
            } => {
                let moved = moved || (at - start).magnitude() > 6.;
                let target = moved
                    .then(|| self.history_drop_target(at, true, ""))
                    .flatten();
                self.history.drag = Some(state::Drag::Lane {
                    id,
                    start,
                    at,
                    moved,
                    target,
                });
            }
        }
        let _ = window;
        cx.notify();
    }

    fn history_drag_end(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.history.drag.take() else {
            return;
        };
        let motion = self.history_motion(cx);
        match drag {
            state::Drag::Split { .. } => {
                self.prefs
                    .set("history.split", f64::from(self.history.split));
            }
            state::Drag::Divider { .. } => self.history_save_lanes(),
            state::Drag::Select { start, moved, .. } => {
                if moved {
                    if let Some((a, b)) = self.history.selection
                        && b > a
                    {
                        self.history.pin = None;
                        self.history.mode = TableMode::Range;
                    }
                } else {
                    self.history_click(start, window);
                }
            }
            state::Drag::Brush { .. } => {}
            state::Drag::Reading {
                id, moved, target, ..
            } => {
                if !moved {
                    // A click on a tab: shown in the active lane.
                    self.history.show(&id);
                } else if let Some(target) = target {
                    match target {
                        state::Drop::Overlay(lane) => self.history.set_overlay(lane, Some(id)),
                        state::Drop::Insert(index) => {
                            self.history.insert_lane(id, index, motion);
                            self.history_make_room();
                        }
                    }
                }
            }
            state::Drag::Lane {
                id, moved, target, ..
            } => {
                if moved && let Some(state::Drop::Insert(index)) = target {
                    self.history.move_lane(id, index);
                }
            }
        }
        let _ = at;
        self.history_save_lanes();
        self.load_history(window, cx);
        cx.notify();
    }

    /// A click on the lanes at `at`: pins the point there, or selects its bucket
    /// when points stand for more than a frame.
    fn history_click(&mut self, at: i64, window: &Window) {
        let Some(points) = self.history.points.as_ref() else {
            return;
        };
        let Some(index) = points.index_at(at) else {
            return;
        };
        let width = self.history_plot_width(window);
        let bucket = series::bucket_for(self.history.span(), width);
        if bucket <= FRAME {
            self.history.pin = Some(points.ts[index]);
            self.history.selection = None;
            self.history.mode = TableMode::Moment;
        } else {
            self.history.selection =
                Some((points.ts[index], points.ts[index] + points.span[index]));
            self.history.pin = None;
            self.history.mode = TableMode::Range;
        }
    }

    /// The time under `x`: the window drawn now, mapped onto the plots' width.
    fn history_time_at(&self, x: Pixels, clamp: bool) -> Option<i64> {
        let (left, width) = self.history.geometry.borrow().plot_x()?;
        let mut f = (f32::from(x) - left) / width;
        if clamp {
            f = f.clamp(0., 1.);
        } else if !(0. ..=1.).contains(&f) {
            return None;
        }
        let (a, b) = self.history.shown_now();
        Some((a + (b - a) * f64::from(f)).round() as i64)
    }

    /// Where a dragged reading (or lane) would land with the pointer at `at`.
    fn history_drop_target(
        &self,
        at: Point<Pixels>,
        lane: bool,
        reading: &str,
    ) -> Option<state::Drop> {
        let geometry = self.history.geometry.borrow();
        let area = geometry.area;
        if at.x < area.left()
            || at.x > area.right()
            || at.y < area.top() - px(20.)
            || at.y > area.bottom() + px(30.)
        {
            return None;
        }
        let lanes = &geometry.lanes;
        let count = lanes.len();
        for (index, (id, bounds, plot)) in lanes.iter().enumerate() {
            let gap = px(lanes::GAP_Y / 2.);
            if at.y < bounds.top() - gap || at.y > bounds.bottom() + gap {
                continue;
            }
            let half = if at.y < bounds.top() + bounds.size.height / 2. {
                index
            } else {
                index + 1
            };
            if lane {
                return Some(state::Drop::Insert(half));
            }
            let in_plot = plot.contains(&at);
            let owner = self.history.lanes.iter().find(|l| l.id == *id);
            if in_plot
                && let Some(owner) = owner
                && owner.reading != reading
            {
                return Some(state::Drop::Overlay(*id));
            }
            if in_plot && owner.is_some_and(|owner| owner.reading == reading) {
                return None;
            }
            return Some(state::Drop::Insert(half));
        }
        let below = lanes
            .last()
            .is_some_and(|(_, bounds, _)| at.y > bounds.bottom());
        Some(state::Drop::Insert(if below { count } else { 0 }))
    }

    fn history_toggle_column(&mut self, key: &'static str) {
        let mode = if self.history.mode == TableMode::Range {
            "range"
        } else {
            "moment"
        };
        let pref = format!("history.columns.{mode}");
        let mut shown = table::shown_columns(
            &self.prefs.get::<Value>(&pref, Value::Null),
            self.history.mode,
        );
        if let Some(index) = shown.iter().position(|k| *k == key) {
            if shown.len() > 1 {
                shown.remove(index);
            }
        } else {
            shown.push(key);
        }
        self.prefs.set(&pref, json!(shown));
    }
}
