//! Native window lifecycle, backdrop, tray and visibility-driven sampling.
mod tray;
mod tray_anchor;
mod tray_icon;
mod tray_identity;
use crate::{
    native_runtime::{Host, NativeWindow},
    sampler::{Control, WindowState},
    settings::{Settings, SettingsStore, Theme, TrayIcon, TrayMenu},
};
use argus_core::tr;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::Instant;
pub(crate) use tray_anchor::Anchor;
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{GUID, PCWSTR, w},
};
static HOST: OnceLock<Host> = OnceLock::new();
static VIEW: AtomicU8 = AtomicU8::new(WindowState::Hidden as u8);
static HAS_SHOWN: AtomicBool = AtomicBool::new(false);
static AWAY: Mutex<Option<(bool, Option<Instant>)>> = Mutex::new(None);
static UPDATE: Mutex<Option<String>> = Mutex::new(None);
static TRAY: Mutex<Option<Arc<tray::Worker>>> = Mutex::new(None);
/// The tray icon's identity, for finding it on screen.
static TRAY_ID: OnceLock<GUID> = OnceLock::new();
/// The shell took `NOTIFYICON_VERSION_4`: callbacks carry the event in the
/// low word and the anchor point in `wParam`, and a right click arrives as
/// `WM_CONTEXTMENU`.
static VERSION_4: AtomicBool = AtomicBool::new(false);
/// When the keyboard last selected the icon; the shell can report one
/// press twice.
static KEY_SELECT: Mutex<Option<Instant>> = Mutex::new(None);
const TRAY_MESSAGE: u32 = WM_APP + 82;
const BADGE_COLOR: [u8; 4] = [0x38, 0xBD, 0xF8, 0xFF];

/// Fault injection stays inside an explicitly isolated test instance.
pub(crate) fn test_tray_delay() -> Option<std::time::Duration> {
    static DELAY: OnceLock<Option<std::time::Duration>> = OnceLock::new();
    *DELAY.get_or_init(|| {
        let args: Vec<_> = std::env::args().collect();
        if !args.iter().any(|arg| arg == "--test")
            || !args.windows(2).any(|pair| {
                pair[0] == "--identifier" && pair[1].starts_with("dev.argus.taskmanager.test.")
            })
        {
            return None;
        }
        let millis = std::env::var("ARGUS_TEST_TRAY_DELAY_MS")
            .ok()?
            .parse::<u64>()
            .ok()?;
        (1..=10_000)
            .contains(&millis)
            .then(|| std::time::Duration::from_millis(millis))
    })
}

fn notify_icon(message: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> bool {
    if let Some(delay) = test_tray_delay() {
        std::thread::sleep(delay);
        return true;
    }
    unsafe { Shell_NotifyIconW(message, data).as_bool() }
}

pub fn draw_badge(rgba: &mut [u8], size: u32, color: [u8; 4]) {
    if size < 12 || rgba.len() < (size * size * 4) as usize {
        return;
    }
    let s = size as f32;
    let cx = s * 0.76;
    let cy = s * 0.76;
    let r_inner = (s * 0.19).max(2.0);
    let r_outer = r_inner + (s * 0.06).max(1.0);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > r_outer + 0.5 {
                continue;
            }
            let outer_cov = (r_outer + 0.5 - dist).clamp(0.0, 1.0);
            let inner_cov = (r_inner + 0.5 - dist).clamp(0.0, 1.0);
            let ring_cov = (outer_cov - inner_cov).clamp(0.0, 1.0);
            let src_r = (color[0] as f32 * inner_cov + 15.0 * ring_cov) / outer_cov.max(1e-5);
            let src_g = (color[1] as f32 * inner_cov + 23.0 * ring_cov) / outer_cov.max(1e-5);
            let src_b = (color[2] as f32 * inner_cov + 42.0 * ring_cov) / outer_cov.max(1e-5);
            let src_a = (color[3] as f32 / 255.0) * outer_cov;
            let idx = ((y * size + x) * 4) as usize;
            let dst_a = rgba[idx + 3] as f32 / 255.0;
            let out_a = src_a + dst_a * (1.0 - src_a);
            if out_a > 0.0 {
                rgba[idx] = ((src_r * src_a + rgba[idx] as f32 * dst_a * (1.0 - src_a)) / out_a)
                    .round() as u8;
                rgba[idx + 1] = ((src_g * src_a + rgba[idx + 1] as f32 * dst_a * (1.0 - src_a))
                    / out_a)
                    .round() as u8;
                rgba[idx + 2] = ((src_b * src_a + rgba[idx + 2] as f32 * dst_a * (1.0 - src_a))
                    / out_a)
                    .round() as u8;
                rgba[idx + 3] = (out_a * 255.0).round() as u8;
            }
        }
    }
}

