//! The CPU view's transitions, drawn with live transforms (`shift::live`): the
//! page is laid out once in its new arrangement and replays that paint while
//! the transforms move, scale and fade its parts, so that no frame of a
//! transition renders or lays out the page again (the chart layer draws the
//! curves inside the same transforms).
//!
//! Another graph (overall or per logical processor) cross-fades in: the
//! outgoing view fades out quickly where it was while the incoming one fades
//! in from a slight zoom. Another core type rearranges the logical
//! processors' cards: cards that stay glide to their new place and size (their
//! frame and curve stretch, their captions move with its corners), cards that
//! go fade out and shrink a little where they were, and cards that come grow
//! in at theirs. Another size of the grid moves the cards along the same way
//! on the theme's transition.
use std::rc::Rc;
use std::time::Duration;
#[cfg(test)]
use std::time::Instant;

use gpui_kit::{App, Bounds, LiveTransform, LiveTransformValue, Pixels, point, px, size};

use super::super::super::{shift::LiveMotions, theme};

/// The incoming view's fade and zoom.
const VIEW_IN: Duration = Duration::from_millis(220);
/// The outgoing view's fade: quick and linear (the theme's `t-fast`), so
/// that the two views overlap only briefly; an accelerating exit would hold
/// the outgoing view over the incoming one for most of its fade.
const VIEW_OUT: Duration = Duration::from_millis(83);
/// The incoming view's scale when it starts (the interface's settle animation).
const VIEW_ZOOM: f32 = 0.985;
/// A card's glide to its new place and size.
const MOVE: Duration = Duration::from_millis(260);
/// A card coming in, shortly after the cards going start to fade: later,
/// another core type (none of the cards staying) would leave the grid
/// nearly empty for a few frames between the two.
const ENTER: Duration = Duration::from_millis(230);
const ENTER_DELAY: Duration = Duration::from_millis(30);
/// A card going: as quick and linear as the outgoing view.
const LEAVE: Duration = Duration::from_millis(83);
const VIEW_KEY: &str = "cpu-view";
const CORES_KEY: &str = "cpu-cores";

fn ms(duration: Duration) -> f32 {
    duration.as_secs_f32() * 1000.
}
/// `curve` at `t`, exactly 0 before the start and 1 at the end.
fn eased(curve: fn(f32) -> f32, t: f32) -> f32 {
    if t <= 0. {
        0.
    } else if t >= 1. {
        1.
    } else {
        curve(t)
    }
}

/// The chart area's graph and its switches.
#[derive(Default)]
pub(super) struct ViewSwitch {
    /// The graph shown (per logical processor or not); `None` before the first.
    shown: Option<bool>,
    /// The graph going while a switch runs.
    leaving: Option<bool>,
    /// The logical processors' cards as drawn when they started to go.
    pub(super) cards: Vec<(usize, Card)>,
    /// The incoming graph's zoom and fade.
    pub(super) area: LiveTransform,
    /// The outgoing graph's fade.
    pub(super) ghost: LiveTransform,
    /// The incoming graph had nothing to draw yet (a grid waiting for its
    /// size): the switch begins again once it has.
    waiting: bool,
}
impl ViewSwitch {
    /// The graph going while `logical` comes in, if a switch runs. A first
    /// graph shows at once, as does any switch under reduced motion.
    /// `cards` gives the cards as drawn, for a grid that starts to go.
    pub(super) fn update(
        &mut self,
        logical: bool,
        animate: bool,
        motions: &LiveMotions,
        cards: impl FnOnce() -> Vec<(usize, Card)>,
        done: impl FnOnce(&mut App) + 'static,
    ) -> Option<bool> {
        let previous = self.shown.replace(logical);
        if previous.is_some_and(|shown| shown != logical) {
            if animate {
                self.leaving = previous;
                self.cards = if previous == Some(true) {
                    cards()
                } else {
                    Vec::new()
                };
                // Fresh transforms: elements of an earlier switch keep theirs.
                self.area = LiveTransform::default();
                self.ghost = LiveTransform::default();
                self.waiting = false;
                self.begin(motions, done);
            } else {
                motions.stop(VIEW_KEY);
            }
        }
        if motions.elapsed(VIEW_KEY).is_none() {
            self.waiting = false;
            self.leaving = None;
            self.cards.clear();
        }
        self.leaving
    }

    /// The incoming graph has nothing to draw yet: its switch waits for it.
    pub(super) fn wait(&mut self) {
        self.waiting |= self.leaving.is_some();
    }

