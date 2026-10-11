//! Resident mode: main window toggling, tray icon and the global hotkey; the
//! window backdrop.

use std::cell::{Cell, RefCell};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COLOR, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL, ICoreWebView2_19, ICoreWebView2Controller,
    ICoreWebView2Controller2,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    HTBOTTOMRIGHT, HTLEFT, WM_EXITSIZEMOVE, WM_NCLBUTTONDOWN, WM_SIZING,
};
use windows::core::{Interface, w};

use crate::settings::SettingsStore;

pub const TRAY_ID: &str = "argus";

/// Window and tray label; dev builds say so, since they can run beside an
/// installed Argus (see tauri.dev.conf.json).
pub const APP_TITLE: &str = if cfg!(debug_assertions) {
    "Argus 开发版"
} else {
    "Argus"
};

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        set_webview_active(&w, true);
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn hide_main<R: Runtime>(w: &WebviewWindow<R>) {
    let _ = w.hide();
    set_webview_active(w, false);
}

/// Hides the window if it is the one in front; otherwise brings it forward.
pub fn toggle_main<R: Runtime>(app: &AppHandle<R>) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let visible = w.is_visible().unwrap_or(false);
    let minimized = w.is_minimized().unwrap_or(false);
    let focused = w.is_focused().unwrap_or(false);
    if visible && !minimized && focused {
        hide_main(&w);
    } else {
        show_main(app);
    }
}

/// Lets the Mica backdrop show through the whole client area.
///
/// Tauri's `transparent: true` would do this with the legacy blur-behind API,
/// which makes DWM alpha-blend the window on every move and visibly drags
/// window moves behind the cursor. Extending the frame into the client area is
/// the Windows 11 way: the window stays a normal opaque window, and only the
/// WebView (background `#00000000` in tauri.conf.json) is transparent.
pub fn extend_backdrop<R: Runtime>(w: &WebviewWindow<R>) {
    use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
    use windows::Win32::UI::Controls::MARGINS;
    if let Ok(hwnd) = w.hwnd() {
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        unsafe {
            let _ = DwmExtendFrameIntoClientArea(HWND(hwnd.0), &margins);
        }
    }
}

thread_local! {
    /// The main window's WebView2 controller, for `resize_proc` (both live on the UI thread).
    static CONTROLLER: RefCell<Option<ICoreWebView2Controller>> = const { RefCell::new(None) };
    static OPAQUE: Cell<bool> = const { Cell::new(false) };
}

/// Keeps a light diagonal line from flickering across the window while it is
/// being resized.
///
/// Until WebView2 has a frame of the new size, Windows shows its last frame
/// stretched to fit, drawn as two triangles; on translucent pixels the shared
/// diagonal is blended twice and shows as a light line. It happens with any
/// translucent page (reproduced with Windows.Graphics.Capture), and opaque
/// pixels look the same however often they are blended. So for the length of a
/// resize the WebView gets an opaque background, the solid color Windows itself
/// shows for Mica on an inactive window, and Mica returns when the resize ends.
pub fn guard_resize_seam<R: Runtime>(w: &WebviewWindow<R>) {
    let _ = w.with_webview(|wv| {
        let controller = wv.controller();
        CONTROLLER.with(|c| *c.borrow_mut() = Some(controller));
    });
    if let Ok(hwnd) = w.hwnd() {
        unsafe {
            let _ = SetWindowSubclass(HWND(hwnd.0), Some(resize_proc), 1, 0);
        }
    }
}

unsafe extern "system" fn resize_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        // Pressing a sizing border arrives before the first size change (the
        // sizing loop runs inside DefSubclassProc), so the WebView can repaint
        // opaque before anything is stretched. WM_SIZING covers keyboard sizing.
        WM_NCLBUTTONDOWN if (HTLEFT..=HTBOTTOMRIGHT).contains(&(wparam.0 as u32)) => {
            set_opaque(true)
        }
        WM_SIZING => set_opaque(true),
        WM_EXITSIZEMOVE => set_opaque(false),
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

fn set_opaque(on: bool) {
    if OPAQUE.replace(on) == on {
        return;
    }
    let color = match (on, apps_use_dark_theme()) {
        (false, _) => COREWEBVIEW2_COLOR {
            A: 0,
            R: 0,
            G: 0,
            B: 0,
        },
        // SolidBackgroundFillColorBase: what Mica falls back to when inactive.
        (true, true) => COREWEBVIEW2_COLOR {
            A: 255,
            R: 0x20,
            G: 0x20,
            B: 0x20,
        },
        (true, false) => COREWEBVIEW2_COLOR {
            A: 255,
            R: 0xF3,
            G: 0xF3,
            B: 0xF3,
        },
    };
    CONTROLLER.with(|c| {
        if let Some(controller) = c.borrow().as_ref()
            && let Ok(controller) = controller.cast::<ICoreWebView2Controller2>()
        {
            unsafe {
                let _ = controller.SetDefaultBackgroundColor(color);
            }
        }
    });
}

/// The app theme Windows uses (and WebView2's prefers-color-scheme follows).
fn apps_use_dark_theme() -> bool {
    let mut light = 1u32;
    let mut size = size_of::<u32>() as u32;
    unsafe {
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut light as *mut u32).cast()),
            Some(&mut size),
        );
    }
    light == 0
}

/// Tells WebView2 whether anyone can see it. Hiding the window alone keeps the
/// renderer painting and holding memory; an invisible controller stops
/// rendering, and the low memory target lets it trim caches. Scripts keep
/// running, so history stays continuous and the page is current on show.
pub fn set_webview_active<R: Runtime>(w: &WebviewWindow<R>, active: bool) {
    let _ = w.with_webview(move |wv| unsafe {
        let controller = wv.controller();
        let _ = controller.SetIsVisible(active);
        if let Ok(core) = controller.CoreWebView2()
            && let Ok(core) = core.cast::<ICoreWebView2_19>()
        {
            let level = if active {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            };
            let _ = core.SetMemoryUsageTargetLevel(level);
        }
    });
}

pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示 Argus", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &PredefinedMenuItem::separator(app)?, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(APP_TITLE)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn set_tray_tooltip<R: Runtime>(app: &AppHandle<R>, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

/// Registers `accelerator` as the show/hide hotkey, replacing `previous`.
pub fn apply_hotkey<R: Runtime>(
    app: &AppHandle<R>,
    previous: Option<&str>,
    accelerator: &str,
) -> Result<(), String> {
    let gs = app.global_shortcut();
    if let Some(prev) = previous.filter(|p| !p.is_empty()) {
        let _ = gs.unregister(prev);
    }
    if accelerator.is_empty() {
        return Ok(());
    }
    if !accelerator.contains('+') {
        return Err("快捷键至少需要一个修饰键（Ctrl、Alt、Shift 或 Win）".into());
    }
    gs.on_shortcut(accelerator, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            toggle_main(app);
        }
    })
    .map_err(|e| format!("无法注册 {accelerator}：{e}。可能已被其他程序占用。"))
}

/// Registers the saved hotkey at startup, remembering why if it fails.
pub fn register_saved_hotkey<R: Runtime>(app: &AppHandle<R>) {
    let store = app.state::<SettingsStore>();
    let hotkey = store.get().hotkey;
    let err = apply_hotkey(app, None, &hotkey).err();
    *store.hotkey_error.lock().unwrap() = err;
}
