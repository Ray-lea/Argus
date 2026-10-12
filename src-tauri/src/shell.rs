//! Resident mode: main window toggling, tray icon and the global hotkey; the
//! window backdrop.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use argus_core::tr;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    ICoreWebView2_19,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, RGN_OR, SetWindowRgn,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, KillTimer, SWP_NOSIZE, SetTimer, WINDOWPOS, WM_WINDOWPOSCHANGING,
};
use windows::core::Interface;

use crate::settings::SettingsStore;

pub const TRAY_ID: &str = "argus";

/// Window and tray label; dev builds say so, since they can run beside an
/// installed Argus (see tauri.dev.conf.json).
pub fn app_title() -> String {
    if cfg!(debug_assertions) {
        tr!("app.devTitle")
    } else {
        "Argus".into()
    }
}

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

/// The main window's WebView container; only the UI thread uses these.
static CONTAINER: AtomicIsize = AtomicIsize::new(0);
static CUT: AtomicBool = AtomicBool::new(false);

const SEAM_SUBCLASS: usize = 1;
const SEAM_TIMER: usize = 0x5EA7;
/// How long the cut stays after the last size change: the WebView catches up
/// with a new size within a few frames.
const SEAM_SETTLE_MS: u32 = 300;

/// Keeps a light diagonal line from flickering across the window while it is
/// being resized, without changing how anything looks.
///
/// Windows 11 clips the window to rounded corners. WebView2 draws into a
/// translucent child surface (Chromium's layered output window) that reaches
/// into the bottom corners, and while the window changes size DWM's fast path
/// for that clip sometimes blends such a surface twice along the client
/// diagonal. The fast path only takes children whose visible area is a plain
/// rectangle: in a bare WebView2 window, square corners, a WebView kept out of
/// the bottom corners and a non-rectangular visible area each removed the line.
/// So from the first size change until shortly after the last, the WebView's
/// container gets a region without its top-left pixel, which is page
/// background (Mica shows through it either way). DWM's general path differs
/// only in the corner anti-aliasing, by a few levels on a few pixels, but costs
/// it a little more per frame, so the region is not kept on.
pub fn guard_resize_seam<R: Runtime>(w: &WebviewWindow<R>) {
    let Ok(hwnd) = w.hwnd() else { return };
    let hwnd = hwnd.0 as isize;
    let _ = w.with_webview(move |wv| unsafe {
        let mut container = HWND::default();
        if wv.controller().ParentWindow(&mut container).is_ok() {
            CONTAINER.store(container.0 as isize, Ordering::Relaxed);
            let _ = SetWindowSubclass(HWND(hwnd as _), Some(seam_proc), SEAM_SUBCLASS, 0);
        }
    });
}

