//! The daily request for a GitHub star: one card at the bottom of the
//! lower-right notice stack, at most once per local day while the reminder
//! is on.
//!
//! The card may lie over a chart, so it is made of solid quads, text and
//! icons only: a path, shadow or gradient there makes Windows draw the curves
//! again in every frame for as long as the card shows (the chart layer's note
//! in `src-gpui/AGENTS.md`). Its motion is a few short one-shots for the same
//! reason; nothing loops.
use super::*;
use std::f32::consts::{PI, TAU};
use widgets::{ToastKind, Tone};

/// Whether the reminder is on; it is until turned off.
pub(super) const REMIND: &str = "star.remind";
/// The local day the card last came in, as `2026-11-10`.
const SHOWN: &str = "star.shown";
/// How long the window has to be in view before the card comes in.
const SETTLE: Duration = Duration::from_secs(5);
/// How long the card stays when left alone, and at least after the pointer
/// leaves it.
const STAY: Duration = Duration::from_secs(30);
const LINGER: Duration = Duration::from_secs(6);
/// After the press that opens GitHub the star stays lit this long, then the
/// card leaves.
const STARRED: Duration = Duration::from_millis(900);
/// The entrance, the leave and the small stars' flight, in seconds.
const ENTER: f32 = 0.32;
const LEAVE: f32 = 0.17;
const FLIGHT: f32 = 0.55;
/// When the small stars leave the badge after the card came in.
const SPARKS_AT: f32 = 0.38;
/// The small stars: the direction each flies from the badge's centre
/// (radians, clockwise from the right) and how much later it starts.
/// None flies straight at the title beside the badge.
const SPARKS: [(f32, f32); 5] = [
    (-2.55, 0.),
    (-1.5, 0.06),
    (-0.75, 0.02),
    (0.85, 0.09),
    (2.35, 0.04),
];

/// The card while it shows.
pub(super) struct Card {
    born: Instant,
    /// When it was first drawn: its entrance runs from there, not from a
    /// first frame that may come a moment after it was asked for.
    drawn: Cell<Option<Instant>>,
    /// When it leaves on its own; `None` while the pointer holds it.
    expires: Option<Instant>,
    leaving: Option<Instant>,
    /// The pointer is on the button that opens GitHub: the star fills.
    lit: bool,
    /// That button was pressed: the star stays filled and its small stars fly
    /// once more.
    starred: Option<Instant>,
}

#[derive(Default)]
pub(super) struct State {
    card: Option<Card>,
    /// Since when everything has been in place for the card to come in.
    ready: Option<Instant>,
    /// This run opened the onboarding, or told a portable copy about the
    /// installer: it is asked for nothing more.
    first_run: bool,
}

/// Whether the card is due on the local day `today`.
fn due(remind: bool, shown: Option<&str>, today: &str) -> bool {
    remind && shown != Some(today)
}

/// The local calendar day, as `2026-11-10`.
fn today() -> String {
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    time::OffsetDateTime::now_utc()
        .to_offset(offset)
        .date()
        .to_string()
}

/// How far a one-shot is `seconds` in: 0 until `delay`, 1 once `span` more
/// has passed.
fn phase(seconds: f32, delay: f32, span: f32) -> f32 {
    ((seconds - delay) / span).clamp(0., 1.)
}

/// An entrance that passes its place a little before it settles there.
fn spring(t: f32) -> f32 {
    theme::bezier(0.34, 1.56, 0.64, 1., t)
}

/// The star's size (1 at rest) and turn (radians): it spins in from small on
/// `pop`, swells for a moment as it fills or empties (`lit`) and once more
/// when pressed (`burst`). All three run 0 to 1.
fn star_pose(pop: f32, lit: f32, burst: f32) -> (f32, f32) {
    let entered = if pop >= 1. {
        1.
    } else {
        0.3 + 0.7 * spring(pop)
    };
    let scale = entered * (1. + 0.14 * (PI * lit).sin()) * (1. + 0.3 * (PI * burst).sin());
    let turn = if pop >= 1. {
        0.
    } else {
        // A fifth of a turn: the star arrives as it left.
        (theme::ease_out(pop) - 1.) * TAU / 5.
    };
    (scale, turn)
}

/// A small star `flight` (0 to 1) into its way out of the badge: how far it
/// is from the centre (px) and how visible. It fades in and out on the way.
fn spark(flight: f32) -> Option<(f32, f32)> {
    (flight > 0. && flight < 1.).then(|| (13. + 15. * theme::ease_out(flight), (PI * flight).sin()))
}

