//! How a dialog comes and goes. It grows from 96 % to its size while fading in
//! (the scale over 250 ms, the fade over the first 83 ms, as a Windows 11
//! dialog opens), and leaves by receding to 96 % while fading out over 100 ms.
//! The surface carries the pose as a live transform about its centre
//! (`Styled::live_transform`), so its shadow, background and content scale
//! and fade as one; at rest it carries none and draws on whole pixels.
use crate::frontend::theme;
use gpui_kit::LiveTransformValue;
use std::time::Duration;

/// The size the dialog grows from and recedes to.
const SMALL: f32 = 0.96;
const ENTER_SCALE: Duration = Duration::from_millis(250);
const ENTER_FADE: Duration = Duration::from_millis(83);
const EXIT_SCALE: Duration = Duration::from_millis(167);
const EXIT_FADE: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug)]
pub(super) struct Pose {
    pub value: LiveTransformValue,
    /// Fully in and not leaving: the surface needs no transform.
    pub settled: bool,
    /// Faded out: the dialog can close.
    pub gone: bool,
}

/// WinUI's `ControlFastOutSlowInKeySpline` (0, 0, 0, 1): most of the change
/// at the start, settling exactly on 1.
fn settle(t: f32) -> f32 {
    if t >= 1. {
        1.
    } else {
        theme::bezier(0., 0., 0., 1., t)
    }
}

fn fraction(elapsed: Duration, length: Duration) -> f32 {
    (elapsed.as_secs_f32() / length.as_secs_f32()).clamp(0., 1.)
}

/// The surface `entered` after its first visible frame and, while it leaves,
/// `left` after it began to. A dialog closed while still coming in recedes
/// from where its entrance had got to.
pub(super) fn pose(entered: Duration, left: Option<Duration>) -> Pose {
    let mut scale = 1. - (1. - SMALL) * (1. - settle(fraction(entered, ENTER_SCALE)));
    let mut opacity = fraction(entered, ENTER_FADE);
    let mut settled = entered >= ENTER_SCALE;
    let mut gone = false;
    if let Some(left) = left {
        scale *= 1. - (1. - SMALL) * settle(fraction(left, EXIT_SCALE));
        opacity *= 1. - fraction(left, EXIT_FADE);
        settled = false;
        gone = left >= EXIT_FADE;
    }
    Pose {
        value: LiveTransformValue::zoom(scale, opacity),
        settled,
        gone,
    }
}

#[cfg(test)]
mod tests {
    use super::{EXIT_FADE, pose};
    use std::time::Duration;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn entrance_grows_in_and_settles_untransformed() {
        let first = pose(ms(0), None).value;
        assert!((first.scale_x - 0.96).abs() < 1e-6 && first.opacity == 0.);
        let faded_in = pose(ms(83), None);
        assert_eq!(faded_in.value.opacity, 1.);
        assert!(faded_in.value.scale_x > 0.98 && faded_in.value.scale_x < 1. && !faded_in.settled);
        let rest = pose(ms(250), None);
        assert!(rest.settled && rest.value.is_identity());
    }

    #[test]
    fn exit_recedes_and_fades_out() {
        let start = pose(ms(1000), Some(ms(0)));
        assert!(start.value.is_identity() && !start.settled && !start.gone);
        let half = pose(ms(1050), Some(ms(50)));
        assert!((half.value.opacity - 0.5).abs() < 1e-6);
        assert!(half.value.scale_x < 0.98 && half.value.scale_x > 0.96);
        let end = pose(ms(1100), Some(EXIT_FADE));
        assert!(end.gone && end.value.opacity == 0.);
    }

    #[test]
    fn never_grows_past_its_size() {
        for entered in (0..400).step_by(7) {
            for left in [None, Some(0), Some(40), Some(90), Some(200)] {
                let value = pose(ms(entered), left.map(ms)).value;
                assert!(value.scale_x <= 1. && value.scale_x >= 0.96 * 0.96 - 1e-6);
                assert!((0. ..=1.).contains(&value.opacity));
            }
        }
    }

    #[test]
    fn closing_during_the_entrance_starts_from_where_it_was() {
        let entering = pose(ms(40), None).value;
        let leaving = pose(ms(40), Some(ms(0))).value;
        assert_eq!(entering.opacity, leaving.opacity);
        assert_eq!(entering.scale_x, leaving.scale_x);
    }
}
