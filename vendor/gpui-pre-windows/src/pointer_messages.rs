//! Argus patch: touch input from `WM_POINTER` messages, and the answers to
//! the system pointer's messages while the windows leave it out.
use super::*;

/// Argus patch: the answer to a message of the system's pointer while the
/// windows leave it out, or `None` for any other message. Nothing reaches
/// `DefWindowProc` either, which would move, size or activate the window.
pub(super) fn ignored_pointer_result(msg: u32) -> Option<isize> {
    match msg {
        // Neither a click nor a touch activates the window; the click is dropped.
        WM_MOUSEACTIVATE => Some(MA_NOACTIVATEANDEAT as isize),
        WM_POINTERACTIVATE => Some(PA_NOACTIVATE as isize),
        // No caption buttons, edges or snap layouts under the pointer.
        WM_NCHITTEST => Some(HTNOWHERE as isize),
        WM_MOUSEMOVE
        | WM_MOUSELEAVE
        | WM_MOUSEHOVER
        | WM_NCMOUSEMOVE
        | WM_NCMOUSELEAVE
        | WM_NCMOUSEHOVER
        | WM_LBUTTONDOWN
        | WM_LBUTTONUP
        | WM_LBUTTONDBLCLK
        | WM_RBUTTONDOWN
        | WM_RBUTTONUP
        | WM_RBUTTONDBLCLK
        | WM_MBUTTONDOWN
        | WM_MBUTTONUP
        | WM_MBUTTONDBLCLK
        | WM_XBUTTONDOWN
        | WM_XBUTTONUP
        | WM_XBUTTONDBLCLK
        | WM_NCLBUTTONDOWN
        | WM_NCLBUTTONUP
        | WM_NCLBUTTONDBLCLK
        | WM_NCRBUTTONDOWN
        | WM_NCRBUTTONUP
        | WM_NCRBUTTONDBLCLK
        | WM_NCMBUTTONDOWN
        | WM_NCMBUTTONUP
        | WM_NCMBUTTONDBLCLK
        | WM_NCXBUTTONDOWN
        | WM_NCXBUTTONUP
        | WM_NCXBUTTONDBLCLK
        | WM_MOUSEWHEEL
        | WM_MOUSEHWHEEL
        | WM_POINTERDOWN
        | WM_POINTERUPDATE
        | WM_POINTERUP
        | WM_POINTERENTER
        | WM_POINTERLEAVE
        | WM_POINTERCAPTURECHANGED
        | WM_POINTERWHEEL
        | WM_POINTERHWHEEL
        | DM_POINTERHITTEST
        | WM_CONTEXTMENU => Some(0),
        _ => None,
    }
}

fn is_touch_pointer(pointer: u32, message: u32, flags: u32) -> bool {
    let mut kind = POINTER_INPUT_TYPE::default();
    if unsafe { GetPointerType(pointer, &mut kind) }.is_ok() {
        return kind == PT_TOUCH;
    }
    let mut source = INPUT_MESSAGE_SOURCE::default();
    let _ = unsafe { GetCurrentInputMessageSource(&mut source) };
    if source.deviceType != IMDT_UNAVAILABLE {
        return source.deviceType == IMDT_TOUCH;
    }
    // A forwarded pointer message can lack both a local pointer ID and a
    // device type. Only adopt an explicit primary contact performing a left press.
    const PRIMARY_PRESS: u32 = POINTER_MESSAGE_FLAG_PRIMARY
        | POINTER_MESSAGE_FLAG_INCONTACT
        | POINTER_MESSAGE_FLAG_FIRSTBUTTON;
    const OTHER_BUTTONS: u32 = POINTER_MESSAGE_FLAG_SECONDBUTTON
        | POINTER_MESSAGE_FLAG_THIRDBUTTON
        | POINTER_MESSAGE_FLAG_FOURTHBUTTON
        | POINTER_MESSAGE_FLAG_FIFTHBUTTON;
    message == WM_POINTERDOWN && flags & (PRIMARY_PRESS | OTHER_BUTTONS) == PRIMARY_PRESS
}

impl WindowsWindowInner {
    fn dispatch_touch(self: &Rc<Self>, event: TouchEvent) {
        let cancelled = event.phase == TouchPhase::Cancelled;
        self.state.pending_touches.borrow_mut().push_back(event);
        if cancelled {
            self.defer_touches();
        } else {
            self.flush_touches();
        }
    }

