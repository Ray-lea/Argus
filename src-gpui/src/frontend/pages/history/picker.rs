//! The custom range, a dialog of its own opened from the range pill's
//! "Custom…" (the pill's menu keeps the common ranges for everyday use): the
//! common ranges on the left; a month of days, each with how much of it was
//! recorded, and the exact start and end; and, folded under them until asked
//! for, the last 30 days hour by hour beside the events found in them. A
//! common range applies at once; days, typed times, hours and events on
//! Apply (a day's double click at once); Cancel or Esc leaves the range as
//! it was.
use super::series::{DAY, GapReason, HOUR, MINUTE, Part, Points};
use super::state::Range;
use super::*;
use crate::frontend::dialogs::Modal;
use crate::frontend::widgets::{Field, Tone};
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::input::{InputEvent, InputState};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// A common range, by what it covers.
#[derive(Clone, Copy, PartialEq)]
enum Preset {
    Last(i64),
    Today,
    Yesterday,
    All,
}
const PRESETS: [(&str, Preset); 10] = [
    ("last5m", Preset::Last(5 * MINUTE)),
    ("last15m", Preset::Last(15 * MINUTE)),
    ("last1h", Preset::Last(HOUR)),
    ("last6h", Preset::Last(6 * HOUR)),
    ("last24h", Preset::Last(DAY)),
    ("last7d", Preset::Last(7 * DAY)),
    ("last30d", Preset::Last(30 * DAY)),
    ("all", Preset::All),
    ("today", Preset::Today),
    ("yesterday", Preset::Yesterday),
];

/// What the 30-day map can show, and the readings each needs.
const HEATS: [(&str, &[&str]); 5] = [
    ("cpu", &["cpu"]),
    ("gpu", &["gpu"]),
    ("mem", &["memUsed", "memTotal"]),
    ("temp", &["cpuTemp"]),
    ("net", &["netReceive"]),
];
/// The event kinds to list, "all" first.
const FILTERS: [&str; 8] = ["all", "cpu", "temp", "mem", "gpu", "net", "disk", "gap"];
/// Days the map and the events cover, today included.
const OVERVIEW_DAYS: usize = 30;
/// The map's height, the room for the hours beside it and the days under it.
const MAP_HEIGHT: f32 = 150.;
const MAP_GUTTER: f32 = 34.;
const MAP_FOOT: f32 = 18.;
/// A calendar day, and a month of them.
const CELL_W: f32 = 30.;
const CELL_H: f32 = 28.;
const MONTH_W: f32 = 7. * CELL_W;
/// The common ranges' column, and the dialog's width: the columns, their
/// border and the dialog's margins.
const PRESETS_W: f32 = 132.;
const PANEL_W: f32 = PRESETS_W + MONTH_W + 2. * 14. + 2. + 48.;
/// Wider with the overview open: the events in a column beside the
/// calendar and the 30 days of hours under it.
const PANEL_WIDE: f32 = 720.;
/// The most events listed.
const EVENTS_SHOWN: usize = 40;

/// The picker while open: the range it would apply, and how it is being chosen.
pub(super) struct Picker {
    pub(super) from: i64,
    pub(super) to: i64,
    /// A first day was clicked: a click on a later day ends the range there.
    picking: bool,
    /// The range ends at the present and follows it.
    pub(super) follow: bool,
    /// The month shown: year and month (1–12).
    month: (i32, u8),
    error: Option<String>,
    filter: &'static str,
    /// Hours dragged over on the map: where the drag began and where it is.
    drag: Option<(usize, usize)>,
    hover: Option<usize>,
    /// Where the map was laid out, for the pointer.
    map: Rc<Cell<Bounds<Pixels>>>,
    /// What the hours were last asked for, and the events when.
    asked: Option<Value>,
    events_asked: Option<Instant>,
    /// The days and hours worked out from the answer, and for which answer.
    derived: Option<(DerivedKey, Derived)>,
}

type DerivedKey = (i64, usize, i64, &'static str);

/// The answer by day: hours recorded and the mean CPU, by local midnight; and
/// the map's 30 × 24 hours, oldest first, with the days they start.
#[derive(Default)]
struct Derived {
    days: HashMap<i64, (u32, f64)>,
    starts: Vec<i64>,
    cells: Vec<Option<f64>>,
}

/// The local date of `ts`.
fn date_of(ts: i64) -> Option<time::Date> {
    local(ts).map(|t| t.date())
}

/// Local midnight of `date`, at the offset in force then.
fn midnight(date: time::Date) -> i64 {
    let at = |offset: time::UtcOffset| {
        time::PrimitiveDateTime::new(date, time::Time::MIDNIGHT)
            .assume_offset(offset)
            .unix_timestamp()
            * 1000
    };
    let guess = at(time::UtcOffset::UTC);
    let offset = local(guess).map_or(time::UtcOffset::UTC, |t| t.offset());
    let first = at(offset);
    // A change of offset between the two is the day's own.
    let offset = local(first).map_or(offset, |t| t.offset());
    at(offset)
}

/// The first of a month and the number of its days.
fn month_days(year: i32, month: u8) -> Option<(time::Date, u8)> {
    let month = time::Month::try_from(month).ok()?;
    let first = time::Date::from_calendar_date(year, month, 1).ok()?;
    Some((first, month.length(year)))
}

fn shift_month((year, month): (i32, u8), by: i32) -> (i32, u8) {
    let index = year * 12 + i32::from(month) - 1 + by;
    (index.div_euclid(12), (index.rem_euclid(12) + 1) as u8)
}

/// `2026/11/19 16:40`, as the picker's boxes show times.
fn format_local(ts: i64) -> String {
    local(ts).map_or_else(String::new, |t| {
        format!(
            "{:04}/{:02}/{:02} {:02}:{:02}",
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute()
        )
    })
}

/// A local time typed as year, month, day and, optionally, hour and minute,
/// with any separators between them.
fn parse_local(text: &str) -> Option<i64> {
    let numbers = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u32>().ok())
        .collect::<Option<Vec<_>>>()?;
    let (year, month, day, hour, minute) = match numbers[..] {
        [y, m, d] => (y, m, d, 0, 0),
        [y, m, d, h, min] => (y, m, d, h, min),
        _ => return None,
    };
    let date = time::Date::from_calendar_date(
        i32::try_from(year).ok()?,
        time::Month::try_from(u8::try_from(month).ok()?).ok()?,
        u8::try_from(day).ok()?,
    )
    .ok()?;
    if hour > 24 || minute > 59 || (hour == 24 && minute > 0) {
        return None;
    }
    Some(midnight(date) + i64::from(hour) * HOUR + i64::from(minute) * MINUTE)
}

