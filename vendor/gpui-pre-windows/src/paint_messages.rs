//! Argus patch: `WM_PAINT` for a hidden window, and the new client size
//! painted on `WM_NCCALCSIZE` before the system applies it.
use super::*;
use std::sync::OnceLock;

/// Takes the update region of a hidden window; false for a visible one, which
/// is drawn as usual. `ValidateRect` succeeds on a hidden window and leaves
/// its update region in place, so the system generates `WM_PAINT` again each
/// time the queue runs empty and the UI thread never idles. Setting the
/// layered attributes of a hidden window without a redirection bitmap leaves
/// such a region behind (a fade-out restoring the opacity once the window is
/// gone). `BeginPaint` takes it whatever the visibility.
pub(crate) fn acknowledge_hidden_paint(handle: HWND) -> bool {
    unsafe {
        if IsWindowVisible(handle).as_bool() {
            return false;
        }
        let mut paint = PAINTSTRUCT::default();
        BeginPaint(handle, &mut paint);
        let _ = EndPaint(handle, &paint);
    }
    true
}

impl WindowsWindowInner {
    /// Paints and presents the client size the system is about to apply.
    ///
    /// The window takes its new rectangle when this message returns, and DWM
    /// composes that rectangle at its next frame, while `WM_SIZE` arrives only
    /// at the end of `SetWindowPos` (4–13 ms later when another process
    /// resized the window). A frame first painted for `WM_SIZE` left the new
    /// size showing the old content, with the window background in the
    /// uncovered strip and the caption buttons at their old place, for two or
    /// three refreshes.
    pub(super) fn draw_before_resize(&self, handle: HWND, client: RECT) {
        static DEFERRED: OnceLock<bool> = OnceLock::new();
        if *DEFERRED.get_or_init(|| std::env::var_os("ARGUS_GPUI_DEFER_RESIZE_PAINT").is_some()) {
            return;
        }
        let new_size = size(
            DevicePixels((client.right - client.left).max(1)),
            DevicePixels((client.bottom - client.top).max(1)),
        );
        let mut current = RECT::default();
        unsafe {
            if !IsWindowVisible(handle).as_bool()
                || IsIconic(handle).as_bool()
                || GetClientRect(handle, &mut current).is_err()
            {
                return;
            }
        }
        // A draw in progress may hold the renderer, and a minimized window has
        // no frame callback until `WM_SIZE` restores it: both keep the
        // `WM_SIZE` path.
        if new_size == size(DevicePixels(current.right), DevicePixels(current.bottom))
            || self.state.draw_coordinator.drawing.get()
        {
            return;
        }
        let Some(request_frame) = self.state.callbacks.request_frame.take() else {
            return;
        };
        self.state.callbacks.request_frame.set(Some(request_frame));
        crate::frame_trace::record(
            16,
            (new_size.width.0 as u64) << 16 | new_size.height.0 as u64,
        );
        self.handle_size_change(new_size, self.state.scale_factor.get());
        self.state.presized.set(Some(new_size));
        self.draw_window(handle, false);
    }
}
