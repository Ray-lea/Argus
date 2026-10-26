//! Resident mode: main window toggling and the tray icon (the hotkey that
//! toggles it is in hotkey.rs), with WebView2 asleep while the window is out
//! of sight; the window backdrop.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, Ordering};
use std::time::Instant;

use argus_core::tr;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    ICoreWebView2_19, ICoreWebView2Controller,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, RGN_OR, SetWindowRgn,
};
use windows::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsIconic, IsWindowVisible, KillTimer, SWP_NOSIZE, SetTimer, WINDOWPOS,
    WM_NCDESTROY, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING,
};
use windows::core::Interface;

use crate::sampler::{Control, WindowState};
use crate::settings::Theme;

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
        crate::window_state::prepare_show(&w);
        crate::window_motion::show(&w);
        let _ = w.set_focus();
    }
    crate::hotkey::retry_failed(app);
}

pub fn hide_main<R: Runtime>(w: &WebviewWindow<R>) {
    crate::window_motion::hide(w);
    // The settings page cannot end a hotkey recording it can no longer see.
    crate::hotkey::resume_if_suspended(w.app_handle());
}

/// What the window is doing, as the sampling cadence and the WebView's sleep
/// see it.
pub fn window_state(hwnd: HWND) -> WindowState {
    if crate::window_motion::changing_visibility() {
        return match VIEW.load(Ordering::Relaxed) {
            state if state == WindowState::Shown as u8 => WindowState::Shown,
            state if state == WindowState::Minimized as u8 => WindowState::Minimized,
            _ => WindowState::Hidden,
        };
    }
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            WindowState::Hidden
        } else if IsIconic(hwnd).as_bool() {
            WindowState::Minimized
        } else {
            WindowState::Shown
        }
    }
}

/// How the window was out of sight, which sets the sampling cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Away {
    Minimized,
    /// Hidden in the tray, or started there.
    Tray,
}

/// Since when, and how, the window is out of sight; `follow_visibility` keeps
/// it. Whatever brings the window back reports it once (`argus-restored`);
/// hiding a minimized window counts as the tray from then on. No time: hidden
/// only until a start shows it (`started_hidden`), which nobody saw as away.
static AWAY: Mutex<Option<(Away, Option<Instant>)>> = Mutex::new(None);

fn went_away(how: Away) {
    let mut away = AWAY.lock().unwrap();
    match *away {
        None => *away = Some((how, Some(Instant::now()))),
        Some((_, since)) if how == Away::Tray => *away = Some((Away::Tray, since)),
        Some(_) => {}
    }
}

/// A start that keeps the window hidden: in the tray until summoned, or, by
/// the task manager replacement (`shown_when_ready`), only until the page is
/// ready. The first showing reports `argus-restored` either way, which also
/// tells the page the window is on screen (`shown` in updates.ts); one that
/// was due right away reports no time away, so the page does not say the
/// refresh rate was lower in the tray while nobody had seen the window yet.
pub fn started_hidden(shown_when_ready: bool) {
    *AWAY.lock().unwrap() = Some((Away::Tray, (!shown_when_ready).then(Instant::now)));
}

/// The window is back in view: tells the page how it was away and for how
/// long, so it can say the refresh rate was lower meanwhile, and samples at
/// once rather than leaving a frame up to a background cadence old on screen
/// (`Focused(true)` does too, but a window restored without activation never
/// gets focus).
fn came_back<R: Runtime>(app: &AppHandle<R>) {
    #[derive(Clone, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Restored {
        from: Away,
        away_ms: u64,
    }
    let Some((from, away_ms)) = take_away() else {
        return;
    };
    if let Some(state) = app.try_state::<crate::AppState>() {
        state.sampler.send(Control::Refresh);
    }
    let _ = app.emit("argus-restored", Restored { from, away_ms });
}