unsafe fn base_tray_icon() -> windows::core::Result<HICON> {
    unsafe {
        LoadIconW(Some(GetModuleHandleW(None)?.into()), PCWSTR(1usize as _))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
    }
}

unsafe fn create_badged_icon(base: HICON) -> Option<HICON> {
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON).max(16) as u32;
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(color_bmp) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(dc);
            return None;
        };
        if bits.is_null() {
            let _ = DeleteObject(color_bmp.into());
            let _ = DeleteDC(dc);
            return None;
        }
        let old = SelectObject(dc, color_bmp.into());
        let _ = DrawIconEx(dc, 0, 0, base, size as i32, size as i32, 0, None, DI_NORMAL);
        let slice = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (size * size * 4) as usize);
        for px in slice.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        draw_badge(slice, size, BADGE_COLOR);
        for px in slice.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        let _ = SelectObject(dc, old);
        let _ = DeleteDC(dc);
        let mask_bmp = CreateBitmap(size as i32, size as i32, 1, 1, None);
        let icon_info = ICONINFO {
            fIcon: windows::core::BOOL(1),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };
        let icon = CreateIconIndirect(&icon_info).ok();
        let _ = DeleteObject(mask_bmp.into());
        let _ = DeleteObject(color_bmp.into());
        icon
    }
}
/// An icon from straight RGBA pixels, `size` square.
fn icon_from_rgba(rgba: &[u8], size: u32) -> Option<HICON> {
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() || rgba.len() != (size * size * 4) as usize {
            let _ = DeleteObject(color.into());
            return None;
        }
        let pixels = std::slice::from_raw_parts_mut(bits.cast::<u8>(), rgba.len());
        for (to, from) in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(rgba.as_chunks::<4>().0)
        {
            *to = [from[2], from[1], from[0], from[3]];
        }
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO {
            fIcon: windows::core::BOOL(1),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        })
        .ok();
        let _ = DeleteObject(mask.into());
        let _ = DeleteObject(color.into());
        icon
    }
}

/// The coverage of `text` set in bold Segoe UI of `font` pixels, centred in
/// `rect` on a `size` square: grey-scale antialiasing drawn white on black,
/// one byte per pixel.
fn text_coverage(text: &str, size: u32, rect: tray_icon::Rect, font: f32) -> Option<Vec<u8>> {
    unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(dc);
            return None;
        };
        let face = wide("Segoe UI");
        let mut name = [0u16; 32];
        name[..face.len()].copy_from_slice(&face);
        let font = CreateFontIndirectW(&LOGFONTW {
            lfHeight: -(font.round() as i32).max(1),
            lfWeight: FW_BOLD.0 as i32,
            lfQuality: ANTIALIASED_QUALITY,
            lfFaceName: name,
            ..Default::default()
        });
        let old_bitmap = SelectObject(dc, bitmap.into());
        let old_font = SelectObject(dc, font.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00ff_ffff));
        let mut area = RECT {
            left: rect[0].round() as i32,
            top: rect[1].round() as i32,
            right: rect[2].round() as i32,
            bottom: rect[3].round() as i32,
        };
        let mut text: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(
            dc,
            &mut text,
            &mut area,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        let _ = GdiFlush();
        let coverage = (!bits.is_null()).then(|| {
            std::slice::from_raw_parts(bits.cast::<u8>(), (size * size * 4) as usize)
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| pixel[1])
                .collect()
        });
        SelectObject(dc, old_font);
        SelectObject(dc, old_bitmap);
        let _ = DeleteObject(font.into());
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        coverage
    }
}

