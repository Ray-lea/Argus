use gpui_test::{Context, IntoElement, Render, TestAppContext, Window, div, px, size};
use std::time::Duration;

struct EmptyView;
impl Render for EmptyView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[test]
fn native_resize_during_window_update_keeps_the_latest_viewport() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| EmptyView);
    let mut platform = cx.test_window(window.into());
    let expected = size(px(1600.), px(1000.));
    window
        .update(&mut cx, |_, _, _| {
            platform.simulate_resize(size(px(1240.), px(800.)));
            platform.simulate_resize(expected);
            platform.simulate_scale_factor_change(1.5);
        })
        .unwrap();
    cx.executor().run_until_parked();
    window
        .update(&mut cx, |_, window, _| {
            assert_eq!(window.viewport_size(), expected);
            assert_eq!(window.scale_factor(), 1.5);
        })
        .unwrap();
}

#[test]
fn uncontended_resize_is_still_synchronous() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| EmptyView);
    let mut platform = cx.test_window(window.into());
    let expected = size(px(1280.), px(720.));
    platform.simulate_resize(expected);
    window
        .update(&mut cx, |_, window, _| {
            assert_eq!(window.viewport_size(), expected)
        })
        .unwrap();
}

#[test]
fn a_nested_native_loop_keeps_the_resize_pending() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| EmptyView);
    let mut platform = cx.test_window(window.into());
    let executor = cx.executor().clone();
    let expected = size(px(1366.), px(768.));
    window
        .update(&mut cx, |_, window, _| {
            platform.simulate_resize(expected);
            executor.run_until_parked();
            assert_ne!(window.viewport_size(), expected);
        })
        .unwrap();
    executor.advance_clock(Duration::from_millis(2));
    executor.run_until_parked();
    window
        .update(&mut cx, |_, window, _| {
            assert_eq!(window.viewport_size(), expected)
        })
        .unwrap();
}

#[test]
fn a_queued_resize_can_outlive_the_closed_window() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| EmptyView);
    let mut platform = cx.test_window(window.into());
    window
        .update(&mut cx, |_, window, _| {
            platform.simulate_resize(size(px(1280.), px(720.)));
            window.remove_window();
        })
        .unwrap();
    cx.executor().run_until_parked();
    assert!(window.update(&mut cx, |_, _, _| ()).is_err());
}
