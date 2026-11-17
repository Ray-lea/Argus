//! The desktop monitor's window, as the system sees it: never activated by
//! a click, dragged by any point unless locked, a double click showing
//! Argus and a right click opening its menu; above other windows or among
//! them, translucent or not, and passing clicks through when asked. The
//! interface draws its content (`frontend::monitors`).
//!
//! The pointer is handled here, before GPUI sees it: a drag moves the window
//! by the pointer's travel on the screen, which stays right however fast the
//! pointer goes, and the menu runs its own message loop outside GPUI's event
//! handling, as the tray's menu does.
use super::{HOST, show_main, wide};
use crate::settings::{DesktopMonitor, SettingsStore};
use argus_core::tr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, MonitorFromRect,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetDoubleClickTime, ReleaseCapture, SetCapture};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

/// The window cannot be dragged.
static LOCKED: AtomicBool = AtomicBool::new(false);
/// A press on the window: where the pointer and the window's client area
/// were (screen pixels), and whether it has moved the window yet.
static PRESS: Mutex<Option<Press>> = Mutex::new(None);
/// When the last click without a drag ended, for a double click.
static CLICKED: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct Press {
    pointer: POINT,
    origin: POINT,
    moved: bool,
}

/// How far the pointer travels before a press becomes a drag.
fn drag_threshold() -> (i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_CXDRAG).max(1),
            GetSystemMetrics(SM_CYDRAG).max(1),
        )
    }
}

fn cursor() -> POINT {
    let mut point = POINT::default();
    let _ = unsafe { GetCursorPos(&mut point) };
    point
}

/// The client area's top-left corner on the screen.
fn client_origin(hwnd: HWND) -> POINT {
    let mut origin = POINT::default();
    let _ = unsafe { ClientToScreen(hwnd, &mut origin) };
    origin
}

fn emit(value: serde_json::Value) {
    if let Some(app) = HOST.get() {
        let _ = app.emit("argus-desktop-monitor", value);
    }
}

/// Handles the pointer and activation of the monitor's window before GPUI.
pub fn install(hwnd: HWND) {
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(procedure), 0x44_534B, 0);
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match message {
        // Clicks leave the active window active.
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN => {
            *PRESS.lock().unwrap() = Some(Press {
                pointer: cursor(),
                origin: client_origin(hwnd),
                moved: false,
            });
            unsafe { SetCapture(hwnd) };
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let mut press = PRESS.lock().unwrap();
            if let Some(press) = press.as_mut()
                && !LOCKED.load(Ordering::Relaxed)
            {
                let now = cursor();
                let (dx, dy) = (now.x - press.pointer.x, now.y - press.pointer.y);
                let (tx, ty) = drag_threshold();
                if press.moved || dx.abs() >= tx || dy.abs() >= ty {
                    press.moved = true;
                    move_client(hwnd, press.origin.x + dx, press.origin.y + dy);
                }
                return LRESULT(0);
            }
        }
        WM_LBUTTONUP => {
            let press = PRESS.lock().unwrap().take();
            unsafe {
                let _ = ReleaseCapture();
            }
            match press {
                Some(press) if press.moved => {
                    let origin = client_origin(hwnd);
                    emit(serde_json::json!({ "moved": [origin.x, origin.y] }));
                }
                Some(_) => {
                    let mut clicked = CLICKED.lock().unwrap();
                    let double = unsafe { GetDoubleClickTime() } as u128;
                    if clicked.is_some_and(|at| at.elapsed().as_millis() <= double) {
                        *clicked = None;
                        drop(clicked);
                        if let Some(app) = HOST.get() {
                            show_main(app);
                        }
                    } else {
                        *clicked = Some(Instant::now());
                    }
                }
                None => {}
            }
            return LRESULT(0);
        }
        WM_CAPTURECHANGED => {
            // Lost to another window mid-drag: where it got to stays.
            if let Some(press) = PRESS.lock().unwrap().take()
                && press.moved
            {
                let origin = client_origin(hwnd);
                emit(serde_json::json!({ "moved": [origin.x, origin.y] }));
            }
        }
        WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => return LRESULT(0),
        WM_RBUTTONUP => {
            menu(hwnd);
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, message, w, l) }
}

