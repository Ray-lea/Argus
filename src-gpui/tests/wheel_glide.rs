//! Wheel glides of the vendored GPUI (`smooth_scroll` in its div.rs) at a
//! fractional scale factor, where a scroller's far edge can lie between
//! device pixels and snapping can fall on either side of the limit.
use gpui_test::{
    AppContext, Context, InteractiveElement, IntoElement, Modifiers, ParentElement, Render,
    ScrollDelta, ScrollEvent, ScrollHandle, ScrollSource, StatefulInteractiveElement, Styled,
    TestAppContext, TouchPhase, UniformListScrollHandle, VisualTestContext, Window, div, point, px,
    uniform_list,
};

/// A 50 px box with content specified in device pixels at 1.5x.
struct Tall {
    scroll: ScrollHandle,
    content_pixels: f32,
}
impl Render for Tall {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .id("list")
                .h(px(50.))
                .w(px(100.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(div().w_full().h(px(self.content_pixels / 1.5))),
        )
    }
}

#[test]
fn a_wheel_glide_leaves_a_bottom_edge_between_device_pixels() {
    let mut cx = TestAppContext::single();
    let scroll = ScrollHandle::new();
    let window = cx.add_window({
        let scroll = scroll.clone();
        |_, _| Tall {
            scroll,
            content_pixels: 254.,
        }
    });
    let frame = |cx: &mut TestAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .unwrap();
    };
    cx.update_window(window.into(), |_, window, _| window.set_scale_factor(1.5))
        .unwrap();
    frame(&mut cx);
    let max = scroll.max_offset().y;
    assert!(
        (f32::from(max) * 1.5).fract().abs() > 1e-3,
        "the bottom edge {max:?} lies on a device pixel; the case needs one between them"
    );
    scroll.set_offset(point(px(0.), -max));
    frame(&mut cx);
    assert_eq!(scroll.offset().y, -max);

    // A notch up, through the scroller's own wheel listener. Its first steps
    // barely move and snap past the edge; they must not end the glide.
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    visual.simulate_event(ScrollEvent {
        position: point(px(50.), px(25.)),
        delta: ScrollDelta::Lines(point(0., 3.)),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    });
    for _ in 0..3 {
        frame(&mut cx);
        assert!(scroll.offset().y >= -max, "a step went past the edge");
    }
    // Past the longest glide (200 ms), the last step lands on the target.
    std::thread::sleep(std::time::Duration::from_millis(250));
    frame(&mut cx);
    let target = -max + px(100.);
    assert!(
        (scroll.offset().y - target).abs() < px(1.),
        "the glide up from the bottom stopped at {:?}, not near {target:?}",
        scroll.offset().y
    );
}

#[test]
fn a_glide_reaches_the_edge_even_when_snapping_would_stop_short() {
    let mut cx = TestAppContext::single();
    let scroll = ScrollHandle::new();
    let window = cx.add_window({
        let scroll = scroll.clone();
        |_, _| Tall {
            scroll,
            content_pixels: 253.,
        }
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.set_scale_factor(1.5);
        window.refresh();
        window.draw(cx).clear(cx);
        let end = -scroll.max_offset().y;
        assert!(window.pixel_snap(end) > end);
        assert!(scroll.scroll_by_lines(point(0., -30.)));
        window.refresh();
        window.draw(cx).clear(cx);
        std::thread::sleep(std::time::Duration::from_millis(250));
        window.refresh();
        window.draw(cx).clear(cx);
        assert_eq!(scroll.offset().y, end);
        // The next notch can now go to the surrounding scroller.
        assert!(!scroll.scroll_by_lines(point(0., -3.)));
    })
    .unwrap();
}

#[test]
fn uniform_list_uses_the_scroll_handles_bottom_edge() {
    struct Rows(UniformListScrollHandle);
    impl Render for Rows {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                uniform_list("rows", 12, |range, _, _| {
                    range.map(|_| div().h(px(30.))).collect()
                })
                .track_scroll(&self.0)
                .w(px(100.))
                .h(px(398. / 1.5)),
            )
        }
    }

    let mut cx = TestAppContext::single();
    let rows = UniformListScrollHandle::new();
    let scroll = rows.0.borrow().base_handle.clone();
    let window = cx.add_window(|_, _| Rows(rows.clone()));
    cx.update_window(window.into(), |_, window, cx| {
        window.set_scale_factor(1.5);
        // The last row and the scrollbar must agree where a glide starts.
        scroll.scroll_to_bottom();
        window.refresh();
        window.draw(cx).clear(cx);
        assert!(scroll.max_offset().y > px(90.));
        assert_eq!(scroll.offset().y, -scroll.max_offset().y);
        assert_eq!(rows.is_scrolled_to_end(), Some(true));
    })
    .unwrap();
}
