#![cfg(target_os = "windows")]
//! The vendored Windows platform's frame clock (`frame_clock.rs` in
//! gpui-pre-windows) invalidates a window only when it asks for a frame. A
//! shown window with nothing to draw receives no `WM_PAINT`, a notification is
//! drawn at the next vblank, and an animation draws every vblank without
//! waking the windows beside it. The clock once invalidated every shown window
//! on every vblank, so an idle window woke the UI thread at the refresh rate.

use std::{
    cell::Cell,
    rc::Rc,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

use gpui_kit::gpui::{
    AppContext, Context, IntoElement, Render, Styled, Window, WindowKind, WindowOptions, div,
};
// The native Windows backend, which owns the frame clock.
use gpui_windows as _;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos, WM_PAINT,
        },
    },
};

static ANIMATED_PAINTS: AtomicU32 = AtomicU32::new(0);
static IDLE_PAINTS: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn count_paints(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    _: usize,
    counter: usize,
) -> LRESULT {
    if message == WM_PAINT {
        // SAFETY: `counter` is the address of one of the statics above.
        unsafe { &*(counter as *const AtomicU32) }.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { DefSubclassProc(hwnd, message, w, l) }
}

#[derive(Default)]
struct Probe {
    renders: Cell<u32>,
    last_render: Cell<Option<Instant>>,
    animate: Cell<bool>,
}

struct ProbeView(Rc<Probe>);

impl Render for ProbeView {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.renders.set(self.0.renders.get() + 1);
        self.0.last_render.set(Some(Instant::now()));
        if self.0.animate.get() {
            window.request_animation_frame();
        }
        div().size_full()
    }
}

#[derive(Debug, Default)]
struct Readings {
    refresh: Option<Duration>,
    idle_paints: u32,
    idle_renders: u32,
    notify_latency: Option<Duration>,
    animation_frames: u32,
    animation_paints: u32,
    neighbour_paints: u32,
    paints_after_animation: u32,
}

fn hwnd(window: &Window) -> HWND {
    let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window).unwrap().as_raw()
    else {
        panic!("Windows fixture");
    };
    HWND(handle.hwnd.get() as _)
}

#[test]
fn a_shown_window_paints_only_when_it_asks_for_a_frame() {
    let readings = Rc::new(Cell::new(None::<Readings>));
    let reported = readings.clone();
    gpui_kit::application().run(move |cx| {
        let mut open = |counter: &'static AtomicU32, x: i32| {
            let probe = Rc::new(Probe::default());
            let view = probe.clone();
            let handle = cx
                .open_window(
                    WindowOptions {
                        titlebar: None,
                        show: false,
                        focus: false,
                        kind: WindowKind::PopUp,
                        // Never activated: without this, GPUI limits the
                        // animation to its inactive-window rate.
                        inactive_frame_interval: None,
                        ..Default::default()
                    },
                    |_, cx| cx.new(|_| ProbeView(view)),
                )
                .unwrap();
            let (hwnd, refresh) = handle
                .update(cx, |_, window, cx| {
                    (hwnd(window), window.refresh_interval(cx))
                })
                .unwrap();
            unsafe {
                assert!(
                    SetWindowSubclass(hwnd, Some(count_paints), 1, counter as *const _ as usize)
                        .as_bool()
                );
                // Shown far off screen, not activated, small enough for a
                // 1024×768 CI screen: the frame clock serves shown windows.
                SetWindowPos(
                    hwnd,
                    None,
                    -4000 + x,
                    -4000,
                    160,
                    100,
                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
                )
                .unwrap();
            }
            (probe, handle, hwnd, refresh)
        };
        let (animated, animated_view, animated_hwnd, refresh) = open(&ANIMATED_PAINTS, 0);
        let (_idle, _idle_view, idle_hwnd, _) = open(&IDLE_PAINTS, 400);

        cx.spawn(async move |cx| {
            let executor = cx.background_executor().clone();
            let wait = |ms| executor.timer(Duration::from_millis(ms));
            let paints =
                || ANIMATED_PAINTS.load(Ordering::Relaxed) + IDLE_PAINTS.load(Ordering::Relaxed);
            let mut readings = Readings {
                refresh,
                ..Default::default()
            };
            // Let the first frames and the show settle.
            wait(500).await;

            let (paints_before, renders_before) = (paints(), animated.renders.get());
            wait(600).await;
            readings.idle_paints = paints() - paints_before;
            readings.idle_renders = animated.renders.get() - renders_before;

            let renders_before = animated.renders.get();
            let notified = Instant::now();
            animated_view.update(cx, |_, _, cx| cx.notify()).unwrap();
            wait(250).await;
            if animated.renders.get() > renders_before {
                readings.notify_latency = animated
                    .last_render
                    .get()
                    .map(|rendered| rendered.saturating_duration_since(notified));
            }

            let renders_before = animated.renders.get();
            let animated_before = ANIMATED_PAINTS.load(Ordering::Relaxed);
            let idle_before = IDLE_PAINTS.load(Ordering::Relaxed);
            animated.animate.set(true);
            animated_view.update(cx, |_, _, cx| cx.notify()).unwrap();
            wait(500).await;
            animated.animate.set(false);
            readings.animation_frames = animated.renders.get() - renders_before;
            readings.animation_paints = ANIMATED_PAINTS.load(Ordering::Relaxed) - animated_before;
            readings.neighbour_paints = IDLE_PAINTS.load(Ordering::Relaxed) - idle_before;

            wait(300).await;
            let paints_before = paints();
            wait(600).await;
            readings.paints_after_animation = paints() - paints_before;

            unsafe {
                let _ = RemoveWindowSubclass(animated_hwnd, Some(count_paints), 1);
                let _ = RemoveWindowSubclass(idle_hwnd, Some(count_paints), 1);
            }
            reported.set(Some(readings));
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let readings = readings.take().expect("the measurement did not finish");
    eprintln!("{readings:?}");
    // The always-running clock painted both windows on every vblank: 36 times
    // each in 0.6 s at 60 Hz, 96 at 160 Hz.
    assert!(
        readings.idle_paints <= 2 && readings.idle_renders == 0,
        "idle windows painted: {readings:?}"
    );
    // A lost wake would leave the request to the clock's one-second
    // device-health timeout.
    assert!(
        readings
            .notify_latency
            .is_some_and(|latency| latency < Duration::from_millis(200)),
        "a notification was drawn late or not at all: {readings:?}"
    );
    // Half a second of an animation asking for every frame. Generous for a
    // loaded machine; a clock that waited a vblank extra per frame would halve it.
    let interval = readings.refresh.unwrap_or(Duration::from_micros(16_667));
    let expected = (Duration::from_millis(500).as_secs_f64() / interval.as_secs_f64()) as u32;
    assert!(
        readings.animation_frames * 10 >= expected * 6,
        "the animation drew {} of about {expected} frames: {readings:?}",
        readings.animation_frames
    );
    assert!(
        readings.neighbour_paints <= 2,
        "an animation woke the idle window beside it: {readings:?}"
    );
    assert!(
        readings.paints_after_animation <= 2,
        "the clock kept painting after the animation ended: {readings:?}"
    );
}
