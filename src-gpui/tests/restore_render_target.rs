#![cfg(target_os = "windows")]

use gpui_kit::gpui::{
    AppContext, Context, IntoElement, Render, Window, WindowOptions, div, px, size,
};
// Enable the Windows backend's GPU readback for this native regression.
use gpui_windows as _;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{HWND, LPARAM, RECT, WPARAM},
    UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SIZE_MINIMIZED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
        SendMessageW, SetWindowPos, WM_SIZE,
    },
};

struct EmptyView;
impl Render for EmptyView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[test]
fn restoring_to_a_new_size_updates_the_render_target() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                show: false,
                focus: false,
                ..Default::default()
            },
            cx,
            |window, cx| {
                window.defer(cx, |window, cx| {
                    let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw()
                    else {
                        panic!("Windows fixture");
                    };
                    let hwnd = HWND(handle.hwnd.get() as _);
                    // Exercise the native restore branch without showing a window
                    // or changing the foreground on the developer's desktop.
                    for (width, height, minimized) in [
                        (640, 480, false),
                        (1600, 1000, true),
                        (800, 600, true),
                        (800, 600, true),
                    ] {
                        unsafe {
                            if minimized {
                                SendMessageW(
                                    hwnd,
                                    WM_SIZE,
                                    Some(WPARAM(SIZE_MINIMIZED as usize)),
                                    Some(LPARAM(0)),
                                );
                            }
                            let mut client = RECT::default();
                            let mut frame = RECT::default();
                            GetClientRect(hwnd, &mut client).unwrap();
                            GetWindowRect(hwnd, &mut frame).unwrap();
                            SetWindowPos(
                                hwnd,
                                None,
                                0,
                                0,
                                width + frame.right - frame.left - client.right,
                                height + frame.bottom - frame.top - client.bottom,
                                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOMOVE,
                            )
                            .unwrap();
                            if width == client.right && height == client.bottom {
                                // An unchanged size need not emit WM_SIZE itself.
                                SendMessageW(
                                    hwnd,
                                    WM_SIZE,
                                    Some(WPARAM(0)),
                                    Some(LPARAM(((height as isize) << 16) | width as isize)),
                                );
                            }
                        }
                        window.bounds_changed(cx);
                        assert_eq!(
                            window.viewport_size(),
                            size(px(width as f32), px(height as f32))
                                .map(|length| length / window.scale_factor())
                        );
                        let image = window.render_to_image().unwrap();
                        assert_eq!(image.dimensions(), (width as u32, height as u32));
                    }
                    cx.quit();
                });
                cx.new(|_| EmptyView)
            },
        )
        .unwrap();
    });
}