/// Whether the taskbar is light (`SystemUsesLightTheme`; dark when unset,
/// as on Windows 10 by default).
fn taskbar_light() -> bool {
    use windows::Win32::System::Registry::*;
    let mut light = 0u32;
    let mut bytes = 4;
    unsafe {
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut light).cast()),
            Some(&mut bytes),
        );
    }
    light != 0
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
/// Opens a web address in the default browser, from a command thread. The
/// shell hands it to the browser directly: a new `explorer.exe` process
/// would start first and pass it on to the running shell, a noticeable wait
/// before the page appears.
pub fn open_url(url: &str) -> Result<(), String> {
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows::core::HSTRING;
    // Shell extensions that handle the address may need COM on this thread.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    // The click that asked for it lets a browser already running come forward
    // with the page.
    let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    let file = HSTRING::from(url);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("open"),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    let opened = unsafe { ShellExecuteExW(&mut info) }.map_err(|e| e.to_string());
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
    opened
}

pub fn app_title() -> String {
    argus_app::config::app_title()
}
pub fn animations_enabled() -> bool {
    let mut enabled = windows::core::BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&raw mut enabled).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    enabled.as_bool()
}
pub fn show_main(app: &Host) {
    if let Some(window) = app.get_window("main") {
        if let Ok(hwnd) = window.hwnd() {
            crate::window_state::prepare_show(hwnd);
            crate::window_motion::show(hwnd);
        }
        let _ = window.set_focus();
    }
    crate::hotkey::retry_failed(app);
}
pub fn hide_main(window: &NativeWindow) {
    if let Ok(hwnd) = window.hwnd() {
        crate::window_motion::hide(hwnd);
    }
    crate::hotkey::resume_if_suspended(window.app_handle());
}
pub fn toggle_main(app: &Host) {
    if let Some(window) = app.get_window("main") {
        if window.is_visible().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false)
            && window.is_focused().unwrap_or(false)
            && !crate::window_motion::is_leaving()
        {
            hide_main(&window);
        } else {
            show_main(app);
        }
    }
}
pub fn window_state(hwnd: HWND) -> WindowState {
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
pub fn started_hidden(shown_when_ready: bool) {
    *AWAY.lock().unwrap() = Some((true, (!shown_when_ready).then(Instant::now)));
}
pub fn resync_visibility(hwnd: HWND) {
    let next = window_state(hwnd);
    let before = VIEW.swap(next as u8, Ordering::Relaxed);
    if before == next as u8 {
        return;
    }
    if next == WindowState::Shown {
        let first_show = !HAS_SHOWN.swap(true, Ordering::Relaxed);
        if let Some(app) = HOST.get() {
            let away = AWAY.lock().unwrap().take();
            app.state::<crate::AppState>()
                .sampler
                .send(Control::Refresh);
            let _ = app.emit(
                "argus-restored",
                serde_json::json!({
                    "from": if away.is_some_and(|a| a.0) { "tray" } else { "minimized" },
                    "awayMs": away.and_then(|a| a.1).map_or(0, |i| i.elapsed().as_millis() as u64),
                    "firstShow": first_show,
                }),
            );
        }
    } else {
        *AWAY.lock().unwrap() = Some((next == WindowState::Hidden, Some(Instant::now())));
    }
}
pub fn follow_visibility(window: &NativeWindow) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let _ = HOST.set(window.app_handle().clone());
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(procedure), 0x415247, 0);
    }
    let state = window_state(hwnd);
    VIEW.store(state as u8, Ordering::Relaxed);
    HAS_SHOWN.store(state == WindowState::Shown, Ordering::Relaxed);
}

pub fn supports_mica() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(detect_mica)
}

fn detect_mica() -> bool {
    // An HWND swap chain ignores alpha. Paint the theme background in that
    // mode so translucent controls and charts have the intended backdrop.
    if gpui_kit::gpui::windows_opaque_window_requested() {
        return false;
    }
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut OSVERSIONINFOW) -> NTSTATUS;
    }
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    let build = unsafe { RtlGetVersion(&mut info) }
        .is_ok()
        .then_some(info.dwBuildNumber);
    // Only isolated development tests may substitute the backdrop's OS build.
    // Keep the composition renderer unchanged to exercise the real solid fallback.
    let test_build = if cfg!(debug_assertions) {
        let args: Vec<_> = std::env::args().collect();
        (args.iter().any(|arg| arg == "--test")
            && args.windows(2).any(|pair| {
                pair[0] == "--identifier" && pair[1].starts_with("dev.argus.taskmanager.test.")
            }))
        .then(|| {
            std::env::var("ARGUS_TEST_BACKDROP_BUILD")
                .ok()?
                .parse()
                .ok()
        })
        .flatten()
    } else {
        None
    };
    supports_system_backdrop(test_build.or(build))
}

