//! The page's state: the window on screen and how it follows the clock, the
//! lanes, the cursor and selection, drags in progress, and the data at hand.
use super::readings::{Channel, Reading};
use super::series::{DAY, HOUR, MINUTE, Points, SECOND, min_span};
use crate::frontend::theme::ease_out as ease;
use gpui_kit::gpui::{Bounds, Pixels, Point};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Direct input (wheel, keys) and layout changes (ranges, lanes) ease over these.
pub(super) const FAST: Duration = Duration::from_millis(167);
pub(super) const SLOW: Duration = Duration::from_millis(250);
/// The narrowest a lane can be dragged, header included.
pub(super) const MIN_LANE: f32 = 76.;

/// A preset range in the range menu, or one picked by its ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Range {
    /// The last so long, following the clock.
    Last(i64),
    /// Everything recorded, following the clock.
    All,
}

pub(super) const PRESETS: [(&str, i64); 6] = [
    ("5m", 5 * MINUTE),
    ("1h", HOUR),
    ("6h", 6 * HOUR),
    ("1d", DAY),
    ("7d", 7 * DAY),
    ("30d", 30 * DAY),
];

impl Range {
    /// The saved preference (`historyRange`); earlier versions saved `24h`.
    pub(super) fn parse(saved: &str) -> Self {
        let saved = if saved == "24h" { "1d" } else { saved };
        if saved == "all" {
            return Self::All;
        }
        PRESETS
            .iter()
            .find(|(id, _)| *id == saved)
            .map_or(Self::Last(5 * MINUTE), |(_, length)| Self::Last(*length))
    }
    pub(super) fn id(self) -> Option<&'static str> {
        match self {
            Self::Last(length) => PRESETS
                .iter()
                .find(|(_, l)| *l == length)
                .map(|(id, _)| *id),
            Self::All => Some("all"),
        }
    }
}

/// What the process table lists: one moment's processes, or a range's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TableMode {
    Moment,
    Range,
}

#[derive(Clone, Debug)]
pub(super) struct Lane {
    pub(super) id: u64,
    pub(super) reading: String,
    /// A second reading on the right axis.
    pub(super) overlay: Option<String>,
    /// Share of the lanes' height, and the share drawn while it changes.
    pub(super) weight: f32,
    pub(super) shown: f32,
    from: f32,
    started: Option<Instant>,
    pub(super) removing: bool,
}

impl Lane {
    fn new(id: u64, reading: String, overlay: Option<String>, weight: f32) -> Self {
        Self {
            id,
            reading,
            overlay,
            weight,
            shown: weight,
            from: weight,
            started: None,
            removing: false,
        }
    }
    /// How much of it is there, 0 to 1, while it comes or goes.
    pub(super) fn presence(&self) -> f32 {
        let full = if self.removing {
            self.from
        } else {
            self.weight
        };
        if full <= 0. {
            0.
        } else {
            (self.shown / full).clamp(0., 1.)
        }
    }
}

/// Where the pointer is over the lanes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Hover {
    /// Window coordinates.
    pub(super) position: Point<Pixels>,
    pub(super) lane: u64,
    /// The time under the pointer.
    pub(super) at: i64,
}

