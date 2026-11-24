use super::{FrameDemand, FrameWake};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

fn window() -> (Arc<FrameWake>, FrameDemand) {
    let clock = Arc::new(FrameWake::default());
    (clock.clone(), FrameDemand::new(clock))
}

#[test]
fn visibility_change_before_wait_is_not_lost() {
    let wake = FrameWake::default();
    let epoch = wake.epoch();
    wake.wake();
    assert!(wake.wait_since(epoch, &AtomicBool::new(false), Duration::from_secs(1)));
}

#[test]
fn idle_wait_wakes_on_restore_and_stop() {
    for stopping in [false, true] {
        let wake = Arc::new(FrameWake::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = {
            let wake = wake.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let epoch = wake.epoch();
                ready_tx.send(()).unwrap();
                done_tx
                    .send(wake.wait_since(epoch, &stop, Duration::from_secs(5)))
                    .unwrap();
            })
        };
        ready_rx.recv().unwrap();
        assert!(done_rx.try_recv().is_err());
        if stopping {
            stop.store(true, Ordering::Release);
        }
        wake.wake();
        assert!(done_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        worker.join().unwrap();
    }
}

#[test]
fn an_idle_clock_still_checks_device_health_after_its_timeout() {
    let wake = FrameWake::default();
    assert!(!wake.wait_since(
        wake.epoch(),
        &AtomicBool::new(false),
        Duration::from_millis(10)
    ));
}

#[test]
fn a_window_without_demand_leaves_the_clock_idle() {
    let (clock, window) = window();
    let epoch = clock.epoch();
    assert!(!window.is_pending());
    assert!(!window.take_at_vblank());
    assert_eq!(clock.epoch(), epoch);
}

#[test]
fn a_request_is_served_by_the_next_vblank_alone() {
    let (clock, window) = window();
    let epoch = clock.epoch();
    // GPUI repeats requests (frame waker, `schedule_frame`): one wake.
    window.request();
    window.request();
    assert_eq!(clock.epoch(), epoch.wrapping_add(1));
    assert!(window.is_pending());
    assert!(window.take_at_vblank());
    assert!(!window.is_pending());
    assert!(!window.take_at_vblank());
}

#[test]
fn a_request_before_the_clock_waits_is_not_lost() {
    let (clock, window) = window();
    // The clock reads the epoch, then finds no demand...
    let epoch = clock.epoch();
    assert!(!window.is_pending());
    // ...and the window asks for a frame before the clock waits.
    window.request();
    assert!(clock.wait_since(epoch, &AtomicBool::new(false), Duration::from_secs(5)));
    assert!(window.take_at_vblank());
}

#[test]
fn a_request_wakes_a_waiting_clock_at_once() {
    let (clock, window) = window();
    let (ready_tx, ready_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let epoch = clock.epoch();
        ready_tx.send(()).unwrap();
        let waited = Instant::now();
        let woken = clock.wait_since(epoch, &AtomicBool::new(false), Duration::from_secs(10));
        (woken, waited.elapsed())
    });
    ready_rx.recv().unwrap();
    window.request();
    let (woken, waited) = worker.join().unwrap();
    assert!(woken, "the clock waited for its timeout");
    assert!(
        waited < Duration::from_secs(5),
        "the clock woke after {waited:?}"
    );
    assert!(window.take_at_vblank());
}

#[test]
fn a_paint_wakes_the_clock_and_each_vblank_during_it_invalidates_the_window() {
    let (clock, window) = window();
    let epoch = clock.epoch();
    {
        let _painting = window.paint();
        // The clock watches the vblanks during a paint it did not start.
        assert_eq!(clock.epoch(), epoch.wrapping_add(1));
        assert!(window.is_pending());
        // Overrunning frames: every vblank during the paint leaves an
        // invalidation behind, so the next frame follows at once.
        assert!(window.take_at_vblank());
        assert!(window.take_at_vblank());
    }
    assert!(!window.is_pending());
    assert!(!window.take_at_vblank());
}

#[test]
fn a_request_made_while_painting_outlives_the_paint() {
    let (_clock, window) = window();
    {
        let _painting = window.paint();
        // An animation step scheduled during the draw.
        window.request();
    }
    assert!(window.is_pending());
    assert!(window.take_at_vblank());
    assert!(!window.is_pending());
}