/// How the window was away and for how long (ms), once.
fn take_away() -> Option<(Away, u64)> {
    let (from, since) = AWAY.lock().unwrap().take()?;
    Some((
        from,
        since.map_or(0, |since| since.elapsed().as_millis() as u64),
    ))
}

/// Hides the window if it is the one in front; otherwise brings it forward.
pub fn toggle_main<R: Runtime>(app: &AppHandle<R>) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let visible = w.is_visible().unwrap_or(false);
    let minimized = w.is_minimized().unwrap_or(false);
    let focused = w.is_focused().unwrap_or(false);
    if visible && !minimized && focused && !crate::window_motion::is_leaving() {
        hide_main(&w);
    } else {
        show_main(app);
    }
}

/// Mica starts with Windows 11; extending the frame on Windows 10 exposes
/// the desktop through the WebView instead of providing a window backdrop.
pub fn supports_mica() -> bool {
    use windows::Win32::Foundation::NTSTATUS;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut OSVERSIONINFOW) -> NTSTATUS;
    }
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    unsafe { RtlGetVersion(&mut info).is_ok() && info.dwBuildNumber >= 22000 }
}

/// Lets the Mica backdrop show through the whole client area.
///
/// Tauri's `transparent: true` would do this with the legacy blur-behind API,
/// which makes DWM alpha-blend the window on every move and visibly drags
/// window moves behind the cursor. Extending the frame into the client area is
/// the Windows 11 way: the window stays a normal opaque window, and only the
/// WebView (background `#00000000` in tauri.conf.json) is transparent.
///
/// The frame grows in from the left edge only, wider than any window, rather
/// than all round (margins of -1). An extension along the top lets DWM draw
/// its own caption buttons behind the page wherever the client area starts at
/// the window's visible top, which a maximized window's does (tao gives it the
/// monitor's work area): two sets of buttons showed. Without a top band DWM
/// draws none, and the window is otherwise the same to the pixel, Mica included.
pub fn extend_backdrop<R: Runtime>(w: &WebviewWindow<R>) {
    use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
    use windows::Win32::UI::Controls::MARGINS;
    if !supports_mica() {
        return;
    }
    if let Ok(hwnd) = w.hwnd() {
        let margins = MARGINS {
            cxLeftWidth: i32::MAX,
            cxRightWidth: 0,
            cyTopHeight: 0,
            cyBottomHeight: 0,
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
    if !supports_mica() {
        return;
    }
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

const VISIBILITY_SUBCLASS: usize = 1;
/// How long a page gets to report its first frame before its WebView sleeps
/// out of sight anyway.
const PAINT_GRACE: std::time::Duration = std::time::Duration::from_secs(60);

/// Whether WebView2 was last told it is visible. wry creates the controller
/// visible, whether the window is shown or not.
static AWAKE: AtomicBool = AtomicBool::new(true);
/// The window state `visibility_proc` saw last (`WindowState as u8`).
static VIEW: AtomicU8 = AtomicU8::new(WindowState::Hidden as u8);
/// Whether the page has drawn itself once. Until then the WebView stays awake
/// even in a hidden window: put to sleep before its first frame, it has
/// nothing to show when the window appears (an empty window for about 0.2 s),
/// while once it has drawn it keeps its last frame and shows that at once.
static PAINTED: AtomicBool = AtomicBool::new(false);

/// Tells WebView2 from now on whether anyone can see the window: not while it
/// is hidden (in the tray, or started there) or minimized, once the page has
/// drawn itself (`painted`). Hiding the window alone keeps the renderer
/// painting and holding memory; an invisible controller stops rendering, and
/// the low memory target lets it trim caches. Scripts keep running, so history
/// stays continuous and the page is current on show.
///
/// Follows the window rather than the code that shows it: every show, hide,
/// minimize and restore ends in `WM_WINDOWPOSCHANGED`, whoever causes it
/// (`show_main`, the taskbar, another program's `SetWindowPos`), and a window
/// started hidden goes to sleep as soon as its page has drawn. The same
/// changes mark the window away (`went_away`) and back (`came_back`), so a
/// restore without activation or a show by another program also reports
/// `argus-restored` and samples at once.
pub fn follow_visibility<R: Runtime>(w: &WebviewWindow<R>) {
    let Ok(hwnd) = w.hwnd() else { return };
    let hwnd = hwnd.0 as isize;
    let app = w.app_handle().clone();
    let _ = w.with_webview(move |wv| unsafe {
        let hwnd = HWND(hwnd as _);
        VIEW.store(window_state(hwnd) as u8, Ordering::Relaxed);
        let follower = Box::into_raw(Box::new(Follower {
            controller: wv.controller(),
            moved: Box::new(move |state| match state {
                WindowState::Shown => came_back(&app),
                WindowState::Minimized => went_away(Away::Minimized),
                WindowState::Hidden => went_away(Away::Tray),
            }),
        }));
        let data = follower as usize;
        if SetWindowSubclass(hwnd, Some(visibility_proc), VISIBILITY_SUBCLASS, data).as_bool() {
            sync_visibility(hwnd, &(*follower).controller);
        } else {
            drop(Box::from_raw(follower));
        }
    });
    // A page that never reports its first frame sleeps out of sight anyway.
    let app = w.app_handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(PAINT_GRACE);
        if let Some(w) = app.get_webview_window("main") {
            painted(&w);
        }
    });
}

/// The page has drawn itself once (`page_painted`), or did not say so in
/// time: from now on its WebView sleeps whenever the window is out of sight.
pub fn painted<R: Runtime>(w: &WebviewWindow<R>) {
    if PAINTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let Ok(hwnd) = w.hwnd() else { return };
    let hwnd = hwnd.0 as isize;
    let _ = w.with_webview(move |wv| sync_visibility(HWND(hwnd as _), &wv.controller()));
}

/// What `visibility_proc` works with, on the UI thread only.
struct Follower {
    controller: ICoreWebView2Controller,
    /// Called when the window state changes.
    moved: Box<dyn Fn(WindowState)>,
}

unsafe extern "system" fn visibility_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let follower = data as *mut Follower;
    if msg == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(visibility_proc), VISIBILITY_SUBCLASS);
            drop(Box::from_raw(follower));
        }
        return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    }
    let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    // After the window's own handling, which gives the WebView its new size.
    if msg == WM_WINDOWPOSCHANGED && !crate::window_motion::changing_visibility() {
        follow_change(hwnd, unsafe { &*follower });
    }
    result
}