/// Where a dragged reading or lane lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Drop {
    /// Onto a lane's plot: overlaid on the right axis.
    Overlay(u64),
    /// Between lanes, or below the last: a new lane at this index.
    Insert(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Brush {
    Start,
    End,
    Move { grab: i64 },
}

#[derive(Clone, Debug)]
pub(super) enum Drag {
    /// Selecting a range on the lanes, from `start` (both are times).
    Select { start: i64, moved: bool, x: f32 },
    /// A divider between lanes `index` and `index + 1`.
    Divider {
        index: usize,
        y: f32,
        weights: (f32, f32),
        height: f32,
    },
    /// The splitter between the chart and the table.
    Split { y: f32, split: f32, height: f32 },
    /// The navigator's window, over the range as it was when the drag began.
    Brush { mode: Brush, extent: (i64, i64) },
    /// A reading from the tabs, or a lane by its grip.
    Reading {
        id: String,
        start: Point<Pixels>,
        at: Point<Pixels>,
        moved: bool,
        target: Option<Drop>,
    },
    Lane {
        id: u64,
        start: Point<Pixels>,
        at: Point<Pixels>,
        moved: bool,
        target: Option<Drop>,
    },
}

/// Where things were laid out in the last frame, for the pointer.
#[derive(Clone, Debug, Default)]
pub(super) struct Geometry {
    /// The lanes' area: headers, plots and dividers.
    pub(super) area: Bounds<Pixels>,
    /// Each lane: its id, its whole box, its plot.
    pub(super) lanes: Vec<(u64, Bounds<Pixels>, Bounds<Pixels>)>,
    /// The navigator's track.
    pub(super) track: Bounds<Pixels>,
    /// The chart card and the page's content, for the splitter.
    pub(super) content: Bounds<Pixels>,
}

impl Geometry {
    /// The plot's left edge and width, the same for every lane.
    pub(super) fn plot_x(&self) -> Option<(f32, f32)> {
        let (_, _, plot) = self.lanes.first()?;
        Some((f32::from(plot.left()), f32::from(plot.size.width).max(1.)))
    }
}

/// The displayed window moving towards the target.
#[derive(Clone, Copy, Debug)]
struct Tween {
    from: (f64, f64),
    started: Instant,
    duration: Duration,
}

pub(in crate::frontend) struct HistoryState {
    /// Following the newest readings: the window's end stays on the clock.
    pub(super) follow: bool,
    pub(super) range: Range,
    /// The navigator's range; while following it ends at the clock.
    pub(super) extent: (i64, i64),
    /// The window asked for, and the one drawn while it eases there.
    pub(super) view: (i64, i64),
    shown: (f64, f64),
    tween: Option<Tween>,
    pub(super) lanes: Vec<Lane>,
    pub(super) active: u64,
    next_lane: u64,
    pub(super) pin: Option<i64>,
    pub(super) selection: Option<(i64, i64)>,
    pub(super) mode: TableMode,
    pub(super) hover: Option<Hover>,
    pub(super) band: bool,
    /// The chart card's share of the page's height.
    pub(super) split: f32,
    pub(super) drag: Option<Drag>,
    pub(super) geometry: Rc<RefCell<Geometry>>,
    /// What was recorded, and the readings that makes.
    pub(super) channels: Vec<Channel>,
    pub(super) readings: Vec<Reading>,
    /// The points of the window on screen (and some way either side).
    pub(super) points: Option<Points>,
    /// The navigator's overview of the whole range.
    pub(super) overview: Option<Points>,
    /// The table's processes: of a moment (`history_frame`), or of a range (`history_table`).
    pub(super) frame: Option<Value>,
    pub(super) table: Option<Value>,
    /// The earliest record, for "All".
    pub(super) first: Option<i64>,
    /// A short note over the lanes ("Back to the latest"), and when it shows until.
    pub(super) toast: Option<(String, Instant)>,
    /// Restored from the preferences once.
    pub(super) restored: bool,
    /// Theme and language the readings' names and colours were made for.
    pub(super) catalogue_key: (bool, u8),
    /// When each query was last sent, and what the table's answer is for.
    pub(super) channels_asked: Option<Instant>,
    pub(super) range_asked: Option<Instant>,
    pub(super) points_asked: Option<Instant>,
    pub(super) overview_asked: Option<Instant>,
    pub(super) frame_at: Option<i64>,
    /// Table rows expanded to their instances, by name.
    pub(super) expanded: std::collections::HashSet<String>,
    /// The table's selected row (name, or name and PID).
    pub(super) selected: Option<String>,
    /// The processes the tooltip ranks, for the point (start, span) they were asked for.
    pub(super) tip: Option<((i64, i64), Value)>,
    pub(super) tip_for: Option<(i64, i64)>,
    /// Each process's trend over the window, for the table in moment mode.
    pub(super) trends: Option<Value>,
    /// The process drawn on the lanes as a dashed curve, by name, and its
    /// readings over the window by trend key.
    pub(super) curve: Option<String>,
    pub(super) curves: std::collections::HashMap<&'static str, Value>,
    /// What each range-table query was last sent for, and when.
    pub(super) ranges_asked: std::collections::HashMap<String, (RangeAsked, Instant)>,
}

/// A range-table query's window and trend: from, to, the trend's reading and
/// how many points it has.
pub(super) type RangeAsked = (i64, i64, &'static str, usize);

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            follow: true,
            range: Range::Last(5 * MINUTE),
            extent: (0, 5 * MINUTE),
            view: (0, 5 * MINUTE),
            shown: (0., (5 * MINUTE) as f64),
            tween: None,
            lanes: vec![Lane::new(1, "cpu".into(), None, 1.)],
            active: 1,
            next_lane: 2,
            pin: None,
            selection: None,
            mode: TableMode::Moment,
            hover: None,
            band: true,
            split: 0.6,
            drag: None,
            geometry: Rc::default(),
            channels: Vec::new(),
            readings: Vec::new(),
            points: None,
            overview: None,
            frame: None,
            table: None,
            first: None,
            toast: None,
            restored: false,
            catalogue_key: (false, u8::MAX),
            channels_asked: None,
            range_asked: None,
            points_asked: None,
            overview_asked: None,
            frame_at: None,
            expanded: Default::default(),
            selected: None,
            tip: None,
            tip_for: None,
            trends: None,
            curve: None,
            curves: Default::default(),
            ranges_asked: Default::default(),
        }
    }
}

