//! The hundred dots (百): a 10 × 10 lattice whose bigger dots draw the A of
//! the logo. They appear in a wave until the A stands and lights up, then
//! turn into a dot-matrix chart of the last ten CPU samples: a column per
//! sample, lit up to its load, every dot of one size.
//!
//! The dots never move. When a sample arrives every column takes the one of
//! its right neighbour and the newest enters at the right, so a sample keeps
//! its dots, their size and their colours all the way to the left.
//!
//! On the about page the page only reserves the room (`placeholder`); a layer
//! of its own above the desktop draws the dots (`HundredLayer`, beside the
//! chart layer and under the dialogs), so the intro's frames and each new
//! sample redraw that layer, not the page. The onboarding's first step draws
//! them in place (`hundred`).
use super::*;

/// The intro, in milliseconds from when the dots appear: the A stands,
/// lights up, then turns into the chart.
const FORMED: f32 = 560.;
const LIT: (f32, f32) = (440., 720.);
const TURN: f32 = 860.;
const TURNED: f32 = 1560.;
/// The dots' size on the about page.
const ABOUT_SIZE: f32 = 72.;
/// The radii, of the pitch: the A's dots, the rest of the lattice and every
/// dot of the chart.
const BIG: f32 = 0.43;
const SMALL: f32 = 0.17;
const EVEN: f32 = 0.34;
/// The opacity of an unlit dot.
const FAINT: f32 = 0.12;

/// Whether the dot in `row`, `column` of the lattice belongs to the A: two
/// legs leaning in, a pair at the apex and the bar.
fn in_a(column: usize, row: usize) -> bool {
    let centre = 4.5 - 0.5 * row as f32;
    let leg = |at: f32| (1. - (at - centre).abs() * 1.15).clamp(0., 1.);
    let x = column as f32;
    (row == 0 && (column == 4 || column == 5))
        || (row == 6 && (2..=7).contains(&column))
        || leg(x).max(leg(9. - x)) > 0.5
}

/// The chart's columns, oldest (0) to newest (9), from `loads` (percent,
/// oldest first): the last ten loads as 0–1, None where a sample is unknown
/// or not taken yet.
fn columns(loads: impl DoubleEndedIterator<Item = Option<f64>>) -> [Option<f32>; 10] {
    let mut columns = [None; 10];
    for (column, load) in columns.iter_mut().rev().zip(loads.rev()) {
        *column = load.map(|load| (load as f32 / 100.).clamp(0., 1.));
    }
    columns
}

/// How a chart dot in `row` shows a column of `load` (0–1): its opacity and
/// how much of the tint it takes. A column is lit up to its load (at least
/// its bottom dot), the top dot solid, those under it half, those above
/// faint; an unknown column is faint throughout.
fn light(load: Option<f32>, row: usize) -> (f32, f32) {
    let Some(load) = load else {
        return (FAINT, 0.);
    };
    let lit = ((load * 10.).round() as usize).clamp(1, 10);
    // 1 for the bottom row.
    let height = 10 - row;
    if height == lit {
        (1., 1.)
    } else if height < lit {
        (0.5, 1.)
    } else {
        (FAINT, 0.)
    }
}

/// Whether a column of `load` takes the amber, as the tray and the charts do
/// from 80 %.
fn high(load: Option<f32>) -> bool {
    load.is_some_and(|load| load >= 0.8)
}

/// What one frame of the dots shows.
#[derive(Clone, Copy)]
struct Frame {
    /// Milliseconds since the dots appeared.
    elapsed: f32,
    /// The chart; None before the first known sample, when the A stays.
    columns: Option<[Option<f32>; 10]>,
    accent: Hsla,
    amber: Hsla,
    ink: Hsla,
    /// The A's opacity before it lights up.
    resting: f32,
}

impl Frame {
    fn new(elapsed: f32, columns: [Option<f32>; 10], p: &theme::Palette) -> Self {
        Self {
            elapsed,
            columns: columns.iter().any(Option::is_some).then_some(columns),
            accent: p.accent,
            amber: p.io,
            ink: p.ink,
            resting: if p.dark { 0.42 } else { 0.36 },
        }
    }

    /// Whether the intro still runs.
    fn moving(&self) -> bool {
        self.elapsed
            < if self.columns.is_some() {
                TURNED
            } else {
                LIT.1
            }
    }

    fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let ease = |from: f32, to: f32| {
            theme::ease_out(((self.elapsed - from) / (to - from)).clamp(0., 1.))
        };
        let lit = ease(LIT.0, LIT.1);
        let turn = self.columns.map_or(0., |_| ease(TURN, TURNED));
        let pitch = bounds.size.width / 10.;
        let Self {
            accent, amber, ink, ..
        } = *self;
        // One dot of the lattice: how big (of the pitch), how strong, how
        // much of it takes a tint and how much of that tint is amber.
        let mut dot =
            |column: usize, row: usize, radius: f32, alpha: f32, tinted: f32, warm: f32| {
                let radius = pitch * radius;
                let centre = point(
                    bounds.origin.x + pitch * (column as f32 + 0.5),
                    bounds.origin.y + pitch * (row as f32 + 0.5),
                );
                let rect = Bounds::new(
                    point(centre.x - radius, centre.y - radius),
                    size(radius * 2., radius * 2.),
                );
                let layers = [
                    (ink, 1. - tinted),
                    (accent, tinted * (1. - warm)),
                    (amber, tinted * warm),
                ];
                for (colour, share) in layers {
                    if share > 0. && alpha > 0. {
                        let fill = Hsla {
                            a: alpha * share,
                            ..colour
                        };
                        window.paint_quad(gpui_kit::fill(rect, fill).corner_radii(radius));
                    }
                }
            };
        for row in 0..10 {
            for column in 0..10 {
                let chart = self.columns.map(|columns| {
                    let load = columns[column];
                    let (alpha, tinted) = light(load, row);
                    (alpha, tinted, if high(load) { 1. } else { 0. })
                });
                // The chart alone: every dot in one size and its column's look.
                if let Some((alpha, tinted, warm)) = chart
                    && turn >= 1.
                {
                    dot(column, row, EVEN, alpha, tinted, warm);
                    continue;
                }
                // Each dot grows in over 180 ms, along the diagonal.
                let start = (row + column) as f32 / 18. * (FORMED - 180.);
                let grown = ease(start, start + 180.);
                if grown <= 0. {
                    continue;
                }
                // The A: its dots big and tinted, lighting up once it stands;
                // the rest small and faint.
                let (radius, alpha, tinted) = if in_a(column, row) {
                    (BIG, self.resting + (1. - self.resting) * lit, 1.)
                } else {
                    (SMALL, FAINT, 0.)
                };
                // Turning into the chart, in place.
                let (radius, alpha, tinted, warm) = match chart {
                    Some((c_alpha, c_tinted, c_warm)) => (
                        radius + (EVEN - radius) * turn,
                        alpha + (c_alpha - alpha) * turn,
                        tinted + (c_tinted - tinted) * turn,
                        c_warm * turn,
                    ),
                    None => (radius, alpha, tinted, 0.),
                };
                dot(
                    column,
                    row,
                    radius * (0.4 + 0.6 * grown),
                    alpha * grown,
                    tinted,
                    warm,
                );
            }
        }
    }
}

/// Milliseconds since `shown`, or the end of the intro with animations off.
fn elapsed(shown: Instant, cx: &App) -> f32 {
    if cx.reduce_motion() {
        f32::MAX
    } else {
        shown.elapsed().as_secs_f32() * 1000.
    }
}

/// The frame for `desktop`'s CPU samples.
fn frame(desktop: &Desktop, shown: Instant, cx: &App) -> Frame {
    let loads = desktop
        .timeline
        .samples
        .get("system.cpu")
        .map(|samples| columns(samples.iter().map(|(_, load)| *load)))
        .unwrap_or_default();
    Frame::new(elapsed(shown, cx), loads, &desktop.palette)
}

/// The dots drawn in place, `size` across, for the onboarding's first step:
/// the intro plays each time the step opens (`key` keeps its start while the
/// dots are drawn every frame); the chart then follows the step's redraws.
pub(super) fn hundred(
    key: &'static str,
    size: f32,
    desktop: &Desktop,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let shown = *window
        .use_keyed_state(key, cx, |_, _| Instant::now())
        .read(cx);
    let frame = frame(desktop, shown, cx);
    if frame.moving() {
        window.request_animation_frame();
    }
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| frame.paint(bounds, window),
    )
    .size(px(size))
    .flex_shrink_0()
}

/// Where the about page left room for the dots, written as it lays out and
/// read by the layer that draws them.
#[derive(Default)]
pub(super) struct Place {
    /// The room, before the page's own movement (`Desktop::page_shift`).
    bounds: Option<Bounds<Pixels>>,
    /// What of the page shows (its scrolling clips it).
    mask: Option<ContentMask<Pixels>>,
    /// The page opening that laid it out (`Desktop::page_epoch`).
    epoch: Option<u64>,
    /// When that opening showed the dots: the intro starts there.
    shown: Option<Instant>,
}