fn supports_system_backdrop(build: Option<u32>) -> bool {
    // The renderer uses DWMWA_SYSTEMBACKDROP_TYPE, introduced in Windows 11 22H2.
    // On build 22000, requesting Mica would leave the transparent surface unfilled.
    build.is_some_and(|build| build >= 22621)
}

pub fn dark(theme: Theme) -> bool {
    match theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => {
            use windows::Win32::System::Registry::*;
            let mut light = 1u32;
            let mut bytes = 4;
            unsafe {
                let _ = RegGetValueW(
                    HKEY_CURRENT_USER,
                    w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
                    w!("AppsUseLightTheme"),
                    RRF_RT_REG_DWORD,
                    None,
                    Some((&raw mut light).cast()),
                    Some(&mut bytes),
                );
            }
            light == 0
        }
    }
}
pub fn apply_theme(app: &Host, theme: Theme) {
    let dark = i32::from(dark(theme));
    if let Some(window) = app.get_window("main")
        && let Ok(hwnd) = window.hwnd()
    {
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark as *const i32).cast(),
                4,
            );
        }
    }
    let _ = app.emit("argus-theme", dark != 0);
}
pub fn apply_language(app: &Host) {
    if let Some(window) = app.get_window("main") {
        let _ = window.set_title(&app_title());
    }
    set_tray_tooltip(app, &app_title());
    let _ = app.emit("argus-language", argus_core::i18n::locale().tag());
}
pub fn tray_tooltip(usage: &str) -> String {
    format!("{}\n{usage}", app_title())
}
pub fn build_tray(app: &Host) -> anyhow::Result<()> {
    if app.exit_code().is_some() {
        return Ok(());
    }
    let Some(window) = app.get_window("main") else {
        return Ok(());
    };
    let hwnd = window.hwnd()?.0 as isize;
    let mut slot = TRAY.lock().unwrap();
    if let Some(worker) = slot.as_ref() {
        worker.recreate();
        return Ok(());
    }
    let guid = tray_identity::guid(&app.config().identifier, &std::env::current_exe()?);
    let _ = TRAY_ID.set(guid);
    let mut native = NativeTray {
        hwnd,
        guid,
        badge: None,
        generation: None,
        live: None,
    };
    let settings = app.state::<SettingsStore>().get();
    let initial = tray::Update {
        title: app_title(),
        look: settings.tray_icon,
        tip: !settings.tray_card,
        light: taskbar_light(),
        ..Default::default()
    };
    let worker =
        tray::Worker::spawn(initial, move |update| native.write(update)).inspect_err(|error| {
            argus_core::diagnostics::warn(
                "tray worker creation",
                argus_core::diagnostics::io(error),
            )
        })?;
    worker.badge(UPDATE.lock().unwrap().is_some());
    *slot = Some(Arc::new(worker));
    Ok(())
}
pub fn shutdown(_app: &Host) {
    if let Some(worker) = TRAY.lock().unwrap().take() {
        worker.shutdown(std::time::Duration::from_millis(100));
    }
}
pub fn set_tray_tooltip(_app: &Host, text: &str) {
    if let Some(worker) = TRAY.lock().unwrap().clone() {
        worker.title(text);
    }
}
/// A tray flyout is open: it shows live readings, so sampling keeps the
/// shown window's pace meanwhile.
static FLYOUT: AtomicBool = AtomicBool::new(false);
pub fn set_flyout_open(open: bool) {
    FLYOUT.store(open, Ordering::Relaxed);
}
/// The window's state as the sampler paces itself by it.
pub fn sampling_state(hwnd: HWND) -> WindowState {
    if FLYOUT.load(Ordering::Relaxed) {
        WindowState::Shown
    } else {
        window_state(hwnd)
    }
}
/// A new sample for the tray: its tooltip and the CPU load a live icon shows.
pub fn set_tray_reading(_app: &Host, text: &str, cpu: f64) {
    if let Some(worker) = TRAY.lock().unwrap().clone() {
        worker.reading(text, cpu.round().clamp(0., 100.) as u8);
    }
}
/// The tray icon's look, its hover card and what a right click opens.
pub fn apply_tray(settings: &Settings) {
    if let Some(worker) = TRAY.lock().unwrap().clone() {
        worker.style(settings.tray_icon, !settings.tray_card);
    }
    if !settings.tray_card
        && let Some(app) = HOST.get()
    {
        let _ = app.emit("argus-tray-card", serde_json::json!({ "open": false }));
    }
}
pub fn set_tray_update(_app: &Host, version: Option<&str>) {
    *UPDATE.lock().unwrap() = version.map(str::to_owned);
    if let Some(worker) = TRAY.lock().unwrap().clone() {
        worker.badge(version.is_some());
    }
}

