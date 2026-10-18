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

/// When the sampling thread samples: the next timed tick, and the last sample.
struct Schedule {
    next_tick: Instant,
    last_sample: Instant,
    /// The cadence when the last sample was taken.
    last_period: Duration,
}

impl Schedule {
    fn new(now: Instant, period: Duration) -> Self {
        Self {
            next_tick: now + period,
            last_sample: now,
            last_period: period,
        }
    }

    fn wait(&self, now: Instant) -> Duration {
        self.next_tick.saturating_duration_since(now)
    }

    /// A changed cadence counts from the last sample.
    fn restart(&mut self, period: Duration) {
        self.next_tick = self.last_sample + period;
    }

    /// A timed tick. The next one stays on the grid of `period`, the cadence
    /// of what the window is doing now (a full period from now after falling
    /// behind); skipping ticks of the shown cadence instead made a 5 s
    /// background cadence 6 s, or 10 s at a 5 s interval.
    fn tick(&mut self, now: Instant, period: Duration) {
        self.next_tick += period;
        if self.next_tick < now {
            self.next_tick = now + period;
        }
    }

    /// Whether a sample on demand is taken now: not this soon after the last
    /// one. That one stays on screen then, but a tick still on a slower
    /// background grid comes in to `period` all the same, so a window back
    /// right after a background tick does not wait out a background cadence.
    fn demand(&mut self, now: Instant, period: Duration) -> bool {
        if now.saturating_duration_since(self.last_sample) >= MIN_SAMPLE_GAP {
            return true;
        }
        self.next_tick = self.next_tick.min(self.last_sample + period);
        false
    }

    /// Records a sample taken now at the cadence `period`. Returns how old the
    /// results the service clients requested at the last sample may be by
    /// now: up to that sample's cadence, which on coming back is the slower
    /// background one. Judged by the shown cadence alone, the first frame
    /// back dropped them (no network column, no disk readings).
    fn taken(&mut self, now: Instant, forced: bool, period: Duration) -> Duration {
        let stale_after = period.max(self.last_period);
        self.last_sample = now;
        self.last_period = period;
        if forced {
            // A sample on demand (the window coming back, say) pulls in a tick
            // still on the slower background grid.
            self.next_tick = self.next_tick.min(now + period);
        }
        stale_after
    }
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
                let mut schedule = Schedule::new(Instant::now(), interval);

                loop {
                    let received = rx.recv_timeout(schedule.wait(Instant::now()));
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
                        Ok(Control::SetInterval(d)) => {
                            interval = d;
                            schedule.restart(cadence(state, paused, interval, minimized, tray));
                            continue;
                        }
                        Ok(Control::SetBackgroundIntervals {
                            minimized: m,
                            tray: t,
                        }) => {
                            (minimized, tray) = (m, t);
                            schedule.restart(cadence(state, paused, interval, minimized, tray));
                            continue;
                        }
                        Ok(Control::SetPaused(p)) => {
                            paused = p;
                            schedule.restart(cadence(state, paused, interval, minimized, tray));
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

                    // The cadence of what the window is doing now.
                    let period = cadence(state, paused, interval, minimized, tray);
                    let now = Instant::now();
                    if !forced {
                        schedule.tick(now, period);
                    }
                    if subscriber.is_none() && observer.is_none() {
                        continue;
                    }
                    if forced && !schedule.demand(now, period) {
                        continue;
                    }

                    let stale_after = schedule.taken(now, forced, period);
                    let mut snapshot = collector.sample();
                    sensors.enrich(&mut snapshot, kernel_sensors, stale_after);
                    network.enrich(&mut snapshot, stale_after);
                    disk.enrich(&mut snapshot, stale_after);
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

    #[test]
    fn coming_back_right_after_a_background_tick_moves_onto_the_shown_cadence() {
        let (s, ms) = (Duration::from_secs, Duration::from_millis);
        let t0 = Instant::now();
        let mut schedule = Schedule::new(t0, s(1));
        // Minimized: a tick at 1 s puts the next one 10 s later.
        schedule.tick(t0 + s(1), s(10));
        schedule.taken(t0 + s(1), false, s(10));
        assert_eq!(schedule.next_tick, t0 + s(11));
        // Restored 100 ms later: too soon for a sample of its own, but the
        // next one comes a shown interval after the last, not 10 s.
        assert!(!schedule.demand(t0 + s(1) + ms(100), s(1)));
        assert_eq!(schedule.next_tick, t0 + s(2));
        // Restored later: sampled at once, and on the shown grid from there.
        let mut schedule = Schedule::new(t0, s(1));
        schedule.tick(t0 + s(1), s(10));
        schedule.taken(t0 + s(1), false, s(10));
        assert!(schedule.demand(t0 + s(4), s(1)));
        schedule.taken(t0 + s(4), true, s(1));
        assert_eq!(schedule.next_tick, t0 + s(5));
    }

    #[test]
    fn the_first_sample_back_takes_what_the_last_background_one_requested() {
        let s = Duration::from_secs;
        let t0 = Instant::now();
        let mut schedule = Schedule::new(t0, s(1));
        // In the tray at 10 s.
        schedule.tick(t0 + s(1), s(10));
        assert_eq!(schedule.taken(t0 + s(1), false, s(10)), s(10));
        // Shown 6 s later: the service results requested at that tick are
        // 6 s old, more than two shown intervals plus a client's 2 s timeout,
        // but within the background cadence they were requested at.
        assert!(schedule.demand(t0 + s(7), s(1)));
        assert_eq!(schedule.taken(t0 + s(7), true, s(1)), s(10));
        // From then on, the shown cadence alone.
        schedule.tick(t0 + s(8), s(1));
        assert_eq!(schedule.taken(t0 + s(8), false, s(1)), s(1));
        // A longer interval chosen meanwhile counts from the sample before.
        schedule.restart(s(5));
        assert_eq!(schedule.next_tick, t0 + s(13));
        assert_eq!(schedule.taken(t0 + s(13), false, s(5)), s(5));
    }
}