    fn flush_touches(self: &Rc<Self>) {
        let Some(mut callback) = self.state.callbacks.input.take() else {
            return;
        };
        let mut released_at = None;
        loop {
            let event = self.state.pending_touches.borrow_mut().pop_front();
            let Some(event) = event else { break };
            released_at = (event.phase == TouchPhase::Ended).then_some(event.position);
            callback(PlatformInput::Touch(event));
        }
        self.state.callbacks.input.set(Some(callback));
        if let Some(position) = released_at {
            self.update_touch_keyboard(position);
        }
    }

    pub(super) fn cancel_touches(self: &Rc<Self>) {
        let events = self.state.touch.borrow_mut().cancel_all();
        if events.is_empty() {
            return;
        }
        self.state.pending_touches.borrow_mut().extend(events);
        // Hiding or disabling a window can synchronously re-enter its window
        // procedure while GPUI is borrowed. Preserve ordering with later
        // touches, but deliver cancellation after the current update returns.
        self.defer_touches();
    }

    fn defer_touches(self: &Rc<Self>) {
        let this = self.clone();
        self.executor
            .spawn(async move { this.flush_touches() })
            .detach();
    }

    fn update_touch_keyboard(&self, position: Point<Pixels>) {
        // Synthetic/hidden/background input must never show desktop UI.
        if unsafe { GetForegroundWindow() } != self.hwnd
            || !unsafe { IsWindowVisible(self.hwnd) }.as_bool()
        {
            return;
        }
        let editing = self
            .with_input_handler(|handler| {
                handler.query_accepts_focused_text_input()
                    && handler
                        .element_bounds()
                        .is_some_and(|bounds| bounds.contains(&position))
            })
            .unwrap_or(false);
        if !editing && !self.state.touch_keyboard_shown.get() {
            return;
        }
        use windows::{UI::ViewManagement::InputPane, Win32::System::WinRT::IInputPaneInterop};
        let pane = windows::core::factory::<InputPane, IInputPaneInterop>()
            .and_then(|factory| unsafe { factory.GetForWindow::<InputPane>(self.hwnd) });
        if let Ok(pane) = pane {
            if editing {
                if pane.TryShow().unwrap_or(false) {
                    self.state.touch_keyboard_shown.set(true);
                }
            } else if pane.TryHide().unwrap_or(false) {
                self.state.touch_keyboard_shown.set(false);
            }
        }
    }

    pub(super) fn handle_touch_msg(
        self: &Rc<Self>,
        handle: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<isize> {
        let pointer = wparam.loword() as u32;
        let flags = u32::from(wparam.hiword());
        if !self.state.touch.borrow().contains(pointer)
            && !is_touch_pointer(pointer, message, flags)
        {
            return None;
        }
        // Consume the entire touch sequence so DefWindowProc does not also
        // promote it to mouse input. Pen and touchpad retain their own paths.
        if matches!(message, WM_POINTERLEAVE | WM_POINTERCAPTURECHANGED)
            || flags & POINTER_MESSAGE_FLAG_CANCELED != 0
        {
            let event = self.state.touch.borrow_mut().cancel(pointer);
            if let Some(event) = event {
                self.dispatch_touch(event);
            }
            return Some(0);
        }
        let phase = match message {
            WM_POINTERDOWN => TouchPhase::Started,
            WM_POINTERUPDATE => TouchPhase::Moved,
            WM_POINTERUP => TouchPhase::Ended,
            _ => return Some(0),
        };
        // Detailed pointer data can expire after another message is retrieved.
        // The message itself still carries the contact's flags and screen point.
        let mut info = POINTER_INFO::default();
        let has_info = unsafe { GetPointerInfo(pointer, &mut info) }.is_ok();
        let mut position = if has_info {
            info.ptPixelLocation
        } else {
            POINT {
                x: lparam.signed_loword().into(),
                y: lparam.signed_hiword().into(),
            }
        };
        let cancelled = has_info && info.pointerFlags.contains(POINTER_FLAG_CANCELED);
        let event = if !cancelled && unsafe { ScreenToClient(handle, &mut position) }.as_bool() {
            self.state.touch.borrow_mut().update(
                pointer,
                phase,
                logical_point(
                    position.x as f32,
                    position.y as f32,
                    self.state.scale_factor.get(),
                ),
            )
        } else {
            self.state.touch.borrow_mut().cancel(pointer)
        };
        if let Some(event) = event {
            self.dispatch_touch(event);
        }
        Some(0)
    }
}