/// The icon a tray update shows.
enum Live {
    /// The app's icon (with its badge).
    Logo,
    /// The live icon already shown.
    Same(HICON),
    /// A newly drawn live icon and its pixels.
    New(HICON, Vec<u8>),
}

struct NativeTray {
    hwnd: isize,
    guid: GUID,
    badge: Option<isize>,
    generation: Option<u64>,
    /// The live icon shown, and the pixels it was made from: a sample that
    /// draws the same pixels keeps it.
    live: Option<(isize, Vec<u8>)>,
}

impl NativeTray {
    fn data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: HWND(self.hwnd as _),
            uID: 1,
            uFlags: NIF_GUID,
            guidItem: self.guid,
            ..Default::default()
        }
    }

    fn write(&mut self, update: tray::Update) {
        self.write_with(update, notify_icon);
    }

    fn write_with(
        &mut self,
        update: tray::Update,
        mut notify: impl FnMut(NOTIFY_ICON_MESSAGE, &NOTIFYICONDATAW) -> bool,
    ) {
        let mut data = self.data();
        if update.closed {
            let _ = notify(NIM_DELETE, &data);
            return;
        }
        let Ok(base) = (unsafe { base_tray_icon() }) else {
            return;
        };
        // A new live icon replaces the one shown only after the shell took it.
        let mut replaced = None;
        data.hIcon = match self.live_icon(&update) {
            Live::New(icon, pixels) => {
                replaced = Some((icon, pixels));
                icon
            }
            Live::Same(icon) => icon,
            Live::Logo => {
                if update.badge && self.badge.is_none() {
                    self.badge = unsafe { create_badged_icon(base) }.map(|icon| icon.0 as isize);
                }
                if update.badge {
                    self.badge.map(|icon| HICON(icon as _)).unwrap_or(base)
                } else {
                    base
                }
            }
        };
        data.uFlags |= NIF_MESSAGE | NIF_ICON | NIF_TIP;
        if update.tip {
            data.uFlags |= NIF_SHOWTIP;
        }
        data.uCallbackMessage = TRAY_MESSAGE;
        let title = wide(&update.title);
        let length = title.len().saturating_sub(1).min(data.szTip.len() - 1);
        data.szTip[..length].copy_from_slice(&title[..length]);
        let known = self.generation == Some(update.generation);
        let (first, retry) = if known {
            (NIM_MODIFY, NIM_ADD)
        } else {
            (NIM_ADD, NIM_MODIFY)
        };
        let added = if notify(first, &data) {
            Some(!known)
        } else if notify(retry, &data) {
            Some(known)
        } else {
            None
        };
        match replaced {
            Some((icon, pixels)) if added.is_some() => {
                self.set_live(Some((icon.0 as isize, pixels)))
            }
            Some((icon, _)) => unsafe {
                let _ = DestroyIcon(icon);
            },
            // Back to the app's icon: the live one is no longer shown.
            None if added.is_some() && update.look == TrayIcon::Logo => self.set_live(None),
            None => {}
        }
        match added {
            Some(added) => {
                if added {
                    // Version 4: the hover card, and the anchor point with each click.
                    let mut version = self.data();
                    version.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                    VERSION_4.store(notify(NIM_SETVERSION, &version), Ordering::Relaxed);
                }
                self.generation = Some(update.generation);
            }
            None => {
                self.generation = None;
                argus_core::diagnostics::warn("tray update", "Shell_NotifyIconW returned false");
            }
        }
    }

    /// The live icon for `update`: a new one when its pixels changed, the
    /// one shown when they did not.
    fn live_icon(&mut self, update: &tray::Update) -> Live {
        let look = match update.look {
            TrayIcon::Logo => return Live::Logo,
            TrayIcon::Load => tray_icon::Look::Load,
            TrayIcon::Number => tray_icon::Look::Number,
        };
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
        let spec = tray_icon::Spec {
            look,
            load: update.load,
            light: update.light,
            badge: update.badge,
        };
        let pixels = tray_icon::render(spec, size, &|text, rect, font| {
            text_coverage(text, size, rect, font)
        });
        if let Some((icon, shown)) = &self.live
            && *shown == pixels
        {
            return Live::Same(HICON(*icon as _));
        }
        match icon_from_rgba(&pixels, size) {
            Some(icon) => Live::New(icon, pixels),
            // Without a drawn icon the app's stands in.
            None => Live::Logo,
        }
    }

    fn set_live(&mut self, live: Option<(isize, Vec<u8>)>) {
        if let Some((icon, _)) = std::mem::replace(&mut self.live, live) {
            unsafe {
                let _ = DestroyIcon(HICON(icon as _));
            }
        }
    }
}