impl HistoryState {
    pub(super) fn span(&self) -> i64 {
        (self.view.1 - self.view.0).max(SECOND)
    }
    /// The window drawn in the last frame.
    pub(super) fn shown_now(&self) -> (f64, f64) {
        self.shown
    }
    pub(super) fn length(&self) -> i64 {
        (self.extent.1 - self.extent.0).max(SECOND)
    }

    /// While following, ties the window and the range to `clock`: the newest
    /// reading's time. Ranges over six hours move a minute at a time.
    pub(super) fn anchor(&mut self, clock: i64) {
        if !self.follow {
            return;
        }
        let span = self.span();
        let length = match self.range {
            Range::All => {
                clock
                    - self
                        .first
                        .unwrap_or(clock - self.length())
                        .min(clock - span)
            }
            _ => self.length(),
        };
        let end = if span > 6 * HOUR {
            clock.div_euclid(MINUTE) * MINUTE
        } else {
            clock
        };
        self.view = (end - span.min(length), end);
        self.extent = (end - length, end);
    }

    /// The window drawn now, easing towards the target; whether it still moves.
    pub(super) fn shown(&mut self, now: Instant, motion: bool) -> ((f64, f64), bool) {
        let target = (self.view.0 as f64, self.view.1 as f64);
        let Some(tween) = self.tween.filter(|_| motion) else {
            self.tween = None;
            self.shown = target;
            return (target, false);
        };
        let progress = (now.saturating_duration_since(tween.started).as_secs_f64()
            / tween.duration.as_secs_f64().max(1e-6))
        .clamp(0., 1.);
        let e = f64::from(ease(progress as f32));
        self.shown = (
            tween.from.0 + (target.0 - tween.from.0) * e,
            tween.from.1 + (target.1 - tween.from.1) * e,
        );
        if progress >= 1. {
            self.tween = None;
        }
        (self.shown, self.tween.is_some())
    }

    /// Moves the target, the drawn window easing there from where it is.
    fn retarget(&mut self, duration: Duration) {
        self.tween = (!duration.is_zero()).then(|| Tween {
            from: self.shown,
            started: Instant::now(),
            duration,
        });
        if duration.is_zero() {
            self.shown = (self.view.0 as f64, self.view.1 as f64);
        }
    }

    /// Shows `a..b`, no longer following.
    pub(super) fn set_window(&mut self, a: i64, b: i64, duration: Duration) {
        self.follow = false;
        self.view = (a, b);
        self.retarget(duration);
    }

    /// Keeps the window's end, at a new width.
    pub(super) fn set_span(&mut self, span: i64, duration: Duration) {
        self.view = (self.view.1 - span, self.view.1);
        self.retarget(duration);
    }