    /// The incoming graph can be drawn: a switch that waited for it begins
    /// again, from the frame after the one that first draws it.
    pub(super) fn ready(&mut self, motions: &LiveMotions, done: impl FnOnce(&mut App) + 'static) {
        if std::mem::take(&mut self.waiting) && self.leaving.is_some() {
            self.begin(motions, done);
        }
    }

    fn begin(&self, motions: &LiveMotions, done: impl FnOnce(&mut App) + 'static) {
        let (area, ghost) = (self.area.clone(), self.ghost.clone());
        motions.start(
            VIEW_KEY,
            VIEW_IN,
            move |elapsed| {
                let (incoming, outgoing) = view_values(elapsed);
                area.set(incoming);
                ghost.set(outgoing);
            },
            done,
        );
    }
}
/// The incoming and outgoing graphs `elapsed` into a switch.
fn view_values(elapsed: Duration) -> (LiveTransformValue, LiveTransformValue) {
    let elapsed = ms(elapsed);
    let incoming = eased(theme::ease_out, elapsed / ms(VIEW_IN));
    let outgoing = 1. - (elapsed / ms(VIEW_OUT)).clamp(0., 1.);
    (
        LiveTransformValue::zoom(lerp(VIEW_ZOOM, 1., incoming), incoming),
        LiveTransformValue {
            opacity: outgoing.max(0.),
            ..LiveTransformValue::default()
        },
    )
}

/// A logical processor's card as drawn: where, and how far it has come in
/// (0 gone, 1 in place; its opacity, and its scale from the theme's
/// `coreEnterScale`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Card {
    pub(super) bounds: Bounds<Pixels>,
    pub(super) presence: f32,
}

/// The live transforms a card in motion is drawn with: the whole card
/// (zoom and fade about its centre), its caption's two ends (moved with the
/// card's top corners) and its chart frame (moved and stretched to the card).
#[derive(Clone, Default)]
pub(super) struct CardLives {
    pub(super) card: LiveTransform,
    pub(super) left: LiveTransform,
    pub(super) right: LiveTransform,
    pub(super) frame: LiveTransform,
}

/// What rearranges the cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reflow {
    /// Another size of the grid: the cards follow it on the theme's transition.
    Resize,
    /// Another core type: cards glide, go and come.
    Filter,
}

/// A card in a rearrangement.
pub(super) struct Moving {
    pub(super) index: usize,
    from: Card,
    to: Card,
    /// Where the card is laid out meanwhile: where it goes, or where it was
    /// for a card that goes.
    pub(super) layout: Bounds<Pixels>,
    pub(super) lives: CardLives,
}

/// The cards moving from where they were drawn to a new arrangement.
pub(super) struct Rearrangement {
    reflow: Reflow,
    pub(super) cards: Vec<Moving>,
    /// The height of a card's caption, above its chart frame.
    header: Pixels,
    enter_scale: f32,
}
impl Rearrangement {
    fn new(
        reflow: Reflow,
        drawn: &[(usize, Card)],
        targets: &[(usize, Bounds<Pixels>)],
        header: Pixels,
        enter_scale: f32,
    ) -> Self {
        let mut cards = Vec::new();
        for (index, bounds) in targets {
            let from = drawn
                .iter()
                .find(|(i, card)| i == index && card.presence > 0.001)
                .map_or(
                    Card {
                        bounds: *bounds,
                        presence: 0.,
                    },
                    |(_, card)| *card,
                );
            cards.push(Moving {
                index: *index,
                from,
                to: Card {
                    bounds: *bounds,
                    presence: 1.,
                },
                layout: *bounds,
                lives: CardLives::default(),
            });
        }
        for (index, card) in drawn {
            if card.presence > 0.001 && !targets.iter().any(|(i, _)| i == index) {
                cards.push(Moving {
                    index: *index,
                    from: *card,
                    to: Card {
                        bounds: card.bounds,
                        presence: 0.,
                    },
                    layout: card.bounds,
                    lives: CardLives::default(),
                });
            }
        }
        cards.sort_by_key(|card| card.index);
        Self {
            reflow,
            cards,
            header,
            enter_scale,
        }
    }

    fn duration(&self) -> Duration {
        match self.reflow {
            Reflow::Filter => MOVE.max(ENTER_DELAY + ENTER).max(LEAVE),
            Reflow::Resize => theme::motion_duration(),
        }
    }

