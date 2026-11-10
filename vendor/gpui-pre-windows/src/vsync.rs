use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use gpui::FrameRequestSource;
use gpui_util::ResultExt;
use windows::Win32::{
    Foundation::{HANDLE, HWND},
    Graphics::Dwm::{DWM_TIMING_INFO, DwmFlush, DwmGetCompositionTimingInfo},
    System::{
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        Performance::QueryPerformanceFrequency,
    },
};
use windows::core::{s, w};

type CompositorClock = unsafe extern "system" fn(u32, *const HANDLE, u32) -> u32;

fn compositor_clock() -> Option<CompositorClock> {
    static CLOCK: LazyLock<Option<CompositorClock>> = LazyLock::new(|| unsafe {
        // The renderer already links dcomp.dll. Resolve the Windows 11 export
        // dynamically so the executable still loads on Windows 10.
        let module = GetModuleHandleW(w!("dcomp.dll")).ok()?;
        let entry = GetProcAddress(module, s!("DCompositionWaitForCompositorClock"))?;
        Some(std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            CompositorClock,
        >(entry))
    });
    *CLOCK
}

pub(crate) fn compositor_clock_available() -> bool {
    compositor_clock().is_some()
}

static QPC_TICKS_PER_SECOND: LazyLock<u64> = LazyLock::new(|| {
    let mut frequency = 0;
    // On systems that run Windows XP or later, the function will always succeed and
    // will thus never return zero.
    unsafe { QueryPerformanceFrequency(&mut frequency).unwrap() };
    frequency as u64
});

const VSYNC_INTERVAL_THRESHOLD: Duration = Duration::from_millis(1);
const DEFAULT_VSYNC_INTERVAL: Duration = Duration::from_micros(16_666); // ~60Hz

pub(crate) struct VSyncProvider {
    interval: Duration,
    f: Box<dyn Fn() -> bool>,
    compositor_clock: bool,
}

impl VSyncProvider {
    pub(crate) fn new() -> Self {
        let interval = get_dwm_interval()
            .context("Failed to get DWM interval")
            .log_err()
            .unwrap_or(DEFAULT_VSYNC_INTERVAL);
        let clock = std::env::var_os("ARGUS_GPUI_USE_DWM_FLUSH")
            .is_none()
            .then(compositor_clock)
            .flatten();
        let f: Box<dyn Fn() -> bool> = if let Some(wait) = clock {
            Box::new(move || unsafe { wait(0, std::ptr::null(), 1000) == 0 })
        } else {
            Box::new(|| unsafe { DwmFlush().is_ok() })
        };
        if std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some() {
            println!(
                "{{\"event\":\"gpu-frame-clock\",\"source\":\"{}\"}}",
                if clock.is_some() {
                    "compositor"
                } else {
                    "dwm-flush"
                }
            );
        }
        Self {
            interval,
            f,
            compositor_clock: clock.is_some(),
        }
    }

    pub(crate) fn wait_for_vsync(&self) -> FrameRequestSource {
        let vsync_start = Instant::now();
        let wait_succeeded = (self.f)();
        let elapsed = vsync_start.elapsed();
        // A successful compositor-clock wait denotes a tick, even when called
        // just before it. Occlusion and failures have distinct status codes.
        // The legacy flush can return early while unavailable, so retain its
        // duration guard and the bounded local cadence for unavailable waits.
        let source =
            if !wait_succeeded || (!self.compositor_clock && elapsed < VSYNC_INTERVAL_THRESHOLD) {
                log::trace!("VSyncProvider is using the local refresh interval");
                std::thread::sleep(self.interval);
                FrameRequestSource::LocalSchedule
            } else {
                FrameRequestSource::NativeCallback
            };
        crate::frame_trace::record(
            0,
            elapsed.as_micros() as u64 * 2
                + u64::from(matches!(source, FrameRequestSource::LocalSchedule)),
        );
        source
    }
}

fn get_dwm_interval() -> Result<Duration> {
    let mut timing_info = DWM_TIMING_INFO {
        cbSize: std::mem::size_of::<DWM_TIMING_INFO>() as u32,
        ..Default::default()
    };
    unsafe { DwmGetCompositionTimingInfo(HWND::default(), &mut timing_info) }?;
    let qpc_period = timing_info.qpcRefreshPeriod;
    let refresh_numerator = timing_info.rateRefresh.uiNumerator;
    let refresh_denominator = timing_info.rateRefresh.uiDenominator;
    let interval = timing_interval(
        qpc_period,
        *QPC_TICKS_PER_SECOND,
        refresh_numerator,
        refresh_denominator,
    );
    if interval.is_none() {
        log::warn!(
            target: "argus::graphics",
            "Invalid DWM timing; qpc_period={}; qpc_frequency={}; refresh_numerator={}; refresh_denominator={}; fallback_us={}",
            qpc_period,
            *QPC_TICKS_PER_SECOND,
            refresh_numerator,
            refresh_denominator,
            DEFAULT_VSYNC_INTERVAL.as_micros(),
        );
    }
    Ok(interval.unwrap_or(DEFAULT_VSYNC_INTERVAL))
}