impl Drop for NativeTray {
    fn drop(&mut self) {
        self.set_live(None);
        if let Some(icon) = self.badge.take() {
            unsafe {
                let _ = DestroyIcon(HICON(icon as _));
            }
        }
    }
}

#[cfg(test)]
mod tray_tests {
    use super::{NativeTray, tray, tray_identity};
    use crate::settings::TrayIcon;
    use std::path::Path;
    use windows::Win32::UI::Shell::{
        NIF_GUID, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NIM_SETVERSION,
    };

    #[test]
    fn tray_requests_keep_identity_through_changes_recreation_retry_and_deletion() {
        let guid = tray_identity::guid(
            "dev.argus.taskmanager.test.tray",
            Path::new(r"C:\Argus\argus.exe"),
        );
        let mut icon = NativeTray {
            hwnd: 1,
            guid,
            badge: None,
            generation: None,
            live: None,
        };
        let mut calls = Vec::new();
        let mut icons = Vec::new();
        let mut adds = 0;
        for (title, badge, generation, closed, look, tip) in [
            ("initial", false, 0, false, TrayIcon::Load, false),
            ("changed tooltip", true, 0, false, TrayIcon::Load, false),
            ("Explorer restarted", false, 1, false, TrayIcon::Load, false),
            ("same pixels", false, 1, false, TrayIcon::Load, true),
            ("app icon", false, 1, false, TrayIcon::Logo, true),
            ("", false, 1, true, TrayIcon::Logo, true),
        ] {
            icon.write_with(
                tray::Update {
                    title: title.into(),
                    badge,
                    generation,
                    closed,
                    look,
                    load: Some(35),
                    tip,
                    ..Default::default()
                },
                |message, data| {
                    assert_eq!(data.guidItem, guid);
                    assert!(data.uFlags.contains(NIF_GUID));
                    if message == NIM_ADD || message == NIM_MODIFY {
                        assert!(data.uFlags.contains(NIF_ICON | NIF_MESSAGE | NIF_TIP));
                        assert_eq!(data.uFlags.contains(NIF_SHOWTIP), tip);
                        icons.push(data.hIcon.0 as isize);
                    }
                    calls.push(message);
                    // Exercise the NIM_ADD -> NIM_MODIFY retry after recreation.
                    if message == NIM_ADD {
                        adds += 1;
                        return adds != 2;
                    }
                    true
                },
            );
        }
        assert_eq!(
            calls,
            [
                NIM_ADD,
                NIM_SETVERSION,
                NIM_MODIFY,
                NIM_ADD,
                NIM_MODIFY,
                NIM_MODIFY,
                NIM_MODIFY,
                NIM_DELETE
            ]
        );
        // The badge drew new pixels; the same pixels keep the icon shown.
        assert_ne!(icons[0], icons[1]);
        assert_eq!(icons[3], icons[4]);
        assert!(icon.live.is_none(), "the app's icon replaced the live one");
        let restarted = NativeTray {
            hwnd: 2,
            guid,
            badge: None,
            generation: None,
            live: None,
        };
        assert_eq!(restarted.data().guidItem, icon.data().guidItem);
        assert_ne!(restarted.data().hWnd, icon.data().hWnd);
    }
}

