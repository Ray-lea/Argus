//! The desktop monitor's window, as the system sees it: never activated by
//! a click, dragged by any point unless locked and never past its
//! display's work area, a double click showing
//! Argus and a right click opening its menu; above other windows or among
//! them, translucent or not, and passing clicks through when asked. The
//! interface draws its content (`frontend::monitors`).
//!
//! The pointer is handled here, before GPUI sees it: a drag moves the window
//! by the pointer's travel on the screen, which stays right however fast the
//! pointer goes, and the menu runs its own message loop outside GPUI's event
//! handling, as the tray's menu does.
use super::{HOST, show_main, wide};
use crate::settings::{DeskLayout, DeskStyle, DesktopMonitor, SettingsStore};
use argus_core::tr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL,
    MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint, MonitorFromRect, MonitorFromWindow,
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
    if super::ignores_pointer(message) {
        return super::ignored_pointer(message);
    }
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
                    // Inside the work area of the display under the pointer.
                    let mut client = RECT::default();
                    let _ = unsafe { GetClientRect(hwnd, &mut client) };
                    let (x, y) = inside(
                        press.origin.x + dx,
                        press.origin.y + dy,
                        client.right,
                        client.bottom,
                        work_area(now.x, now.y),
                    );
                    move_client(hwnd, x, y);
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
        // How it looks: a panel in one of its styles, or a strip.
        let Ok(looks) = CreatePopupMenu() else {
            let _ = DestroyMenu(menu);
            return;
        };
        let look_items = [
            (10, tr!("components.desktop.menu.list"), MF_STRING),
            (11, tr!("components.desktop.menu.curves"), MF_STRING),
            (12, tr!("components.desktop.menu.gauges"), MF_STRING),
            (0, String::new(), MF_SEPARATOR),
            (13, tr!("components.desktop.menu.strip"), MF_STRING),
        ];
        let look_texts: Vec<Vec<u16>> = look_items.iter().map(|(_, text, _)| wide(text)).collect();
        for ((id, _, flags), text) in look_items.iter().zip(&look_texts) {
            let text = if *flags == MF_SEPARATOR {
                PCWSTR::null()
            } else {
                PCWSTR(text.as_ptr())
            };
            let _ = AppendMenuW(looks, *flags, *id, text);
        }
        let chosen = match (monitor.layout, monitor.style) {
            (DeskLayout::Strip, _) => 13,
            (DeskLayout::Panel, DeskStyle::List) => 10,
            (DeskLayout::Panel, DeskStyle::Curves) => 11,
            (DeskLayout::Panel, DeskStyle::Gauges) => 12,
        };
        let _ = CheckMenuRadioItem(looks, 10, 13, chosen, MF_BYCOMMAND.0);
        let items = [
            (1, tr!("components.desktop.menu.open"), MF_STRING),
            (0, String::new(), MF_SEPARATOR),
            (
                looks.0 as usize,
                tr!("components.desktop.menu.look"),
                MF_POPUP,
            ),
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
            10 => "look:list",
            11 => "look:curves",
            12 => "look:gauges",
            13 => "look:strip",
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

/// The work area of the display nearest (x, y), physical pixels.
fn work_area(x: i32, y: i32) -> RECT {
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    monitor_work(monitor)
}

/// A display's work area, physical pixels.
fn monitor_work(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> RECT {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let _ = unsafe { GetMonitorInfoW(monitor, &mut info) };
    info.rcWork
}

/// The width of the work area of the display at `at`, or the primary
/// display's, physical pixels.
pub fn work_width(at: Option<(i32, i32)>) -> i32 {
    let work = match at {
        Some((x, y)) => work_area(x, y),
        None => {
            monitor_work(unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) })
        }
    };
    work.right - work.left
}

/// A client area of `width` × `height` at (x, y) moved the least to lie
/// inside `work`; at its top-left corner when it is larger.
pub fn inside(x: i32, y: i32, width: i32, height: i32, work: RECT) -> (i32, i32) {
    (
        x.min(work.right - width).max(work.left),
        y.min(work.bottom - height).max(work.top),
    )
}

/// Where a window at (x, y) goes as its client area turns from
/// `old_width` wide to `width` × `height`: it keeps its right edge when
/// its middle is on the right half of its display, its left edge
/// otherwise, and stays inside that display's work area.
pub fn resized(x: i32, y: i32, old_width: i32, width: i32, height: i32) -> (i32, i32) {
    let work = work_area(x + old_width / 2, y);
    inside(kept(x, old_width, width, work), y, width, height, work)
}

/// The left edge of a window `old_width` wide at `x` that turns `width`
/// wide, its right edge kept on the right half of `work`.
pub fn kept(x: i32, old_width: i32, width: i32, work: RECT) -> i32 {
    if x + old_width / 2 > work.left + (work.right - work.left) / 2 {
        x + old_width - width
    } else {
        x
    }
}

/// The window's client area lies inside its display's work area.
pub fn on_screen(hwnd: HWND) -> bool {
    let work = monitor_work(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) });
    let origin = client_origin(hwnd);
    let mut client = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut client) };
    origin.x >= work.left
        && origin.y >= work.top
        && origin.x + client.right <= work.right
        && origin.y + client.bottom <= work.bottom
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

#[cfg(test)]
mod tests {
    use super::{inside, kept};
    use windows::Win32::Foundation::RECT;

    const WORK: RECT = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1032,
    };

    #[test]
    fn the_monitor_stays_inside_the_work_area() {
        assert_eq!(inside(100, 100, 264, 300, WORK), (100, 100));
        // Past each edge, back to it.
        assert_eq!(inside(-40, -10, 264, 300, WORK), (0, 0));
        assert_eq!(inside(1800, 900, 264, 300, WORK), (1656, 732));
        // Larger than the work area: its top-left corner shows.
        assert_eq!(inside(50, 50, 2400, 300, WORK), (0, 50));
        // On another display, left of the primary one.
        let left = RECT {
            left: -1280,
            top: 0,
            right: 0,
            bottom: 984,
        };
        assert_eq!(inside(-100, 20, 264, 300, left), (-264, 20));
    }

    #[test]
    fn a_resized_monitor_keeps_its_nearer_edge() {
        // On the right half its right edge stays; on the left, its left.
        assert_eq!(kept(1632, 264, 600, WORK), 1296);
        assert_eq!(kept(24, 264, 600, WORK), 24);
        assert_eq!(kept(1296, 600, 264, WORK), 1632);
    }
}
