#![cfg(target_os = "windows")]

use gpui_kit::gpui::{
    AppContext, Bounds, Context, Corners, IntoElement, ParentElement, Render, RenderImage, Styled,
    Window, WindowOptions, canvas, div, point, px, rgb, size,
};
use gpui_windows as _;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::Arc;
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SetWindowPos},
};

struct ImageEdges;

// Tile size, drawn size, alpha and expected composited gray over the background.
const CASES: [(u32, u32, u8, u8); 4] = [
    (4, 64, 255, 40),
    (4, 80, 128, 60),
    (1, 48, 255, 40),
    (4, 2, 255, 40),
];

impl Render for ImageEdges {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x505050)).child(
            canvas(
                |_, _, _| {},
                |_, _, window, _| {
                    // A cropped tile with vivid neighbors makes atlas bleed
                    // deterministic, independent of process icons or allocation order.
                    let scale = window.scale_factor();
                    let bounds = |x, y, w, h| {
                        Bounds::new(
                            point(px(x / scale), px(y / scale)),
                            size(px(w / scale), px(h / scale)),
                        )
                    };
                    for (index, (tile, drawn, alpha, _)) in CASES.into_iter().enumerate() {
                        let image = image::RgbaImage::from_fn(tile + 2, tile + 2, |x, y| {
                            image::Rgba(if x == 0 || y == 0 || x == tile + 1 || y == tile + 1 {
                                [0, 0, 255, 255]
                            } else {
                                [40, 40, 40, alpha]
                            })
                        });
                        let image = Arc::new(RenderImage::new(vec![image::Frame::new(image)]));
                        let x = 32. + index as f32 * 112.;
                        let drawn = drawn as f32;
                        let padding = drawn / tile as f32;
                        window
                            .paint_image(
                                bounds(x, 32., drawn, drawn),
                                bounds(
                                    x - padding,
                                    32. - padding,
                                    drawn + 2. * padding,
                                    drawn + 2. * padding,
                                ),
                                Corners::default(),
                                image,
                                0,
                                false,
                            )
                            .unwrap();
                    }
                },
            )
            .size_full(),
        )
    }
}

#[test]
fn stretched_image_does_not_sample_neighboring_atlas_texels() {
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
                    // Draw and read only the hidden window's private GPU target.
                    let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw()
                    else {
                        panic!("Windows fixture");
                    };
                    unsafe {
                        SetWindowPos(
                            HWND(handle.hwnd.get() as _),
                            None,
                            0,
                            0,
                            520,
                            240,
                            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOZORDER,
                        )
                        .unwrap();
                    }
                    window.bounds_changed(cx);
                    let _ = window.draw(cx);
                    let image = window.render_to_image().unwrap();
                    for (index, (_, drawn, _, gray)) in CASES.into_iter().enumerate() {
                        let left = 32 + index as u32 * 112;
                        for y in 32..32 + drawn {
                            for x in left..left + drawn {
                                let pixel = image.get_pixel(x, y).0;
                                assert!(
                                    pixel[..3].iter().all(|channel| channel.abs_diff(gray) <= 1)
                                        && pixel[3] == 255,
                                    "atlas bleed in case {index} at ({x}, {y}): {pixel:?}"
                                );
                            }
                        }
                    }
                    cx.quit();
                });
                cx.new(|_| ImageEdges)
            },
        )
        .unwrap();
    });
}
