//! Native regression fixture: a synchronous display notification during setup.
//! The window stays hidden and no display settings are changed.
use gpui_kit::gpui::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::Duration;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{SendMessageW, WM_DISPLAYCHANGE},
};

struct Probe;
impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x253747))
            .child("Argus display notification test")
    }
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                show: false,
                focus: false,
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(50.), px(50.)),
                    size(px(640.), px(480.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                window.defer(cx, |window, _| {
                    let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw()
                    else {
                        panic!("Windows fixture");
                    };
                    println!("native display callback begin");
                    unsafe {
                        SendMessageW(
                            HWND(handle.hwnd.get() as _),
                            WM_DISPLAYCHANGE,
                            Some(WPARAM(32)),
                            Some(LPARAM(0)),
                        );
                    }
                    println!("native display callback returned");
                });
                cx.new(|_| Probe)
            },
        )
        .expect("open isolated probe window");
        cx.spawn(async |cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
}
