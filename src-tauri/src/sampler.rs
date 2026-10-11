//! The sampling thread: produces snapshots on a fixed cadence and streams them
//! to the UI over a Tauri channel.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use argus_core::{Collector, Snapshot};
use tauri::ipc::Channel;

/// Shortest window over which CPU percentages are still meaningful.
const MIN_SAMPLE_GAP: Duration = Duration::from_millis(250);
/// Cadence while the window is hidden: history stays continuous and the
/// first frame after showing is fresh, at a fraction of the cost.
const BACKGROUND_INTERVAL: Duration = Duration::from_secs(5);
pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(1000);

type Probe = Box<dyn Fn() -> bool + Send>;
type Observer = Box<dyn Fn(&Snapshot) + Send>;

pub enum Control {
    Subscribe(Channel<Snapshot>),
    SetInterval(Duration),
    SetPaused(bool),
    /// Take a sample now (e.g. right after ending a process, or on show).
    Refresh,
    /// Tells whether the window is currently hidden or minimized.
    SetHiddenProbe(Probe),
    /// Called with every snapshot on the sampler thread (tray tooltip).
    SetObserver(Observer),
}

pub struct Sampler {
    tx: Sender<Control>,
}

impl Sampler {
    pub fn spawn(collector: Collector) -> Self {
        let (tx, rx) = mpsc::channel::<Control>();
        std::thread::Builder::new()
            .name("argus-sampler".into())
            .spawn(move || {
                let mut collector = collector;
                let mut subscriber: Option<Channel<Snapshot>> = None;
                let mut hidden: Option<Probe> = None;
                let mut observer: Option<Observer> = None;
                let mut interval = DEFAULT_INTERVAL;
                let mut paused = false;
                let mut next_tick = Instant::now() + interval;
                let mut last_sample = Instant::now();

                loop {
                    let wait = next_tick.saturating_duration_since(Instant::now());
                    let forced = match rx.recv_timeout(wait) {
                        Ok(Control::Subscribe(ch)) => {
                            subscriber = Some(ch);
                            true
                        }
                        Ok(Control::SetInterval(d)) => {
                            interval = d;
                            next_tick = last_sample + interval;
                            continue;
                        }
                        Ok(Control::SetPaused(p)) => {
                            paused = p;
                            continue;
                        }
                        Ok(Control::Refresh) => true,
                        Ok(Control::SetHiddenProbe(probe)) => {
                            hidden = Some(probe);
                            continue;
                        }
                        Ok(Control::SetObserver(f)) => {
                            observer = Some(f);
                            continue;
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            next_tick += interval;
                            if next_tick < Instant::now() {
                                next_tick = Instant::now() + interval;
                            }
                            false
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                    };

                    if subscriber.is_none() && observer.is_none() {
                        continue;
                    }
                    let since = last_sample.elapsed();
                    let skip = if forced {
                        since < MIN_SAMPLE_GAP
                    } else if hidden.as_ref().is_some_and(|h| h()) {
                        since < BACKGROUND_INTERVAL
                    } else {
                        paused
                    };
                    if skip {
                        continue;
                    }

                    last_sample = Instant::now();
                    let snapshot = collector.sample();
                    if let Some(f) = &observer {
                        f(&snapshot);
                    }
                    if let Some(ch) = &subscriber
                        && ch.send(snapshot).is_err()
                    {
                        subscriber = None;
                    }
                }
            })
            .expect("spawn sampler thread");
        Self { tx }
    }

    pub fn send(&self, control: Control) {
        let _ = self.tx.send(control);
    }
}