impl Desktop {
    /// The start opened the onboarding, or a portable copy's notice came in:
    /// this run does not ask for a star.
    pub(in crate::frontend) fn skip_star_this_run(&mut self) {
        self.ui.star.first_run = true;
    }

    /// Each second: brings the card in on a day it has not shown yet, once
    /// the window has been in view for a while. Not over a dialog, not in
    /// the run of the first start, and not in a run of the task manager
    /// replacement (`holds_back`): the day stays open for a later start.
    pub(in crate::frontend) fn announce_star(&mut self) -> bool {
        if self.options.test
            || self.ui.star.card.is_some()
            || self.ui.star.first_run
            || self.holds_back()
        {
            return false;
        }
        let visible = self
            .host
            .get_window("main")
            .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false));
        let ready = visible
            && self.welcome_checked
            && self.modal.is_none()
            && !self.snapshot.is_null()
            && due(
                self.prefs.get(REMIND, true),
                self.prefs.get::<Option<String>>(SHOWN, None).as_deref(),
                &today(),
            );
        if !ready {
            self.ui.star.ready = None;
            return false;
        }
        if self
            .ui
            .star
            .ready
            .get_or_insert_with(Instant::now)
            .elapsed()
            < SETTLE
        {
            return false;
        }
        self.prefs.set(SHOWN, today());
        self.show_star();
        true
    }

    /// Brings the card in, whatever the day.
    pub(in crate::frontend) fn show_star(&mut self) {
        let now = Instant::now();
        self.ui.star.ready = None;
        self.ui.star.card = Some(Card {
            born: now,
            drawn: Cell::new(None),
            expires: Some(now + STAY),
            leaving: None,
            lit: false,
            starred: None,
        });
    }

    /// Sends the card away, if it shows.
    pub(in crate::frontend) fn dismiss_star(&mut self) {
        if let Some(card) = self.ui.star.card.as_mut().filter(|c| c.leaving.is_none()) {
            card.leaving = Some(Instant::now());
        }
    }

    /// The quarter-second clock: the card leaves when its time is up or
    /// shortly after its star was pressed, and is dropped once it is gone.
    pub(in crate::frontend) fn tick_star(&mut self) -> bool {
        let now = Instant::now();
        let Some(card) = &mut self.ui.star.card else {
            return false;
        };
        if let Some(at) = card.leaving {
            let gone = now.duration_since(at).as_secs_f32() >= LEAVE;
            if gone {
                self.ui.star.card = None;
            }
            return gone;
        }
        let over = card
            .starred
            .is_some_and(|at| now.duration_since(at) >= STARRED)
            || card.expires.is_some_and(|at| at <= now);
        if over {
            card.leaving = Some(now);
        }
        over
    }

    /// What a test reads of the reminder: whether its card shows and whether
    /// it is on.
    pub(in crate::frontend) fn star_test_state(&self) -> Value {
        json!({
            "shown": self.ui.star.card.as_ref().is_some_and(|card| card.leaving.is_none()),
            "remind": self.prefs.get(REMIND, true),
        })
    }

    /// Opens the repository and lights the star; the card leaves a moment
    /// later. The request was answered, so the reminder turns off.
    fn star_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self
            .ui
            .star
            .card
            .as_mut()
            .filter(|c| c.leaving.is_none() && c.starred.is_none())
        else {
            return;
        };
        card.starred = Some(Instant::now());
        card.expires = None;
        self.prefs.set(REMIND, false);
        self.request("open_repository", json!({}), None, window, cx);
        cx.notify();
    }

    /// Turns the reminder off and says where it can be turned on again.
    fn star_quiet(&mut self, cx: &mut Context<Self>) {
        if !self
            .ui
            .star
            .card
            .as_ref()
            .is_some_and(|c| c.leaving.is_none() && c.starred.is_none())
        {
            return;
        }
        self.prefs.set(REMIND, false);
        self.dismiss_star();
        self.toast_with(
            ToastKind::Info,
            label("shell.star.off"),
            8000,
            vec![(
                label("shell.star.settings"),
                Rc::new(
                    |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
                        this.locate_setting("update", "starReminder", window, cx)
                    },
                ),
            )],
        );
        cx.notify();
    }

    /// A button of the card, with its place recorded for tests.
    fn star_button(&self, button: Button, anchor: &'static str) -> Button {
        if !self.options.test {
            return button;
        }
        let anchors = self.ui.anchors.clone();
        button.child(
            canvas(
                move |bounds, _, _| {
                    anchors.borrow_mut().insert(anchor.into(), bounds);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
    }

    /// The card, and whether it is still moving (the stack then asks for the
    /// next frame).
    pub(in crate::frontend) fn star_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, bool)> {
        let card = self.ui.star.card.as_ref()?;
        let p = self.palette;
        let reduce = cx.reduce_motion();
        let drawn = card.drawn.get().unwrap_or_else(|| {
            let now = Instant::now();
            card.drawn.set(Some(now));
            now
        });
        let age = drawn.elapsed().as_secs_f32();
        // A part's one-shot `delay` seconds after the card came in.
        let part = |delay: f32, span: f32| {
            if reduce { 1. } else { phase(age, delay, span) }
        };
        let leave = card.leaving.map(|at| {
            if reduce {
                1.
            } else {
                phase(at.elapsed().as_secs_f32(), 0., LEAVE)
            }
        });
        if leave == Some(1.) {
            return None;
        }
        let pressed = card
            .starred
            .filter(|_| !reduce)
            .map(|at| at.elapsed().as_secs_f32());
        let burst = pressed.map_or(0., |seconds| phase(seconds, 0., 0.5));
        let last_spark = SPARKS.iter().fold(0f32, |late, spark| late.max(spark.1)) + FLIGHT;
        let moving = !reduce
            && (age < SPARKS_AT + last_spark
                || leave.is_some()
                || pressed.is_some_and(|seconds| seconds < last_spark));
        let entered = theme::ease_out(part(0., ENTER));
        let left = leave.map(theme::ease_in).unwrap_or(0.);
        // The title, the text and the buttons follow the card in, one after another.
        let reveal = |delay: f32| theme::ease_out(part(delay, 0.3));
        let lit = gpui_kit::base::motion::transition(
            "star-lit",
            if card.lit || card.starred.is_some() {
                1f32
            } else {
                0.
            },
            theme::fluent(if reduce { 0 } else { 220 }, theme::ease_out),
            window,
            cx,
        );
        let surface = widgets::solid(p.flyout);
        let (scale, turn) = star_pose(part(0.12, 0.5), lit, burst);
        let star = |name: &str, alpha: f32| {
            (alpha > 0.).then(|| {
                let star = icon(name, p.io.opacity(alpha))
                    .size(px(22.))
                    .absolute()
                    .top(px(9.))
                    .left(px(9.));
                // At rest it is drawn as it is, on whole pixels.
                if (scale - 1.).abs() < 0.001 && turn.abs() < 0.001 {
                    star
                } else {
                    star.transform(
                        Transformation::scale(size(scale, scale)).with_rotation(radians(turn)),
                    )
                }
            })
        };
        let sparks = SPARKS.iter().filter_map(|(direction, late)| {
            let flight = match pressed {
                Some(seconds) => phase(seconds, *late, FLIGHT),
                None => part(SPARKS_AT + late, FLIGHT),
            };
            let (distance, alpha) = spark(flight)?;
            const SIZE: f32 = 7.;
            Some(
                icon("StarFilledIcon", p.io.opacity(alpha))
                    .size(px(SIZE))
                    .absolute()
                    .left(px(20. + distance * direction.cos() - SIZE / 2.))
                    .top(px(20. + distance * direction.sin() - SIZE / 2.))
                    .transform(Transformation::rotate(radians(flight * 1.4))),
            )
        });
        let badge = div()
            .relative()
            .size(px(40.))
            .flex_shrink_0()
            .rounded(px(10.))
            .bg(theme::mix(surface, p.io, 0.14 + 0.12 * lit))
            .children(star("StarIcon", 1. - lit))
            .children(star("StarFilledIcon", lit))
            .children(sparks);
        let row = |delay: f32| {
            let shown = reveal(delay);
            div().relative().top(px((1. - shown) * 6.)).opacity(shown)
        };
        let words = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                row(0.1)
                    // Clear of the close button.
                    .pr(px(22.))
                    .text_size(theme::font(14.))
                    .font_weight(theme::LABEL_WEIGHT)
                    .child(label("shell.star.title")),
            )
            .child(
                row(0.16)
                    .line_height(relative(1.5))
                    .text_color(p.muted)
                    .child(label("shell.star.body")),
            );
        let open = self
            .star_button(
                self.btn(
                    "star-open",
                    label("shell.star.open"),
                    Some(if card.lit || card.starred.is_some() {
                        "StarFilledIcon"
                    } else {
                        "StarIcon"
                    }),
                    Tone::Accent,
                ),
                "star-open",
            )
            .flex_1()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if let Some(card) = &mut this.ui.star.card {
                    card.lit = *hovered;
                }
                cx.notify();
            }))
            .on_click(cx.listener(|this, _, window, cx| this.star_open(window, cx)));
        let quiet = self
            .star_button(
                self.btn("star-quiet", label("shell.star.quiet"), None, Tone::Plain),
                "star-quiet",
            )
            .on_click(cx.listener(|this, _, _, cx| this.star_quiet(cx)));
        let close = self
            .star_button(
                self.btn_small("star-close", "", Some("DismissIcon"), Tone::Ghost),
                "star-close",
            )
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .accessibility_label(label("common.close"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.dismiss_star();
                cx.notify();
            }));
        let element = div()
            .id("star-card")
            .occlude()
            .relative()
            .top(px((1. - entered) * 20.))
            .left(px(left * 24.))
            .opacity(entered * (1. - left))
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(16.))
            .border_1()
            .border_color(p.stroke_strong)
            .rounded(px(8.))
            // No shadow, as the other notices.
            .bg(surface)
            .text_color(p.ink)
            .text_size(theme::font(13.))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                // Held while the pointer is on it.
                if let Some(card) = this.ui.star.card.as_mut().filter(|c| c.starred.is_none()) {
                    card.expires =
                        (!*hovered).then(|| (card.born + STAY).max(Instant::now() + LINGER));
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(badge)
                    .child(words),
            )
            .child(
                row(0.22)
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(open)
                    .child(quiet),
            )
            .child(close)
            .into_any_element();
        Some((element, moving))
    }
}

