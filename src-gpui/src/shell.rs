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
        Graphics::Dwm::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};
static HOST: OnceLock<Host> = OnceLock::new();
static VIEW: AtomicU8 = AtomicU8::new(WindowState::Hidden as u8);
static AWAY: Mutex<Option<(bool, Option<Instant>)>> = Mutex::new(None);
static UPDATE: Mutex<Option<String>> = Mutex::new(None);
const TRAY_MESSAGE: u32 = WM_APP + 82;
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
    let _ = app.emit("argus-tray-update", version);
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
    if message == WM_WINDOWPOSCHANGED {
        resync_visibility(hwnd);
    }
    result
}