    /// A range from the menu: the last so long (or everything), following the
    /// clock; or fixed ends, unless they reach the present.
    pub(super) fn set_range(&mut self, range: Range, clock: i64) {
        self.range = range;
        self.pin = None;
        self.selection = None;
        self.mode = TableMode::Moment;
        match range {
            Range::Last(length) => {
                self.follow = true;
                self.extent = (clock - length, clock);
                self.view = self.extent;
            }
            Range::All => {
                self.follow = true;
                let first = self.first.unwrap_or(clock - DAY).min(clock - 5 * MINUTE);
                self.extent = (first, clock);
                self.view = self.extent;
            }
        }
        self.retarget(SLOW);
    }

    /// Back to the newest readings, following them, at the width shown.
    pub(super) fn go_latest(&mut self, clock: i64) {
        let span = self.span().min(self.length());
        let length = self.length();
        self.follow = true;
        self.pin = None;
        self.selection = None;
        self.mode = TableMode::Moment;
        self.extent = (clock - length, clock);
        self.view = (clock - span, clock);
        self.retarget(SLOW);
    }

    /// Stops following where the window is.
    pub(super) fn pause(&mut self) {
        self.follow = false;
    }

    /// Zooms by `factor` around `at`; while following the end stays on the clock.
    pub(super) fn zoom(&mut self, factor: f64, at: i64, duration: Duration) {
        let length = self.length();
        let narrowest = min_span(length);
        let span = ((self.span() as f64 * factor).round() as i64).clamp(narrowest, length);
        if self.follow {
            self.set_span(span, duration);
            return;
        }
        let (a, b) = self.view;
        let at = at.clamp(a, b);
        let ratio = (at - a) as f64 / (b - a).max(1) as f64;
        let mut from = at - (span as f64 * ratio).round() as i64;
        from = from.clamp(self.extent.0, (self.extent.1 - span).max(self.extent.0));
        self.set_window(from, from + span, duration);
    }

    /// Moves the window by `dt` inside the range; this pauses following.
    pub(super) fn pan(&mut self, dt: i64, duration: Duration) {
        let span = self.span();
        let from =
            (self.view.0 + dt).clamp(self.extent.0, (self.extent.1 - span).max(self.extent.0));
        self.set_window(from, from + span, duration);
    }

    /// The whole range again.
    pub(super) fn reset_zoom(&mut self) {
        if self.follow {
            self.set_span(self.length(), SLOW);
        } else {
            self.set_window(self.extent.0, self.extent.1, SLOW);
        }
    }

    pub(super) fn clear_cursor(&mut self) -> bool {
        if self.pin.is_none() && self.selection.is_none() {
            return false;
        }
        self.pin = None;
        self.selection = None;
        self.mode = TableMode::Moment;
        true
    }

    /// Shows the selection across the lanes; one too narrow widens to the
    /// narrowest window around its middle.
    pub(super) fn zoom_selection(&mut self) {
        let Some((mut a, mut b)) = self.selection else {
            return;
        };
        let narrowest = min_span(self.length());
        if b - a < narrowest {
            let middle = (a + b) / 2;
            a = middle - narrowest / 2;
            b = middle + narrowest / 2;
        }
        self.selection = None;
        self.mode = TableMode::Range;
        self.set_window(a.max(self.extent.0), b.min(self.extent.1), SLOW);
    }

    // ---- Lanes ------------------------------------------------------------------

    pub(super) fn live_lanes(&self) -> impl Iterator<Item = &Lane> {
        self.lanes.iter().filter(|lane| !lane.removing)
    }

    pub(super) fn active_lane(&self) -> &Lane {
        self.live_lanes()
            .find(|lane| lane.id == self.active)
            .or_else(|| self.live_lanes().next())
            .unwrap_or(&self.lanes[0])
    }

    /// A new lane showing `reading` at `index`, growing in.
    pub(super) fn insert_lane(&mut self, reading: String, index: usize, motion: bool) {
        let id = self.next_lane;
        self.next_lane += 1;
        let mut lane = Lane::new(id, reading, None, 1.);
        if motion {
            lane.shown = 0.;
            lane.from = 0.;
            lane.started = Some(Instant::now());
        }
        self.lanes.insert(index.min(self.lanes.len()), lane);
        self.active = id;
    }

