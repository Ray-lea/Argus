//! The Shell may stop pumping messages. Keep one latest tray state on a
//! dedicated worker so neither sampling nor the interface waits for Explorer.
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(super) struct Update {
    pub title: String,
    pub badge: bool,
    pub generation: u64,
    pub closed: bool,
}

struct State {
    update: Update,
    revision: u64,
    done: bool,
}

pub(super) struct Worker {
    shared: Arc<(Mutex<State>, Condvar)>,
}

impl Worker {
    pub fn spawn(
        title: String,
        mut write: impl FnMut(Update) + Send + 'static,
    ) -> std::io::Result<Self> {
        let shared = Arc::new((
            Mutex::new(State {
                update: Update {
                    title,
                    badge: false,
                    generation: 0,
                    closed: false,
                },
                revision: 1,
                done: false,
            }),
            Condvar::new(),
        ));
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("argus-tray".into())
            .spawn(move || {
                let mut applied = 0;
                loop {
                    let mut state = worker.0.lock().unwrap();
                    while !state.update.closed && state.revision == applied {
                        state = worker.1.wait(state).unwrap();
                    }
                    applied = state.revision;
                    let update = state.update.clone();
                    let closed = update.closed;
                    drop(state);
                    write(update);
                    if closed {
                        break;
                    }
                }
                drop(write);
                worker.0.lock().unwrap().done = true;
                worker.1.notify_all();
            })?;
        Ok(Self { shared })
    }

    pub fn title(&self, title: &str) {
        self.update(|state| {
            // An unchanged reading also retries an Explorer endpoint that recovered.
            if state.title != title {
                state.title = title.to_owned();
            }
            true
        });
    }

    pub fn badge(&self, badge: bool) {
        self.update(|state| {
            if state.badge == badge {
                return false;
            }
            state.badge = badge;
            true
        });
    }

    pub fn recreate(&self) {
        self.update(|state| {
            state.generation = state.generation.wrapping_add(1);
            true
        });
    }

    fn update(&self, update: impl FnOnce(&mut Update) -> bool) {
        let mut state = self.shared.0.lock().unwrap();
        if state.update.closed || !update(&mut state.update) {
            return;
        }
        state.revision = state.revision.wrapping_add(1);
        self.shared.1.notify_one();
    }

    pub fn shutdown(&self, budget: Duration) {
        let until = Instant::now() + budget;
        let mut state = self.shared.0.lock().unwrap();
        state.update.closed = true;
        self.shared.1.notify_all();
        while !state.done && Instant::now() < until {
            state = self
                .shared
                .1
                .wait_timeout(state, until.saturating_duration_since(Instant::now()))
                .unwrap()
                .0;
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown(Duration::ZERO);
    }
}

#[cfg(test)]
mod tests {
    use super::Worker;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    #[test]
    fn a_hung_shell_does_not_block_updates_and_only_the_latest_state_is_sent() {
        let (entered, waiting) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (seen, results) = mpsc::channel();
        let mut first = true;
        let worker = Worker::spawn("initial".into(), move |update| {
            if first {
                first = false;
                entered.send(()).unwrap();
                let _ = gate.recv_timeout(Duration::from_secs(3));
            }
            seen.send(update).unwrap();
        })
        .unwrap();
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        let started = Instant::now();
        for i in 0..20_000 {
            worker.title(&i.to_string());
        }
        worker.badge(true);
        worker.recreate();
        assert!(started.elapsed() < Duration::from_millis(200));
        release.send(()).unwrap();
        assert_eq!(
            results.recv_timeout(Duration::from_secs(2)).unwrap().title,
            "initial"
        );
        let last = results.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(last.title, "19999");
        assert!(last.badge && last.generation == 1);
        worker.shutdown(Duration::from_secs(1));
        assert!(results.recv_timeout(Duration::from_secs(2)).unwrap().closed);
    }

    #[test]
    fn shutdown_has_a_deadline_while_the_shell_is_unresponsive() {
        let (entered, waiting) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut first = true;
        let worker = Worker::spawn("initial".into(), move |_| {
            if first {
                first = false;
                entered.send(()).unwrap();
                let _ = gate.recv_timeout(Duration::from_secs(3));
            }
        })
        .unwrap();
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        let started = Instant::now();
        worker.shutdown(Duration::from_millis(20));
        assert!(started.elapsed() < Duration::from_millis(200));
        release.send(()).unwrap();
        worker.shutdown(Duration::from_secs(1));
    }
}
