use gpui_test::{Context, DisplayId, IntoElement, Render, TestAppContext, Window, div};
use std::{cell::RefCell, rc::Rc, time::Duration};

struct EmptyView;
impl Render for EmptyView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[test]
fn monitor_change_during_window_update_is_delivered_after_the_borrow() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| EmptyView);
    cx.simulate_display_added(DisplayId::new(2), Some(Duration::from_millis(10)));
    let platform = cx.test_window(window.into());
    window
        .update(&mut cx, |_, _, _| {
            platform.simulate_move_to_display(DisplayId::new(2));
        })
        .unwrap();
    cx.executor().run_until_parked();
    window
        .update(&mut cx, |_, window, cx| {
            assert_eq!(window.display(cx).unwrap().id(), DisplayId::new(2));
            assert_eq!(window.refresh_interval(cx), Some(Duration::from_millis(10)));
        })
        .unwrap();
}

#[test]
fn repeated_monitor_changes_are_coalesced_to_the_latest_display() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| EmptyView);
    cx.simulate_display_added(DisplayId::new(2), Some(Duration::from_millis(10)));
    cx.simulate_display_added(DisplayId::new(3), Some(Duration::from_millis(8)));
    let notifications = Rc::new(RefCell::new(Vec::new()));
    let observed = notifications.clone();
    let _subscription = window
        .update(&mut cx, |_, window, _| {
            window.observe_window_display(move |window, cx| {
                observed.borrow_mut().push((
                    window.display(cx).unwrap().id(),
                    window.refresh_interval(cx),
                ));
            })
        })
        .unwrap();
    let platform = cx.test_window(window.into());
    window
        .update(&mut cx, |_, _, _| {
            platform.simulate_move_to_display(DisplayId::new(2));
            platform.simulate_move_to_display(DisplayId::new(3));
            assert!(notifications.borrow().is_empty());
        })
        .unwrap();
    cx.executor().run_until_parked();
    assert_eq!(
        *notifications.borrow(),
        [(DisplayId::new(3), Some(Duration::from_millis(8)))]
    );
    // Uncontended notifications stay synchronous; unchanged displays are ignored.
    platform.simulate_move_to_display(DisplayId::new(2));
    platform.simulate_move_to_display(DisplayId::new(2));
    assert_eq!(
        *notifications.borrow(),
        [
            (DisplayId::new(3), Some(Duration::from_millis(8))),
            (DisplayId::new(2), Some(Duration::from_millis(10)))
        ]
    );
}

#[test]
fn native_nested_loop_retries_after_the_outer_update_returns() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| EmptyView);
    cx.simulate_display_added(DisplayId::new(2), Some(Duration::from_millis(10)));
    let platform = cx.test_window(window.into());
    let executor = cx.executor().clone();
    window
        .update(&mut cx, |_, window, cx| {
            platform.simulate_move_to_display(DisplayId::new(2));
            executor.run_until_parked();
            assert_ne!(window.display(cx).unwrap().id(), DisplayId::new(2));
        })
        .unwrap();
    executor.advance_clock(Duration::from_millis(2));
    executor.run_until_parked();
    window
        .update(&mut cx, |_, window, cx| {
            assert_eq!(window.display(cx).unwrap().id(), DisplayId::new(2));
            assert_eq!(window.refresh_interval(cx), Some(Duration::from_millis(10)));
        })
        .unwrap();
}

#[test]
fn pending_monitor_change_tolerates_a_closed_window() {
    let mut cx = TestAppContext::single();
    let window = cx.add_window(|_, _| EmptyView);
    cx.simulate_display_added(DisplayId::new(2), Some(Duration::from_millis(10)));
    let platform = cx.test_window(window.into());
    window
        .update(&mut cx, |_, window, _| {
            platform.simulate_move_to_display(DisplayId::new(2));
            window.remove_window();
        })
        .unwrap();
    cx.executor().run_until_parked();
    assert!(window.update(&mut cx, |_, _, _| ()).is_err());
}
