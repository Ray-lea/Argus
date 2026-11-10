use super::FrameWake;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

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
