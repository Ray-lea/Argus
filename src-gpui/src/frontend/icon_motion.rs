//! Icons that move: a control's icon plays one short motion when the pointer
//! comes onto the control, and comes in anew when the control swaps it for
//! another (pause for play).
//!
//! An icon is one rasterized sprite, so a motion is a transform of the whole
//! icon: a turn, a swing, a nudge, a swell or a squash. Every motion ends on
//! the icon exactly as it rests, drawn untransformed on whole pixels, and
//! none repeats. Under reduced motion nothing starts.
use super::*;
use std::f32::consts::PI;

/// What an icon does when its control is pointed at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Motion {
    /// Turns from this many degrees back to its place. The icon looks the
    /// same at the start: a whole turn, or one of its own symmetries.
    Spin(f32),
    /// Swings this many degrees and back.
    Swing(f32),
    /// Moves this far (px) and back.
    Nudge(f32, f32),
    /// Swells a little and settles.
    Pop,
    /// Closes and opens, as an eye.
    Blink,
    /// Comes in for the icon it replaced.
    Enter,
}

/// How an icon is drawn at one moment of its motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pose {
    pub scale: (f32, f32),
    /// Clockwise, in radians.
    pub turn: f32,
    pub shift: (f32, f32),
    pub alpha: f32,
}

impl Pose {
    const REST: Pose = Pose {
        scale: (1., 1.),
        turn: 0.,
        shift: (0., 0.),
        alpha: 1.,
    };
}

/// Out quickly and back at leisure: 0 at both ends, 1 a fifth of the way in.
fn there_and_back(t: f32) -> f32 {
    (PI * theme::ease_enter(t)).sin()
}

impl Motion {
    /// The motion of a product icon. Icons with a direction follow it, those
    /// that turn in real life turn, and the rest swell.
    pub(super) fn of(name: &str) -> Self {
        match name {
            // The gear has six teeth: a sixth of a turn brings it onto itself.
            "SettingsIcon" => Self::Spin(-60.),
            "RefreshIcon" | "RestartIcon" => Self::Spin(-360.),
            "ResetIcon" => Self::Spin(360.),
            "HistoryIcon" | "DataIcon" => Self::Swing(-40.),
            "ThemeIcon" => Self::Swing(180.),
            "PinIcon" | "UnpinIcon" => Self::Swing(-18.),
            "TimerIcon" | "SparkleIcon" | "MotionIcon" => Self::Swing(18.),
            "ServicesIcon" => Self::Swing(12.),
            "TrashIcon" => Self::Swing(-10.),
            "UpdateIcon" | "DownloadIcon" | "TrayIcon" | "SortDescendingIcon" | "ChevronDown" => {
                Self::Nudge(0., 2.)
            }
            "SortAscendingIcon" | "ChevronUp" | "TransferIcon" => Self::Nudge(0., -2.),
            "ExternalIcon" | "ExternalLinkIcon" => Self::Nudge(1.5, -1.5),
            "GoToIcon" | "ArrowRightIcon" | "ChevronRight" | "RunTaskIcon" => Self::Nudge(2., 0.),
            "KeyboardIcon" => Self::Nudge(0., 1.),
            "EyeIcon" | "EyeOffIcon" => Self::Blink,
            _ => Self::Pop,
        }
    }

    pub(super) fn duration(self) -> Duration {
        Duration::from_millis(match self {
            Self::Spin(degrees) if degrees.abs() >= 180. => 620,
            Self::Spin(_) => 420,
            Self::Swing(_) => 460,
            Self::Nudge(..) => 420,
            Self::Pop => 320,
            Self::Blink => 240,
            Self::Enter => 260,
        })
    }