#[cfg(test)]
mod tray_action_tests {
    use super::{NIN_KEYSELECT, TrayAction, tray_action};
    use windows::Win32::UI::Shell::{NIN_POPUPCLOSE, NIN_POPUPOPEN};
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_CONTEXTMENU, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONUP,
    };

    #[test]
    fn a_right_click_opens_once_with_either_callback_version() {
        assert_eq!(tray_action(WM_RBUTTONUP, true), None);
        assert_eq!(tray_action(WM_CONTEXTMENU, true), Some(TrayAction::Context));
        assert_eq!(tray_action(WM_RBUTTONUP, false), Some(TrayAction::Context));
        for version_4 in [true, false] {
            assert_eq!(
                tray_action(WM_LBUTTONUP, version_4),
                Some(TrayAction::Toggle)
            );
            assert_eq!(tray_action(WM_LBUTTONDOWN, version_4), None);
            assert_eq!(tray_action(WM_MOUSEMOVE, version_4), None);
        }
        assert_eq!(
            tray_action(NIN_KEYSELECT, true),
            Some(TrayAction::KeyToggle)
        );
        assert_eq!(tray_action(NIN_POPUPOPEN, true), Some(TrayAction::CardOpen));
        assert_eq!(
            tray_action(NIN_POPUPCLOSE, true),
            Some(TrayAction::CardClose)
        );
    }
}

#[cfg(test)]
mod backdrop_tests {
    use super::supports_system_backdrop;

    #[test]
    fn system_backdrop_requires_the_api_available_since_windows_11_22h2() {
        for build in [None, Some(19045), Some(22000), Some(22620)] {
            assert!(!supports_system_backdrop(build), "build={build:?}");
        }
        for build in [22621, 22631, 26100] {
            assert!(supports_system_backdrop(Some(build)), "build={build}");
        }
    }
}

fn taskbar_created_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}
fn tray_menu(app: &Host, hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let show = wide(&tr!("tray.show"));
        let quit = wide(&tr!("tray.quit"));
        let _ = AppendMenuW(menu, MF_STRING, 1, PCWSTR(show.as_ptr()));
        if let Some(version) = UPDATE.lock().unwrap().as_deref() {
            let text = wide(&tr!("tray.update", version = version));
            let _ = AppendMenuW(menu, MF_STRING, 2, PCWSTR(text.as_ptr()));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, 3, PCWSTR(quit.as_ptr()));
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
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
        match selected {
            1 => show_main(app),
            2 => {
                show_main(app);
                let _ = app.emit("argus-show-update", ());
            }
            3 => app.exit(0),
            _ => {}
        }
    }
}
/// `NIN_SELECT | NINF_KEY`, which the `windows` crate does not export.
const NIN_KEYSELECT: u32 = WM_USER | 1;

/// What a tray callback asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrayAction {
    /// A click: show or hide the window.
    Toggle,
    /// Enter or Space on the icon.
    KeyToggle,
    /// A right click or the menu key: the quick panel or the menu.
    Context,
    /// The pointer rests on the icon / left it.
    CardOpen,
    CardClose,
}

/// The action of a tray callback's event. With version 4 a right click
/// comes as `WM_CONTEXTMENU` after `WM_RBUTTONUP`; before it, only the latter.
fn tray_action(event: u32, version_4: bool) -> Option<TrayAction> {
    match event {
        WM_LBUTTONUP => Some(TrayAction::Toggle),
        NIN_KEYSELECT => Some(TrayAction::KeyToggle),
        WM_CONTEXTMENU => Some(TrayAction::Context),
        WM_RBUTTONUP if !version_4 => Some(TrayAction::Context),
        NIN_POPUPOPEN => Some(TrayAction::CardOpen),
        NIN_POPUPCLOSE => Some(TrayAction::CardClose),
        _ => None,
    }
}