unsafe extern "system" fn seam_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // Before the new size is applied, so the region is in place for the first
    // frame at that size. Covers dragging, snapping, maximizing and DPI changes.
    if msg == WM_WINDOWPOSCHANGING {
        let pos = unsafe { &*(lparam.0 as *const WINDOWPOS) };
        let mut rect = RECT::default();
        if !pos.flags.contains(SWP_NOSIZE)
            && unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok()
            && (pos.cx, pos.cy) != (rect.right - rect.left, rect.bottom - rect.top)
        {
            set_corner_cut(true);
            unsafe { SetTimer(Some(hwnd), SEAM_TIMER, SEAM_SETTLE_MS, Some(seam_settled)) };
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

unsafe extern "system" fn seam_settled(hwnd: HWND, _msg: u32, id: usize, _time: u32) {
    let _ = unsafe { KillTimer(Some(hwnd), id) };
    set_corner_cut(false);
}

fn set_corner_cut(on: bool) {
    if CUT.swap(on, Ordering::Relaxed) == on {
        return;
    }
    let container = HWND(CONTAINER.load(Ordering::Relaxed) as _);
    unsafe {
        if !on {
            SetWindowRgn(container, None, true);
            return;
        }
        // Far past any window size, so it needs no update while the size changes.
        const FAR: i32 = 1 << 20;
        let region = CreateRectRgn(0, 1, FAR, FAR);
        let top = CreateRectRgn(1, 0, FAR, 1);
        CombineRgn(Some(region), Some(region), Some(top), RGN_OR);
        let _ = DeleteObject(top.into());
        // On success the window owns the region.
        if SetWindowRgn(container, Some(region), true) == 0 {
            let _ = DeleteObject(region.into());
        }
    }
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

/// The tray menu, with "restart to update" while an update is ready.
fn tray_menu<R: Runtime>(app: &AppHandle<R>, update: Option<&str>) -> tauri::Result<Menu<R>> {
    let show = MenuItem::with_id(app, "show", tr!("tray.show"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr!("tray.quit"), true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    match update {
        Some(version) => {
            let text = tr!("tray.update", version = version);
            let update = MenuItem::with_id(app, "update", text, true, None::<&str>)?;
            Menu::with_items(app, &[&show, &update, &separator, &quit])
        }
        None => Menu::with_items(app, &[&show, &separator, &quit]),
    }
}

/// The version the tray menu currently offers to install.
static TRAY_UPDATE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn set_tray_update<R: Runtime>(app: &AppHandle<R>, version: Option<&str>) {
    let mut current = TRAY_UPDATE.lock().unwrap();
    if current.as_deref() == version {
        return;
    }
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), tray_menu(app, version)) {
        let _ = tray.set_menu(Some(menu));
        *current = version.map(str::to_string);
    }
}

/// Redoes the native text after a language change: tray menu, window title
/// and the hotkey's error. The tray tooltip follows with the next sample.
pub fn apply_language<R: Runtime>(app: &AppHandle<R>) {
    let update = TRAY_UPDATE.lock().unwrap().clone();
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), tray_menu(app, update.as_deref())) {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(app_title()));
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(&app_title());
    }
    let store = app.state::<SettingsStore>();
    if store.hotkey_error.lock().unwrap().is_some() {
        let hotkey = store.get().hotkey;
        let err = apply_hotkey(app, Some(&hotkey), &hotkey).err();
        *store.hotkey_error.lock().unwrap() = err;
    }
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(app_title())
        .menu(&tray_menu(app, None)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "update" => {
                // install() blocks while it starts the installer and queries the
                // window; keep it off the event loop thread.
                let app = app.clone();
                std::thread::spawn(move || {
                    if crate::updater::install(&app).is_err() {
                        show_main(&app);
                        let _ = app.emit("argus-open-settings", "update");
                    }
                });
            }
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
        return Err(tr!("hotkey.needsModifier"));
    }
    gs.on_shortcut(accelerator, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            toggle_main(app);
        }
    })
    .map_err(|e| {
        let label = shortcut_label(accelerator);
        if e.to_string().contains("already registered") {
            tr!("hotkey.taken", shortcut = label)
        } else {
            tr!("hotkey.failed", shortcut = label, error = e)
        }
    })
}

/// How a shortcut reads in messages: "Ctrl+Alt+Esc" rather than "Ctrl+Alt+Escape".
fn shortcut_label(accelerator: &str) -> String {
    accelerator
        .split('+')
        .map(|part| match part {
            "Super" => "Win",
            "Escape" => "Esc",
            _ => part
                .strip_prefix("Key")
                .or_else(|| part.strip_prefix("Digit"))
                .filter(|rest| rest.len() == 1)
                .unwrap_or(part),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Registers the saved hotkey at startup, remembering why if it fails.
pub fn register_saved_hotkey<R: Runtime>(app: &AppHandle<R>) {
    let store = app.state::<SettingsStore>();
    let hotkey = store.get().hotkey;
    let err = apply_hotkey(app, None, &hotkey).err();
    *store.hotkey_error.lock().unwrap() = err;
}