/// Moves the window so that its client area's top-left corner is at (x, y).
fn move_client(hwnd: HWND, x: i32, y: i32) {
    let origin = client_origin(hwnd);
    let mut frame = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut frame) };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            frame.left + x - origin.x,
            frame.top + y - origin.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// The monitor's menu at the pointer. Showing Argus happens here; the
/// interface changes the settings for the other items.
fn menu(hwnd: HWND) {
    let Some(app) = HOST.get() else {
        return;
    };
    let monitor = app.state::<SettingsStore>().get().desktop_monitor;
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let check = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
        let items = [
            (1, tr!("components.desktop.menu.open"), MF_STRING),
            (0, String::new(), MF_SEPARATOR),
            (
                2,
                tr!("components.desktop.menu.topmost"),
                MF_STRING | check(monitor.topmost),
            ),
            (
                3,
                tr!("components.desktop.menu.locked"),
                MF_STRING | check(monitor.locked),
            ),
            (0, String::new(), MF_SEPARATOR),
            (4, tr!("components.desktop.menu.settings"), MF_STRING),
            (5, tr!("components.desktop.menu.hide"), MF_STRING),
        ];
        let texts: Vec<Vec<u16>> = items.iter().map(|(_, text, _)| wide(text)).collect();
        for ((id, _, flags), text) in items.iter().zip(&texts) {
            let text = if *flags == MF_SEPARATOR {
                PCWSTR::null()
            } else {
                PCWSTR(text.as_ptr())
            };
            let _ = AppendMenuW(menu, *flags, *id, text);
        }
        let point = cursor();
        let _ = SetForegroundWindow(hwnd);
        let selected = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        )
        .0;
        let _ = DestroyMenu(menu);
        let item = match selected {
            1 => {
                show_main(app);
                return;
            }
            2 => "topmost",
            3 => "locked",
            4 => "settings",
            5 => "hide",
            _ => return,
        };
        emit(serde_json::json!({ "menu": item }));
    }
}

/// Where the monitor opens, physical pixels: where it was left, if that is
/// still on a display, or else `margin` inside the top-right corner of the
/// primary display's work area. `width` and `height` are its client size.
pub fn origin(position: Option<[i32; 2]>, width: i32, height: i32, margin: i32) -> (i32, i32) {
    if let Some([x, y]) = position {
        let rect = RECT {
            left: x,
            top: y,
            right: x + width.max(1),
            bottom: y + height.max(1),
        };
        if !unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) }.is_invalid() {
            return (x, y);
        }
    }
    let monitor = unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let _ = unsafe { GetMonitorInfoW(monitor, &mut info) };
    (info.rcWork.right - width - margin, info.rcWork.top + margin)
}

/// Gives the window a client area of `width` × `height` at (x, y) and the
/// monitor's settings, and shows it without activating it.
pub fn show(hwnd: HWND, monitor: &DesktopMonitor, x: i32, y: i32, width: i32, height: i32) {
    LOCKED.store(monitor.locked, Ordering::Relaxed);
    let alpha = (f32::from(monitor.opacity.clamp(10, 100)) * 2.55).round() as u8;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let through = WS_EX_TRANSPARENT.0 as isize;
        let layered = WS_EX_LAYERED.0 as isize;
        // Clicks pass through a layered window marked transparent.
        let wanted = if monitor.click_through {
            style | through | layered
        } else if alpha < 255 {
            (style & !through) | layered
        } else {
            style & !through & !layered
        };
        if wanted != style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
        }
        if wanted & layered != 0 {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        }
        let mut frame = RECT::default();
        let mut client = RECT::default();
        let _ = GetWindowRect(hwnd, &mut frame);
        let _ = GetClientRect(hwnd, &mut client);
        let origin = client_origin(hwnd);
        // A test instance's monitor stays beneath the desktop's windows.
        let test = std::env::args().any(|arg| arg == "--test");
        let _ = SetWindowPos(
            hwnd,
            Some(if test {
                HWND_BOTTOM
            } else if monitor.topmost {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            }),
            x - (origin.x - frame.left),
            y - (origin.y - frame.top),
            width + (frame.right - frame.left) - client.right,
            height + (frame.bottom - frame.top) - client.bottom,
            SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
        );
    }
}

/// The client area's top-left corner, physical pixels.
pub fn position(hwnd: HWND) -> (i32, i32) {
    let origin = client_origin(hwnd);
    (origin.x, origin.y)
}
