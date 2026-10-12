//! The sampling thread: produces snapshots on a fixed cadence and streams them
//! to the UI over a Tauri channel.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use argus_core::history::{self, HistoryRecorder};
use argus_core::{Collector, Snapshot};
use argus_service::{NetworkClient, RuleRunner};
use tauri::ipc::Channel;

/// Shortest window over which CPU percentages are still meaningful.
const MIN_SAMPLE_GAP: Duration = Duration::from_millis(250);
/// Cadence while the window is hidden or the view is paused: history and the
/// process rules keep going, and the first frame after showing is fresh, at a
/// fraction of the cost.
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
    /// Starts recording history into this folder, or stops with None.
    SetHistoryDir(Option<PathBuf>),
    /// Deletes the history in this folder; replies once the files are gone.
    /// Goes through this thread so no frame is being written meanwhile.
    ClearHistory(PathBuf, Sender<io::Result<()>>),
}

pub struct Sampler {
    tx: Sender<Control>,
}

impl Sampler {
    /// Starts the sampling thread. The collector is built there, so its setup
    /// (PDH queries, adapter enumeration, a priming sample) overlaps the
    /// WebView's startup instead of delaying it.
    /// Every sample also queues process rules and goes into the history.
    pub fn spawn(
        make_collector: impl FnOnce() -> Collector + Send + 'static,
        rules: Arc<RuleRunner>,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<Control>();
        std::thread::Builder::new()
            .name("argus-sampler".into())
            .spawn(move || {
                let mut collector = make_collector();
                let mut network = NetworkClient::spawn();
                let mut subscriber: Option<Channel<Snapshot>> = None;
                let mut hidden: Option<Probe> = None;
                let mut observer: Option<Observer> = None;
                let mut history: Option<HistoryRecorder> = None;
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
                        Ok(Control::SetHistoryDir(dir)) => {
                            history = dir.map(HistoryRecorder::new);
                            continue;
                        }
                        Ok(Control::ClearHistory(dir, reply)) => {
                            let result = match &mut history {
                                Some(recorder) => recorder.clear(),
                                None => history::clear(&dir),
                            };
                            let _ = reply.send(result);
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
                    let background = paused || hidden.as_ref().is_some_and(|h| h());
                    let skip = if forced {
                        since < MIN_SAMPLE_GAP
                    } else if background {
                        since < BACKGROUND_INTERVAL
                    } else {
                        false
                    };
                    if skip {
                        continue;
                    }

                    last_sample = Instant::now();
                    let mut snapshot = collector.sample();
                    network.enrich(
                        &mut snapshot,
                        if background {
                            BACKGROUND_INTERVAL
                        } else {
                            interval
                        },
                    );
                    rules.observe(&snapshot.processes);
                    if let Some(recorder) = &mut history {
                        recorder.record(&snapshot);
                    }
                    if let Some(f) = &observer {
                        f(&snapshot);
                    }
                    // A paused view stays as it is, except right after an action.
                    if let Some(ch) = &subscriber
                        && (forced || !paused)
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