    /// Removes a lane, shrinking it away; the last one stays.
    pub(super) fn remove_lane(&mut self, id: u64, motion: bool) {
        if self.live_lanes().count() < 2 {
            return;
        }
        if let Some(lane) = self.lanes.iter_mut().find(|lane| lane.id == id) {
            lane.removing = true;
            lane.from = lane.shown;
            lane.weight = 0.;
            lane.started = Some(Instant::now());
            if !motion {
                lane.shown = 0.;
            }
        }
        if !motion {
            self.lanes
                .retain(|lane| !(lane.removing && lane.shown <= 0.));
        }
        let next = self.live_lanes().next().map(|lane| lane.id);
        if self.active == id
            && let Some(next) = next
        {
            self.active = next;
        }
    }

    /// Shows `reading` in the active lane, or makes active the lane that shows it.
    pub(super) fn show(&mut self, reading: &str) {
        let owner = self
            .live_lanes()
            .find(|lane| lane.reading == reading)
            .map(|lane| lane.id);
        if let Some(owner) = owner {
            self.active = owner;
            return;
        }
        let active = self.active_lane().id;
        self.set_reading(active, reading);
    }

    pub(super) fn set_reading(&mut self, lane: u64, reading: &str) {
        if let Some(lane) = self.lanes.iter_mut().find(|l| l.id == lane) {
            lane.reading = reading.into();
            if lane.overlay.as_deref() == Some(reading) {
                lane.overlay = None;
            }
            self.active = lane.id;
        }
    }

    pub(super) fn set_overlay(&mut self, lane: u64, reading: Option<String>) {
        if let Some(lane) = self.lanes.iter_mut().find(|l| l.id == lane) {
            lane.overlay = reading.filter(|r| *r != lane.reading);
            self.active = lane.id;
        }
    }

    /// Moves the lane `id` to `index` (counted before it is taken out).
    pub(super) fn move_lane(&mut self, id: u64, index: usize) {
        let Some(from) = self.lanes.iter().position(|lane| lane.id == id) else {
            return;
        };
        let mut to = index;
        if to > from {
            to -= 1;
        }
        if to != from {
            let lane = self.lanes.remove(from);
            self.lanes.insert(to.min(self.lanes.len()), lane);
        }
    }

    /// Steps the lanes' heights; whether any still moves.
    pub(super) fn step_lanes(&mut self, now: Instant, motion: bool) -> bool {
        let mut moving = false;
        for lane in &mut self.lanes {
            let Some(started) = lane.started else {
                continue;
            };
            let progress = if motion {
                (now.saturating_duration_since(started).as_secs_f32() / SLOW.as_secs_f32())
                    .clamp(0., 1.)
            } else {
                1.
            };
            lane.shown = lane.from + (lane.weight - lane.from) * ease(progress);
            if progress >= 1. {
                lane.shown = lane.weight;
                lane.started = None;
            } else {
                moving = true;
            }
        }
        self.lanes
            .retain(|lane| !(lane.removing && lane.started.is_none() && lane.shown <= 0.));
        moving
    }

    /// The lanes as the preference keeps them.
    pub(super) fn saved_lanes(&self) -> Value {
        json!(self
            .live_lanes()
            .map(|lane| json!({"reading": lane.reading, "overlay": lane.overlay, "weight": lane.weight}))
            .collect::<Vec<_>>())
    }

    /// Lanes from the preference; none saved keeps the default.
    pub(super) fn restore_lanes(&mut self, saved: &Value) {
        let Some(list) = saved.as_array().filter(|list| !list.is_empty()) else {
            return;
        };
        let lanes = list
            .iter()
            .filter_map(|lane| {
                let reading = lane["reading"].as_str()?.to_owned();
                let overlay = lane["overlay"].as_str().map(str::to_owned);
                let weight = lane["weight"].as_f64().unwrap_or(1.).clamp(0.1, 10.) as f32;
                Some((reading, overlay, weight))
            })
            .collect::<Vec<_>>();
        if lanes.is_empty() {
            return;
        }
        self.lanes = lanes
            .into_iter()
            .enumerate()
            .map(|(index, (reading, overlay, weight))| {
                Lane::new(index as u64 + 1, reading, overlay, weight)
            })
            .collect();
        self.next_lane = self.lanes.len() as u64 + 1;
        self.active = 1;
    }

