//! The sampling thread: produces snapshots on a fixed cadence and streams them
//! to renderer subscribers through a local channel.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::channel::Channel;
use argus_core::history::{self, HistoryRecorder};
use argus_core::power::PowerTracker;
use argus_core::watch::{SeriesQuery, WatchAction, WatchError, WatchRecorder, WatchView};
use argus_core::{Collector, Snapshot};
use argus_service::{DiskClient, NetworkClient, RuleRunner, SensorClient};

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

/// Publish an action's refresh once a native process query that started after
/// it is accepted. Cached rows and late pre-action replies
/// must not consume it, even when their enclosing snapshot is newly built.
fn publish_refresh(pending: &mut Option<Instant>, observed: Option<Instant>) -> bool {
    let Some(requested) = *pending else {
        return false;
    };
    let ready = observed.is_some_and(|started| started >= requested);
    if ready {
        *pending = None;
    }
    ready
}

pub enum Control {
    Shutdown(Sender<()>),
    Subscribe(Channel<Snapshot>),
    SetInterval(Duration),
    /// How often to sample while minimized and while in the tray.
    SetBackgroundIntervals {
        minimized: Duration,
        tray: Duration,
    },
    SetKernelSensors(bool),
    /// Which process rows carry the counters only optional columns and the
    /// details pane show. With `fill`, rows that join are sampled at once
    /// rather than after an interval.
    SetExtraCounters {
        rows: argus_core::ExtraCounters,
        fill: bool,
    },
    SetPaused(bool),
    /// Take a sample now (e.g. right after ending a process, or on show).
    Refresh,
    /// Tells what the window is currently doing.
    SetWindowProbe(Probe),
    /// Starts recording history into this folder, or stops with None.
    SetHistoryDir(Option<PathBuf>),
    SetHistoryOptions(PathBuf, history::HistoryOptions),
    /// Close both writers and acknowledge after maintenance has quiesced.
    PauseHistory(Sender<()>),
    ResumeWatch {
        dir: PathBuf,
        keep_observation: bool,
    },
    /// Deletes the history in this folder; replies once the files are gone.
    /// Goes through this thread so no frame is being written meanwhile.
    ClearHistory(PathBuf, Sender<io::Result<()>>),
    Watch(
        PathBuf,
        WatchAction,
        Sender<Result<Option<WatchView>, WatchError>>,
    ),
    /// One observed run's readings over a window, for the observation page.
    WatchSeries(
        PathBuf,
        SeriesQuery,
        Sender<Result<history::HistorySeries, WatchError>>,
    ),
}

fn maintenance_due(previous: Option<(i64, Instant)>, day: i64, now: Instant) -> bool {
    previous.is_none_or(|(old, time)| {
        old != day || now.saturating_duration_since(time) >= Duration::from_secs(3600)
    })
}
enum MaintenanceJob {
    Run(PathBuf, history::HistoryOptions, i64, i64, u64),
    Barrier(Sender<()>),
}
struct Maintenance {
    tx: Sender<MaintenanceJob>,
    dir: Option<PathBuf>,
    last: Option<(i64, Instant)>,
    generation: Arc<std::sync::atomic::AtomicU64>,
}
impl Maintenance {
    fn start() -> Self {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        let (tx, rx) = mpsc::channel();
        let generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let current = generation.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("argus-history-maintenance".into())
            .spawn(move || {
                if let Err(e) =
                    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) }
                {
                    argus_core::diagnostics::warn(
                        "history worker priority",
                        format!("HRESULT={:#x}", e.code().0),
                    );
                }
                while let Ok(job) = rx.recv() {
                    match job {
                        MaintenanceJob::Run(path, options, today, active, version) => {
                            let cancelled =
                                || current.load(std::sync::atomic::Ordering::Acquire) != version;
                            if !cancelled()
                                && let Err(e) =
                                    history::maintain_at(&path, options, today, active, cancelled)
                            {
                                argus_core::diagnostics::file_failure(
                                    "history maintenance",
                                    &path,
                                    &e,
                                );
                            }
                        }
                        MaintenanceJob::Barrier(reply) => {
                            let _ = reply.send(());
                        }
                    }
                }
            })
        {
            argus_core::diagnostics::warn("history worker", argus_core::diagnostics::io(&e));
        }
        Self {
            tx,
            generation,
            dir: None,
            last: None,
        }
    }
    fn tick(
        &mut self,
        options: history::HistoryOptions,
        ts: i64,
        now: Instant,
        recorder: Option<&HistoryRecorder>,
    ) {
        let today = ts.div_euclid(86_400_000);
        let Some(dir) = &self.dir else { return };
        if !maintenance_due(self.last, today, now) {
            return;
        }
        self.last = Some((today, now));
        let active = recorder
            .and_then(HistoryRecorder::active_day)
            .unwrap_or(i64::MIN);
        let generation = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            .wrapping_add(1);
        let _ = self.tx.send(MaintenanceJob::Run(
            dir.clone(),
            options,
            today,
            active,
            generation,
        ));
    }
    fn barrier(&self, reply: Sender<()>) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if self
            .tx
            .send(MaintenanceJob::Barrier(reply.clone()))
            .is_err()
        {
            let _ = reply.send(());
        }
    }
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
    /// No snapshot has been taken since the collector established its baselines.
    first_sample: bool,
}

