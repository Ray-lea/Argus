//! Refresh sensor IPC independently of slow process/device collection.
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

pub(super) fn run(rx: mpsc::Receiver<Option<Duration>>, mut read: impl FnMut()) {
    let mut period: Option<Duration> = None;
    let mut next = Instant::now();
    loop {
        let command = if period.is_some() {
            match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                Ok(value) => Some(value),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(value) => Some(value),
                Err(_) => break,
            }
        };
        if let Some(mut latest) = command {
            for value in rx.try_iter() {
                latest = value;
            }
            let now = Instant::now();
            if period != latest {
                next = now;
            }
            period = latest;
        }
        let Some(period_now) = period else { continue };
        let now = Instant::now();
        if now < next {
            continue;
        }
        read();
        next = Instant::now() + period_now;
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn collection_pauses_do_not_stop_refresh_and_disable_stops_the_worker() {
        let (tx, rx) = mpsc::channel();
        let (sample, received) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            run(rx, || {
                let _ = sample.send(());
            })
        });
        assert!(received.recv_timeout(Duration::from_millis(30)).is_err());
        tx.send(Some(Duration::from_millis(10))).unwrap();
        for _ in 0..4 {
            received.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        tx.send(None).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        while received.try_recv().is_ok() {}
        assert!(received.recv_timeout(Duration::from_millis(30)).is_err());
        tx.send(Some(Duration::from_millis(10))).unwrap();
        received.recv_timeout(Duration::from_secs(1)).unwrap();
        drop(tx);
        worker.join().unwrap();
    }
}
