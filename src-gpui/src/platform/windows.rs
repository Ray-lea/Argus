//! Windows integration stays outside the GPUI view model.
use crate::{AppState, Options, native_runtime::Host, sampler::Control};
use gpui_kit::{App, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

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
        crate::shell::restore_window_state(host, &native, options);
        crate::shell::follow_visibility(&native);
        crate::shell::apply_theme(
            host,
            host.state::<crate::settings::SettingsStore>().get().theme,
        );

        let hwnd = native.hwnd().expect("native window").0 as isize;
        host.state::<AppState>()
            .sampler
            .send(Control::SetWindowProbe(Box::new(move || {
                crate::shell::window_state(windows::Win32::Foundation::HWND(hwnd as _))
            })));
        if !options.test {
            crate::hotkey::init(host, &native);
            let _ = crate::shell::build_tray(host);
        }
        if options.hidden {
            crate::shell::started_hidden(host.state::<AppState>().task_manager_start);
        }
    }
    if let Err(error) = crate::instance::install(host, |app, args| {
        if argus_service::taskmgr::ready::queue(&args) {
            let _ = app.emit("argus-taskmgr-ready", ());
        } else {
            crate::shell::show_main(app);
        }
    }) {
        log::error!("instance receiver failed: code={}", error.code().0);
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
pub(crate) fn update_targets() -> [String; 2] {
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    [platform.clone(), format!("{platform}-nsis")]
}