impl Schedule {
    fn new(now: Instant, period: Duration) -> Self {
        Self {
            next_tick: now + MIN_SAMPLE_GAP,
            last_sample: now,
            last_period: period,
            first_sample: true,
        }
    }

    fn wait(&self, now: Instant) -> Duration {
        self.next_tick.saturating_duration_since(now)
    }

    /// A changed cadence counts from the last sample.
    fn restart(&mut self, period: Duration) {
        // Startup controls may choose a long foreground or tray interval.
        // Keep the first snapshot due once the rate counters have a baseline.
        let wait = if self.first_sample {
            MIN_SAMPLE_GAP
        } else {
            period
        };
        self.next_tick = self.last_sample + wait;
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

    /// A sample on demand came too soon and is owed: the next one comes once
    /// the minimum gap is over, not a (paused, say) cadence later.
    fn owe(&mut self) {
        self.next_tick = self.next_tick.min(self.last_sample + MIN_SAMPLE_GAP);
    }

    /// Records a sample taken now at the cadence `period`. Returns how old the
    /// results the service clients requested at the last sample may be by
    /// now: up to that sample's cadence, which on coming back is the slower
    /// background one. Judged by the shown cadence alone, the first frame
    /// back dropped them (no network column, no disk readings).
    fn taken(&mut self, now: Instant, forced: bool, period: Duration) -> Duration {
        let stale_after = period.max(self.last_period);
        if self.first_sample {
            self.next_tick = now + period;
            self.first_sample = false;
        }
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
    /// Fixtures supply snapshots in the frontend and need no native collectors
    /// or privileged-service clients. Controls on this handle are discarded.
    pub fn inactive() -> Self {
        let (tx, _rx) = mpsc::channel();
        Self { tx }
    }

    /// Starts the sampling thread. The collector is built there, so its setup
    /// (PDH queries, adapter enumeration, a priming sample) overlaps the
    /// native window startup instead of delaying it.
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
                let mut power = PowerTracker::default();
                let mut kernel_sensors = false;
                let mut extra_counters = argus_core::ExtraCounters::Off;
                let mut subscriber: Option<Channel<Snapshot>> = None;
                let mut window: Option<Probe> = None;
                let mut history: Option<HistoryRecorder> = None;
                let mut watch: Option<WatchRecorder> = None;
                let mut suspended_watch = None;
                let mut maintenance = Maintenance::start();
                let mut history_options = history::HistoryOptions::default();
                let mut interval = DEFAULT_INTERVAL;
                let mut minimized = BACKGROUND_INTERVAL;
                let mut tray = BACKGROUND_INTERVAL;
                let mut paused = false;
                let mut schedule = Schedule::new(Instant::now(), interval);
                // Both immediately sampled and deferred actions need an
                // accepted post-action process query before their refresh ends.
                let mut owed = None;

                loop {
                    let received = rx.recv_timeout(schedule.wait(Instant::now()));
                    let state = window.as_ref().map_or(WindowState::Shown, |probe| probe());
                    let forced = match received {
                        Ok(Control::SetKernelSensors(enabled)) => {
                            kernel_sensors = enabled;
                            if !enabled {
                                sensors.disable();
                            }
                            true
                        }
                        Ok(Control::SetExtraCounters { rows, fill }) => {
                            let added = fill && rows.adds_to(&extra_counters);
                            collector.set_extra_counters(rows.clone());
                            extra_counters = rows;
                            if !added {
                                continue;
                            }
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
                        Ok(Control::Shutdown(reply)) => {
                            drop(history);
                            let _ = reply.send(());
                            return;
                        }
                        Ok(Control::Refresh) => true,
                        Ok(Control::SetWindowProbe(probe)) => {
                            window = Some(probe);
                            continue;
                        }
                        Ok(Control::SetHistoryOptions(dir, options)) => {
                            history_options = options;
                            if let Some(rec) = &mut history {
                                rec.configure(options.recording);
                            }
                            maintenance.dir = Some(dir);
                            maintenance.last = None;
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as i64;
                            maintenance.tick(options, ts, Instant::now(), history.as_ref());
                            continue;
                        }
                        Ok(Control::PauseHistory(reply)) => {
                            drop(history.take());
                            suspended_watch = watch.take().map(WatchRecorder::suspend);
                            maintenance.dir = None;
                            maintenance.barrier(reply);
                            continue;
                        }
                        Ok(Control::ResumeWatch {
                            dir,
                            keep_observation,
                        }) => {
                            if let Some(resume) =
                                suspended_watch.take().filter(|_| keep_observation)
                            {
                                watch = resume(dir)
                                    .inspect_err(|_| {
                                        argus_core::diagnostics::warn(
                                            "history observation resume",
                                            "storage unavailable",
                                        )
                                    })
                                    .ok();
                            }
                            continue;
                        }
                        Ok(Control::SetHistoryDir(dir)) => {
                            if history.as_ref().map(HistoryRecorder::directory) != dir.as_deref() {
                                if let Some(mut recorder) = history.take() {
                                    recorder.stop(dir.is_none());
                                }
                                history = dir.map(|p| {
                                    let mut rec = HistoryRecorder::start(p);
                                    rec.configure(history_options.recording);
                                    rec
                                });
                            }
                            continue;
                        }
                        Ok(Control::Watch(dir, action, reply)) => {
                            let result = if let Some(recorder) = &mut watch {
                                recorder.action(action)
                            } else if matches!(action, WatchAction::Delete) {
                                WatchRecorder::delete_saved(&dir).map(|()| None)
                            } else {
                                WatchRecorder::open(dir).and_then(|mut recorder| {
                                    let result = recorder.action(action);
                                    watch = Some(recorder);
                                    result
                                })
                            };
                            let _ = reply.send(result);
                            continue;
                        }
                        Ok(Control::WatchSeries(dir, query, reply)) => {
                            // A saved observation is read as `Watch` reads it,
                            // unless it waits to resume after a folder move.
                            if watch.is_none() && suspended_watch.is_none() {
                                watch = WatchRecorder::open(dir).ok();
                            }
                            let result =
                                watch.as_ref().map_or(Err(WatchError::Missing), |recorder| {
                                    recorder.series(&query)
                                });
                            let _ = reply.send(result);
                            continue;
                        }
                        Ok(Control::ClearHistory(dir, reply)) => {
                            let observed = match &mut watch {
                                Some(recorder) => recorder.action(WatchAction::Delete).map(|_| ()),
                                None => WatchRecorder::delete_saved(&dir),
                            };
                            let result = observed
                                .map_err(|_| io::Error::other("process observation cleanup failed"))
                                .and_then(|()| match &mut history {
                                    Some(recorder) => recorder.clear(),
                                    None => history::clear(&dir),
                                });
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
                    if subscriber.is_none() {
                        continue;
                    }
                    if forced {
                        owed = Some(now);
                    }
                    if forced && !schedule.demand(now, period) {
                        schedule.owe();
                        continue;
                    }

                    let stale_after = schedule.taken(now, forced, period);
                    let mut snapshot = collector.sample();
                    sensors.enrich(&mut snapshot, kernel_sensors, period, stale_after);
                    network.enrich(&mut snapshot, stale_after);
                    disk.enrich(&mut snapshot, stale_after);
                    if let Some(failure) = disk.new_failure() {
                        log::warn!("per-process disk I/O unavailable: {failure}");
                    }
                    let logical_cpus = snapshot.system.logical_cpus;
                    power.apply(&mut snapshot.processes, logical_cpus, Instant::now());
                    rules.observe(&snapshot.processes);
                    if let Some(recorder) = &mut history {
                        recorder.record(&snapshot);
                    }
                    maintenance.tick(history_options, snapshot.timestamp, now, history.as_ref());
                    if let Some(recorder) = &mut watch
                        && recorder.record_at_interval(&snapshot, stale_after).is_err()
                    {
                        argus_core::diagnostics::warn(
                            "process observation",
                            "recording interrupted",
                        );
                    }
                    // A paused view stays as it is, except right after an action.
                    let send =
                        publish_refresh(&mut owed, collector.process_sample_started()) || !paused;
                    if let Some(ch) = &subscriber
                        && send
                        && ch.send(snapshot).is_err()
                    {
                        subscriber = None;
                    }
                }
            })
            .expect("spawn sampler thread");
        Self { tx }
    }

    pub fn shutdown(&self) {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Control::Shutdown(tx)).is_ok() {
            // A storage driver can keep the worker inside a query indefinitely.
            let _ = rx.recv_timeout(Duration::from_secs(2));
        }
    }

    pub fn send(&self, control: Control) {
        let _ = self.tx.send(control);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_returns_while_a_worker_still_holds_the_request() {
        let (tx, rx) = mpsc::channel();
        let (done, result) = mpsc::channel();
        std::thread::spawn(move || {
            Sampler { tx }.shutdown();
            let _ = done.send(());
        });
        let Control::Shutdown(reply) = rx.recv_timeout(Duration::from_secs(2)).unwrap() else {
            panic!("unexpected sampler control");
        };
        assert_eq!(
            result.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        );
        result.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(reply.send(()).is_err());
    }

    #[test]
    fn paused_action_refresh_survives_cached_rows_and_a_late_pre_action_reply() {
        let before = Instant::now();
        let requested = before + Duration::from_millis(250);
        let mut pending = Some(requested);
        // No result, a reused snapshot, and a newly delivered query that began
        // before the action all leave the paused view waiting for the new state.
        for observed in [
            None,
            Some(before),
            Some(before),
            Some(requested - Duration::from_millis(1)),
        ] {
            assert!(!publish_refresh(&mut pending, observed));
            assert_eq!(pending, Some(requested));
        }
        assert!(publish_refresh(&mut pending, Some(requested)));
        assert_eq!(pending, None);
        assert!(!publish_refresh(
            &mut pending,
            Some(requested + Duration::from_secs(5))
        ));
    }

    #[test]
    fn history_maintenance_is_due_at_start_rollover_and_hourly() {
        let start = Instant::now();
        let previous = Some((100, start));
        assert!(maintenance_due(None, 100, start));
        assert!(!maintenance_due(
            previous,
            100,
            start + Duration::from_secs(3599)
        ));
        assert!(maintenance_due(
            previous,
            100,
            start + Duration::from_secs(3600)
        ));
        assert!(maintenance_due(previous, 101, start));
    }

    #[test]
    fn disabled_recorder_expires_history_at_midnight() {
        let dir =
            std::env::temp_dir().join(format!("argus-disabled-retention-{}", std::process::id()));
        history::prepare_directory(&dir, None).unwrap();
        let past = dir.join("1970-01-02.hist");
        std::fs::write(&past, b"old").unwrap();
        let mut m = Maintenance::start();
        m.dir = Some(dir.clone());
        let mut options = history::HistoryOptions::default();
        options.limit.days = Some(7);
        let start = Instant::now();
        for (ts, exists) in [(8 * 86_400_000 - 1, true), (8 * 86_400_000, false)] {
            m.tick(options, ts, start, None);
            let (tx, rx) = mpsc::channel();
            m.tx.send(MaintenanceJob::Barrier(tx)).unwrap();
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(past.exists(), exists);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn another_action_requires_an_observation_after_the_latest_request() {
        let first = Instant::now();
        let second = first + Duration::from_secs(1);
        let mut pending = Some(second);
        assert!(!publish_refresh(&mut pending, Some(first)));
        assert_eq!(pending, Some(second));
        assert!(publish_refresh(&mut pending, Some(second)));
        assert_eq!(pending, None);
    }

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
    fn every_cadence_keeps_the_sensor_service_lease() {
        // The service keeps counter baselines (CPU power, effective clocks)
        // across requests up to this far apart; a longer cadence would lose
        // them in the tray or minimized and on the first frames back.
        let longest = crate::settings::BACKGROUND_INTERVALS
            .iter()
            .map(|ms| Duration::from_millis(*ms))
            .chain([PAUSED_INTERVAL, BACKGROUND_INTERVAL, DEFAULT_INTERVAL])
            .max()
            .unwrap();
        assert!(longest <= argus_service::MAX_SENSOR_POLL);
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
    fn a_refresh_too_soon_after_a_paused_sample_comes_once_the_gap_is_over() {
        let (s, ms) = (Duration::from_secs, Duration::from_millis);
        let t0 = Instant::now();
        // Paused: samples every 5 s, the last one at 5 s.
        let mut schedule = Schedule::new(t0, s(5));
        schedule.tick(t0 + s(5), s(5));
        schedule.taken(t0 + s(5), false, s(5));
        assert_eq!(schedule.next_tick, t0 + s(10));
        // An action asks for a refresh 100 ms later: too soon, so it is owed
        // and comes at the end of the gap rather than 5 s on.
        assert!(!schedule.demand(t0 + s(5) + ms(100), s(5)));
        schedule.owe();
        assert_eq!(schedule.next_tick, t0 + s(5) + MIN_SAMPLE_GAP);
        assert_eq!(schedule.wait(t0 + s(5) + ms(100)), MIN_SAMPLE_GAP - ms(100));
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

    #[test]
    fn startup_controls_keep_the_first_snapshot_on_the_minimum_gap() {
        let t0 = Instant::now();
        let (s, ms) = (Duration::from_secs, Duration::from_millis);
        for state in [
            WindowState::Shown,
            WindowState::Minimized,
            WindowState::Hidden,
        ] {
            let mut schedule = Schedule::new(t0, s(1));
            assert_eq!(schedule.wait(t0), MIN_SAMPLE_GAP);
            // Subscribe, saved interval, background intervals and pause may all
            // arrive before the first snapshot; none may defer it to 1–30 s.
            assert!(!schedule.demand(t0 + ms(10), s(1)));
            schedule.owe();
            schedule.restart(cadence(state, false, s(10), s(30), s(30)));
            schedule.restart(cadence(state, true, s(10), s(30), s(30)));
            assert_eq!(schedule.wait(t0 + ms(10)), MIN_SAMPLE_GAP - ms(10));
            assert!(!schedule.demand(t0 + MIN_SAMPLE_GAP - ms(1), s(30)));
            assert!(schedule.demand(t0 + MIN_SAMPLE_GAP, s(30)));
        }
    }

    #[test]
    fn the_first_snapshot_starts_the_selected_regular_cadence() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        for period in [MIN_SAMPLE_GAP, s(1), s(5), s(30)] {
            for forced in [false, true] {
                let mut schedule = Schedule::new(t0, s(1));
                schedule.restart(period);
                let first = t0 + MIN_SAMPLE_GAP;
                if !forced {
                    schedule.tick(first, period);
                }
                schedule.taken(first, forced, period);
                assert_eq!(schedule.next_tick, first + period);
                // Once a snapshot exists, normal interval changes and refresh
                // coalescing still count from that snapshot.
                schedule.restart(s(10));
                assert_eq!(schedule.next_tick, first + s(10));
                assert!(!schedule.demand(first, s(1)));
            }
        }
    }
}