#[cfg(test)]
mod tests {
    use super::{due, phase, spark, star_pose};

    #[test]
    fn the_card_is_due_once_per_day_while_the_reminder_is_on() {
        assert!(due(true, None, "2026-11-10"));
        assert!(!due(true, Some("2026-11-10"), "2026-11-10"));
        assert!(due(true, Some("2026-11-09"), "2026-11-10"));
        // A clock set back is another day too.
        assert!(due(true, Some("2026-11-11"), "2026-11-10"));
        for shown in [None, Some("2026-11-09"), Some("2026-11-10")] {
            assert!(!due(false, shown, "2026-11-10"));
        }
    }

    #[test]
    fn a_one_shot_waits_for_its_delay_and_ends_after_its_span() {
        assert_eq!(phase(0., 0.1, 0.3), 0.);
        assert_eq!(phase(0.1, 0.1, 0.3), 0.);
        assert!((phase(0.25, 0.1, 0.3) - 0.5).abs() < 1e-6);
        assert_eq!(phase(0.4, 0.1, 0.3), 1.);
        assert_eq!(phase(9., 0.1, 0.3), 1.);
    }

    #[test]
    fn the_star_rests_untransformed_whether_lit_or_not() {
        // Drawn on whole pixels at rest: exactly its size and upright.
        for lit in [0., 1.] {
            for burst in [0., 1.] {
                let (scale, turn) = star_pose(1., lit, burst);
                assert!((scale - 1.).abs() < 1e-6);
                assert_eq!(turn, 0.);
            }
        }
        // It comes in small and turned, passes its size and swells when pressed.
        let (scale, turn) = star_pose(0., 0., 0.);
        assert!(scale < 0.5 && turn < -1.);
        assert!((0..100).any(|step| star_pose(step as f32 / 100., 0., 0.).0 > 1.02));
        assert!(star_pose(1., 1., 0.5).0 > 1.2);
        assert!(star_pose(1., 0.5, 0.).0 > 1.1);
    }

    #[test]
    fn a_small_star_is_drawn_only_while_it_flies() {
        assert!(spark(0.).is_none());
        assert!(spark(1.).is_none());
        let (near, _) = spark(0.05).unwrap();
        let (far, _) = spark(0.95).unwrap();
        assert!(near < far && far <= 28.);
        let (_, visible) = spark(0.5).unwrap();
        assert!(visible > 0.99);
    }
}
