//! A user-requested activation can rise above an obstructing topmost window.
//! The temporary elevation ends when this window loses activation or hides.
use std::cell::Cell;
#[cfg(test)]
mod pressure_tests;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        System::{
            StationsAndDesktops::{EnumDesktopWindows, GetThreadDesktop},
            Threading::GetCurrentThreadId,
        },
        UI::{
            Input::KeyboardAndMouse::SetFocus,
            Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
    core::BOOL,
};

const SUBCLASS: usize = 0x4143_5456;
const REQUEST: u32 = WM_APP + 0x48;
struct State {
    raised: Cell<bool>,
}

fn state(hwnd: HWND) -> Option<*const State> {
    let mut value = 0;
    unsafe { GetWindowSubclass(hwnd, Some(procedure), SUBCLASS, Some(&mut value)) }
        .as_bool()
        .then_some(value as *const State)
}

pub(crate) fn install(hwnd: HWND) {
    if state(hwnd).is_some() {
        return;
    }
    let value = Box::into_raw(Box::new(State {
        raised: Cell::new(false),
    }));
    if !unsafe { SetWindowSubclass(hwnd, Some(procedure), SUBCLASS, value as usize) }.as_bool() {
        unsafe {
            drop(Box::from_raw(value));
        }
    } else if unsafe { GetForegroundWindow() } == hwnd {
        raise_if_obstructed(hwnd);
    }
}

fn covers(target: RECT, obstacle: RECT) -> bool {
    let width = i64::from(target.right) - i64::from(target.left);
    let height = i64::from(target.bottom) - i64::from(target.top);
    if width <= 0 || height <= 0 {
        return false;
    }
    let x = (i64::from(target.left) + i64::from(target.right)) / 2;
    let y = (i64::from(target.top) + i64::from(target.bottom)) / 2;
    let area = (i64::from(target.right.min(obstacle.right))
        - i64::from(target.left.max(obstacle.left)))
    .max(0)
        * (i64::from(target.bottom.min(obstacle.bottom)) - i64::from(target.top.max(obstacle.top)))
            .max(0);
    x >= i64::from(obstacle.left)
        && x < i64::from(obstacle.right)
        && y >= i64::from(obstacle.top)
        && y < i64::from(obstacle.bottom)
        && area >= width * height / 3
}

fn obstructed(hwnd: HWND) -> bool {
    struct Search {
        target: HWND,
        bounds: RECT,
        found: bool,
    }
    unsafe extern "system" fn visit(candidate: HWND, context: LPARAM) -> BOOL {
        let search = unsafe { &mut *(context.0 as *mut Search) };
        if candidate == search.target {
            return false.into();
        }
        unsafe {
            if !IsWindowVisible(candidate).as_bool()
                || IsIconic(candidate).as_bool()
                || GetWindowLongPtrW(candidate, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0
                || GetAncestor(candidate, GA_ROOTOWNER) == search.target
            {
                return true.into();
            }
            let mut cloak = 0u32;
            let _ =
                DwmGetWindowAttribute(candidate, DWMWA_CLOAKED, (&mut cloak as *mut u32).cast(), 4);
            if cloak != 0 {
                return true.into();
            }
            let mut bounds = RECT::default();
            if GetWindowRect(candidate, &mut bounds).is_ok() && covers(search.bounds, bounds) {
                search.found = true;
                return false.into();
            }
        }
        true.into()
    }
    let mut search = Search {
        target: hwnd,
        bounds: RECT::default(),
        found: false,
    };
    unsafe {
        if GetWindowRect(hwnd, &mut search.bounds).is_err() {
            return false;
        }
        let Ok(desktop) = GetThreadDesktop(GetCurrentThreadId()) else {
            return false;
        };
        let _ = EnumDesktopWindows(
            Some(desktop),
            Some(visit),
            LPARAM((&mut search as *mut Search) as isize),
        );
    }
    search.found
}

fn raise_if_obstructed(hwnd: HWND) {
    unsafe {
        if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0
            && obstructed(hwnd)
            && let Some(state) = state(hwnd)
        {
            (*state).raised.set(true);
            if SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .is_err()
                && let Some(state) = self::state(hwnd)
            {
                (*state).raised.set(false);
            }
        }
    }
}

pub(crate) fn focus(hwnd: HWND) {
    unsafe {
        if GetWindowThreadProcessId(hwnd, None) != GetCurrentThreadId() {
            let _ = PostMessageW(Some(hwnd), REQUEST, WPARAM(0), LPARAM(0));
            return;
        }
        raise_if_obstructed(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
    }
}

fn restore_after(hwnd: HWND, next: HWND) -> HWND {
    unsafe {
        for candidate in [GetForegroundWindow(), next] {
            let root = GetAncestor(candidate, GA_ROOT);
            if !root.is_invalid()
                && root != hwnd
                && GetAncestor(root, GA_ROOTOWNER) != hwnd
                && IsWindowVisible(root).as_bool()
                && GetWindowLongPtrW(root, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0
            {
                // Inserting behind a normal window also removes WS_EX_TOPMOST.
                // HWND_NOTOPMOST alone would cover the newly active window if
                // its input queue has already completed the foreground switch.
                return root;
            }
        }
    }
    HWND_NOTOPMOST
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    if message == REQUEST {
        focus(hwnd);
        return LRESULT(0);
    }
    if message == WM_ACTIVATE && w.0 & 0xffff != WA_INACTIVE as usize {
        raise_if_obstructed(hwnd);
    }
    if (message == WM_ACTIVATE && w.0 & 0xffff == WA_INACTIVE as usize)
        || (message == WM_SHOWWINDOW && w.0 == 0)
    {
        let restore = unsafe { &*(data as *const State) }.raised.replace(false);
        let next = if message == WM_ACTIVATE {
            HWND(l.0 as _)
        } else {
            HWND::default()
        };
        if restore
            && unsafe {
                SetWindowPos(
                    hwnd,
                    Some(restore_after(hwnd, next)),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            }
            .is_err()
            && let Some(state) = state(hwnd)
        {
            unsafe {
                (*state).raised.set(true);
            }
        }
    }
    if message == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(procedure), SUBCLASS);
            drop(Box::from_raw(data as *mut State));
        }
    }
    unsafe { DefSubclassProc(hwnd, message, w, l) }
}

#[cfg(test)]
mod tests {
    use super::covers;
    use windows::Win32::Foundation::RECT;
    #[test]
    fn large_obstructions_are_distinct_from_small_tooltips_and_empty_windows() {
        let target = RECT {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 800,
        };
        assert!(covers(
            target,
            RECT {
                left: -10,
                top: -10,
                right: 1200,
                bottom: 900
            }
        ));
        assert!(!covers(
            target,
            RECT {
                left: 450,
                top: 390,
                right: 550,
                bottom: 410
            }
        ));
        assert!(!covers(RECT::default(), target));
    }
}
