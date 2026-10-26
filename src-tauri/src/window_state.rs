//! Remember normal placement rather than minimized/maximized screen bounds.
//! A hidden start stays hidden; maximization is applied at its first showing.

use crate::preferences::{Preferences, WindowPlacement};
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicIsize, Ordering},
};
use tauri::{Manager, Runtime, WebviewWindow};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const SUBCLASS: usize = 1;
const TIMER: usize = 0xA673;
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static MAXIMIZE_ON_SHOW: AtomicBool = AtomicBool::new(false);
static SAVE: OnceLock<Box<dyn Fn(WindowPlacement) + Send + Sync>> = OnceLock::new();

fn placement(hwnd: HWND) -> Option<WINDOWPLACEMENT> {
    let mut placement = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe { GetWindowPlacement(hwnd, &mut placement) }.ok()?;
    Some(placement)
}

pub fn install<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(hwnd) = window.hwnd() else { return };
    let store = window.state::<Preferences>();
    if let Some(saved) = store.window() {
        let [left, top, right, bottom] = saved.rect;
        let restored = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            showCmd: SW_HIDE.0 as u32,
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
            MAXIMIZE_ON_SHOW.store(saved.maximized, Ordering::Relaxed);
        }
    }
    let app = window.app_handle().clone();
    let _ = SAVE.set(Box::new(move |saved| {
        app.state::<Preferences>().save_window(saved)
    }));
    WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(window_proc), SUBCLASS, 0);
    }
}

pub fn prepare_show<R: Runtime>(window: &WebviewWindow<R>) {
    if let Ok(hwnd) = window.hwnd() {
        prepare_hwnd(hwnd);
    }
}

fn prepare_hwnd(hwnd: HWND) {
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

pub fn flush() {
    let hwnd = HWND(WINDOW.load(Ordering::Relaxed) as *mut _);
    if hwnd.0.is_null() || crate::window_motion::changing_visibility() {
        return;
    }
    if let Some(p) = placement(hwnd) {
        let r = p.rcNormalPosition;
        let maximized = MAXIMIZE_ON_SHOW.load(Ordering::Relaxed)
            || unsafe { IsZoomed(hwnd).as_bool() }
            || (unsafe { IsIconic(hwnd).as_bool() } && p.flags.contains(WPF_RESTORETOMAXIMIZED));
        if let Some(save) = SAVE.get() {
            save(WindowPlacement {
                rect: [r.left, r.top, r.right, r.bottom],
                maximized,
            });
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
            prepare_hwnd(hwnd);
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