/// On the window thread, after a group of native calls settles. Intermediate
/// style changes must not put WebView2 to sleep or report a second restoration.
pub fn resync_visibility(hwnd: HWND) {
    let mut data = 0;
    if unsafe {
        GetWindowSubclass(
            hwnd,
            Some(visibility_proc),
            VISIBILITY_SUBCLASS,
            Some(&mut data),
        )
    }
    .as_bool()
    {
        follow_change(hwnd, unsafe { &*(data as *const Follower) });
    }
}

fn follow_change(hwnd: HWND, follower: &Follower) {
    sync_visibility(hwnd, &follower.controller);
    // The WebView is awake by now when the window came back.
    let state = window_state(hwnd);
    if VIEW.swap(state as u8, Ordering::Relaxed) != state as u8 {
        (follower.moved)(state);
    }
}

fn sync_visibility(hwnd: HWND, controller: &ICoreWebView2Controller) {
    let awake = !PAINTED.load(Ordering::Relaxed) || window_state(hwnd) == WindowState::Shown;
    if AWAKE.swap(awake, Ordering::Relaxed) == awake {
        return;
    }
    unsafe {
        let _ = controller.SetIsVisible(awake);
        if let Ok(core) = controller.CoreWebView2()
            && let Ok(core) = core.cast::<ICoreWebView2_19>()
        {
            let level = if awake {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            };
            let _ = core.SetMemoryUsageTargetLevel(level);
        }
    }
}

