use gpui::{
    AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, WindowControlArea, WindowOptions, div, px,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{cell::Cell, rc::Rc};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    Graphics::Gdi::ClientToScreen,
    UI::WindowsAndMessaging::*,
};

struct Caption(Rc<Cell<usize>>);

impl Render for Caption {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.0.clone();
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .size(px(64.))
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(
                div()
                    .id("client")
                    .size(px(64.))
                    .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
            )
    }
}

fn screen_position(hwnd: HWND, scale: f32, x: f32) -> LPARAM {
    let mut point = POINT {
        x: (x * scale) as i32,
        y: (32. * scale) as i32,
    };
    unsafe { ClientToScreen(hwnd, &mut point) }.ok().unwrap();
    LPARAM(((u32::from(point.y as u16) << 16) | u32::from(point.x as u16)) as isize)
}

#[test]
fn forwarded_primary_taps_reach_the_client_without_a_mouse_move() {
    let hits = Rc::new(Cell::new([0; 2]));
    let observed = hits.clone();
    let activations = Rc::new(Cell::new(0));
    let clicks = activations.clone();
    let counts = Rc::new(Cell::new([0; 5]));
    let after_release = counts.clone();
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                show: false,
                focus: false,
                ..Default::default()
            },
            cx,
            move |window, cx| {
                window.defer(cx, move |window, cx| {
                    window.set_content_zoom(1.5, cx);
                    window.draw(cx).clear(cx);
                    let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw()
                    else {
                        unreachable!();
                    };
                    let scale = window.scale_factor();
                    cx.spawn(async move |cx| {
                        let hwnd = HWND(handle.hwnd.get() as _);
                        let [caption, client] = [48., 80.].map(|x| screen_position(hwnd, scale, x));
                        let hits = [caption, client].map(|at| unsafe {
                            SendMessageW(hwnd, WM_NCHITTEST, None, Some(at)).0
                        });
                        observed.set(hits);
                        let contact =
                            POINTER_MESSAGE_FLAG_INCONTACT | POINTER_MESSAGE_FLAG_FIRSTBUTTON;
                        let down = contact | POINTER_MESSAGE_FLAG_PRIMARY;
                        let send = |message, flags: u32| unsafe {
                            // A window message alone has no system POINTER_INFO.
                            let wparam = WPARAM(((flags as usize) << 16) | 0xffea);
                            SendMessageW(hwnd, message, Some(wparam), Some(client))
                        };
                        let results = [
                            (contact, 0),
                            (down | POINTER_MESSAGE_FLAG_SECONDBUTTON, 0),
                            (down, POINTER_MESSAGE_FLAG_CANCELED),
                            (down, POINTER_MESSAGE_FLAG_PRIMARY),
                            (down, POINTER_MESSAGE_FLAG_PRIMARY),
                        ]
                        .map(|(press, release)| {
                            send(WM_POINTERDOWN, press);
                            send(WM_POINTERUP, release);
                            activations.get()
                        });
                        after_release.set(results);
                        cx.update(|cx| cx.quit());
                    })
                    .detach();
                });
                cx.new(|_| Caption(clicks))
            },
        )
        .unwrap();
    });
    assert_eq!(hits.get(), [HTCAPTION as isize, HTCLIENT as isize]);
    assert_eq!(counts.get(), [0, 0, 0, 1, 2]);
}
