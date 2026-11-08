//! Native window lifecycle, backdrop, tray and visibility-driven sampling.
mod tray;
mod tray_identity;
use crate::{
    native_runtime::{Host, NativeWindow},
    sampler::{Control, WindowState},
    settings::{SettingsStore, Theme},
};
use argus_core::tr;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU8, Ordering},
};
use std::time::Instant;
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
static AWAY: Mutex<Option<(bool, Option<Instant>)>> = Mutex::new(None);
static UPDATE: Mutex<Option<String>> = Mutex::new(None);
static TRAY: Mutex<Option<Arc<tray::Worker>>> = Mutex::new(None);
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
        if let Some(app) = HOST.get() {
            let away = AWAY.lock().unwrap().take();
            app.state::<crate::AppState>()
                .sampler
                .send(Control::Refresh);
            let _=app.emit("argus-restored",serde_json::json!({"from":if away.is_some_and(|a|a.0){"tray"}else{"minimized"},"awayMs":away.and_then(|a|a.1).map_or(0,|i|i.elapsed().as_millis() as u64)}));
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
    VIEW.store(window_state(hwnd) as u8, Ordering::Relaxed);
}

pub fn supports_mica() -> bool {
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
    unsafe { RtlGetVersion(&mut info).is_ok() && info.dwBuildNumber >= 22000 }
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
    let mut native = NativeTray {
        hwnd,
        guid: tray_identity::guid(&app.config().identifier, &std::env::current_exe()?),
        badge: None,
        generation: None,
    };
    let worker = tray::Worker::spawn(app_title(), move |update| native.write(update)).inspect_err(
        |error| {
            argus_core::diagnostics::warn(
                "tray worker creation",
                argus_core::diagnostics::io(error),
            )
        },
    )?;
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
pub fn set_tray_update(_app: &Host, version: Option<&str>) {
    *UPDATE.lock().unwrap() = version.map(str::to_owned);
    if let Some(worker) = TRAY.lock().unwrap().clone() {
        worker.badge(version.is_some());
    }
}

struct NativeTray {
    hwnd: isize,
    guid: GUID,
    badge: Option<isize>,
    generation: Option<u64>,
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
        if update.badge && self.badge.is_none() {
            self.badge = unsafe { create_badged_icon(base) }.map(|icon| icon.0 as isize);
        }
        data.hIcon = if update.badge {
            self.badge.map(|icon| HICON(icon as _)).unwrap_or(base)
        } else {
            base
        };
        data.uFlags |= NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_MESSAGE;
        let title = wide(&update.title);
        let length = title.len().saturating_sub(1).min(data.szTip.len() - 1);
        data.szTip[..length].copy_from_slice(&title[..length]);
        let (first, retry) = if self.generation == Some(update.generation) {
            (NIM_MODIFY, NIM_ADD)
        } else {
            (NIM_ADD, NIM_MODIFY)
        };
        if notify(first, &data) || notify(retry, &data) {
            self.generation = Some(update.generation);
        } else {
            self.generation = None;
            argus_core::diagnostics::warn("tray update", "Shell_NotifyIconW returned false");
        }
    }
}

impl Drop for NativeTray {
    fn drop(&mut self) {
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
    use std::path::Path;
    use windows::Win32::UI::Shell::{
        NIF_GUID, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
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
        };
        let mut calls = Vec::new();
        for (title, badge, generation, closed) in [
            ("initial", false, 0, false),
            ("changed tooltip", true, 0, false),
            ("Explorer restarted", false, 1, false),
            ("", false, 1, true),
        ] {
            icon.write_with(
                tray::Update {
                    title: title.into(),
                    badge,
                    generation,
                    closed,
                },
                |message, data| {
                    assert_eq!(data.guidItem, guid);
                    assert!(data.uFlags.contains(NIF_GUID));
                    if message != NIM_DELETE {
                        assert!(data.uFlags.contains(NIF_ICON | NIF_MESSAGE | NIF_TIP));
                    }
                    calls.push(message);
                    // Exercise the NIM_ADD -> NIM_MODIFY retry after recreation.
                    calls.len() != 3
                },
            );
        }
        assert_eq!(
            calls,
            [NIM_ADD, NIM_MODIFY, NIM_ADD, NIM_MODIFY, NIM_DELETE]
        );
        let restarted = NativeTray {
            hwnd: 2,
            guid,
            badge: None,
            generation: None,
        };
        assert_eq!(restarted.data().guidItem, icon.data().guidItem);
        assert_ne!(restarted.data().hWnd, icon.data().hWnd);
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
            match l.0 as u32 {
                WM_LBUTTONUP => toggle_main(app),
                WM_RBUTTONUP | WM_CONTEXTMENU => tray_menu(app, hwnd),
                _ => {}
            }
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
        }
    }
    let result = unsafe { DefSubclassProc(hwnd, message, w, l) };
    // A fade reports the completed change itself (window_motion).
    if message == WM_WINDOWPOSCHANGED && !crate::window_motion::changing_visibility() {
        resync_visibility(hwnd);
    }
    result
}