/// The tray menu, with an entry for the update dialog while an update waits.
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

/// The version the tray currently offers. Held only to read or change it,
/// never across a tray or menu call: off the main thread those wait for it,
/// and the main thread takes this lock too (`apply_language` for
/// `set_language`, `set_tray_update` for `set_update_settings`), so the two
/// would wait for each other forever.
static TRAY_UPDATE: Mutex<Option<String>> = Mutex::new(None);

/// Offers an update in the tray (menu entry, a dot on the icon, a line in the
/// tooltip), or stops offering one. An update kept from an earlier run can be
/// offered before the tray exists; `build_tray` picks it up.
pub fn set_tray_update<R: Runtime>(app: &AppHandle<R>, version: Option<&str>) {
    {
        let mut current = TRAY_UPDATE.lock().unwrap();
        if current.as_deref() == version {
            return;
        }
        *current = version.map(str::to_string);
    }
    // On the main thread (at once when called there, else queued without
    // waiting), from the offer current by then: changes in quick succession
    // and a new tray cannot pass each other.
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || show_tray_update(&handle));
}

/// Puts the current offer on the tray, if there is one yet. Main thread.
fn show_tray_update<R: Runtime>(app: &AppHandle<R>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let version = TRAY_UPDATE.lock().unwrap().clone();
    if let Ok(menu) = tray_menu(app, version.as_deref()) {
        let _ = tray.set_menu(Some(menu));
    }
    let icon = match version {
        Some(_) => badged_icon(app),
        None => app.default_window_icon().cloned(),
    };
    if icon.is_some() {
        let _ = tray.set_icon(icon);
    }
}

/// The tray tooltip: the app, the usage line, and an offered update.
pub fn tray_tooltip(usage: &str) -> String {
    let mut tip = format!("{}\n{usage}", app_title());
    if let Some(version) = TRAY_UPDATE.lock().unwrap().as_deref() {
        tip.push('\n');
        tip.push_str(&tr!("tray.updateReady", version = version));
    }
    tip
}

/// The app icon with an accent dot in its lower right corner, cut out of the
/// icon by a thin transparent ring so it reads on any part of it.
fn badged_icon<R: Runtime>(app: &AppHandle<R>) -> Option<tauri::image::Image<'static>> {
    let icon = app.default_window_icon()?;
    let (width, height) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    draw_badge(&mut rgba, width, height, badge_color());
    Some(tauri::image::Image::new_owned(rgba, width, height))
}

/// Draws the dot onto straight-alpha RGBA pixels, edges anti-aliased.
fn draw_badge(rgba: &mut [u8], width: u32, height: u32, [r, g, b]: [u8; 3]) {
    let side = width.min(height) as f32;
    let radius = side * 0.2;
    let ring = (side / 16.0).max(1.0);
    let (cx, cy) = (
        width as f32 - radius - ring * 0.5,
        height as f32 - radius - ring * 0.5,
    );
    for y in 0..height {
        for x in 0..width {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let cut = (radius + ring - d + 0.5).clamp(0.0, 1.0);
            if cut == 0.0 {
                continue;
            }
            let px = &mut rgba[((y * width + x) * 4) as usize..][..4];
            let under = px[3] as f32 / 255.0 * (1.0 - cut);
            let dot = (radius - d + 0.5).clamp(0.0, 1.0);
            let alpha = dot + under * (1.0 - dot);
            if alpha <= 0.0 {
                px[3] = 0;
                continue;
            }
            for (channel, color) in px[..3].iter_mut().zip([r, g, b]) {
                let mixed = (color as f32 * dot + *channel as f32 * under * (1.0 - dot)) / alpha;
                *channel = mixed.round() as u8;
            }
            px[3] = (alpha * 255.0).round() as u8;
        }
    }
}

