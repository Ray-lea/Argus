//! Argus patch: platform callbacks that arrive while the app is borrowed,
//! moves that change only the position, content zoom and the native
//! caption hit test.
use super::*;

impl Window {
    /// Notify position observers without invalidating unchanged local geometry.
    pub(super) fn position_changed(&mut self, cx: &mut App) {
        if self.scale_factor != self.platform_window.scale_factor()
            || self.viewport_size != self.platform_window.content_size()
        {
            self.bounds_changed(cx);
            return;
        }
        // The platform already updated its screen origin. Local layout and
        // paint caches remain valid while only the window's position changes.
        self.mouse_position = self.platform_window.mouse_position();
        self.bounds_observers
            .clone()
            .retain(|callback| callback(self, cx));
    }

    pub(super) fn try_touch_input(
        handle: AnyWindowHandle,
        cx: &crate::AsyncApp,
        touches: &RefCell<VecDeque<TouchEvent>>,
    ) -> bool {
        let Some(app) = cx.app.upgrade() else {
            touches.borrow_mut().clear();
            return true;
        };
        let Ok(mut app) = app.try_borrow_mut() else {
            return false;
        };
        // Native modal loops can deliver cancellation while the outer window
        // update is still borrowed. Keep contacts ordered until it returns.
        let result = app.update(|cx| {
            handle.update(cx, |_, window, cx| {
                loop {
                    let event = touches.borrow_mut().pop_front();
                    let Some(event) = event else { break };
                    window.dispatch_event(PlatformInput::Touch(event), cx);
                }
            })
        });
        if result.is_err() {
            touches.borrow_mut().clear();
        }
        true
    }

    pub(super) fn try_display_changed(handle: AnyWindowHandle, cx: &crate::AsyncApp) -> bool {
        let Some(app) = cx.app.upgrade() else {
            return true;
        };
        let Ok(mut app) = app.try_borrow_mut() else {
            return false;
        };
        app.update(|cx| Self::display_changed(handle, cx));
        true
    }

    pub(super) fn try_bounds_changed(handle: AnyWindowHandle, cx: &crate::AsyncApp) -> bool {
        let Some(app) = cx.app.upgrade() else {
            return true;
        };
        let Ok(mut app) = app.try_borrow_mut() else {
            return false;
        };
        app.update(|cx| {
            handle
                .update(cx, |_, window, cx| window.bounds_changed(cx))
                .log_err();
        });
        true
    }

    /// Scales everything in the window by `zoom` on top of the display scale
    /// factor, like page zoom in a browser: the scale factor grows by `zoom`,
    /// the viewport shrinks by it and pointer positions arrive in the zoomed
    /// coordinates, so layout, painting and hit testing stay consistent. The
    /// window keeps its size and position on screen. Platforms without
    /// support ignore it.
    pub fn set_content_zoom(&mut self, zoom: f32, cx: &mut App) {
        self.platform_window.set_content_zoom(zoom);
        if self.scale_factor != self.platform_window.scale_factor()
            || self.viewport_size != self.platform_window.content_size()
        {
            self.bounds_changed(cx);
        }
    }

    /// Return the native window control under a point in local window coordinates.
    pub fn window_control_area_at(&self, position: Point<Pixels>) -> Option<WindowControlArea> {
        let hit_test = self.rendered_frame.hit_test(position);
        self.rendered_frame
            .window_control_hitboxes
            .iter()
            .find(|(_, hitbox)| hit_test.ids.contains(&hitbox.id))
            .map(|(area, _)| *area)
    }

    /// Return native control regions from the most recently presented scene.
    #[cfg(feature = "profiler")]
    pub fn window_control_regions(&self) -> Vec<(WindowControlArea, Bounds<Pixels>)> {
        self.rendered_frame
            .window_control_hitboxes
            .iter()
            .map(|(area, hitbox)| (*area, hitbox.bounds))
            .collect()
    }
}
