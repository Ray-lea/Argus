//! Windows integration stays outside the GPUI view model.
use crate::{AppState, Options, native_runtime::Host, sampler::Control};
use gpui_kit::{App, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[path = "file_drop.rs"]
mod file_drop;

pub(crate) fn attach_window(host: &Host, options: &Options, window: &mut Window, cx: &mut App) {
    if let Ok(handle) = window.window_handle()
        && let RawWindowHandle::Win32(handle) = handle.as_raw()
    {
        host.bind_window(windows::Win32::Foundation::HWND(handle.hwnd.get() as _));
    }
    if let Some(native) = host.get_window("main") {
        if options.hidden
            && let Err(error) = native.prepare_hidden_size(options.width, options.height)
        {
            log::warn!(
                "initial native size failed: {}",
                argus_core::diagnostics::failure(&error.to_string())
            );
        }
        if options.hidden {
            // SetWindowPos dispatches WM_SIZE synchronously while this window is
            // already borrowed. Refresh GPUI's viewport after that callback returns.
            window.bounds_changed(cx);
        }
        if !options.test
            && let (Ok(hwnd), Ok(dir)) = (native.hwnd(), host.path().app_config_dir())
        {
            crate::window_state::install(hwnd, dir.join("preferences.json"));
        }
        crate::shell::follow_visibility(&native);
        if let Ok(hwnd) = native.hwnd() {
            crate::hotkey::install_key_context(hwnd);
            file_drop::install(host, hwnd);
            argus_service::admin::window_relaunch(&host.config().identifier, hwnd);
            crate::window_motion::install(hwnd);
            crate::activation::install(hwnd);
        }
        crate::shell::apply_theme(
            host,
            host.state::<crate::settings::SettingsStore>().get().theme,
        );

        let hwnd = native.hwnd().expect("native window").0 as isize;
        host.state::<AppState>()
            .sampler
            .send(Control::SetWindowProbe(Box::new(move || {
                crate::shell::sampling_state(windows::Win32::Foundation::HWND(hwnd as _))
            })));
        if !options.test {
            crate::hotkey::init(host, &native);
        }
        if !options.test || crate::shell::test_tray_delay().is_some() {
            let _ = crate::shell::build_tray(host);
        }
        crate::shell::apply_taskbar_monitor(
            host,
            &host
                .state::<crate::settings::SettingsStore>()
                .get()
                .taskbar_monitor,
        );
        if options.hidden {
            crate::shell::started_hidden(host.state::<AppState>().task_manager_start);
        }
    }
    if let Err(error) = crate::instance::install(host, |app, args| {
        // An administrator start's verified handoff replaces this process.
        if !argus_core::privilege::is_elevated()
            && argus_service::admin::trusted_handoff(&app.config().identifier, &args)
        {
            app.exit(0);
            return;
        }
        argus_service::admin::acknowledge(&args);
        if argus_service::taskmgr::ready::queue(&args) {
            let _ = app.emit("argus-taskmgr-ready", ());
        } else if !args.iter().any(|arg| arg == "--hidden") {
            crate::shell::show_main(app);
        }
    }) {
        log::error!("instance receiver failed: code={}", error.code().0);
    }
}
/// Keeps the main window above other windows (`on`), or returns it to the
/// normal level.
pub(crate) fn keep_on_top(host: &Host, on: bool) {
    if let Some(hwnd) = host
        .get_window("main")
        .and_then(|native| native.hwnd().ok())
        && let Err(error) = crate::activation::keep_on_top(hwnd, on)
    {
        argus_core::diagnostics::warn(
            "keep window on top",
            format!("on={on} code={:#010x}", error.code().0),
        );
    }
}
pub(crate) fn supports_backdrop() -> bool {
    crate::shell::supports_mica()
}
pub(crate) fn is_dark(theme: crate::settings::Theme) -> bool {
    crate::shell::dark(theme)
}
pub(crate) fn system_motion() -> bool {
    crate::shell::animations_enabled()
}
/// The longest time between the two clicks of a double-click.
pub(crate) fn double_click_time() -> std::time::Duration {
    let ms = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() };
    std::time::Duration::from_millis(u64::from(ms))
}
pub(crate) const KERNEL_SENSORS_SUPPORTED: bool = argus_service::driver::SUPPORTED;
pub(crate) const INSTALLER_ARCH: &str = if cfg!(target_arch = "aarch64") {
    "arm64"
} else {
    "x64"
};

pub(crate) fn update_targets() -> [String; 2] {
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    [platform.clone(), format!("{platform}-nsis")]
}