impl Picker {
    fn new(from: i64, to: i64, follow: bool) -> Self {
        let month = date_of(to).map_or((2026, 1), |d| (d.year(), u8::from(d.month())));
        Self {
            from,
            to,
            picking: false,
            follow,
            month,
            error: None,
            filter: "all",
            drag: None,
            hover: None,
            map: Rc::new(Cell::new(Bounds::default())),
            asked: None,
            events_asked: None,
            derived: None,
        }
    }
}

impl Desktop {
    /// Opens the picker on the range shown, with the boxes filled in.
    pub(super) fn history_open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_dialog("historyRange", json!({}), window, cx);
        let now = self.history_now();
        let (from, to) = self.history.extent;
        let picker = Picker::new(from, to.min(now), self.history.follow);
        self.history_picker_inputs(window, cx);
        self.history.picker = Some(picker);
        self.history_picker_fill(window, cx);
        self.history_picker_load(window, cx);
        cx.notify();
    }

    /// The dialog closed (Cancel, Esc, a range applied): the pending range goes.
    pub(in crate::frontend) fn history_picker_closed(&mut self) {
        self.history.picker = None;
    }

    /// Closes the dialog once a range is applied.
    fn history_end_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history.picker = None;
        if self
            .modal
            .as_ref()
            .is_some_and(|m| m.kind == "historyRange")
        {
            self.close_overlay(window, cx);
        }
    }

    /// The start and end boxes, made once: Enter in either applies.
    fn history_picker_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for key in ["historyFrom", "historyTo"] {
            if self.ui.inputs.contains_key(key) {
                continue;
            }
            let input = cx.new(|cx| InputState::new(window, cx));
            self.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |this, _, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.history_picker_apply(window, cx);
                    }
                },
            ));
            self.ui.inputs.insert(key.into(), input);
        }
    }

    /// Writes the pending range into the boxes.
    fn history_picker_fill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = &self.history.picker else {
            return;
        };
        for (key, ts) in [("historyFrom", picker.from), ("historyTo", picker.to)] {
            if let Some(input) = self.ui.inputs.get(key).cloned() {
                let text = format_local(ts);
                input.update(cx, |state, cx| state.set_value(text, window, cx));
            }
        }
    }

    /// The pending range from a day, the map or an event: the boxes follow,
    /// and so does the calendar when the range leaves it.
    fn history_picker_set(
        &mut self,
        from: i64,
        to: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = self.history_now();
        let first = self.history.first.unwrap_or(from);
        let Some(picker) = &mut self.history.picker else {
            return;
        };
        picker.from = from.max(first.min(from)).min(now);
        picker.to = to.min(now).max(picker.from + MINUTE);
        picker.follow = picker.to >= now - MINUTE;
        picker.error = None;
        if let Some(end) = date_of(picker.to - 1) {
            picker.month = (end.year(), u8::from(end.month()));
        }
        self.history_picker_fill(window, cx);
        self.history_picker_load(window, cx);
        cx.notify();
    }

    /// Asks for the hours of the months shown and of the last 30 days, and
    /// for the events once the overview shows.
    fn history_picker_load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = self.history_now();
        let overview = self.prefs.get::<bool>("history.overview", false);
        let heat = self.history_heat();
        let Some(picker) = &mut self.history.picker else {
            return;
        };
        let today = date_of(now);
        let shown = picker.month;
        let from = month_days(shown.0, shown.1).map(|(first, _)| midnight(first));
        let start = today.map(|d| midnight(d - time::Duration::days(OVERVIEW_DAYS as i64 - 1)));
        let (Some(from), Some(start)) = (from, start) else {
            return;
        };
        let from = from.min(start);
        let mut keys = vec!["cpu"];
        if let Some((_, needs)) = HEATS.iter().find(|(key, _)| *key == heat) {
            keys.extend(needs.iter().filter(|k| **k != "cpu"));
        }
        let args = json!({
            "from": from,
            "to": now,
            "bucket": HOUR,
            "origin": from,
            "keys": keys,
        });
        let ask_hours = picker.asked.as_ref() != Some(&args);
        if ask_hours {
            picker.asked = Some(args.clone());
        }
        let ask_events = overview
            && picker
                .events_asked
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(60));
        if ask_events {
            picker.events_asked = Some(Instant::now());
        }
        if ask_hours {
            self.request("history_points", args, Some("historyDays"), window, cx);
        }
        if ask_events {
            let args = json!({ "from": start, "to": now });
            self.request("history_events", args, Some("historyEvents"), window, cx);
        }
    }

    /// The reading the map shows (a preference).
    fn history_heat(&self) -> &'static str {
        let saved = self.prefs.get::<String>("history.heat", "cpu".into());
        HEATS
            .iter()
            .map(|(key, _)| *key)
            .find(|key| *key == saved)
            .unwrap_or("cpu")
    }

    /// Applies a common range at once.
    fn history_picker_preset(
        &mut self,
        preset: Preset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let clock = self.history_clock();
        let now = self.history_now();
        let today = date_of(now).map_or(now - now.rem_euclid(DAY), midnight);
        match preset {
            Preset::Last(length) => {
                self.history.set_range(Range::Last(length), clock);
                if let Some(id) = Range::Last(length).id() {
                    self.prefs.set("historyRange", id);
                }
            }
            Preset::Today => {
                self.history
                    .set_range(Range::Last((clock - today).max(MINUTE)), clock);
                self.history.range_name = Some("history.picker.today");
            }
            Preset::Yesterday => {
                let start = date_of(today - HOUR).map_or(today - DAY, midnight);
                self.history.set_fixed(start, today);
                self.history.range_name = Some("history.picker.yesterday");
            }
            Preset::All => {
                self.history.set_range(Range::All, clock);
                self.prefs.set("historyRange", "all");
            }
        }
        self.history_end_picker(window, cx);
        self.load_history(window, cx);
        cx.notify();
    }

    /// Applies the times in the boxes, or says what is wrong with them.
    pub(in crate::frontend) fn history_picker_apply(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(follow) = self.history.picker.as_ref().map(|p| p.follow) else {
            return;
        };
        let text = |key: &str| {
            self.ui
                .inputs
                .get(key)
                .map(|input| input.read(cx).value().to_string())
                .unwrap_or_default()
        };
        let now = self.history_now();
        let first = self.history.first;
        let from = parse_local(&text("historyFrom"));
        let to = if follow {
            Some(now)
        } else {
            parse_local(&text("historyTo"))
        };
        let error = match (from, to) {
            (Some(a), Some(b)) if b <= a => Some(label("history.picker.error.order")),
            (Some(a), Some(b)) if first.is_none_or(|first| b < first) || a > now => {
                Some(match first {
                    Some(first) => tr!(
                        "history.picker.error.none",
                        first = moment_label(first, false)
                    ),
                    None => label("history.picker.error.empty"),
                })
            }
            (Some(_), Some(_)) => None,
            _ => Some(tr!(
                "history.picker.error.format",
                example = format_local(now)
            )),
        };
        if let Some(error) = error {
            if let Some(picker) = &mut self.history.picker {
                picker.error = Some(error);
            }
            cx.notify();
            return;
        }
        let (Some(a), Some(b)) = (from, to) else {
            return;
        };
        let a = a.max(first.unwrap_or(a));
        let clock = self.history_clock();
        if follow {
            self.history
                .set_range(Range::Last((clock - a).max(MINUTE)), clock);
        } else {
            self.history.set_fixed(a, b.min(now));
        }
        self.history_end_picker(window, cx);
        self.load_history(window, cx);
        cx.notify();
    }

    /// The days and hours from the answer, worked out once per answer.
    fn history_picker_derive(&mut self) {
        let heat = self.history_heat();
        let now = self.history_now();
        let Some(answer) = self.data.get("historyDays") else {
            return;
        };
        let Some(picker) = &mut self.history.picker else {
            return;
        };
        let Some(asked) = picker.asked.clone().or_else(|| {
            // Test snapshots answer without asking: read the window from the answer.
            let ts = answer["ts"].as_array()?;
            Some(json!({"from": ts.first()?, "to": now, "bucket": HOUR, "keys": [heat]}))
        }) else {
            return;
        };
        let points = Points::parse(answer, &asked);
        let key = (
            points.ts.first().copied().unwrap_or(0),
            points.ts.len(),
            points.to,
            heat,
        );
        if picker.derived.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        // Local midnights over the answer's window, for days of 23 or 25 hours too.
        let mut starts = Vec::new();
        if let (Some(mut day), Some(last)) = (date_of(points.from), date_of(now)) {
            while day <= last {
                starts.push(midnight(day));
                let Some(next) = day.next_day() else {
                    break;
                };
                day = next;
            }
        }
        let day_of = |ts: i64| {
            let i = starts.partition_point(|start| *start <= ts);
            i.checked_sub(1).map(|i| (i, starts[i]))
        };
        let mut days: HashMap<i64, (u32, f64)> = HashMap::new();
        let map_from = starts.len().saturating_sub(OVERVIEW_DAYS);
        let mut cells = vec![None; OVERVIEW_DAYS * 24];
        for (i, ts) in points.ts.iter().enumerate() {
            let Some((index, start)) = day_of(*ts) else {
                continue;
            };
            if let Some(cpu) = points.value("cpu", Part::Mean, i) {
                let entry = days.entry(start).or_default();
                entry.0 += 1;
                entry.1 += cpu;
            }
            if index < map_from {
                continue;
            }
            let hour = ((ts - start) / HOUR).clamp(0, 23) as usize;
            let value = match heat {
                "mem" => points
                    .value("memUsed", Part::Mean, i)
                    .zip(points.value("memTotal", Part::Mean, i))
                    .filter(|(_, total)| *total > 0.)
                    .map(|(used, total)| used / total * 100.),
                "gpu" => points.value("gpu", Part::Mean, i),
                "temp" => points.value("cpuTemp", Part::Mean, i),
                "net" => points.value("netReceive", Part::Mean, i),
                _ => points.value("cpu", Part::Mean, i),
            };
            if let Some(cell) = cells.get_mut((index - map_from) * 24 + hour) {
                *cell = value.or(*cell);
            }
        }
        for entry in days.values_mut() {
            entry.1 /= f64::from(entry.0.max(1));
        }
        let starts = starts[map_from..].to_vec();
        picker.derived = Some((
            key,
            Derived {
                days,
                starts,
                cells,
            },
        ));
    }

    /// The custom range as a dialog (`historyRange`), opened from the range
    /// pill's "Custom…": the common ranges, a month of days with the exact
    /// ends, and folded under them the last 30 days with their events; Cancel
    /// and Apply in its footer.
    pub(in crate::frontend) fn history_range_dialog(
        &mut self,
        dialog: Dialog,
        modal: &Modal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let id = modal.id;
        if self.history.picker.is_none() {
            return dialog;
        }
        self.history_picker_derive();
        let p = self.palette;
        let overview = self.prefs.get::<bool>("history.overview", false);
        let body = div()
            .pt(px(22.))
            .px(px(24.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .text_size(theme::font(13.))
            .line_height(relative(1.4))
            .child(self.dialog_title(modal.title.clone()));
        // The common ranges and the calendar, with the fold under them and,
        // open, the map of hours under that.
        let left = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .w(px(PRESETS_W + MONTH_W + 28. + 2.))
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(6.))
                    .child(self.history_picker_presets(cx))
                    .child(self.history_picker_main(window, cx)),
            )
            .child(self.history_picker_fold(overview, cx))
            .when(overview, |d| d.child(self.history_overview(cx)));
        let body = if overview {
            // The events in a column of their own, from the top to the bottom
            // of what is beside them: laid over its room rather than in it,
            // so that a long list scrolls inside instead of growing the dialog.
            body.child(
                div().flex().gap(px(14.)).child(left).child(
                    div().relative().flex_1().min_w_0().child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .px(px(12.))
                            .pt(px(8.))
                            .pb(px(10.))
                            .border_1()
                            .border_color(p.stroke)
                            .rounded(px(6.))
                            .child(self.history_events_list(window, cx)),
                    ),
                ),
            )
        } else {
            body.child(left)
        };
        dialog.child(self.dialog_body(id).child(body)).footer(
            self.measure(id, 1).child(
                self.dialog_footer()
                    .child(
                        self.btn(
                            "history-picker-cancel",
                            label("common.cancel"),
                            None,
                            Tone::Plain,
                        )
                        .min_w(px(96.))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.close_overlay(window, cx)),
                        ),
                    )
                    .child(
                        self.btn(
                            "history-picker-apply",
                            label("history.picker.apply"),
                            None,
                            Tone::Accent,
                        )
                        .min_w(px(96.))
                        .track_focus(&self.primary_focus(id, cx))
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.history_picker_apply(window, cx)
                            }),
                        ),
                    ),
            ),
        )
    }

    /// The dialog's width: wider with the 30 days open.
    pub(in crate::frontend) fn history_range_width(&self) -> f32 {
        if self.prefs.get::<bool>("history.overview", false) {
            PANEL_WIDE
        } else {
            PANEL_W
        }
    }

    /// The common ranges, the one in force marked.
    fn history_picker_presets(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let current = |preset: Preset| match (preset, self.history.range) {
            (Preset::Last(a), Range::Last(b)) => a == b && self.history.range_name.is_none(),
            (Preset::All, Range::All) => true,
            (Preset::Today, _) => self.history.range_name == Some("history.picker.today"),
            (Preset::Yesterday, _) => self.history.range_name == Some("history.picker.yesterday"),
            _ => false,
        };
        div()
            .flex_shrink_0()
            .w(px(PRESETS_W))
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .border_r_1()
            .border_color(p.stroke)
            .children(PRESETS.iter().map(|(key, preset)| {
                let preset = *preset;
                let on = current(preset);
                div()
                    .when(*key == "today", |d| {
                        d.mt(px(4.)).border_t_1().border_color(p.stroke).pt(px(4.))
                    })
                    .id(SharedString::from(format!("history-preset-{key}")))
                    .flex()
                    .items_center()
                    .h(px(26.))
                    .px(px(10.))
                    .rounded(px(5.))
                    .text_size(theme::font(12.5))
                    .cursor_pointer()
                    .when(on, |d| {
                        d.bg(p.hover_strong).font_weight(FontWeight::SEMIBOLD)
                    })
                    .hover(move |s| s.bg(p.hover_strong))
                    .child(label(&format!("history.picker.{key}")))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.history_picker_preset(preset, window, cx);
                    }))
            }))
            .into_any_element()
    }

    /// The month, the exact ends, following and Apply.
    fn history_picker_main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let Some(picker) = &self.history.picker else {
            return div().into_any_element();
        };
        let month = picker.month;
        let follow = picker.follow;
        let error = picker.error.clone();
        let field =
            |this: &Self, key: &str, name: &str, enabled: bool, window: &Window, cx: &App| {
                let input = this.ui.inputs.get(key).cloned()?;
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().flex_shrink_0().text_color(p.muted).child(label(name)))
                        .child(div().flex_1().min_w_0().child(this.text_field(
                            &input,
                            widgets::input(&input),
                            Field::Boxed,
                            format_local(0).into(),
                            enabled,
                            window,
                            cx,
                        ))),
                )
            };
        let follow_box = div()
            .id("history-picker-follow")
            .flex()
            .items_start()
            .gap(px(8.))
            .cursor_pointer()
            .text_color(p.muted)
            .child(self.check_box(follow, false))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(label("history.picker.follow")),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                let now = this.history_now();
                if let Some(picker) = &mut this.history.picker {
                    picker.follow = !follow;
                    if picker.follow {
                        picker.to = now;
                    }
                }
                this.history_picker_fill(window, cx);
                cx.notify();
            }));
        div()
            .flex_1()
            .min_w_0()
            .px(px(14.))
            .pt(px(8.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(theme::font(12.5))
            .child(self.history_month(month, cx))
            .children(field(
                self,
                "historyFrom",
                "history.picker.start",
                true,
                window,
                cx,
            ))
            .children(field(
                self,
                "historyTo",
                "history.picker.end",
                !follow,
                window,
                cx,
            ))
            .child(follow_box)
            .when_some(error, |d, error| {
                d.child(
                    div()
                        .text_size(theme::font(12.))
                        .text_color(p.danger)
                        .child(error),
                )
            })
            .into_any_element()
    }

    /// The fold over the last 30 days and their events.
    fn history_picker_fold(&self, overview: bool, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        div()
            .id("history-picker-overview")
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(24.))
            .text_size(theme::font(12.5))
            .text_color(p.muted)
            .cursor_pointer()
            .hover(move |s| s.text_color(p.ink))
            .child(
                icon(
                    if overview {
                        "ChevronDown"
                    } else {
                        "ChevronRight"
                    },
                    p.muted,
                )
                .size(px(14.)),
            )
            .child(label("history.picker.overview"))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.prefs.set("history.overview", !overview);
                this.history_picker_load(window, cx);
                cx.notify();
            }))
            .into_any_element()
    }

    /// A month of days, each with how much of it was recorded and how busy
    /// the CPU was; the range marked. The arrows go a month either way.
    fn history_month(&self, (year, month): (i32, u8), cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let Some(picker) = &self.history.picker else {
            return div().into_any_element();
        };
        let Some((start, length)) = month_days(year, month) else {
            return div().into_any_element();
        };
        let now = self.history_now();
        let today = date_of(now);
        let first_day = self.history.first.and_then(date_of);
        let derived = picker.derived.as_ref().map(|(_, d)| d);
        let (from, to) = (picker.from, picker.to);
        let start_day = date_of(from);
        let end_day = date_of(to - 1);
        let names = label("format.monthNames");
        let month_name = names
            .split_whitespace()
            .nth(usize::from(month) - 1)
            .map_or_else(|| month.to_string(), str::to_owned);
        let arrow = |id: &'static str, glyph: &'static str, by: i32| {
            div()
                .id(id)
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover_strong))
                .child(icon(glyph, p.muted).size(px(14.)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(picker) = &mut this.history.picker {
                        picker.month = shift_month(picker.month, by);
                    }
                    this.history_picker_load(window, cx);
                    cx.notify();
                }))
        };
        let head = div()
            .flex()
            .items_center()
            .h(px(22.))
            .mb(px(2.))
            .text_size(theme::font(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .child(arrow("history-month-previous", "ChevronLeft", -1))
            .child(div().flex_1().text_center().child(tr!(
                "history.picker.month",
                year = year,
                month = month_name
            )))
            .child(arrow("history-month-next", "ChevronRight", 1));
        let weekdays = label("history.picker.weekdays");
        let week = div()
            .flex()
            .children(weekdays.split_whitespace().map(|name| {
                div()
                    .w(px(CELL_W))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(theme::font(11.5))
                    .text_color(p.faint)
                    .child(name.to_owned())
            }));
        let lead = usize::from(start.weekday().number_days_from_monday());
        let mut cells: Vec<AnyElement> = (0..lead)
            .map(|_| div().w(px(CELL_W)).h(px(CELL_H)).into_any_element())
            .collect();
        for day in 1..=length {
            let Ok(date) = time::Date::from_calendar_date(year, start.month(), day) else {
                continue;
            };
            let at = midnight(date);
            let off = today.is_none_or(|t| date > t) || first_day.is_none_or(|f| date < f);
            let (hours, cpu) = derived
                .and_then(|d| d.days.get(&at).copied())
                .unwrap_or((0, 0.));
            let inside = start_day
                .zip(end_day)
                .is_some_and(|(a, b)| date >= a && date <= b);
            let is_start = start_day == Some(date);
            let is_end = end_day == Some(date);
            let ends = inside && (is_start || is_end);
            let is_today = today == Some(date);
            let coverage = (hours as f32 / 24.).min(1.);
            let tint = 0.35 + 0.65 * (cpu as f32 / 30.).min(1.);
            let text_color = if ends {
                p.on_accent
            } else if off {
                p.faint
            } else {
                p.ink
            };
            let mut cell = div()
                .id(SharedString::from(format!("history-day-{at}")))
                .relative()
                .w(px(CELL_W))
                .h(px(CELL_H))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(2.))
                .text_size(theme::font(12.))
                .text_color(text_color)
                .when(off, |d| d.opacity(0.55))
                .when(inside && !ends, |d| d.bg(p.accent.opacity(0.16)))
                .when(ends, |d| d.bg(p.accent))
                .when(ends && is_start && !is_end, |d| d.rounded_l(px(6.)))
                .when(ends && is_end && !is_start, |d| d.rounded_r(px(6.)))
                .when(ends && is_start && is_end, |d| d.rounded(px(6.)))
                .when(!inside, |d| d.rounded(px(6.)))
                .child(day.to_string())
                .child(
                    div()
                        .w(px(18.))
                        .h(px(3.))
                        .rounded(px(2.))
                        .bg(if ends {
                            p.on_accent.opacity(0.35)
                        } else {
                            p.stroke_strong
                        })
                        .overflow_hidden()
                        .child(div().h_full().w(relative(coverage)).bg(if ends {
                            p.on_accent
                        } else {
                            p.cpu.opacity(tint)
                        })),
                )
                .when(is_today, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(3.))
                            .right(px(5.))
                            .size(px(4.))
                            .rounded_full()
                            .bg(if ends { p.on_accent } else { p.accent }),
                    )
                });
            if !off {
                let tip = tr!(
                    "history.picker.dayTip",
                    date = month_day(at),
                    count = hours,
                    cpu = format!("{cpu:.0}%")
                );
                cell = cell
                    .cursor_pointer()
                    .when(!ends, |d| d.hover(move |s| s.bg(p.hover_strong)))
                    .tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        let next = date.next_day().map_or(at + DAY, midnight);
                        // A double click shows that day at once.
                        if event.click_count() >= 2 {
                            this.history_picker_set(at, next, window, cx);
                            this.history_picker_apply(window, cx);
                            return;
                        }
                        let (from, picking) = this
                            .history
                            .picker
                            .as_ref()
                            .map_or((at, false), |p| (p.from, p.picking));
                        if picking && at >= from {
                            this.history_picker_set(from, next, window, cx);
                            if let Some(picker) = &mut this.history.picker {
                                picker.picking = false;
                            }
                        } else {
                            this.history_picker_set(at, next, window, cx);
                            if let Some(picker) = &mut this.history.picker {
                                picker.picking = true;
                            }
                        }
                    }));
            }
            cells.push(cell.into_any_element());
        }
        div()
            .w(px(MONTH_W))
            .flex()
            .flex_col()
            .child(head)
            .child(week)
            .child(div().flex().flex_wrap().children(cells))
            .into_any_element()
    }

    /// The last 30 days hour by hour, the reading they show chosen over them.
    fn history_overview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let heat = self.history_heat();
        let select = self.select(
            "history-heat",
            label("history.picker.heatTitle"),
            HEATS
                .iter()
                .map(|(key, _)| label(&format!("history.picker.heat.{key}")))
                .collect(),
            HEATS.iter().position(|(key, _)| *key == heat),
            false,
            |this, index, window, cx| {
                this.prefs.set("history.heat", HEATS[index].0);
                this.history_picker_load(window, cx);
                cx.notify();
            },
            cx,
        );
        let hint = self
            .history
            .picker
            .as_ref()
            .and_then(|picker| {
                let (_, derived) = picker.derived.as_ref()?;
                let cell = picker.hover?;
                let start = derived.starts.get(cell / 24)? + (cell % 24) as i64 * HOUR;
                let value = derived.cells.get(cell).copied().flatten();
                Some(format!(
                    "{} {}–{} · {}",
                    month_day(start),
                    clock(start, false),
                    clock(start + HOUR, false),
                    value.map_or_else(|| label("history.picker.noRecord"), |v| heat_text(heat, v))
                ))
            })
            .unwrap_or_else(|| label("history.picker.heatHint"));
        let map = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(26.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label("history.picker.heatTitle")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(theme::font(11.5))
                            .text_color(p.faint)
                            .child(hint),
                    )
                    .child(select),
            )
            .child(self.history_map(heat, cx));
        map.into_any_element()
    }

    /// The map: a column a day, a row an hour, shaded by the reading; the
    /// range outlined. Dragging over hours, or a click on a day's label,
    /// picks them.
    fn history_map(&self, heat: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let Some(picker) = &self.history.picker else {
            return div().into_any_element();
        };
        let bounds_cell = picker.map.clone();
        let derived = picker.derived.as_ref().map(|(_, d)| d);
        let starts = derived.map(|d| d.starts.clone()).unwrap_or_default();
        let cells = derived.map(|d| d.cells.clone()).unwrap_or_default();
        let (from, to) = match picker.drag {
            Some((a, b)) => {
                let (lo, hi) = (a.min(b), a.max(b));
                let at = |cell: usize| starts.get(cell / 24).map(|s| s + (cell % 24) as i64 * HOUR);
                (
                    at(lo).unwrap_or(picker.from),
                    at(hi).map_or(picker.to, |t| t + HOUR),
                )
            }
            None => (picker.from, picker.to),
        };
        let color = match heat {
            "gpu" => p.gpu,
            "mem" => p.memory,
            "temp" => p.temperature,
            "net" => p.network,
            _ => p.cpu,
        };
        let now = self.history_now();
        // Values shade from the lowest to the highest shown (a log scale for rates).
        let scale = move |v: f64| {
            if heat == "net" {
                (1. + v / 1024.).log10()
            } else {
                v
            }
        };
        let (low, high) = cells
            .iter()
            .flatten()
            .fold((f64::MAX, f64::MIN), |(lo, hi), v| {
                (lo.min(scale(*v)), hi.max(scale(*v)))
            });
        let low = if matches!(heat, "cpu" | "gpu" | "mem" | "net") {
            0.
        } else {
            low
        };
        let span = (high - low).max(1e-6);
        let stroke = p.stroke;
        let accent = p.accent;
        let days = starts.len();
        let paint_starts = starts.clone();
        let canvas = canvas(
            move |bounds, _, _| bounds_cell.set(bounds),
            move |bounds, _, window, _| {
                if days == 0 {
                    return;
                }
                let w = f32::from(bounds.size.width) / days as f32;
                let h = f32::from(bounds.size.height) / 24.;
                for (cell, value) in cells.iter().enumerate() {
                    let (day, hour) = (cell / 24, cell % 24);
                    let Some(start) = paint_starts.get(day).map(|s| s + hour as i64 * HOUR) else {
                        continue;
                    };
                    if start >= now {
                        continue;
                    }
                    let rect = Bounds::new(
                        point(
                            bounds.origin.x + px(day as f32 * w + 0.5),
                            bounds.origin.y + px(hour as f32 * h + 0.5),
                        ),
                        size(px((w - 1.).max(1.)), px((h - 1.).max(1.))),
                    );
                    let shade = match value {
                        Some(v) => {
                            let f = ((scale(*v) - low) / span).clamp(0., 1.) as f32;
                            color.opacity(0.1 + 0.9 * f)
                        }
                        None => stroke.opacity(0.35),
                    };
                    window.paint_quad(gpui_kit::fill(rect, shade));
                }
                // The range, outlined day by day.
                for (day, start) in paint_starts.iter().enumerate() {
                    let end = start + DAY;
                    let (a, b) = (from.max(*start), to.min(end));
                    if b <= a {
                        continue;
                    }
                    let top = (a - start) as f32 / DAY as f32 * f32::from(bounds.size.height);
                    let bottom = (b - start) as f32 / DAY as f32 * f32::from(bounds.size.height);
                    let rect = Bounds::new(
                        point(
                            bounds.origin.x + px(day as f32 * w),
                            bounds.origin.y + px(top),
                        ),
                        size(px(w), px((bottom - top).max(2.))),
                    );
                    window.paint_quad(gpui_kit::outline(rect, accent, BorderStyle::Solid));
                }
            },
        )
        .size_full();
        let hours = div()
            .absolute()
            .left_0()
            .top_0()
            .w(px(MAP_GUTTER - 4.))
            .h(px(MAP_HEIGHT))
            .children([0, 6, 12, 18, 24].map(|hour| {
                div()
                    .absolute()
                    .right_0()
                    .top(px(hour as f32 / 24. * MAP_HEIGHT - 7.))
                    .text_size(theme::font(10.5))
                    .text_color(p.faint)
                    .child(format!("{hour:02}:00"))
            }));
        let labels = starts.iter().enumerate().filter_map(|(day, start)| {
            let last = day + 1 == starts.len();
            if !last && (starts.len() - 1 - day) % 7 != 0 {
                return None;
            }
            let text = if last {
                label("history.picker.today")
            } else {
                month_day(*start)
            };
            let start = *start;
            Some(
                div()
                    .id(("history-map-day", day))
                    .absolute()
                    .top(px(MAP_HEIGHT + 3.))
                    .left(relative((day as f32 + 0.5) / days.max(1) as f32))
                    .ml(px(-22.))
                    .w(px(44.))
                    .text_center()
                    .text_size(theme::font(10.5))
                    .text_color(if last { p.accent } else { p.faint })
                    .cursor_pointer()
                    .child(text)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let end = date_of(start)
                            .and_then(|d| d.next_day())
                            .map_or(start + DAY, midnight);
                        this.history_picker_set(start, end, window, cx);
                    })),
            )
        });
        let at = |this: &Self, position: Point<Pixels>| -> Option<usize> {
            let picker = this.history.picker.as_ref()?;
            let bounds = picker.map.get();
            let days = picker.derived.as_ref()?.1.starts.len();
            if days == 0 || !bounds.contains(&position) {
                return None;
            }
            let x = f32::from(position.x - bounds.origin.x) / f32::from(bounds.size.width);
            let y = f32::from(position.y - bounds.origin.y) / f32::from(bounds.size.height);
            let day = ((x * days as f32) as usize).min(days - 1);
            let hour = ((y * 24.) as usize).min(23);
            Some(day * 24 + hour)
        };
        div()
            .relative()
            .h(px(MAP_HEIGHT + MAP_FOOT))
            .child(hours)
            .child(
                div()
                    .id("history-map")
                    .absolute()
                    .left(px(MAP_GUTTER))
                    .right_0()
                    .top_0()
                    .h(px(MAP_HEIGHT))
                    .cursor_pointer()
                    .child(canvas)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            if let Some(cell) = at(this, event.position)
                                && let Some(picker) = &mut this.history.picker
                            {
                                picker.drag = Some((cell, cell));
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        let cell = at(this, event.position);
                        let Some(picker) = &mut this.history.picker else {
                            return;
                        };
                        let mut changed = picker.hover != cell;
                        picker.hover = cell;
                        if let (Some((first, last)), Some(cell)) = (picker.drag, cell)
                            && last != cell
                        {
                            picker.drag = Some((first, cell));
                            changed = true;
                        }
                        if changed {
                            cx.notify();
                        }
                    }))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered && let Some(picker) = &mut this.history.picker {
                            picker.hover = None;
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            let Some(picker) = &mut this.history.picker else {
                                return;
                            };
                            let Some((a, b)) = picker.drag.take() else {
                                return;
                            };
                            let starts = picker
                                .derived
                                .as_ref()
                                .map(|(_, d)| d.starts.clone())
                                .unwrap_or_default();
                            let at = |cell: usize| {
                                starts.get(cell / 24).map(|s| s + (cell % 24) as i64 * HOUR)
                            };
                            if let (Some(from), Some(to)) = (at(a.min(b)), at(a.max(b))) {
                                this.history_picker_set(from, to + HOUR, window, cx);
                            }
                        }),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(MAP_GUTTER))
                    .right_0()
                    .top_0()
                    .h(px(MAP_HEIGHT + MAP_FOOT))
                    .children(labels),
            )
            .into_any_element()
    }

    /// The events on the days picked, newest first, by kind.
    fn history_events_list(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let Some(picker) = &self.history.picker else {
            return div().into_any_element();
        };
        let filter = picker.filter;
        let day = date_of(picker.from).map_or(picker.from, midnight);
        let end = picker.to.max(day + DAY);
        let answer = self.data.get("historyEvents");
        let all = answer
            .map(|a| values(&a["events"]).to_vec())
            .unwrap_or_default();
        let shown = all
            .iter()
            .filter(|e| {
                let (a, b) = (number(e, "from") as i64, number(e, "to") as i64);
                b > day && a < end && (filter == "all" || text(e, "kind") == filter)
            })
            .take(EVENTS_SHOWN)
            .cloned()
            .collect::<Vec<_>>();
        let chips = div()
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .children(FILTERS.iter().map(|key| {
                let on = filter == *key;
                div()
                    .id(SharedString::from(format!("history-filter-{key}")))
                    .h(px(22.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .border_1()
                    .border_color(if on {
                        gpui_kit::gpui::transparent_black()
                    } else {
                        p.stroke
                    })
                    .when(on, |d| d.bg(p.accent).text_color(p.on_accent))
                    .when(!on, |d| {
                        d.text_color(p.muted).hover(move |s| s.bg(p.hover_strong))
                    })
                    .text_size(theme::font(12.))
                    .cursor_pointer()
                    .child(label(&format!("history.picker.filter.{key}")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(picker) = &mut this.history.picker {
                            picker.filter = key;
                        }
                        cx.notify();
                    }))
            }));
        let rows = shown.iter().enumerate().map(|(index, event)| {
            let kind = text(event, "kind").to_owned();
            let (from, to) = (number(event, "from") as i64, number(event, "to") as i64);
            let (glyph, color, name) = if kind == "gap" {
                let reason = GapReason::parse(text(event, "reason"));
                let glyph = match reason {
                    GapReason::Sleep => "MoonIcon",
                    GapReason::Shutdown => "PowerIcon",
                    GapReason::Off => "EyeOffIcon",
                    _ => "SquareDashedIcon",
                };
                (glyph, p.faint, label(reason.key()))
            } else {
                let (glyph, color) = match kind.as_str() {
                    "temp" => ("ThermometerIcon", p.temperature),
                    "mem" => ("MemoryIcon", p.memory),
                    "gpu" => ("GpuIcon", p.gpu),
                    "net" => ("DownloadIcon", p.network),
                    "disk" => ("DiskIcon", p.io),
                    _ => ("CpuIcon", p.cpu),
                };
                (glyph, color, label(&format!("history.picker.event.{kind}")))
            };
            let mut detail = format!("{}–{}", clock(from, false), clock(to, false));
            if !same_day(from, to - 1) {
                detail = range_label(from, to, false);
            }
            if let Some(peak) = event["peak"].as_f64() {
                detail = tr!(
                    "history.picker.eventPeak",
                    time = detail,
                    peak = heat_text(event_heat(&kind), peak)
                );
            }
            if let Some(name) = event["top"]["name"].as_str() {
                detail = format!("{detail} · {name}");
            }
            div()
                .id(("history-event", index))
                .flex()
                .gap(px(8.))
                .px(px(8.))
                .py(px(6.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover))
                .child(
                    div()
                        .flex_shrink_0()
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .bg(color.opacity(0.16))
                        .child(icon(glyph, color).size(px(14.))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_size(theme::font(12.5))
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(theme::font(11.5))
                                        .text_color(p.faint)
                                        .child(duration_label(to - from)),
                                ),
                        )
                        .child(
                            // Wrapped rather than cut: the busiest process ends it.
                            div()
                                .text_size(theme::font(11.5))
                                .text_color(p.muted)
                                .child(detail),
                        ),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    let pad = (MINUTE * 10).max((to - from) * 3 / 2);
                    this.history_picker_set(from - pad, to + pad, window, cx);
                }))
        });
        let count = tr!(
            "history.picker.eventsCount",
            range = days_label(day, end),
            count = shown.len()
        );
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(26.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label("history.picker.eventsTitle")),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(theme::font(11.5))
                            .text_color(p.faint)
                            .child(count),
                    ),
            )
            .child(chips)
            .child(
                div()
                    .id("history-events")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(shown.is_empty(), |d| {
                        d.child(
                            div()
                                .pt(px(8.))
                                .text_size(theme::font(12.))
                                .text_color(p.faint)
                                .child(label(if answer.is_some() {
                                    "history.picker.noEvents"
                                } else {
                                    "history.picker.eventsLoading"
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// Whole days as dates (one day, or the first and last), else the times.
fn days_label(from: i64, to: i64) -> String {
    let whole = date_of(from).map(midnight) == Some(from) && date_of(to).map(midnight) == Some(to);
    if !whole {
        return range_label(from, to, false);
    }
    match date_of(to - 1).map(midnight) {
        Some(last) if last > from => format!("{} – {}", month_day(from), month_day(last)),
        _ => month_day(from),
    }
}

/// The map's reading for an event kind.
fn event_heat(kind: &str) -> &'static str {
    match kind {
        "temp" => "temp",
        "mem" => "mem",
        "gpu" => "gpu",
        "net" | "disk" => "net",
        _ => "cpu",
    }
}

/// A value on the map or of an event, in its unit.
fn heat_text(heat: &str, value: f64) -> String {
    match heat {
        "temp" => format!("{value:.0}°C"),
        "net" => theme::bytes(value, true),
        _ => format!("{value:.0}%"),
    }
}

#[cfg(test)]
mod tests {
    use super::{format_local, parse_local, shift_month};

    #[test]
    fn months_shift_across_years() {
        assert_eq!(shift_month((2026, 1), -1), (2025, 12));
        assert_eq!(shift_month((2026, 12), 1), (2027, 1));
        assert_eq!(shift_month((2026, 11), 0), (2026, 11));
    }

    #[test]
    fn typed_times_read_back_as_written() {
        let ts = parse_local("2026/11/19 16:40").unwrap();
        assert_eq!(format_local(ts), "2026/11/19 16:40");
        assert_eq!(parse_local("2026-11-19 16:40"), Some(ts));
        assert!(parse_local("2026/11/19").is_some());
        assert_eq!(parse_local("2026/13/01 00:00"), None);
        assert_eq!(parse_local("2026/11/19 25:00"), None);
        assert_eq!(parse_local("tomorrow"), None);
    }
}
