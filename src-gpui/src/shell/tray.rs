//! The Shell may stop pumping messages. Keep one latest tray state on a
//! dedicated worker so neither sampling nor the interface waits for Explorer.
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub(super) struct Update {
    pub title: String,
    pub badge: bool,
    pub generation: u64,
    pub closed: bool,
    /// What the icon shows (`tray_icon`).
    pub look: crate::settings::TrayIcon,
    /// The runner's frame, wrapped into `tray_icon::FRAMES`.
    pub frame: u8,
    /// The latest CPU load in percent, for a live icon.
    pub load: Option<u8>,
    /// The recent loads, oldest first, ending with `load`: the columns of the
    /// live icon that shows them.
    pub recent: [Option<u8>; super::tray_icon::RECENT],
    /// Memory in use in percent; the runner's colour.
    pub memory: Option<u8>,
    /// The taskbar is light (`SystemUsesLightTheme`).
    pub light: bool,
    /// The system tooltip shows on hover; off, the hover card does.
    pub tip: bool,
    /// The latest notification from the icon; the icon shows each one once.
    pub notification: Option<Notification>,
}

/// A Windows notification from the tray icon (`NIF_INFO`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Notification {
    /// Counts up from 1: a notification the icon has shown is not shown again
    /// when the state is written for something else.
    pub number: u64,
    pub title: String,
    pub text: String,
}

struct State {
    update: Update,
    revision: u64,
    runner_epoch: u64,
    done: bool,
}

/// How long the runner holds a frame at `load` percent: RunCat365's pace,
/// `500 / max(1, load / 5)` ms, which tops out at 40 frames a second. Only
/// the runner animates; every other look waits for a new sample.
fn frame_interval(update: &Update) -> Option<Duration> {
    if update.look != crate::settings::TrayIcon::Runner {
        return None;
    }
    let rate = (f32::from(update.load.unwrap_or(0).min(100)) / 5.).max(1.);
    Some(Duration::from_millis((500. / rate).round() as u64))
}

pub(super) struct Worker {
    shared: Arc<(Mutex<State>, Condvar)>,
}