    /// A card `elapsed` into the rearrangement.
    fn sample(&self, card: &Moving, elapsed: Duration) -> Card {
        let (from, to) = (card.from, card.to);
        let elapsed = ms(elapsed);
        let (moved, came) = match self.reflow {
            Reflow::Resize => {
                let t = eased(
                    gpui_kit::base::animation::ease_out_cubic,
                    elapsed / ms(theme::motion_duration()),
                );
                (t, t)
            }
            Reflow::Filter => (
                eased(theme::ease_move, elapsed / ms(MOVE)),
                if to.presence > from.presence {
                    eased(theme::ease_out, (elapsed - ms(ENTER_DELAY)) / ms(ENTER))
                } else if to.presence < from.presence {
                    (elapsed / ms(LEAVE)).clamp(0., 1.)
                } else {
                    1.
                },
            ),
        };
        Card {
            bounds: lerp_bounds(from.bounds, to.bounds, moved),
            presence: lerp(from.presence, to.presence, came),
        }
    }

    /// The cards as drawn `elapsed` into the rearrangement (those gone left out).
    pub(super) fn drawn(&self, elapsed: Duration) -> Vec<(usize, Card)> {
        self.cards
            .iter()
            .map(|card| (card.index, self.sample(card, elapsed)))
            .filter(|(_, card)| card.presence > 0.001)
            .collect()
    }

    /// Sets the cards' live transforms for `elapsed`.
    fn apply(&self, elapsed: Duration) {
        let header = self.header;
        let frame = |bounds: Bounds<Pixels>| {
            Bounds::new(
                point(bounds.origin.x, bounds.origin.y + header),
                size(bounds.size.width, (bounds.size.height - header).max(px(1.))),
            )
        };
        for card in &self.cards {
            let drawn = self.sample(card, elapsed);
            let laid = card.layout;
            let scale = lerp(self.enter_scale, 1., drawn.presence);
            card.lives.card.set(LiveTransformValue::zoom(
                scale,
                drawn.presence.clamp(0., 1.),
            ));
            let moved = |offset: gpui_kit::Point<Pixels>| LiveTransformValue {
                offset,
                ..LiveTransformValue::default()
            };
            card.lives
                .left
                .set(moved(drawn.bounds.origin - laid.origin));
            card.lives.right.set(moved(point(
                drawn.bounds.right() - laid.right(),
                drawn.bounds.top() - laid.top(),
            )));
            let (to, from) = (frame(drawn.bounds), frame(laid));
            card.lives.frame.set(LiveTransformValue {
                offset: to.origin - from.origin,
                scale_x: f32::from(to.size.width) / f32::from(from.size.width).max(1.),
                scale_y: f32::from(to.size.height) / f32::from(from.size.height).max(1.),
                ..LiveTransformValue::default()
            });
        }
    }
}

/// The logical processors' cards and their rearrangements.
#[derive(Default)]
pub(super) struct CoreMotion {
    /// The core type shown; `None` before the grid's first display.
    filter: Option<String>,
    /// Whether the grid has been laid out at its real size in this display:
    /// until then the cards take their places at once.
    settled: bool,
    /// Where the cards belong.
    targets: Vec<(usize, Bounds<Pixels>)>,
    moving: Option<Rc<Rearrangement>>,
}
impl CoreMotion {
    /// The rearrangement to draw, if one runs, given where the cards of
    /// `filter` belong (`targets`) and whether the grid has its real size
    /// (`measured`). A first display, or reduced motion, places the cards at
    /// once; another core type or another arrangement starts a
    /// rearrangement from where the cards are drawn.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(
        &mut self,
        filter: &str,
        targets: &[(usize, Bounds<Pixels>)],
        measured: bool,
        animate: bool,
        header: Pixels,
        enter_scale: f32,
        motions: &LiveMotions,
        done: impl FnOnce(&mut App) + 'static,
    ) -> Option<Rc<Rearrangement>> {
        if motions.elapsed(CORES_KEY).is_none() {
            self.moving = None;
        }
        let first = self.filter.is_none();
        let filtered = self.filter.as_deref().is_some_and(|shown| shown != filter);
        self.filter = Some(filter.to_owned());
        if first || !self.settled || !animate {
            self.settled |= measured;
            motions.stop(CORES_KEY);
            self.moving = None;
            self.targets = targets.to_vec();
            return None;
        }
        let reflow = if filtered {
            Some(Reflow::Filter)
        } else if self.targets != targets {
            Some(Reflow::Resize)
        } else {
            None
        };
        if let Some(reflow) = reflow {
            let drawn = self.drawn(motions);
            let moving = Rc::new(Rearrangement::new(
                reflow,
                &drawn,
                targets,
                header,
                enter_scale,
            ));
            self.targets = targets.to_vec();
            self.moving = Some(moving.clone());
            let stepped = moving.clone();
            motions.start(
                CORES_KEY,
                moving.duration(),
                move |elapsed| stepped.apply(elapsed),
                done,
            );
        }
        self.moving.clone()
    }

    /// The grid turned out to have the size its first display was laid out at.
    pub(super) fn measured(&mut self) {
        if self.filter.is_some() {
            self.settled = true;
        }
    }

    /// The cards as drawn now.
    pub(super) fn drawn(&self, motions: &LiveMotions) -> Vec<(usize, Card)> {
        match (&self.moving, motions.elapsed(CORES_KEY)) {
            (Some(moving), Some(elapsed)) => moving.drawn(elapsed),
            _ => self
                .targets
                .iter()
                .map(|(index, bounds)| {
                    (
                        *index,
                        Card {
                            bounds: *bounds,
                            presence: 1.,
                        },
                    )
                })
                .collect(),
        }
    }

    /// Forgets the grid (no longer shown): its next display is a first one.
    pub(super) fn reset(&mut self, motions: &LiveMotions) {
        if self.filter.is_some() {
            motions.stop(CORES_KEY);
            *self = Self::default();
        }
    }
}

