//! The Windows contact lifetimes and GPUI gesture path used by Argus.
extern crate gpui_test as gpui;

#[path = "../../vendor/gpui-pre-windows/src/touch.rs"]
mod touch;

use gpui::{
    Context, DispatchPhase, GestureTuning, InteractiveElement, IntoElement, LongPressEvent,
    MouseButton, ParentElement, PlatformInput, Render, ScrollHandle, StatefulInteractiveElement,
    Styled, TestAppContext, TouchEvent, TouchId, TouchPhase, VisualTestContext, Window, canvas,
    div, point, px,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Default)]
struct Observed {
    clicks: Vec<usize>,
    menus: usize,
    long_presses: usize,
}

struct Content {
    observed: Rc<RefCell<Observed>>,
    scroll: ScrollHandle,
    claim_long_press: bool,
}

impl Render for Content {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let taps = self.observed.clone();
        let menus = self.observed.clone();
        let held = self.observed.clone();
        let claim = self.claim_long_press;
        div()
            .id("scroller")
            .size(px(200.))
            .overflow_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .id("touch-target")
                    .w(px(1500.))
                    .h(px(1500.))
                    .on_mouse_down(MouseButton::Left, move |event, _, _| {
                        taps.borrow_mut().clicks.push(event.click_count);
                    })
                    .on_mouse_down(MouseButton::Right, move |_, _, _| {
                        menus.borrow_mut().menus += 1;
                    })
                    .child(canvas(
                        |_, _, _| {},
                        move |_, _, window, _| {
                            let held = held.clone();
                            window.on_mouse_event(
                                move |event: &LongPressEvent, phase, window, _| {
                                    if claim
                                        && phase == DispatchPhase::Bubble
                                        && event.phase == TouchPhase::Started
                                    {
                                        held.borrow_mut().long_presses += 1;
                                        window.prevent_default();
                                    }
                                },
                            );
                        },
                    )),
            )
    }
}

fn setup(claim: bool) -> (VisualTestContext, Rc<RefCell<Observed>>, ScrollHandle) {
    let mut cx = TestAppContext::single();
    let observed = Rc::new(RefCell::new(Observed::default()));
    let scroll = ScrollHandle::new();
    let window = cx.add_window({
        let observed = observed.clone();
        let scroll = scroll.clone();
        move |_, _| Content {
            observed,
            scroll,
            claim_long_press: claim,
        }
    });
    cx.run_until_parked();
    (
        VisualTestContext::from_window(window.into(), &cx),
        observed,
        scroll,
    )
}

fn send(cx: &mut VisualTestContext, id: u64, phase: TouchPhase, y: f32) {
    cx.simulate_event(TouchEvent {
        id: TouchId(id),
        phase,
        position: point(px(40.), px(y)),
        ..Default::default()
    });
}

#[test]
fn taps_commit_on_release_and_accumulate_double_taps() {
    let (mut cx, observed, _) = setup(false);
    send(&mut cx, 1, TouchPhase::Started, 100.);
    assert!(observed.borrow().clicks.is_empty());
    send(&mut cx, 1, TouchPhase::Ended, 101.);
    cx.executor().advance_clock(Duration::from_millis(60));
    send(&mut cx, 2, TouchPhase::Started, 100.);
    send(&mut cx, 2, TouchPhase::Ended, 100.);
    assert_eq!(observed.borrow().clicks, [1, 2]);
}

#[test]
fn panning_scrolls_without_clicking_and_cancellation_stops_it() {
    let (mut cx, observed, scroll) = setup(false);
    send(&mut cx, 1, TouchPhase::Started, 150.);
    cx.executor().advance_clock(Duration::from_millis(16));
    send(&mut cx, 1, TouchPhase::Moved, 100.);
    cx.executor().advance_clock(Duration::from_millis(16));
    send(&mut cx, 1, TouchPhase::Moved, 50.);
    assert!(scroll.offset().y < px(-50.));
    send(&mut cx, 1, TouchPhase::Cancelled, 50.);
    let offset = scroll.offset();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(scroll.offset(), offset);
    assert!(observed.borrow().clicks.is_empty());
    assert_eq!(observed.borrow().menus, 0);
}