impl Worker {
    pub fn spawn(
        initial: Update,
        mut write: impl FnMut(Update) + Send + 'static,
    ) -> std::io::Result<Self> {
        let shared = Arc::new((
            Mutex::new(State {
                update: initial,
                revision: 1,
                runner_epoch: 1,
                done: false,
            }),
            Condvar::new(),
        ));
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("argus-tray".into())
            .spawn(move || {
                let mut applied = 0;
                let mut seen_epoch = 0;
                let mut last_step = Instant::now();
                loop {
                    let mut state = worker.0.lock().unwrap();
                    while !state.update.closed && state.revision == applied {
                        let Some(interval) = frame_interval(&state.update) else {
                            state = worker.1.wait(state).unwrap();
                            continue;
                        };
                        if seen_epoch != state.runner_epoch {
                            seen_epoch = state.runner_epoch;
                            last_step = Instant::now();
                        }
                        let due = last_step + interval;
                        let now = Instant::now();
                        if now >= due {
                            state.update.frame =
                                (state.update.frame + 1) % super::tray_icon::FRAMES as u8;
                            state.revision = state.revision.wrapping_add(1);
                            last_step = now;
                            break;
                        }
                        state = worker.1.wait_timeout(state, due - now).unwrap().0;
                    }
                    if !state.update.closed
                        && let Some(interval) = frame_interval(&state.update)
                    {
                        if seen_epoch != state.runner_epoch {
                            seen_epoch = state.runner_epoch;
                            last_step = Instant::now();
                        } else {
                            let now = Instant::now();
                            if now >= last_step + interval {
                                state.update.frame =
                                    (state.update.frame + 1) % super::tray_icon::FRAMES as u8;
                                last_step = now;
                            }
                        }
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
        self.update(|state, _| {
            // An unchanged reading also retries an Explorer endpoint that recovered.
            if state.title != title {
                state.title = title.to_owned();
            }
            true
        });
    }

    /// A new sample: the tooltip, the load a live icon shows, and the memory
    /// use that colours the runner.
    pub fn reading(&self, title: &str, load: u8, memory: u8) {
        self.update(|state, _| {
            if state.title != title {
                state.title = title.to_owned();
            }
            state.load = Some(load);
            state.memory = Some(memory);
            state.recent.rotate_left(1);
            *state.recent.last_mut().unwrap() = Some(load);
            true
        });
    }

    /// The icon's look and whether the system tooltip shows.
    pub fn style(&self, look: crate::settings::TrayIcon, tip: bool) {
        self.update(|state, epoch| {
            if state.look == look && state.tip == tip {
                return false;
            }
            // A newly chosen runner starts from its first frame.
            if state.look != look {
                state.frame = 0;
                if look == crate::settings::TrayIcon::Runner {
                    *epoch = epoch.wrapping_add(1);
                }
            }
            state.look = look;
            state.tip = tip;
            true
        });
    }

    /// The taskbar's theme, which a live icon's glyph follows.
    pub fn taskbar(&self, light: bool) {
        self.update(|state, _| {
            if state.light == light {
                return false;
            }
            state.light = light;
            true
        });
    }

    /// Shows a Windows notification from the icon, once.
    pub fn notify(&self, title: &str, text: &str) {
        self.update(|state, _| {
            let number = state.notification.as_ref().map_or(0, |n| n.number) + 1;
            state.notification = Some(Notification {
                number,
                title: title.to_owned(),
                text: text.to_owned(),
            });
            true
        });
    }

    pub fn badge(&self, badge: bool) {
        self.update(|state, _| {
            if state.badge == badge {
                return false;
            }
            state.badge = badge;
            true
        });
    }

    pub fn recreate(&self) {
        self.update(|state, _| {
            state.generation = state.generation.wrapping_add(1);
            true
        });
    }

    fn update(&self, update: impl FnOnce(&mut Update, &mut u64) -> bool) {
        let mut state = self.shared.0.lock().unwrap();
        if state.update.closed {
            return;
        }
        let State {
            update: current,
            runner_epoch,
            ..
        } = &mut *state;
        if !update(current, runner_epoch) {
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
    use super::{Update, Worker};
    use crate::settings::TrayIcon;
    use crate::shell::tray_icon::FRAMES;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    fn initial() -> Update {
        Update {
            title: "initial".into(),
            ..Update::default()
        }
    }

    #[test]
    fn a_hung_shell_does_not_block_updates_and_only_the_latest_state_is_sent() {
        let (entered, waiting) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (seen, results) = mpsc::channel();
        let mut first = true;
        let worker = Worker::spawn(initial(), move |update| {
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
        for load in 0..=100 {
            worker.reading("19999", load, load);
        }
        worker.style(TrayIcon::Number, true);
        worker.taskbar(true);
        worker.badge(true);
        worker.notify("first", "text");
        worker.notify("second", "text");
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
        assert_eq!(last.load, Some(100));
        assert_eq!(last.memory, Some(100));
        // Readings sent while the shell was busy still reach the columns.
        assert_eq!(last.recent, [95, 96, 97, 98, 99, 100].map(Some));
        assert!(last.look == TrayIcon::Number && last.tip && last.light);
        // Only the latest notification is left to show.
        let notification = last.notification.expect("a notification");
        assert_eq!(
            (notification.number, notification.title.as_str()),
            (2, "second")
        );
        worker.shutdown(Duration::from_secs(1));
        assert!(results.recv_timeout(Duration::from_secs(2)).unwrap().closed);
    }

    #[test]
    fn shutdown_has_a_deadline_while_the_shell_is_unresponsive() {
        let (entered, waiting) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut first = true;
        let worker = Worker::spawn(initial(), move |_| {
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

    #[test]
    fn the_runner_pace_follows_the_load_and_stops_at_forty_frames_a_second() {
        let at = |look: TrayIcon, load: Option<u8>| {
            super::frame_interval(&Update {
                look,
                load,
                ..Update::default()
            })
        };
        // RunCat365's pace: 500 / max(1, load / 5) ms.
        assert_eq!(
            at(TrayIcon::Runner, Some(100)),
            Some(Duration::from_millis(25))
        );
        assert_eq!(
            at(TrayIcon::Runner, Some(50)),
            Some(Duration::from_millis(50))
        );
        assert_eq!(
            at(TrayIcon::Runner, Some(25)),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            at(TrayIcon::Runner, Some(0)),
            Some(Duration::from_millis(500))
        );
        assert_eq!(at(TrayIcon::Runner, None), Some(Duration::from_millis(500)));
        // Every other look waits for the next sample.
        for look in [
            TrayIcon::Logo,
            TrayIcon::Load,
            TrayIcon::Share,
            TrayIcon::Number,
        ] {
            assert_eq!(at(look, Some(100)), None, "{look:?}");
        }
    }

    #[test]
    fn the_runner_advances_frames_without_new_samples() {
        let (seen, frames) = mpsc::channel();
        let worker = Worker::spawn(
            Update {
                look: TrayIcon::Runner,
                load: Some(100),
                ..Update::default()
            },
            move |update| {
                if !update.closed {
                    let _ = seen.send(update.frame);
                }
            },
        )
        .unwrap();
        let mut drawn = Vec::new();
        while drawn.len() < 6 {
            drawn.push(frames.recv_timeout(Duration::from_secs(2)).unwrap());
        }
        worker.shutdown(Duration::from_secs(1));
        assert_eq!(drawn[0], 0);
        for pair in drawn.windows(2) {
            assert_eq!(pair[1], (pair[0] + 1) % FRAMES as u8);
        }
    }

    #[test]
    fn samples_arriving_faster_than_a_frame_do_not_stall_the_runner() {
        let (seen, frames) = mpsc::channel();
        // 50 % load holds each frame for 50 ms; samples every 12 ms must not
        // push the deadline back on every reading.
        let worker = Worker::spawn(
            Update {
                look: TrayIcon::Runner,
                load: Some(50),
                ..Update::default()
            },
            move |update| {
                if !update.closed {
                    let _ = seen.send(update.frame);
                }
            },
        )
        .unwrap();
        for tick in 0..16 {
            std::thread::sleep(Duration::from_millis(12));
            worker.reading(&format!("tick {tick}"), 50, 40);
        }
        worker.shutdown(Duration::from_secs(1));
        let mut distinct: Vec<u8> = Vec::new();
        for frame in frames.try_iter() {
            if distinct.last().copied() != Some(frame) {
                distinct.push(frame);
            }
        }
        assert!(distinct.len() >= 3, "frames seen: {distinct:?}");
        assert_eq!(distinct[0], 0);
        for pair in distinct.windows(2) {
            assert_eq!(pair[1], (pair[0] + 1) % FRAMES as u8);
        }
    }

    #[test]
    fn a_still_icon_does_not_repaint_without_a_new_sample() {
        let (seen, frames) = mpsc::channel();
        let worker = Worker::spawn(
            Update {
                look: TrayIcon::Load,
                load: Some(100),
                ..Update::default()
            },
            move |update| {
                if !update.closed {
                    let _ = seen.send(update.frame);
                }
            },
        )
        .unwrap();
        frames.recv_timeout(Duration::from_secs(2)).unwrap();
        // Well past the runner's fastest frame: a still icon stays put.
        assert!(frames.recv_timeout(Duration::from_millis(200)).is_err());
        worker.shutdown(Duration::from_secs(1));
    }
}