/// The room for the dots on the about page; a new opening of the page plays
/// the intro again.
pub(super) fn placeholder(place: Rc<RefCell<Place>>, epoch: u64) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            let mut place = place.borrow_mut();
            place.bounds = Some(bounds);
            place.mask = Some(window.content_mask());
            if place.epoch != Some(epoch) {
                place.epoch = Some(epoch);
                place.shown = Some(Instant::now());
            }
        },
        |_, _, _, _| {},
    )
    .size(px(ABOUT_SIZE))
    .flex_shrink_0()
}

/// Draws the about page's dots above the desktop, where the page left room.
/// It renders with every frame of the window, so a new sample (which redraws
/// the status bar) reaches it without redrawing the page.
pub(super) struct HundredLayer {
    desktop: WeakEntity<Desktop>,
    place: Rc<RefCell<Place>>,
    shift: Rc<Cell<Point<Pixels>>>,
}

impl HundredLayer {
    pub(super) fn new(
        desktop: WeakEntity<Desktop>,
        place: Rc<RefCell<Place>>,
        shift: Rc<Cell<Point<Pixels>>>,
    ) -> Self {
        Self {
            desktop,
            place,
            shift,
        }
    }
}

impl Render for HundredLayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layer = div().absolute().inset_0();
        let Some(desktop) = self.desktop.upgrade() else {
            return layer;
        };
        let desktop = desktop.read(cx);
        // Only while the about page that left the room is the one shown.
        let shown = {
            let place = self.place.borrow();
            match place.shown {
                Some(shown)
                    if desktop.page == "about" && place.epoch == Some(desktop.page_epoch) =>
                {
                    shown
                }
                _ => return layer,
            }
        };
        let frame = frame(desktop, shown, cx);
        if frame.moving() {
            window.request_animation_frame();
        }
        let (place, shift) = (self.place.clone(), self.shift.clone());
        layer.child(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let place = place.borrow();
                    let (Some(bounds), Some(mask)) = (place.bounds, place.mask) else {
                        return;
                    };
                    // Where the page is drawn, within what of it shows.
                    let shift = shift.get();
                    let bounds = Bounds::new(bounds.origin + shift, bounds.size);
                    let mask = ContentMask {
                        bounds: Bounds::new(mask.bounds.origin + shift, mask.bounds.size),
                    };
                    window.with_content_mask(Some(mask), |window| frame.paint(bounds, window));
                },
            )
            .size_full(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{FAINT, columns, high, in_a, light};

    #[test]
    fn the_hundred_dots_draw_an_a() {
        // The apex pair, both legs and the bar, mirrored left to right.
        assert!(in_a(4, 0) && in_a(5, 0) && in_a(0, 9) && in_a(9, 9) && in_a(3, 6));
        assert!(!in_a(0, 0) && !in_a(5, 9));
        for row in 0..10 {
            for column in 0..10 {
                assert_eq!(in_a(column, row), in_a(9 - column, row), "{column},{row}");
            }
        }
    }

    #[test]
    fn each_sample_keeps_its_column_until_the_next_one_moves_it_left() {
        let loads = |count: usize| (0..count).map(|n| Some(n as f64 * 5.));
        // The newest at the right, the ten last ones only.
        let full = columns(loads(14));
        assert_eq!(full[9], Some(0.65));
        assert_eq!(full[0], Some(0.2));
        // One sample later each load stands a column further left, unchanged.
        let next = columns(loads(15));
        assert_eq!(next[..9], full[1..]);
        // Unknown samples and those not taken yet stay unknown.
        let short = columns([Some(40.), None, Some(150.)].into_iter());
        assert_eq!(short[6..], [None, Some(0.4), None, Some(1.)]);
    }

    #[test]
    fn a_column_is_lit_up_to_its_load() {
        // 40 %: the fourth dot from the bottom (row 6) solid, half below,
        // faint above.
        assert_eq!(light(Some(0.4), 6), (1., 1.));
        assert_eq!(light(Some(0.4), 9), (0.5, 1.));
        assert_eq!(light(Some(0.4), 5), (FAINT, 0.));
        // Even an idle column lights its bottom dot; a full one its top.
        assert_eq!(light(Some(0.), 9), (1., 1.));
        assert_eq!(light(Some(1.), 0), (1., 1.));
        // Unknown: faint throughout, never amber.
        assert_eq!(light(None, 9), (FAINT, 0.));
        assert!(!high(None) && high(Some(0.8)) && !high(Some(0.79)));
    }
}
