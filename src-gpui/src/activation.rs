//! A user-requested activation can rise above an obstructing topmost window.
//! The temporary elevation ends when this window loses activation or hides.
use std::cell::Cell;
#[cfg(test)]
mod pressure_tests;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Gdi::{
                CombineRgn, CreateRectRgn, CreateRectRgnIndirect, DeleteObject, GDI_REGION_TYPE,
                GetWindowRgn, HRGN, NULLREGION, RGN_DIFF,
            },
        },
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

/// Whether `hwnd` is cloaked: on another virtual desktop, or hidden by
/// the system though visible to it.
pub(crate) fn cloaked(hwnd: HWND) -> bool {
    let mut cloak = 0u32;
    let _ =
        unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloak as *mut u32).cast(), 4) };
    cloak != 0
}

/// Whether a topmost window above `hwnd` covers it.
fn obstructed(hwnd: HWND) -> bool {
    covered_by(hwnd, true)
}

struct OwnedRegion(HRGN);

impl Drop for OwnedRegion {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DeleteObject(self.0.into());
            }
        }
    }
}

fn frame_bounds(hwnd: HWND) -> Option<RECT> {
    let mut bounds = RECT::default();
    let extended = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut bounds as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
    };
    if extended.is_err() && unsafe { GetWindowRect(hwnd, &mut bounds) }.is_err() {
        return None;
    }
    (bounds.right > bounds.left && bounds.bottom > bounds.top).then_some(bounds)
}

#[cfg(test)]
fn region_occluded(target: RECT, obstacles: impl IntoIterator<Item = RECT>) -> bool {
    if target.right <= target.left || target.bottom <= target.top {
        return true;
    }
    let remaining = OwnedRegion(unsafe { CreateRectRgnIndirect(&target) });
    if remaining.0.is_invalid() {
        return false;
    }
    for obstacle in obstacles {
        if obstacle.right <= obstacle.left || obstacle.bottom <= obstacle.top {
            continue;
        }
        let sub = OwnedRegion(unsafe { CreateRectRgnIndirect(&obstacle) });
        if sub.0.is_invalid() {
            continue;
        }
        if unsafe { CombineRgn(Some(remaining.0), Some(remaining.0), Some(sub.0), RGN_DIFF) }
            == NULLREGION
        {
            return true;
        }
    }
    false
}

/// Whether `hwnd` is completely hidden from view: cloaked, minimized, invisible,
/// or 100% covered by opaque top-level windows above it on the same desktop.
pub(crate) fn occluded(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() || cloaked(hwnd) {
            return true;
        }
    }
    let Some(target_bounds) = frame_bounds(hwnd) else {
        return true;
    };
    struct OcclusionSearch {
        target: HWND,
        target_bounds: RECT,
        remaining: HRGN,
        probe: HRGN,
        occluded: bool,
    }
    unsafe extern "system" fn visit(candidate: HWND, context: LPARAM) -> BOOL {
        let search = unsafe { &mut *(context.0 as *mut OcclusionSearch) };
        if candidate == search.target {
            return false.into();
        }
        unsafe {
            let style = GetWindowLongPtrW(candidate, GWL_EXSTYLE) as u32;
            if !IsWindowVisible(candidate).as_bool()
                || IsIconic(candidate).as_bool()
                || style & (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0 | WS_EX_TOOLWINDOW.0) != 0
                || GetAncestor(candidate, GA_ROOTOWNER) == search.target
                || cloaked(candidate)
                || GetWindowRgn(candidate, search.probe) != GDI_REGION_TYPE(0)
            {
                return true.into();
            }
            let Some(mut bounds) = frame_bounds(candidate) else {
                return true.into();
            };
            // Unmaximized Windows 11 frames have rounded corners (~8 px DIP);
            // inset slightly so exposed corners underneath still count as visible.
            if !IsZoomed(candidate).as_bool() {
                bounds.left += 12;
                bounds.top += 12;
                bounds.right -= 12;
                bounds.bottom -= 12;
            }
            if bounds.right <= bounds.left
                || bounds.bottom <= bounds.top
                || bounds.right <= search.target_bounds.left
                || bounds.left >= search.target_bounds.right
                || bounds.bottom <= search.target_bounds.top
                || bounds.top >= search.target_bounds.bottom
            {
                return true.into();
            }
            let sub = OwnedRegion(CreateRectRgnIndirect(&bounds));
            if !sub.0.is_invalid()
                && CombineRgn(
                    Some(search.remaining),
                    Some(search.remaining),
                    Some(sub.0),
                    RGN_DIFF,
                ) == NULLREGION
            {
                search.occluded = true;
                return false.into();
            }
        }
        true.into()
    }
    let remaining = OwnedRegion(unsafe { CreateRectRgnIndirect(&target_bounds) });
    let probe = OwnedRegion(unsafe { CreateRectRgn(0, 0, 0, 0) });
    if remaining.0.is_invalid() || probe.0.is_invalid() {
        return false;
    }
    let mut search = OcclusionSearch {
        target: hwnd,
        target_bounds,
        remaining: remaining.0,
        probe: probe.0,
        occluded: false,
    };
    unsafe {
        if let Ok(desktop) = GetThreadDesktop(GetCurrentThreadId()) {
            let _ = EnumDesktopWindows(
                Some(desktop),
                Some(visit),
                LPARAM((&mut search as *mut OcclusionSearch) as isize),
            );
        }
    }
    search.occluded
}

/// Whether an application's window above `hwnd` covers it: one opened
/// or used after it, shown over the middle of it and at least a third of
/// it. Topmost windows, tool windows and click-through overlays (a
/// game's or a graphics driver's, often as large as the display) do not
/// count: they float over everything.
pub(crate) fn covered(hwnd: HWND) -> bool {
    covered_by(hwnd, false)
}

/// Whether a window above `hwnd` covers it: a topmost one (`topmost`),
/// or an application's ordinary one.
fn covered_by(hwnd: HWND, topmost: bool) -> bool {
    struct Search {
        target: HWND,
        bounds: RECT,
        topmost: bool,
        found: bool,
    }
    unsafe extern "system" fn visit(candidate: HWND, context: LPARAM) -> BOOL {
        let search = unsafe { &mut *(context.0 as *mut Search) };
        if candidate == search.target {
            return false.into();
        }
        unsafe {
            let style = GetWindowLongPtrW(candidate, GWL_EXSTYLE) as u32;
            let kind = if search.topmost {
                style & WS_EX_TOPMOST.0 != 0
            } else {
                style & (WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0) == 0
            };
            if !IsWindowVisible(candidate).as_bool()
                || IsIconic(candidate).as_bool()
                || !kind
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
        topmost,
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
    use super::{covers, region_occluded};
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

    #[test]
    fn region_occlusion_handles_single_split_and_partial_blockers() {
        let target = RECT {
            left: 100,
            top: 100,
            right: 900,
            bottom: 700,
        };
        assert!(region_occluded(
            target,
            [RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080
            }]
        ));
        // Two side-by-side windows covering the left and right halves together.
        assert!(region_occluded(
            target,
            [
                RECT {
                    left: 0,
                    top: 0,
                    right: 500,
                    bottom: 800
                },
                RECT {
                    left: 500,
                    top: 0,
                    right: 1000,
                    bottom: 800
                },
            ]
        ));
        // A sliver on the right edge remains visible.
        assert!(!region_occluded(
            target,
            [RECT {
                left: 0,
                top: 0,
                right: 890,
                bottom: 800
            }]
        ));
    }
}