#[test]
fn a_release_with_coalesced_movement_scrolls_instead_of_clicking() {
    let (mut cx, observed, scroll) = setup(false);
    send(&mut cx, 1, TouchPhase::Started, 150.);
    send(&mut cx, 1, TouchPhase::Ended, 50.);
    assert_eq!(scroll.offset().y, px(-100.));
    assert!(observed.borrow().clicks.is_empty());
}

#[test]
fn horizontal_pan_preserves_the_vertical_offset() {
    let (mut cx, observed, scroll) = setup(false);
    for (phase, x) in [
        (TouchPhase::Started, 150.),
        (TouchPhase::Moved, 50.),
        (TouchPhase::Cancelled, 50.),
    ] {
        cx.simulate_event(TouchEvent {
            id: TouchId(1),
            phase,
            position: point(px(x), px(100.)),
            ..Default::default()
        });
    }
    assert_eq!(scroll.offset(), point(px(-100.), px(0.)));
    assert!(observed.borrow().clicks.is_empty());
}

#[test]
fn cancelled_or_secondary_contacts_do_not_commit_clicks() {
    let (mut cx, observed, _) = setup(false);
    send(&mut cx, 1, TouchPhase::Started, 100.);
    send(&mut cx, 2, TouchPhase::Started, 110.);
    send(&mut cx, 2, TouchPhase::Ended, 110.);
    send(&mut cx, 1, TouchPhase::Cancelled, 100.);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(observed.borrow().clicks.is_empty());
    assert_eq!(observed.borrow().menus, 0);
    send(&mut cx, 3, TouchPhase::Started, 100.);
    send(&mut cx, 3, TouchPhase::Ended, 100.);
    assert_eq!(observed.borrow().clicks, [1]);
}

#[test]
#[cfg(target_os = "windows")]
fn long_press_opens_one_context_menu_and_release_never_clicks() {
    let (mut cx, observed, _) = setup(false);
    send(&mut cx, 1, TouchPhase::Started, 100.);
    cx.executor()
        .advance_clock(GestureTuning::default().long_press_duration + Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(observed.borrow().menus, 1);
    send(&mut cx, 1, TouchPhase::Ended, 100.);
    assert_eq!(observed.borrow().menus, 1);
    assert!(observed.borrow().clicks.is_empty());
}

#[test]
fn claimed_long_press_preserves_text_selection_and_tooltip_handlers() {
    let (mut cx, observed, _) = setup(true);
    send(&mut cx, 1, TouchPhase::Started, 100.);
    cx.executor()
        .advance_clock(GestureTuning::default().long_press_duration + Duration::from_millis(1));
    cx.run_until_parked();
    send(&mut cx, 1, TouchPhase::Ended, 100.);
    assert_eq!(observed.borrow().long_presses, 1);
    assert_eq!(observed.borrow().menus, 0);
    assert!(observed.borrow().clicks.is_empty());
}

#[test]
fn nested_native_input_preserves_cancel_then_new_tap_order() {
    let (mut cx, observed, _) = setup(false);
    send(&mut cx, 1, TouchPhase::Started, 100.);
    let handle = cx.update(|window, _| window.window_handle());
    let mut platform = cx.test_window(handle);
    let executor = cx.executor().clone();
    cx.update(|_, _| {
        for (id, phase) in [
            (1, TouchPhase::Cancelled),
            (2, TouchPhase::Started),
            (2, TouchPhase::Ended),
        ] {
            platform.simulate_input(PlatformInput::Touch(TouchEvent {
                id: TouchId(id),
                phase,
                position: point(px(40.), px(100.)),
                ..Default::default()
            }));
        }
        executor.run_until_parked();
        assert!(observed.borrow().clicks.is_empty());
    });
    executor.advance_clock(Duration::from_millis(2));
    executor.run_until_parked();
    assert_eq!(observed.borrow().clicks, [1]);
    assert_eq!(observed.borrow().menus, 0);
}

#[test]
fn queued_native_input_is_discarded_when_the_window_closes() {
    let (mut cx, observed, _) = setup(false);
    let handle = cx.update(|window, _| window.window_handle());
    let mut platform = cx.test_window(handle);
    cx.update(|window, _| {
        platform.simulate_input(PlatformInput::Touch(TouchEvent {
            id: TouchId(1),
            phase: TouchPhase::Started,
            position: point(px(40.), px(100.)),
            ..Default::default()
        }));
        window.remove_window();
    });
    cx.run_until_parked();
    assert!(observed.borrow().clicks.is_empty());
}
