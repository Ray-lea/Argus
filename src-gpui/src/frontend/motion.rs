//! Source timestamps determine the scroll clock; native frames only choose when to paint it.
use std::time::{Duration, Instant};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Fluid,
    Balanced,
    Pixel,
    Off,
}
impl Mode {
    pub fn parse(value: &str) -> Self {
        match value {
            "balanced" => Self::Balanced,
            "pixel" => Self::Pixel,
            "off" => Self::Off,
            _ => Self::Fluid,
        }
    }
}
pub(super) struct Clock {
    source: f64,
    anchor: Instant,
    latest: f64,
    interval: f64,
    pause: Option<f64>,
    pub mode: Mode,
}
impl Clock {
    pub fn new(mode: Mode) -> Self {
        Self {
            source: 0.,
            anchor: Instant::now(),
            latest: 0.,
            interval: 1000.,
            pause: None,
            mode,
        }
    }
    pub fn accept(&mut self, source: f64, interval: f64, now: Instant, wall_now: f64) {
        self.source = wall_now;
        self.anchor = now;
        self.latest = source;
        self.interval = interval;
    }
    pub fn end(&self, now: Instant) -> f64 {
        if let Some(at) = self.pause {
            return at;
        }
        if self.mode == Mode::Off {
            return self.latest;
        }
        let lag = self.interval * config()["chartLagIntervals"].as_f64().expect("chart lag")
            + config()["chartLagSlackMs"]
                .as_f64()
                .expect("chart lag slack");
        (self.source + now.saturating_duration_since(self.anchor).as_secs_f64() * 1000.)
            .min(self.latest + lag)
            - lag
    }
    pub fn set_paused(&mut self, paused: bool) {
        if paused && self.pause.is_none() {
            self.pause = Some(self.end(Instant::now()));
        } else if !paused {
            self.pause = None;
        }
    }
    pub fn pause_at(&mut self, time: f64) {
        self.pause = Some(time);
    }
    pub fn moving(&self) -> bool {
        self.mode != Mode::Off
            && self.pause.is_none()
            && self.latest > 0.
            && self.end(Instant::now()) < self.latest
    }
}
pub(super) fn config() -> &'static serde_json::Value {
    static CONFIG: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    CONFIG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../resources/motion.json"))
            .expect("motion configuration")
    })
}
pub(super) fn frame_period(mode: Mode, refresh: Option<Duration>) -> Duration {
    let refresh = refresh.unwrap_or_else(|| {
        Duration::from_secs_f64(
            1. / config()["fallbackRefreshHz"]
                .as_f64()
                .expect("fallback refresh"),
        )
    });
    if mode == Mode::Balanced {
        refresh.mul_f64(
            (1. / refresh.as_secs_f64()
                / config()["balancedTargetHz"]
                    .as_f64()
                    .expect("balanced frame rate"))
            .floor()
            .max(1.),
        )
    } else {
        refresh
    }
}
#[cfg(test)]
mod tests {
    use super::{Clock, Mode};
    use std::time::{Duration, Instant};
    #[test]
    fn receipt_bursts_do_not_compress_capture_time() {
        let now = Instant::now();
        let mut clock = Clock::new(Mode::Fluid);
        clock.accept(10000., 1000., now, 10000.);
        clock.accept(11000., 1000., now + Duration::from_millis(2200), 12200.);
        let before = clock.end(now + Duration::from_millis(2200));
        clock.accept(12000., 1000., now + Duration::from_millis(2201), 12201.);
        assert!((clock.end(now + Duration::from_millis(2201)) - before - 1.).abs() < 0.01);
    }
    #[test]
    fn stale_sources_stop_and_clock_changes_rebase() {
        let now = Instant::now();
        let mut clock = Clock::new(Mode::Fluid);
        clock.accept(10000., 1000., now, 10000.);
        assert_eq!(clock.end(now + Duration::from_secs(30)), 10000.);
        clock.accept(500., 1000., now + Duration::from_secs(31), 500.);
        assert_eq!(
            clock.end(now + Duration::from_secs(31)),
            500. - super::config()["chartLagIntervals"].as_f64().unwrap() * 1000.
                - super::config()["chartLagSlackMs"].as_f64().unwrap()
        );
    }
}
