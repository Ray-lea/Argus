//! The VK and modifiers of the owned window's current message. This context
//! exists only while GPUI dispatches that message; no global keyboard hook.
use std::cell::Cell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stroke {
    pub vk: u16,
    pub down: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}
thread_local! { static CURRENT: Cell<Option<Stroke>> = const { Cell::new(None) }; }

pub(super) fn current(down: bool) -> Option<Stroke> {
    CURRENT.get().filter(|event| event.down == down)
}

pub(crate) fn install_key_context(hwnd: HWND) {
    if !unsafe { SetWindowSubclass(hwnd, Some(input_proc), 0x4152_4b49, 0) }.as_bool() {
        argus_core::diagnostics::warn(
            "hotkey.input",
            argus_core::diagnostics::code(unsafe { windows::Win32::Foundation::GetLastError() }.0),
        );
    }
}

struct Restore(Option<Stroke>);
impl Drop for Restore {
    fn drop(&mut self) {
        CURRENT.set(self.0);
    }
}

unsafe extern "system" fn input_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    let event = matches!(message, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP).then(|| {
        let pressed = |key: u16| (unsafe { GetKeyState(key as i32) }) < 0;
        Stroke {
            vk: wparam.0 as u16,
            down: matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN),
            ctrl: pressed(VK_CONTROL.0),
            alt: pressed(VK_MENU.0),
            shift: pressed(VK_SHIFT.0),
            win: pressed(VK_LWIN.0) || pressed(VK_RWIN.0),
        }
    });
    let _restore = Restore(CURRENT.replace(event));
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::{CURRENT, Stroke, current, install_key_context};
    use std::cell::RefCell;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SendMessageW,
        UnregisterClassW, WM_KEYDOWN, WM_KEYUP, WNDCLASSW, WS_POPUP,
    };
    use windows::core::w;
    thread_local! { static OBSERVED: RefCell<Vec<Option<Stroke>>> = const { RefCell::new(Vec::new()) }; }

    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> LRESULT {
        if matches!(message, WM_KEYDOWN | WM_KEYUP) {
            OBSERVED.with_borrow_mut(|events| events.push(current(message == WM_KEYDOWN)));
            if message == WM_KEYDOWN {
                unsafe {
                    SendMessageW(hwnd, WM_KEYUP, Some(wp), Some(lp));
                }
                OBSERVED.with_borrow_mut(|events| events.push(current(true)));
            }
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, message, wp, lp) }
    }

    #[test]
    fn native_dispatch_preserves_key_identity_and_restores_nested_context() {
        let instance = unsafe { GetModuleHandleW(None) }.unwrap();
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: w!("ArgusHotkeyInputTest"),
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassW(&class) }, 0);
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                class.lpszClassName,
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .unwrap();
        install_key_context(hwnd);
        OBSERVED.with_borrow_mut(Vec::clear);
        for vk in [0x31, 0x41, 0x59, 0xba, 0xdc, 0x61] {
            unsafe {
                SendMessageW(hwnd, WM_KEYDOWN, Some(WPARAM(vk)), Some(LPARAM(1)));
            }
            assert!(CURRENT.get().is_none());
        }
        let events = OBSERVED.take();
        unsafe {
            DestroyWindow(hwnd).unwrap();
            UnregisterClassW(class.lpszClassName, Some(instance.into())).unwrap();
        }
        for (events, vk) in events
            .as_chunks::<3>()
            .0
            .iter()
            .zip([0x31, 0x41, 0x59, 0xba, 0xdc, 0x61])
        {
            assert_eq!(
                events
                    .iter()
                    .map(|event| event.map(|event| (event.vk, event.down)))
                    .collect::<Vec<_>>(),
                [Some((vk, true)), Some((vk, false)), Some((vk, true))]
            );
        }
        assert_eq!(events.len(), 18);
    }
}
