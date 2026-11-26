//! Argus patch: wheel glides driven through a scroll handle.
use super::*;

impl ScrollHandle {
    /// Argus patch: scrolls by wheel lines as a notch does, gliding toward the
    /// target (see `smooth_scroll`). Returns whether the target moved; the
    /// caller notifies the view that paints the scrolled element.
    pub fn scroll_by_lines(&self, lines: Point<f32>) -> bool {
        let delta = point(
            px(lines.x * smooth_scroll::PIXELS_PER_LINE),
            px(lines.y * smooth_scroll::PIXELS_PER_LINE),
        );
        self.glide_to(self.wheel_target() + delta)
    }

    /// Argus patch: glides to an offset, clamped to the scrollable range, with
    /// the wheel's animation. Returns whether the target moved.
    pub fn glide_to(&self, to: Point<Pixels>) -> bool {
        let state = self.0.borrow();
        let max = state.max_offset;
        let key = Rc::as_ptr(&state.offset) as usize;
        let current = *state.offset.borrow();
        let clamp = |p: Point<Pixels>| point(p.x.clamp(-max.x, px(0.)), p.y.clamp(-max.y, px(0.)));
        let target = clamp(smooth_scroll::target(key, current));
        let to = clamp(to);
        if to == target {
            return false;
        }
        smooth_scroll::glide_to(key, current, to);
        true
    }

    /// Argus patch: where the offset is heading: a running glide's target, else the offset.
    pub fn wheel_target(&self) -> Point<Pixels> {
        let state = self.0.borrow();
        let key = Rc::as_ptr(&state.offset) as usize;
        smooth_scroll::target(key, *state.offset.borrow())
    }

    /// Argus patch: whether a glide of this handle's offset is running: it
    /// ends during the prepaint that takes its last step, or, when its
    /// scroller is no longer drawn, once it would have ended.
    pub fn gliding(&self) -> bool {
        let state = self.0.borrow();
        smooth_scroll::stepping(Rc::as_ptr(&state.offset) as usize)
    }

    /// Argus patch: from now on, the glides of the scroller tracked by this
    /// handle ask for isolated frames (`Window::request_isolated_animation_frame`):
    /// only the nearest cached view around the scroller is drawn again, and
    /// the cached views around that one replay. Whatever reads the offset
    /// outside that view while rendering or painting must then ask for its
    /// own frames while the handle glides (see [`Self::gliding`]).
    pub fn isolate_glides(&self) {
        smooth_scroll::isolate(&self.0.borrow().offset);
    }
}
