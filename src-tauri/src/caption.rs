//! The window has no system title bar: the page's toolbar is the caption and
//! draws its own minimize, maximize and close buttons (`decorations: false`;
//! tao keeps the frame's shadow, rounded corners, snapping and side resizing).
//!
//! Windows 11 offers snap layouts when the pointer rests on a window's
//! maximize button, which it finds by `WM_NCHITTEST` returning `HTMAXBUTTON`.
//! WebView2 answers hit tests for the whole page itself, so a transparent child
//! window lies over the page's maximize button and answers for it. Clicks on it
//! maximize or restore the window; hover and press are passed to the page,
//! which draws the button.
//!
//! On Windows 11 DWM paints a band along the top edge (8 px at 100% scale),
//! partly behind the transparent page, in the caption color. That is normally
//! Mica, like the rest of the window; with "Show accent color on title bars and
//! window borders" it is the accent color, a colored strip over the toolbar.
//! While that setting is on, the caption color is set to `DWMWA_COLOR_NONE`:
//! no fill, so the band shows the Mica behind it, active or not. (A solid
//! color cannot stand in for Mica, which takes on the wallpaper's tint. Taking
//! back the 1 px of non-client area tao leaves at the top is no way out either:
//! DWM then paints a whole caption, system buttons included, behind the page.)

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Runtime, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, NULL_BRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, HTMAXBUTTON, HWND_TOP, IsZoomed, PostMessageW,
    RegisterClassW, SC_MAXIMIZE, SC_RESTORE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos,
    WINDOW_EX_STYLE, WM_DPICHANGED, WM_DWMCOLORIZATIONCOLORCHANGED, WM_NCHITTEST,
    WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
    WM_SETTINGCHANGE, WM_SIZE, WM_SYSCOMMAND, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};
use windows::core::w;

/// Where the page draws its maximize button, in CSS pixels: `right` is the
/// distance from the right edge of the window to the button's right edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct ButtonRect {
    right: f64,
    top: f64,
    width: f64,
    height: f64,
}

const SUBCLASS: usize = 2;
/// `DWMWA_COLOR_NONE`: no border at all, not even the accent-colored one; as
/// the caption color, no fill.
const COLOR_NONE: u32 = 0xFFFF_FFFE;
/// `DWMWA_COLOR_DEFAULT`: the system's own caption color.
const COLOR_DEFAULT: u32 = 0xFFFF_FFFF;

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static OVERLAY: AtomicIsize = AtomicIsize::new(0);
static RECT_CSS: Mutex<Option<ButtonRect>> = Mutex::new(None);
/// Sends hover and press of the maximize button to the page.
static NOTIFY: OnceLock<Box<dyn Fn(&'static str) + Send + Sync>> = OnceLock::new();
/// Pointer state over the overlay; only the UI thread touches it.
static STATE: Mutex<(bool, bool)> = Mutex::new((false, false));

/// Removes the frame's border and puts the snap layout overlay in place.
pub fn install<R: Runtime>(window: &WebviewWindow<R>, app: &AppHandle<R>) {
    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd = HWND(hwnd.0);
    let none = COLOR_NONE;
    unsafe {
        // Windows 10 has no such attribute and keeps its thin border.
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            (&raw const none).cast(),
            size_of::<u32>() as u32,
        );
    }
    let app = app.clone();
    let _ = NOTIFY.set(Box::new(move |state| {
        let _ = app.emit("argus-caption-maximize", state);
    }));
    WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
    paint_caption();
    unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(overlay_proc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            hbrBackground: HBRUSH(GetStockObject(NULL_BRUSH).0),
            lpszClassName: w!("ArgusCaptionMaximize"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let Ok(overlay) = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("ArgusCaptionMaximize"),
            None,
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(hwnd),
            None,
            Some(class.hInstance),
            None,
        ) else {
            return;
        };
        OVERLAY.store(overlay.0 as isize, Ordering::Relaxed);
        let _ = SetWindowSubclass(hwnd, Some(window_proc), SUBCLASS, 0);
    }
    place();
}

/// The page reports where its maximize button is (and again when that changes).
pub fn set_maximize_button(rect: ButtonRect) {
    *RECT_CSS.lock().unwrap() = Some(rect);
    place();
}

/// "Show accent color on title bars and window borders".
fn accent_on_title_bars() -> bool {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\DWM"),
            w!("ColorPrevalence"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    read.is_ok() && value != 0
}

/// Keeps the accent color off the band along the top edge; see the module docs.
fn paint_caption() {
    let window = HWND(WINDOW.load(Ordering::Relaxed) as _);
    if window.0.is_null() {
        return;
    }
    let color = if accent_on_title_bars() {
        COLOR_NONE
    } else {
        COLOR_DEFAULT
    };
    unsafe {
        // Windows 10 has no such attribute.
        let _ = DwmSetWindowAttribute(
            window,
            DWMWA_CAPTION_COLOR,
            (&raw const color).cast(),
            size_of::<u32>() as u32,
        );
    }
}

/// Lays the overlay over the page's maximize button, above the WebView.
fn place() {
    let window = HWND(WINDOW.load(Ordering::Relaxed) as _);
    let overlay = HWND(OVERLAY.load(Ordering::Relaxed) as _);
    let Some(rect) = *RECT_CSS.lock().unwrap() else {
        return;
    };
    if overlay.0.is_null() {
        return;
    }
    let mut client = RECT::default();
    unsafe {
        if GetClientRect(window, &mut client).is_err() {
            return;
        }
        let scale = f64::from(GetDpiForWindow(window)) / 96.0;
        let px = |v: f64| (v * scale).round() as i32;
        let _ = SetWindowPos(
            overlay,
            Some(HWND_TOP),
            client.right - px(rect.right + rect.width),
            px(rect.top),
            px(rect.width),
            px(rect.height),
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    match msg {
        WM_SIZE | WM_DPICHANGED => place(),
        // The accent setting may have changed.
        WM_SETTINGCHANGE | WM_DWMCOLORIZATIONCOLORCHANGED => paint_caption(),
        _ => {}
    }
    result
}

fn notify(state: &'static str) {
    if let Some(notify) = NOTIFY.get() {
        notify(state);
    }
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => return LRESULT(HTMAXBUTTON as isize),
        WM_NCMOUSEMOVE => {
            let mut state = STATE.lock().unwrap();
            if !state.0 {
                state.0 = true;
                drop(state);
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = unsafe { TrackMouseEvent(&mut track) };
                notify("hover");
            }
            return LRESULT(0);
        }
        WM_NCMOUSELEAVE => {
            *STATE.lock().unwrap() = (false, false);
            notify("leave");
            return LRESULT(0);
        }
        // The system's own caption button handling would run a tracking loop
        // on this child window; the press and release are handled here.
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
            STATE.lock().unwrap().1 = true;
            notify("press");
            return LRESULT(0);
        }
        WM_NCLBUTTONUP => {
            let pressed = std::mem::replace(&mut STATE.lock().unwrap().1, false);
            notify("hover");
            if pressed {
                let window = HWND(WINDOW.load(Ordering::Relaxed) as _);
                let command = if unsafe { IsZoomed(window) }.as_bool() {
                    SC_RESTORE
                } else {
                    SC_MAXIMIZE
                };
                let _ = unsafe {
                    PostMessageW(
                        Some(window),
                        WM_SYSCOMMAND,
                        WPARAM(command as usize),
                        LPARAM(0),
                    )
                };
            }
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
