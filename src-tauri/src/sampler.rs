//! The sampling thread: produces snapshots on a fixed cadence and streams them
//! to the UI over a Tauri channel.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use argus_core::history::{self, HistoryRecorder};
use argus_core::{Collector, Snapshot};
use argus_service::{DiskClient, NetworkClient, RuleRunner, SensorClient};
use tauri::ipc::Channel;

/// Shortest window over which CPU percentages are still meaningful.
const MIN_SAMPLE_GAP: Duration = Duration::from_millis(250);
/// Cadence while the window is minimized or in the tray until the settings
/// say otherwise: history and the process rules keep going, and the first
/// frame after showing is fresh, at a fraction of the cost.
pub const BACKGROUND_INTERVAL: Duration = Duration::from_secs(5);
/// Cadence while the view is paused, for history and the rules.
const PAUSED_INTERVAL: Duration = Duration::from_secs(5);
pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(1000);

/// What the window is doing, as far as the cadence goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    Shown,
    Minimized,
    /// Hidden in the tray, or not shown yet (started with --hidden).
    Hidden,
}

type Probe = Box<dyn Fn() -> WindowState + Send>;
type Observer = Box<dyn Fn(&Snapshot) + Send>;

pub enum Control {
    Subscribe(Channel<Snapshot>),
    SetInterval(Duration),
    /// How often to sample while minimized and while in the tray.
    SetBackgroundIntervals {
        minimized: Duration,
        tray: Duration,
    },
    SetKernelSensors(bool),
    SetPaused(bool),
    /// Take a sample now (e.g. right after ending a process, or on show).
    Refresh,
    /// Tells what the window is currently doing.
    SetWindowProbe(Probe),
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

/// How often to sample: the chosen interval while shown, the background ones
/// while minimized or in the tray, and no faster than every 5 s while paused.
fn cadence(
    state: WindowState,
    paused: bool,
    interval: Duration,
    minimized: Duration,
    tray: Duration,
) -> Duration {
    match state {
        WindowState::Hidden => tray,
        WindowState::Minimized => minimized,
        WindowState::Shown if paused => interval.max(PAUSED_INTERVAL),
        WindowState::Shown => interval,
    }
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
                let mut disk = DiskClient::spawn();
                let mut sensors = SensorClient::spawn();
                let mut kernel_sensors = false;
                let mut subscriber: Option<Channel<Snapshot>> = None;
                let mut window: Option<Probe> = None;
                let mut observer: Option<Observer> = None;
                let mut history: Option<HistoryRecorder> = None;
                let mut interval = DEFAULT_INTERVAL;
                let mut minimized = BACKGROUND_INTERVAL;
                let mut tray = BACKGROUND_INTERVAL;
                let mut paused = false;
                let mut next_tick = Instant::now() + interval;
                let mut last_sample = Instant::now();

                loop {
                    let wait = next_tick.saturating_duration_since(Instant::now());
                    let received = rx.recv_timeout(wait);
                    let state = window.as_ref().map_or(WindowState::Shown, |probe| probe());
                    let forced = match received {
                        Ok(Control::SetKernelSensors(enabled)) => {
                            kernel_sensors = enabled;
                            true
                        }
                        Ok(Control::Subscribe(ch)) => {
                            subscriber = Some(ch);
                            true
                        }
                        // A changed cadence counts from the last sample.
                        Ok(Control::SetInterval(d)) => {
                            interval = d;
                            next_tick =
                                last_sample + cadence(state, paused, interval, minimized, tray);
                            continue;
                        }
                        Ok(Control::SetBackgroundIntervals {
                            minimized: m,
                            tray: t,
                        }) => {
                            (minimized, tray) = (m, t);
                            next_tick =
                                last_sample + cadence(state, paused, interval, minimized, tray);
                            continue;
                        }
                        Ok(Control::SetPaused(p)) => {
                            paused = p;
                            next_tick =
                                last_sample + cadence(state, paused, interval, minimized, tray);
                            continue;
                        }
                        Ok(Control::Refresh) => true,
                        Ok(Control::SetWindowProbe(probe)) => {
                            window = Some(probe);
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
                        Err(RecvTimeoutError::Timeout) => false,
                        Err(RecvTimeoutError::Disconnected) => return,
                    };

                    // The cadence of what the window is doing now. Timed samples
                    // stay on its grid (a full period from now after falling
                    // behind); skipping ticks of the shown cadence instead made
                    // a 5 s background cadence 6 s, or 10 s at a 5 s interval.
                    let period = cadence(state, paused, interval, minimized, tray);
                    if !forced {
                        next_tick += period;
                        if next_tick < Instant::now() {
                            next_tick = Instant::now() + period;
                        }
                    }
                    if subscriber.is_none() && observer.is_none() {
                        continue;
                    }
                    if forced && last_sample.elapsed() < MIN_SAMPLE_GAP {
                        continue;
                    }

                    last_sample = Instant::now();
                    if forced {
                        // A sample on demand (the window coming back, say) pulls in
                        // a tick still on the slower background grid.
                        next_tick = next_tick.min(last_sample + period);
                    }
                    let mut snapshot = collector.sample();
                    sensors.enrich(&mut snapshot, kernel_sensors, period);
                    network.enrich(&mut snapshot, period);
                    disk.enrich(&mut snapshot, period);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_window_state_samples_at_its_own_cadence() {
        let s = Duration::from_secs;
        let ms = Duration::from_millis;
        let pick = |state, paused| cadence(state, paused, ms(500), s(10), s(30));
        assert_eq!(pick(WindowState::Shown, false), ms(500));
        assert_eq!(pick(WindowState::Minimized, false), s(10));
        assert_eq!(pick(WindowState::Hidden, false), s(30));
        // Paused: history and rules every 5 s, or the interval if longer.
        assert_eq!(pick(WindowState::Shown, true), s(5));
        assert_eq!(cadence(WindowState::Shown, true, s(10), s(5), s(5)), s(10));
        // Out of sight, the background cadences apply even while paused.
        assert_eq!(pick(WindowState::Minimized, true), s(10));
    }
}