    /// The icon `t` (0 to 1) into the motion; at rest once it is over.
    pub(super) fn pose(self, t: f32) -> Pose {
        if t >= 1. {
            return Pose::REST;
        }
        let t = t.max(0.);
        match self {
            Self::Spin(degrees) => Pose {
                turn: degrees.to_radians() * (1. - theme::ease_move(t)),
                ..Pose::REST
            },
            Self::Swing(degrees) => Pose {
                turn: degrees.to_radians() * there_and_back(t),
                ..Pose::REST
            },
            Self::Nudge(x, y) => {
                let far = there_and_back(t);
                Pose {
                    shift: (x * far, y * far),
                    ..Pose::REST
                }
            }
            Self::Pop => {
                let scale = 1. + 0.14 * there_and_back(t);
                Pose {
                    scale: (scale, scale),
                    ..Pose::REST
                }
            }
            Self::Blink => Pose {
                scale: (1., 1. - 0.82 * (PI * t).sin()),
                ..Pose::REST
            },
            Self::Enter => {
                // Passes its size a little before it settles.
                let scale = 0.6 + 0.4 * theme::bezier(0.34, 1.56, 0.64, 1., t);
                Pose {
                    scale: (scale, scale),
                    alpha: theme::ease_out((t / 0.5).min(1.)),
                    ..Pose::REST
                }
            }
        }
    }
}

/// One control's icon: how often it was set moving, and what it last drew.
pub(super) struct Play {
    plays: u64,
    name: String,
    /// The last play was for a new icon rather than for the pointer.
    entered: bool,
}

/// Every control's icon, shared with the controls' hover listeners; and
/// whether motion is reduced, for the icons swapped while drawing.
#[derive(Default)]
pub(super) struct IconMotions {
    plays: Rc<RefCell<HashMap<SharedString, Play>>>,
    still: Cell<bool>,
}

impl IconMotions {
    /// Follows the interface's motion setting.
    pub(super) fn set_still(&self, still: bool) {
        self.still.set(still);
    }
}

impl Desktop {
    /// The icon of the control `control`, drawn through the motion it was
    /// last set on (`icon_probe`). Without a colour it takes the text
    /// colour around it.
    pub(in crate::frontend) fn moving_icon(
        &self,
        control: &SharedString,
        name: &str,
        color: Option<Hsla>,
        side: Option<Pixels>,
    ) -> AnyElement {
        let still = self.ui.icon_motions.still.get();
        let (plays, entered) = {
            let mut all = self.ui.icon_motions.plays.borrow_mut();
            // Notices and dialogs bring controls under ever new names: start
            // over rather than keep every one of them.
            if all.len() > 512 && !all.contains_key(control) {
                all.clear();
            }
            let play = all.entry(control.clone()).or_insert_with(|| Play {
                plays: 0,
                name: name.to_owned(),
                entered: false,
            });
            // The control shows another icon than it did: that one comes in.
            if play.name != name {
                play.name = name.to_owned();
                if !still {
                    play.plays += 1;
                    play.entered = true;
                }
            }
            (play.plays, play.entered)
        };
        let glyph = match color {
            Some(color) => icon(name, color),
            None => theme::glyph(name),
        };
        let glyph = match side {
            Some(side) => glyph.size(side),
            None => glyph,
        };
        if plays == 0 || still {
            return glyph.into_any_element();
        }
        let motion = if entered {
            Motion::Enter
        } else {
            Motion::of(name)
        };
        // A new id for each play: the animation starts over.
        glyph
            .with_animation(
                ElementId::NamedInteger("icon-motion".into(), plays),
                Animation::new(motion.duration()),
                move |glyph, t| {
                    let pose = motion.pose(t);
                    if pose == Pose::REST {
                        return glyph;
                    }
                    let glyph = glyph.transform(
                        Transformation::scale(size(pose.scale.0, pose.scale.1))
                            .with_rotation(radians(pose.turn))
                            .with_translation(point(px(pose.shift.0), px(pose.shift.1))),
                    );
                    match color {
                        Some(color) if pose.alpha < 1. => {
                            glyph.text_color(color.opacity(pose.alpha))
                        }
                        _ => glyph,
                    }
                },
            )
            .into_any_element()
    }