    /// Resizes lanes `index` and `index + 1` by `dy` pixels of a lanes' area
    /// `height` tall; neither gets narrower than `MIN_LANE`.
    pub(super) fn resize(&mut self, index: usize, (a, b): (f32, f32), dy: f32, height: f32) {
        let sum: f32 = self
            .live_lanes()
            .map(|lane| lane.weight)
            .sum::<f32>()
            .max(1e-3);
        let per_pixel = sum / height.max(1.);
        let narrowest = MIN_LANE * per_pixel;
        let mut first = a + dy * per_pixel;
        let mut second = b - dy * per_pixel;
        if first < narrowest {
            second -= narrowest - first;
            first = narrowest;
        }
        if second < narrowest {
            first -= narrowest - second;
            second = narrowest;
        }
        let ids = self
            .lanes
            .iter()
            .filter(|lane| !lane.removing)
            .map(|lane| lane.id)
            .collect::<Vec<_>>();
        for (id, weight) in [(ids.get(index), first), (ids.get(index + 1), second)] {
            if let Some(lane) = id.and_then(|id| self.lanes.iter_mut().find(|lane| lane.id == *id))
            {
                lane.weight = weight.max(narrowest);
                lane.shown = lane.weight;
                lane.from = lane.weight;
                lane.started = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryState, Range, TableMode};
    use crate::frontend::pages::history::series::{HOUR, MINUTE, SECOND};
    use std::time::Duration;

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn following_ties_the_window_to_the_clock_and_zoom_keeps_its_end() {
        let mut state = HistoryState::default();
        state.set_range(Range::Last(HOUR), NOW);
        state.anchor(NOW + 7 * SECOND);
        assert_eq!(state.view, (NOW + 7 * SECOND - HOUR, NOW + 7 * SECOND));
        state.zoom(0.5, NOW, Duration::ZERO);
        assert!(state.follow);
        assert_eq!(state.view.1, NOW + 7 * SECOND);
        assert_eq!(state.span(), HOUR / 2);
        // The narrowest window is a fifth of the range.
        state.zoom(0.01, NOW, Duration::ZERO);
        assert_eq!(state.span(), 10 * MINUTE);
    }

    #[test]
    fn panning_pauses_and_only_going_back_to_the_latest_follows_again() {
        let mut state = HistoryState::default();
        state.set_range(Range::Last(HOUR), NOW);
        state.zoom(0.5, NOW, Duration::ZERO);
        state.pan(-10 * MINUTE, Duration::ZERO);
        assert!(!state.follow);
        // Back to the far right is still paused.
        state.pan(HOUR, Duration::ZERO);
        assert!(!state.follow);
        assert_eq!(state.view.1, NOW);
        state.anchor(NOW + MINUTE);
        assert_eq!(state.view.1, NOW, "a paused window stays put");
        state.pin = Some(NOW - MINUTE);
        state.go_latest(NOW + MINUTE);
        assert!(state.follow && state.pin.is_none());
        assert_eq!(state.view, (NOW + MINUTE - HOUR / 2, NOW + MINUTE));
        assert_eq!(state.mode, TableMode::Moment);
    }

    #[test]
    fn saved_ranges_read_back_and_unknown_ones_fall_back() {
        assert_eq!(Range::parse("24h"), Range::Last(24 * HOUR));
        assert_eq!(Range::parse("all"), Range::All);
        assert_eq!(Range::parse("nonsense"), Range::Last(5 * MINUTE));
    }

    #[test]
    fn lanes_keep_their_order_and_at_least_one() {
        let mut state = HistoryState::default();
        state.insert_lane("memUsed".into(), 1, false);
        state.insert_lane("disk".into(), 2, false);
        state.move_lane(3, 0);
        assert_eq!(
            state
                .lanes
                .iter()
                .map(|l| l.reading.as_str())
                .collect::<Vec<_>>(),
            ["disk", "cpu", "memUsed"]
        );
        state.show("memUsed");
        assert_eq!(state.active, 2);
        state.remove_lane(1, false);
        state.remove_lane(2, false);
        assert_eq!(state.lanes.len(), 1);
        state.remove_lane(3, false);
        assert_eq!(state.lanes.len(), 1, "the last lane stays");
        let saved = state.saved_lanes();
        let mut restored = HistoryState::default();
        restored.restore_lanes(&saved);
        assert_eq!(restored.lanes[0].reading, "disk");
    }
}