fn timing_interval(
    qpc_period: u64,
    qpc_frequency: u64,
    refresh_numerator: u32,
    refresh_denominator: u32,
) -> Option<Duration> {
    // Some drivers return a refresh-rate count instead of a QPC interval.
    // Reject implausibly short QPC periods, then use the reciprocal frequency.
    retrieve_duration(qpc_period, qpc_frequency)
        .filter(|interval| *interval >= VSYNC_INTERVAL_THRESHOLD)
        .or_else(|| retrieve_duration(refresh_denominator as u64, refresh_numerator as u64))
}

fn retrieve_duration(counts: u64, ticks_per_second: u64) -> Option<Duration> {
    // Multiply before dividing: ordinary refresh frequencies are below 1 MHz.
    let micros = (u128::from(counts) * 1_000_000).checked_div(u128::from(ticks_per_second))?;
    // Invalid timing must not suspend fallback scheduling for seconds or longer.
    (1..=1_000_000)
        .contains(&micros)
        .then(|| Duration::from_micros(micros as u64))
}

#[cfg(test)]
mod timing_tests {
    use super::{retrieve_duration, timing_interval};
    use std::time::Duration;

    #[test]
    fn normal_qpc_timing_keeps_microsecond_precision() {
        assert_eq!(
            timing_interval(166_666, 10_000_000, 60, 1),
            Some(Duration::from_micros(16_666)),
        );
        assert_eq!(
            retrieve_duration(59_659, 3_579_545),
            Some(Duration::from_micros(16_666)),
        );
    }

    #[test]
    fn unreliable_qpc_uses_integer_and_fractional_refresh_rates() {
        for (numerator, denominator, micros) in
            [(60, 1, 16_666), (60_000, 1_001, 16_683), (144, 1, 6_944)]
        {
            assert_eq!(
                timing_interval(60, 10_000_000, numerator, denominator),
                Some(Duration::from_micros(micros)),
            );
        }
    }

    #[test]
    fn invalid_qpc_can_recover_from_valid_refresh_data() {
        for (period, frequency) in [(0, 10_000_000), (1, 0), (u64::MAX, 1)] {
            assert_eq!(
                timing_interval(period, frequency, 60, 1),
                Some(Duration::from_micros(16_666)),
            );
        }
    }

    #[test]
    fn unusable_timing_selects_the_callers_default() {
        for (numerator, denominator) in [(0, 1), (60, 0), (1, u32::MAX)] {
            assert_eq!(timing_interval(0, 10_000_000, numerator, denominator), None);
        }
        assert_eq!(retrieve_duration(u64::MAX, 1), None);
        assert_eq!(retrieve_duration(1, u64::MAX), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_compositor_wait_reports_native_callback() {
        let provider = VSyncProvider {
            interval: Duration::ZERO,
            compositor_clock: false,
            f: Box::new(|| {
                std::thread::sleep(VSYNC_INTERVAL_THRESHOLD * 2);
                true
            }),
        };

        assert_eq!(
            provider.wait_for_vsync(),
            FrameRequestSource::NativeCallback
        );
    }

    #[test]
    fn failed_compositor_wait_reports_local_schedule_even_after_threshold() {
        let provider = VSyncProvider {
            interval: Duration::ZERO,
            compositor_clock: false,
            f: Box::new(|| {
                std::thread::sleep(VSYNC_INTERVAL_THRESHOLD * 2);
                false
            }),
        };

        assert_eq!(provider.wait_for_vsync(), FrameRequestSource::LocalSchedule);
    }

    #[test]
    fn successful_short_clock_wait_preserves_the_native_tick() {
        let provider = VSyncProvider {
            interval: Duration::ZERO,
            compositor_clock: true,
            f: Box::new(|| true),
        };
        assert_eq!(
            provider.wait_for_vsync(),
            FrameRequestSource::NativeCallback
        );
    }

    #[test]
    fn unavailable_clock_uses_local_schedule() {
        let provider = VSyncProvider {
            interval: Duration::ZERO,
            compositor_clock: true,
            f: Box::new(|| false),
        };
        assert_eq!(provider.wait_for_vsync(), FrameRequestSource::LocalSchedule);
    }
}