    /// A cover the control `control` lays over itself (it must be
    /// `relative`): the pointer coming onto it sets the control's icon
    /// moving. The control draws again by its own hover style.
    pub(in crate::frontend) fn icon_probe(&self, control: &SharedString) -> Stateful<Div> {
        let plays = self.ui.icon_motions.plays.clone();
        let control = control.clone();
        div()
            .id("icon-probe")
            .absolute()
            .inset_0()
            // Where tests take hold of the control, under its own name.
            .when(self.options.test, |probe| {
                let anchors = self.ui.anchors.clone();
                let control = control.clone();
                probe.child(
                    canvas(
                        move |bounds, _, _| {
                            anchors.borrow_mut().insert(control.clone(), bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
            })
            .on_hover(move |hovered, _, cx| {
                if *hovered
                    && !cx.reduce_motion()
                    && let Some(play) = plays.borrow_mut().get_mut(&control)
                {
                    play.plays += 1;
                    play.entered = false;
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{Motion, Pose, there_and_back};

    fn at_rest(pose: Pose) -> bool {
        (pose.scale.0 - 1.).abs() < 1e-4
            && (pose.scale.1 - 1.).abs() < 1e-4
            && pose.turn.abs() < 1e-4
            && pose.shift.0.abs() < 1e-4
            && pose.shift.1.abs() < 1e-4
            && pose.alpha > 0.9999
    }

    const ALL: [Motion; 8] = [
        Motion::Spin(-60.),
        Motion::Spin(360.),
        Motion::Swing(-18.),
        Motion::Nudge(2., 0.),
        Motion::Nudge(1.5, -1.5),
        Motion::Pop,
        Motion::Blink,
        Motion::Enter,
    ];

    #[test]
    fn every_motion_ends_on_the_icon_at_rest() {
        for motion in ALL {
            assert_eq!(motion.pose(1.), Pose::REST, "{motion:?}");
            assert_eq!(motion.pose(1.5), Pose::REST, "{motion:?}");
            assert!(motion.duration().as_millis() <= 700, "{motion:?}");
            // Close to rest just before the end: nothing jumps on the last frame.
            let last = motion.pose(0.995);
            assert!((last.scale.0 - 1.).abs() < 0.02, "{motion:?}");
            assert!((last.scale.1 - 1.).abs() < 0.02, "{motion:?}");
            assert!(last.turn.abs() < 0.03, "{motion:?}");
            assert!(
                last.shift.0.abs() < 0.1 && last.shift.1.abs() < 0.1,
                "{motion:?}"
            );
            assert!(last.alpha > 0.99, "{motion:?}");
        }
    }

    #[test]
    fn motions_that_go_and_return_start_at_rest_too() {
        assert!(there_and_back(0.).abs() < 1e-5);
        assert!(there_and_back(0.2) > 0.9);
        for motion in [
            Motion::Swing(-18.),
            Motion::Nudge(2., 0.),
            Motion::Pop,
            Motion::Blink,
        ] {
            assert!(at_rest(motion.pose(0.)), "{motion:?}");
        }
        // A nudge goes where it points and no further.
        let far = (0..=100)
            .map(|step| Motion::Nudge(2., -1.).pose(step as f32 / 100.).shift)
            .fold((0f32, 0f32), |far, at| (far.0.max(at.0), far.1.min(at.1)));
        assert!(far.0 > 1.9 && far.0 <= 2.);
        assert!(far.1 < -0.95 && far.1 >= -1.);
    }

    #[test]
    fn a_spin_starts_on_a_symmetry_of_the_icon_and_turns_one_way() {
        // The gear turns clockwise through a sixth of a turn.
        let start = Motion::Spin(-60.).pose(0.).turn;
        assert!((start + std::f32::consts::FRAC_PI_3).abs() < 1e-4);
        let mut last = start;
        for step in 1..=100 {
            let turn = Motion::Spin(-60.).pose(step as f32 / 100.).turn;
            assert!(turn >= last - 1e-4);
            last = turn;
        }
        assert!(Motion::Spin(360.).pose(0.).turn > 6.2);
    }

    #[test]
    fn icons_get_the_motion_their_shape_asks_for() {
        assert_eq!(Motion::of("SettingsIcon"), Motion::Spin(-60.));
        assert_eq!(Motion::of("RestartIcon"), Motion::of("RefreshIcon"));
        assert_eq!(Motion::of("EyeOffIcon"), Motion::Blink);
        assert_eq!(Motion::of("UpdateIcon"), Motion::Nudge(0., 2.));
        assert_eq!(Motion::of("ProcessesIcon"), Motion::Pop);
        assert_eq!(Motion::of("NoSuchIcon"), Motion::Pop);
        // An icon coming in grows from small and faint.
        let coming = Motion::Enter.pose(0.);
        assert!(coming.scale.0 < 0.7 && coming.alpha < 0.1);
    }
}