/// The accent shade Windows uses on the taskbar: a light one on a dark
/// taskbar, a dark one on a light taskbar.
fn badge_color() -> [u8; 3] {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::w;
    let mut light = 0u32;
    let mut size = size_of::<u32>() as u32;
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut light).cast()),
            Some(&mut size),
        )
    };
    let light = read.is_ok() && light != 0;
    UISettings::new()
        .and_then(|s| {
            s.GetColorValue(if light {
                UIColorType::AccentDark1
            } else {
                UIColorType::AccentLight2
            })
        })
        .map(|c| [c.R, c.G, c.B])
        .unwrap_or(if light {
            [0x00, 0x67, 0xc0]
        } else {
            [0x4c, 0xc2, 0xff]
        })
}

/// Redoes the native text after a language change: tray menu and window
/// title. The tray tooltip follows with the next sample.
pub fn apply_language<R: Runtime>(app: &AppHandle<R>) {
    let update = TRAY_UPDATE.lock().unwrap().clone();
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), tray_menu(app, update.as_deref())) {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(app_title()));
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(&app_title());
    }
}

/// Sets the main window's color theme. Tauri passes the window's theme on to
/// WebView2's preferred color scheme, so the page's `prefers-color-scheme`,
/// the title bar and Mica all follow.
pub fn apply_theme<R: Runtime>(app: &AppHandle<R>, theme: Theme) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_theme(match theme {
            Theme::System => None,
            Theme::Light => Some(tauri::Theme::Light),
            Theme::Dark => Some(tauri::Theme::Dark),
        });
    }
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(app_title())
        .menu(&tray_menu(app, None)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            // The update dialog: what changed, then the user installs it there.
            "update" => {
                show_main(app);
                let _ = app.emit("argus-show-update", ());
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
    // An offer that came before the tray.
    let offered = TRAY_UPDATE.lock().unwrap().is_some();
    if offered {
        show_tray_update(app);
    }
    Ok(())
}

pub fn set_tray_tooltip<R: Runtime>(app: &AppHandle<R>, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only test that touches `AWAY`, a process-wide static.
    #[test]
    fn time_away_counts_from_leaving_sight_and_not_for_a_start_shown_when_ready() {
        let wait = || std::thread::sleep(std::time::Duration::from_millis(30));
        // Started for the task manager replacement: shown once the page is
        // ready, which is no time away.
        started_hidden(true);
        wait();
        assert_eq!(take_away(), Some((Away::Tray, 0)));
        assert_eq!(take_away(), None, "reported once");
        // Started in the tray (autostart).
        started_hidden(false);
        wait();
        assert!(take_away().is_some_and(|(from, ms)| from == Away::Tray && ms >= 30));
        // Minimized, then hidden: the tray, since it was first minimized.
        went_away(Away::Minimized);
        wait();
        went_away(Away::Tray);
        assert!(take_away().is_some_and(|(from, ms)| from == Away::Tray && ms >= 30));
        // Hidden, then minimized while hidden: still the tray.
        went_away(Away::Tray);
        went_away(Away::Minimized);
        assert!(take_away().is_some_and(|(from, _)| from == Away::Tray));
    }

    #[test]
    fn the_badge_sits_in_a_cleared_ring() {
        // An opaque white 32 px icon: radius 6.4, ring 2, centre (24.6, 24.6).
        let (w, h) = (32u32, 32u32);
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        draw_badge(&mut rgba, w, h, [0, 103, 192]);
        let px = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        assert_eq!(px(24, 24), [0, 103, 192, 255], "the dot");
        assert_eq!(px(24, 17)[3], 0, "the ring between dot and icon");
        assert_eq!(px(24, 14), [255, 255, 255, 255], "the icon past the ring");
        assert_eq!(px(0, 0), [255, 255, 255, 255], "the far corner");
        // Edges are blended, not stepped.
        assert!((1..255).contains(&px(24, 16)[3]) && (1..255).contains(&px(24, 18)[3]));
    }
}
