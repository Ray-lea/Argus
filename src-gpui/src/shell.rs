//! Native window lifecycle, backdrop, tray and visibility-driven sampling.
use crate::{
    native_runtime::{Host, NativeWindow},
    sampler::{Control, WindowState},
    settings::{SettingsStore, Theme},
};
use argus_core::tr;
use std::sync::{
    Mutex, OnceLock,
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
    core::{PCWSTR, w},
};
static HOST: OnceLock<Host> = OnceLock::new();
static VIEW: AtomicU8 = AtomicU8::new(WindowState::Hidden as u8);
static AWAY: Mutex<Option<(bool, Option<Instant>)>> = Mutex::new(None);
static UPDATE: Mutex<Option<String>> = Mutex::new(None);
static BADGE_ICON: Mutex<Option<isize>> = Mutex::new(None);
const TRAY_MESSAGE: u32 = WM_APP + 82;
const BADGE_COLOR: [u8; 4] = [0x38, 0xBD, 0xF8, 0xFF];

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
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    crate::hotkey::retry_failed(app);
}
pub fn hide_main(window: &NativeWindow) {
    let _ = window.hide();
    crate::hotkey::resume_if_suspended(window.app_handle());
}
pub fn toggle_main(app: &Host) {
    if let Some(window) = app.get_window("main") {
        if window.is_visible().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false)
            && window.is_focused().unwrap_or(false)
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
    if let Some(app) = HOST.get() {
        let _ = app.emit("argus-visibility", next == WindowState::Shown);
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
fn tray_data(app: &Host) -> Option<NOTIFYICONDATAW> {
    Some(NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: app.get_window("main")?.hwnd().ok()?,
        uID: 1,
        ..Default::default()
    })
}
pub fn build_tray(app: &Host) -> anyhow::Result<()> {
    let Some(mut data) = tray_data(app) else {
        return Ok(());
    };
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = TRAY_MESSAGE;
    unsafe {
        data.hIcon = LoadIconW(Some(GetModuleHandleW(None)?.into()), PCWSTR(1usize as _))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))?;
        let title = wide(&app_title());
        let n = title.len().min(data.szTip.len());
        data.szTip[..n].copy_from_slice(&title[..n]);
        anyhow::ensure!(
            Shell_NotifyIconW(NIM_ADD, &data).as_bool(),
            "tray creation failed"
        );
    }
    Ok(())
}
pub fn shutdown(app: &Host) {
    if let Some(data) = tray_data(app) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }
    if let Some(raw) = BADGE_ICON.lock().unwrap().take() {
        unsafe {
            let _ = DestroyIcon(HICON(raw as _));
        }
    }
}
pub fn set_tray_tooltip(app: &Host, text: &str) {
    if let Some(mut data) = tray_data(app) {
        data.uFlags = NIF_TIP;
        let text = wide(text);
        let n = (text.len() - 1).min(data.szTip.len() - 1);
        data.szTip[..n].copy_from_slice(&text[..n]);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }
}
pub fn set_tray_update(app: &Host, version: Option<&str>) {
    *UPDATE.lock().unwrap() = version.map(str::to_owned);
    if let Some(mut data) = tray_data(app)
        && let Ok(base) = unsafe { base_tray_icon() }
    {
        data.uFlags = NIF_ICON;
        if version.is_some() {
            let mut slot = BADGE_ICON.lock().unwrap();
            let icon = slot
                .map(|raw| HICON(raw as _))
                .or_else(|| {
                    let created = unsafe { create_badged_icon(base) }?;
                    *slot = Some(created.0 as isize);
                    Some(created)
                })
                .unwrap_or(base);
            data.hIcon = icon;
        } else {
            data.hIcon = base;
        }
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }
    let _ = app.emit("argus-tray-update", version);
}
pub fn save_window_state(app: &Host, hwnd: HWND) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let mut placement = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    if unsafe { GetWindowPlacement(hwnd, &mut placement) }.is_err() {
        return;
    }
    let rect = placement.rcNormalPosition;
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return;
    }
    let state = serde_json::json!({
        "x": rect.left,
        "y": rect.top,
        "width": width,
        "height": height,
        "maximized": placement.showCmd == SW_SHOWMAXIMIZED.0 as u32,
    });
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("window-state.json"), state.to_string());
}
pub fn restore_window_state(app: &Host, native: &NativeWindow, options: &crate::Options) {
    if options.test || options.hidden {
        return;
    }
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join("window-state.json")) else {
        return;
    };
    let Ok(state) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    let (Some(x), Some(y), Some(w), Some(h)) = (
        state["x"].as_i64().map(|v| v as i32),
        state["y"].as_i64().map(|v| v as i32),
        state["width"].as_i64().map(|v| v as i32),
        state["height"].as_i64().map(|v| v as i32),
    ) else {
        return;
    };
    if w < 400 || h < 300 {
        return;
    }
    let rect = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    let Ok(hwnd) = native.hwnd() else {
        return;
    };
    unsafe {
        if !MonitorFromRect(&rect, MONITOR_DEFAULTTONULL).is_invalid() {
            let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
        if state["maximized"].as_bool() == Some(true) {
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        }
    }
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
        if message == TRAY_MESSAGE {
            match l.0 as u32 {
                WM_LBUTTONUP => toggle_main(app),
                WM_RBUTTONUP | WM_CONTEXTMENU => tray_menu(app, hwnd),
                _ => {}
            }
            return LRESULT(0);
        }
        if message == WM_EXITSIZEMOVE {
            save_window_state(app, hwnd);
        }
        if message == WM_CLOSE {
            save_window_state(app, hwnd);
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
    if message == WM_WINDOWPOSCHANGED {
        resync_visibility(hwnd);
    }
    result
}