/// From `from` toward `to` by `t`, exactly `to` at the end.
fn lerp(from: f32, to: f32, t: f32) -> f32 {
    if t >= 1. { to } else { from + (to - from) * t }
}
fn lerp_bounds(from: Bounds<Pixels>, to: Bounds<Pixels>, t: f32) -> Bounds<Pixels> {
    let at = |a: Pixels, b: Pixels| px(lerp(f32::from(a), f32::from(b), t));
    Bounds::new(
        point(
            at(from.origin.x, to.origin.x),
            at(from.origin.y, to.origin.y),
        ),
        size(
            at(from.size.width, to.size.width),
            at(from.size.height, to.size.height),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(0.)), size(px(100.), px(50.)))
    }
    fn update(
        motion: &mut CoreMotion,
        motions: &LiveMotions,
        filter: &str,
        targets: &[(usize, Bounds<Pixels>)],
    ) -> Option<Rc<Rearrangement>> {
        motion.update(filter, targets, true, true, px(20.), 0.96, motions, |_| {})
    }
    /// Frames at 165 Hz, as the chart layer steps the animations.
    struct Frames {
        now: Instant,
    }
    impl Frames {
        fn new() -> Self {
            Self {
                now: Instant::now(),
            }
        }
        /// Draws frames for `ms` milliseconds.
        fn run(&mut self, motions: &LiveMotions, ms: u64) {
            let end = self.now + Duration::from_millis(ms);
            while self.now < end {
                self.now += Duration::from_micros(6_061);
                motions.step(self.now);
            }
        }
    }

    #[test]
    fn first_display_and_reduced_motion_place_cards_at_once() {
        let motions = LiveMotions::default();
        let mut motion = CoreMotion::default();
        assert!(update(&mut motion, &motions, "all", &[(0, bounds(0.))]).is_none());
        let moving = motion.update(
            "p",
            &[(1, bounds(0.))],
            true,
            false,
            px(20.),
            0.96,
            &motions,
            |_| {},
        );
        assert!(moving.is_none() && !motions.active());
        assert_eq!(
            motion.drawn(&motions),
            vec![(
                1,
                Card {
                    bounds: bounds(0.),
                    presence: 1.
                }
            )]
        );
    }

    #[test]
    fn another_filter_moves_stays_leaves_and_enters() {
        let motions = LiveMotions::default();
        let mut frames = Frames::new();
        let mut motion = CoreMotion::default();
        let all = [(0, bounds(0.)), (1, bounds(110.))];
        update(&mut motion, &motions, "all", &all);
        // Card 1 stays and moves to the left; card 0 goes; card 2 comes.
        let only = [(1, bounds(0.)), (2, bounds(110.))];
        let moving = update(&mut motion, &motions, "p", &only).expect("rearrangement");
        let card = |index: usize| moving.cards.iter().find(|c| c.index == index).unwrap();
        // Laid out where they go (where it was, for the card going).
        assert_eq!(card(1).layout, bounds(0.));
        assert_eq!(card(0).layout, bounds(0.));
        assert_eq!(card(2).layout, bounds(110.));
        // The change's own frame draws the cards as they were.
        assert_eq!(card(1).lives.left.get().offset.x, px(110.));
        assert_eq!(card(2).lives.card.get().opacity, 0.);
        assert_eq!(card(0).lives.card.get().opacity, 1.);
        // It begins at the next frame.
        frames.run(&motions, 1);
        assert_eq!(card(1).lives.left.get().offset.x, px(110.));
        frames.run(&motions, 130);
        let left = card(1).lives.left.get().offset.x;
        assert!(left > px(0.) && left < px(110.));
        let coming = card(2).lives.card.get();
        assert!(coming.opacity > 0. && coming.opacity < 1. && coming.scale_x < 1.);
        assert_eq!(card(0).lives.card.get().opacity, 0.);
        assert!(motion.drawn(&motions).iter().all(|(index, _)| *index != 0));
        frames.run(&motions, 300);
        assert!(!motions.active());
        assert_eq!(card(1).lives.left.get(), LiveTransformValue::default());
        assert_eq!(card(1).lives.frame.get(), LiveTransformValue::default());
        assert_eq!(card(2).lives.card.get(), LiveTransformValue::default());
        assert!(update(&mut motion, &motions, "p", &only).is_none());
    }

    #[test]
    fn a_slow_frame_delays_rather_than_skips() {
        let motions = LiveMotions::default();
        let mut motion = CoreMotion::default();
        update(&mut motion, &motions, "all", &[(0, bounds(0.))]);
        let moving = update(&mut motion, &motions, "p", &[(0, bounds(110.))]).unwrap();
        let start = Instant::now();
        motions.step(start);
        // A frame of 100 ms advances it by one 60 Hz refresh at most.
        motions.step(start + Duration::from_millis(100));
        assert_eq!(motions.elapsed(CORES_KEY), Some(MAX_STEP_FOR_TESTS));
        assert!(moving.cards[0].lives.left.get().offset.x < px(0.));
    }

    #[test]
    fn a_gliding_frame_stretches_to_the_card() {
        let motions = LiveMotions::default();
        let mut motion = CoreMotion::default();
        let small = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(60.)));
        let large = Bounds::new(point(px(0.), px(0.)), size(px(200.), px(120.)));
        update(&mut motion, &motions, "all", &[(0, small)]);
        let moving = update(&mut motion, &motions, "p", &[(0, large)]).unwrap();
        let frame = moving.cards[0].lives.frame.get();
        // Laid out large; drawn as the small card's frame (below a 20 px caption).
        assert_eq!(frame.scale_x, 0.5);
        assert_eq!(frame.scale_y, 40. / 100.);
        assert_eq!(frame.lengths, 1.);
        assert_eq!(moving.cards[0].lives.right.get().offset.x, px(-100.));
    }

    #[test]
    fn view_switch_cross_fades_and_zooms() {
        let motions = LiveMotions::default();
        let mut frames = Frames::new();
        let mut switch = ViewSwitch::default();
        assert_eq!(switch.update(false, true, &motions, Vec::new, |_| {}), None);
        assert_eq!(
            switch.update(true, true, &motions, Vec::new, |_| {}),
            Some(false)
        );
        let area = switch.area.get();
        assert!(area.opacity == 0. && (area.scale_x - VIEW_ZOOM).abs() < 1e-6);
        assert_eq!(switch.ghost.get().opacity, 1.);
        frames.run(&motions, 60);
        let area = switch.area.get();
        assert!(area.opacity > 0. && area.opacity < 1.);
        assert!(area.scale_x > VIEW_ZOOM && area.scale_x < 1.);
        let ghost = switch.ghost.get().opacity;
        assert!(ghost > 0. && ghost < 1.);
        frames.run(&motions, 300);
        assert_eq!(switch.area.get(), LiveTransformValue::default());
        assert_eq!(switch.ghost.get().opacity, 0.);
        assert_eq!(switch.update(true, true, &motions, Vec::new, |_| {}), None);
        // Reduced motion switches at once.
        assert_eq!(
            switch.update(false, false, &motions, Vec::new, |_| {}),
            None
        );
        assert!(!motions.active());
    }

    #[test]
    fn a_switch_waits_for_a_grid_without_cards() {
        let motions = LiveMotions::default();
        let mut frames = Frames::new();
        let mut switch = ViewSwitch::default();
        switch.update(false, true, &motions, Vec::new, |_| {});
        switch.update(true, true, &motions, Vec::new, |_| {});
        switch.wait();
        frames.run(&motions, 30);
        assert!(switch.area.get().opacity > 0.);
        // The cards are drawn: it begins again from nothing.
        switch.ready(&motions, |_| {});
        assert_eq!(switch.area.get().opacity, 0.);
        frames.run(&motions, 60);
        assert!(switch.area.get().opacity > 0.);
        // Once only.
        let before = switch.area.get();
        switch.ready(&motions, |_| {});
        assert_eq!(switch.area.get(), before);
    }

    const MAX_STEP_FOR_TESTS: Duration = Duration::from_micros(16_667);
}
