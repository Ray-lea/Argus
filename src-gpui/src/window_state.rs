//! Remember normal placement rather than minimized/maximized screen bounds.
//! A hidden start stays hidden; maximization is applied at its first showing.
//! The placement lives in `preferences.json` (`argus_app::preferences`).

use argus_app::preferences::WindowPlacement;
use std::{
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicIsize, Ordering},
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const SUBCLASS: usize = 0x5753_5441;
const TIMER: usize = 0xA673;
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static MAXIMIZE_ON_SHOW: AtomicBool = AtomicBool::new(false);
static FILE: OnceLock<PathBuf> = OnceLock::new();

fn placement(hwnd: HWND) -> Option<WINDOWPLACEMENT> {
    let mut placement = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe { GetWindowPlacement(hwnd, &mut placement) }.ok()?;
    Some(placement)
}

/// Restores the saved placement and saves it again as the window moves.
/// `file` is the preferences file.
pub fn install(hwnd: HWND, file: PathBuf) {
    if let Some(saved) = argus_app::preferences::window(&file) {
        let [left, top, right, bottom] = saved.rect;
        let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
        let restored = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            // A window already shown takes the placement now; a hidden one
            // stays hidden and maximizes when it is first shown.
            showCmd: match (visible, saved.maximized) {
                (false, _) => SW_HIDE.0 as u32,
                (true, true) => SW_SHOWMAXIMIZED.0 as u32,
                (true, false) => SW_SHOWNORMAL.0 as u32,
            },
            rcNormalPosition: RECT {
                left,
                top,
                right,
                bottom,
            },
            ..Default::default()
        };
        // Windows relocates a placement whose monitor has disappeared.
        if unsafe { SetWindowPlacement(hwnd, &restored) }.is_ok() {
            MAXIMIZE_ON_SHOW.store(saved.maximized && !visible, Ordering::Relaxed);
        }
    }
    let _ = FILE.set(file);
    WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(window_proc), SUBCLASS, 0);
    }
}

/// Before the window is shown: a maximized placement saved while it was
/// hidden applies now.
pub fn prepare_show(hwnd: HWND) {
    if !MAXIMIZE_ON_SHOW.swap(false, Ordering::Relaxed) {
        return;
    }
    if let Some(mut saved) = placement(hwnd) {
        // Restore a maximized placement without activating the user's desktop.
        saved.flags = WPF_RESTORETOMAXIMIZED;
        saved.showCmd = SW_SHOWMINNOACTIVE.0 as u32;
        unsafe {
            let _ = SetWindowPlacement(hwnd, &saved);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

/// Saves the current placement (also before the process exits).
pub fn flush() {
    let hwnd = HWND(WINDOW.load(Ordering::Relaxed) as *mut _);
    if hwnd.0.is_null() || crate::window_motion::changing_visibility() {
        return;
    }
    let Some(file) = FILE.get() else {
        return;
    };
    if let Some(p) = placement(hwnd) {
        let r = p.rcNormalPosition;
        let maximized = MAXIMIZE_ON_SHOW.load(Ordering::Relaxed)
            || unsafe { IsZoomed(hwnd).as_bool() }
            || (unsafe { IsIconic(hwnd).as_bool() } && p.flags.contains(WPF_RESTORETOMAXIMIZED));
        let saved = WindowPlacement {
            rect: [r.left, r.top, r.right, r.bottom],
            maximized,
        };
        if let Err(error) = argus_app::preferences::save_window(file, saved) {
            argus_core::diagnostics::warn(
                "save window placement",
                argus_core::diagnostics::io(&error),
            );
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if matches!(msg, WM_CLOSE | WM_QUERYENDSESSION | WM_EXITSIZEMOVE) {
        flush();
    }
    let result = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
    if msg == WM_WINDOWPOSCHANGED && !crate::window_motion::changing_visibility() {
        if unsafe { IsWindowVisible(hwnd).as_bool() } {
            prepare_show(hwnd);
        }
        unsafe {
            SetTimer(Some(hwnd), TIMER, 500, Some(settled));
        }
    }
    result
}

unsafe extern "system" fn settled(hwnd: HWND, _: u32, id: usize, _: u32) {
    unsafe {
        let _ = KillTimer(Some(hwnd), id);
    }
    flush();
}
