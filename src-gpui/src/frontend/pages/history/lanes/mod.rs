//! The chart card: lanes of readings, each with its title, axes and plot; the
//! time axis under them; the cursor, selection and drop marks over them; and
//! the navigator below.
use super::axes::{Axes, AxisEase, Eased, Mark, crossfade_marks, marks};
use super::bar::ReadingMenu;
use super::readings::Reading;
use super::series::{BANDED, Part, Points, STEP_CORNER, tick_step, ticks};
use super::state::{Drag, Drop, Glide, Hover, Lane};
use super::transition::OverlayFade;
use super::*;

mod column;
mod curves;
mod header;
mod lane;

/// The axis labels' columns either side of the plots; every lane keeps both,
/// so that the plots line up under one time axis.
pub(super) const GUTTER: f32 = 48.;
/// A lane's title row.
pub(super) const HEADER: f32 = 30.;
/// The room between lanes, where the divider sits.
pub(super) const GAP_Y: f32 = 14.;
/// The time axis under the lanes.
const AXIS: f32 = 22.;
/// The chart card's padding.
const PAD: f32 = 10.;
/// The navigator's row.
const NAV: f32 = 32.;

/// The chart card's height that gives `lanes` lanes room to read: about
/// 128 px each, with the axis and the navigator.
pub(super) fn comfortable_height(lanes: f32) -> f32 {
    lanes * 128. + (lanes - 1.).max(0.) * GAP_Y + AXIS + NAV + 2. * PAD + 8.
}

/// A lane's curves fading out on a range switch: drawn on the axes and in
/// the buckets of the window shown before it (`Crossfade::from`), with the
/// points it showed (`HistoryState::previous`).
pub(super) struct Leaving {
    axes: Axes,
    bucket: i64,
    window: (f64, f64),
    alpha: f32,
}

/// What a lane is drawn with besides its own settings.
pub(super) struct LaneDraw<'a> {
    /// How far it has come in (or gone), 0 to 1.
    presence: f32,
    /// Whether there are other lanes.
    multi: bool,
    /// Its axes as they ease, and their rows.
    eased: &'a Eased,
    /// The points drawn.
    points: Option<&'a Points>,
    /// A range switch: the curves fading out, and how opaque the new ones are.
    fade: Option<(&'a Points, &'a Leaving, f32)>,
    /// The overlay coming in or going, and how opaque it is.
    overlay_fade: Option<&'a (OverlayFade, f32)>,
    /// The bucket width asked for.
    bucket: i64,
    /// The window shown.
    window: (f64, f64),
    /// The plot's width as laid out last: measured before the lanes record
    /// their places anew (`history_plot_width` falls back to an estimate from
    /// the window while they do, and marks placed by it stood off their times).
    plot_width: f32,
    /// The clock lines' times, and those drawn into the curves' textures
    /// while the window glides.
    ticks: &'a Arc<Vec<i64>>,
    clock_lines: Option<&'a (Arc<Vec<f64>>, Hsla)>,
    /// The point the readings in the title show.
    read_index: Option<usize>,
}

/// The width of the widest of `samples` at `size` and `weight`: a value slot
/// that keeps its width while the value in it changes.
pub(super) fn slot_width(
    samples: &[String],
    size: Pixels,
    weight: FontWeight,
    window: &Window,
    cx: &App,
) -> Pixels {
    samples
        .iter()
        .map(|text| {
            window
                .text_system()
                .layout_line(text, size, &[theme::text_run(text, weight, cx)], None)
                .width
        })
        .fold(px(0.), Pixels::max)
        .ceil()
        + px(1.)
}

impl Desktop {
    /// The moment the readings describe: the pointer's, the pinned one, else
    /// the newest reading shown.
    pub(super) fn history_read_at(&self) -> i64 {
        if let Some(hover) = self.history.hover {
            return hover.at;
        }
        if let Some(pin) = self.history.pin {
            return pin;
        }
        if self.history.follow {
            self.history_clock()
        } else {
            self.history.view.1
        }
    }

    /// The point the readings show, if one covers the moment.
    pub(super) fn history_read_index(&self) -> Option<usize> {
        let points = self.history.points.as_ref()?;
        let at = self.history_read_at();
        points.index_at(at).or_else(|| {
            // Following, the newest point covers the present.
            (self.history.hover.is_none() && self.history.pin.is_none())
                .then(|| points.ts.len().checked_sub(1))
                .flatten()
        })
    }

    /// The chart card's height: its share of the page, enough for every lane.
    pub(super) fn history_chart_height(&self, window: &Window) -> f32 {
        let content = self.history.geometry.borrow().content.size.height;
        let content = if content > px(0.) {
            f32::from(content)
        } else {
            f32::from(window.viewport_size().height) - 140.
        };
        // Its share of the page; lanes that do not fit there scroll inside.
        (content * self.history.split).max(260.)
    }

    /// The chart card: the lanes over the time axis, and the navigator,
    /// filling the region it is drawn in.
    pub(super) fn history_chart(
        &mut self,
        shown: (f64, f64),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        // The points drawn: spliced with those they replaced while the window
        // eases from them, for the lanes and the navigator alike.
        let mut drawn = self.history.drawn_points();
        // Following a range in buckets, the curves end at the newest complete
        // bucket, and one that completes is drawn on (`reveal_to`).
        self.history.reveal_moving = false;
        if self.history.follow
            && self.history.fading.is_none()
            && !self.history_flowing(cx)
            && let Some(points) = drawn.as_mut().filter(|points| points.bucket > SECOND)
        {
            let motion = self.history_motion(cx);
            let (cutoff, moving) =
                self.history
                    .reveal_to(points.settled, points.bucket, Instant::now(), motion);
            *points = points.until(cutoff);
            self.history.reveal_moving = moving;
        }
        let lanes = self.history_lanes(shown, drawn.as_ref(), window, cx);
        let navigator = self.history_navigator(drawn.as_ref(), window, cx);
        div()
            .id("history-chart")
            .flex_none()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(PAD))
            .pb(px(8.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            // Opaque, as the live charts' surface: Windows moves curves on
            // the compositor only over an opaque backdrop, and draws those
            // over the window material again in every frame.
            .bg(p.chart_surface)
            .child(lanes)
            .child(navigator)
            .into_any_element()
    }

    /// The button at the end of the reading tabs adding a lane.
    pub(super) fn history_add_lane(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        div()
            .id("history-add-lane")
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
                "history-add-lane",
                self.ui.anchors.clone(),
            ))
            .tooltip(|window, cx| widgets::tooltip(label("history.lane.addTitle"), window, cx))
            .on_click(cx.listener(|this, _, window, cx| {
                let exclude = this
                    .history
                    .live_lanes()
                    .map(|lane| lane.reading.clone())
                    .collect::<Vec<_>>();
                let menu = ReadingMenu {
                    title: Some("history.menu.addTitle"),
                    current: None,
                    exclude: &exclude,
                    pairs: true,
                    none: None,
                    add: false,
                };
                this.history_open_readings(menu, Command::AddLane, "history-add-lane", window, cx);
            }))
            .into_any_element()
    }
}