/// The tray icon and its display; when the shell cannot say where the icon
/// is, a one-pixel icon at `point`.
fn tray_anchor(hwnd: HWND, point: POINT) -> Anchor {
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    let id = NOTIFYICONIDENTIFIER {
        cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: hwnd,
        uID: 1,
        guidItem: TRAY_ID.get().copied().unwrap_or_default(),
    };
    let icon = unsafe { Shell_NotifyIconGetRect(&id) }.unwrap_or(RECT {
        left: point.x,
        top: point.y,
        right: point.x + 1,
        bottom: point.y + 1,
    });
    let monitor = unsafe { MonitorFromRect(&icon, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let _ = unsafe { GetMonitorInfoW(monitor, &mut info) };
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    let corners = |r: RECT| [r.left, r.top, r.right, r.bottom];
    Anchor {
        icon: corners(icon),
        work: corners(info.rcWork),
        monitor: corners(info.rcMonitor),
        scale: dpi_x.max(96) as f32 / 96.,
    }
}

/// Where a tray icon near the right end of the primary display's taskbar
/// would be, for test instances, which open the flyouts without the shell.
pub fn sample_anchor() -> Anchor {
    let monitor = unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let _ = unsafe { GetMonitorInfoW(monitor, &mut info) };
    let scale = |logical: i32| {
        logical * unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() }.max(96) as i32 / 96
    };
    let (work, whole) = (info.rcWork, info.rcMonitor);
    Anchor {
        icon: [
            work.right - scale(160),
            whole.bottom - scale(40),
            work.right - scale(136),
            whole.bottom - scale(8),
        ],
        work: [work.left, work.top, work.right, work.bottom],
        monitor: [whole.left, whole.top, whole.right, whole.bottom],
        scale: unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() }.max(96) as f32 / 96.,
    }
}

fn tray_callback(app: &Host, hwnd: HWND, w: WPARAM, l: LPARAM) {
    let version_4 = VERSION_4.load(Ordering::Relaxed);
    let Some(action) = tray_action((l.0 as u32) & 0xffff, version_4) else {
        return;
    };
    // Version 4 gives the anchor point with the event; otherwise the pointer.
    let point = if version_4 {
        POINT {
            x: (w.0 & 0xffff) as u16 as i16 as i32,
            y: ((w.0 >> 16) & 0xffff) as u16 as i16 as i32,
        }
    } else {
        let mut point = POINT::default();
        let _ = unsafe { GetCursorPos(&mut point) };
        point
    };
    let close_card = || {
        let _ = app.emit("argus-tray-card", serde_json::json!({ "open": false }));
    };
    match action {
        TrayAction::Toggle => {
            close_card();
            toggle_main(app);
        }
        TrayAction::KeyToggle => {
            let mut last = KEY_SELECT.lock().unwrap();
            if last.is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(300)) {
                return;
            }
            *last = Some(Instant::now());
            drop(last);
            toggle_main(app);
        }
        TrayAction::Context => {
            close_card();
            if app.state::<SettingsStore>().get().tray_menu == TrayMenu::Menu {
                tray_menu(app, hwnd);
            } else {
                let _ = app.emit("argus-tray-panel", tray_anchor(hwnd, point));
            }
        }
        TrayAction::CardOpen => {
            if app.state::<SettingsStore>().get().tray_card {
                let _ = app.emit(
                    "argus-tray-card",
                    serde_json::json!({ "open": true, "anchor": tray_anchor(hwnd, point) }),
                );
            }
        }
        TrayAction::CardClose => close_card(),
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
    if let Some(app) = HOST.get() {
        if message != 0
            && message == taskbar_created_message()
            && let Some(worker) = TRAY.lock().unwrap().clone()
        {
            worker.recreate();
        }
        if message == TRAY_MESSAGE {
            tray_callback(app, hwnd, w, l);
            return LRESULT(0);
        }
        if message == WM_CLOSE {
            if app.state::<SettingsStore>().get().close_to_tray {
                if let Some(window) = app.get_window("main") {
                    hide_main(&window);
                }
            } else {
                app.exit(0);
            }
            return LRESULT(0);
        }
        if message == WM_SETFOCUS {
            app.state::<crate::AppState>()
                .sampler
                .send(Control::Refresh);
            crate::hotkey::retry_failed(app);
        }
        if message == WM_KILLFOCUS {
            let _ = app.emit("argus-blur", ());
        }
        if message == WM_SETTINGCHANGE {
            apply_theme(app, app.state::<SettingsStore>().get().theme);
            // The taskbar's theme can change apart from the apps'.
            if let Some(worker) = TRAY.lock().unwrap().clone() {
                worker.taskbar(taskbar_light());
            }
        }
    }
    let result = unsafe { DefSubclassProc(hwnd, message, w, l) };
    // A fade reports the completed change itself (window_motion).
    if message == WM_WINDOWPOSCHANGED && !crate::window_motion::changing_visibility() {
        resync_visibility(hwnd);
    }
    result
}
