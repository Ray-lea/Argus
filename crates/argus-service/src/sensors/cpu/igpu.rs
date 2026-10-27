//! Read-only Comet Lake GT sensors through the signed IntelMCHBAR module.
//! Intel 615212-003 (Comet Lake), sections 9.8 / 9.9 / 9.30 / 9.31 /
//! 9.32 / 9.36 / 9.38 / 9.41. Frequencies use documented nominal steps.
//! Newer Xe layouts use different ratio bits and must not enter this reader.

use std::time::{Duration, Instant};

use argus_core::hardware::{
    CpuInfo, CpuSensors, IntegratedGpuLimit, IntegratedGpuLimitReason as Reason,
    IntegratedGpuSensors, SensorState, intel_mchbar_supported,
};

use super::{Module, PciGuard};

const BLOB: &[u8] = include_bytes!("../../../vendor/pawnio/IntelMCHBAR.bin");

#[derive(Clone, Copy)]
enum Register {
    GtActivity = 0x5848,
    MediaActivity = 0x5858,
    Limits = 0x5900,
    MemoryClock = 0x5918,
    Performance = 0x5948,
    Temperature = 0x5980,
    Capabilities = 0x5998,
}

#[derive(Default, Debug)]
struct Registers {
    temperature: Option<u32>,
    performance: Option<u32>,
    capabilities: Option<u32>,
    memory: Option<u32>,
    limits: Option<u32>,
}

#[derive(Default)]
struct ActivityCounter(Option<(u64, Instant)>);

impl ActivityCounter {
    fn sample(&mut self, value: Option<u64>, now: Instant) -> Option<f32> {
        let Some(value) = value.filter(|v| *v != u64::MAX) else {
            self.0 = None;
            return None;
        };
        let (before, at) = self.0.replace((value, now))?;
        let seconds = now.checked_duration_since(at)?.as_secs_f64();
        if !(0.05..=30.0).contains(&seconds) {
            return None;
        }
        // Gen9 slice/media activity counts at 24 MHz, independently of GT PLL.
        let percent = value.checked_sub(before)? as f64 / (seconds * 24_000_000.0) * 100.0;
        (percent <= 105.0).then_some(percent.min(100.0) as f32)
    }
}

pub(super) struct Reader {
    supported: bool,
    module: Option<Module>,
    attempt: Option<Instant>,
    state: SensorState,
    gt_activity: ActivityCounter,
    media_activity: ActivityCounter,
    last_diagnostic: Option<Instant>,
}

impl Reader {
    pub fn new(cpu: &CpuInfo) -> Self {
        Self {
            supported: intel_mchbar_supported(cpu),
            module: None,
            attempt: None,
            state: SensorState::Pending,
            gt_activity: ActivityCounter::default(),
            media_activity: ActivityCounter::default(),
            last_diagnostic: None,
        }
    }

    pub fn sample(&mut self, frame: &mut CpuSensors) {
        if !self.supported {
            return;
        }
        if self.module.is_none()
            && self
                .attempt
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(30))
        {
            self.attempt = Some(Instant::now());
            // The fixed module reads PCI configuration only while opening.
            if let Some(_guard) = PciGuard::acquire() {
                match Module::open(BLOB) {
                    Ok(module) => self.module = Some(module),
                    Err(state) => self.state = state,
                }
            } else {
                self.state = SensorState::NoData;
            }
        }
        frame.integrated_gpu = Some(if let Some(module) = &self.module {
            let read = |register: Register| {
                module
                    .read(b"ioctl_read_dword", &[register as u64])
                    .and_then(|v| u32::try_from(v).ok())
            };
            let raw = Registers {
                temperature: read(Register::Temperature),
                performance: read(Register::Performance),
                capabilities: read(Register::Capabilities),
                memory: read(Register::MemoryClock),
                limits: read(Register::Limits),
            };
            let mut result = decode(&raw);
            let gt = module.read(b"ioctl_read_qword", &[Register::GtActivity as u64]);
            let gt_at = Instant::now();
            let media = module.read(b"ioctl_read_qword", &[Register::MediaActivity as u64]);
            let media_at = Instant::now();
            let supported = valid_capabilities(raw.capabilities).is_some();
            result.gt_activity_percent = self.gt_activity.sample(gt.filter(|_| supported), gt_at);
            result.media_activity_percent = self
                .media_activity
                .sample(media.filter(|_| supported), media_at);
            if result.temperature_celsius.is_none()
                || result.clock_mhz.is_none()
                || result.vid_volts.is_none()
                || result.memory_clock_mhz.is_none()
                || result.limits.is_empty()
            {
                // Raw values change each sample, so throttle before forming a
                // diagnostic key rather than treating each value as a new error.
                if self
                    .last_diagnostic
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(60))
                {
                    self.last_diagnostic = Some(Instant::now());
                    argus_core::diagnostics::warn(
                        "Intel iGPU MCHBAR sensors unavailable",
                        format!(
                            "model=0xa5; registers={raw:?}; gt_counter={gt:?}; media_counter={media:?}; state={:?}",
                            result.state
                        ),
                    );
                }
            }
            result
        } else {
            IntegratedGpuSensors {
                state: self.state,
                ..Default::default()
            }
        });
    }
}

fn valid_capabilities(value: Option<u32>) -> Option<u32> {
    value.filter(|v| {
        let max = v & 0xff;
        v & 0xff00_0000 == 0 && (1..=60).contains(&max) && ((v >> 16) & 0xff) <= max
    })
}

fn decode(raw: &Registers) -> IntegratedGpuSensors {
    let temperature_celsius = raw
        .temperature
        .filter(|v| v & !0xff == 0 && (1..=150).contains(v))
        .map(|v| v as f32);
    // A nonzero RP0 capability distinguishes an enabled GT from zero-filled
    // unsupported registers. Do not clamp the live ratio to RP0: OC can exceed it.
    let capabilities = valid_capabilities(raw.capabilities);
    let performance = raw
        .performance
        .filter(|v| capabilities.is_some() && v & 0xfc00_0000 == 0);
    // 615212 section 9.32 defines a nominal 16.666 MHz step. This is not a
    // measurement of the board's reference-clock deviation. 5F60h is not a
    // documented BCLK source for this layout and returned zero on the original PC.
    let clock_mhz = performance
        .map(|v| ((v >> 17) & 0x1ff) as f32 * 50.0 / 3.0)
        .filter(|v| *v <= 3000.0);
    let slice_clock_stopped = performance.map(|v| ((v >> 17) & 0x1ff) == 0);
    let unslice_clock_mhz = performance
        .map(|v| ((v >> 8) & 0x1ff) as f32 * 50.0 / 3.0)
        .filter(|v| *v <= 3000.0);
    let minimum_clock_mhz = capabilities
        .map(|v| ((v >> 16) & 0xff) as f32 * 50.0)
        .filter(|v| (1.0..=3000.0).contains(v));
    let maximum_clock_mhz = capabilities
        .map(|v| (v & 0xff) as f32 * 50.0)
        .filter(|v| (1.0..=3000.0).contains(v));
    // 615212 section 9.31: QCLK ratio [6:0], reference selector [7].
    // Section 9.41 defines DCLK = QCLK / 2. Do not label QCLK as DRAM clock,
    // or apply a later controller's [9:2]/[10] layout to Comet Lake.
    let memory_clock_mhz = raw
        .memory
        .filter(|v| *v != u32::MAX)
        .map(|v| {
            let ratio = (v & 0x7f) as f32;
            if v & (1 << 7) != 0 {
                ratio * 50.0
            } else {
                ratio * 200.0 / 3.0
            }
        })
        .filter(|v| (100.0..=10000.0).contains(v));
    // Legacy 8-bit SVID: zero requests off; 01h is 250 mV, then 5 mV steps.
    // Keep this separate from measured voltage throughout IPC and the UI.
    let vid_volts = performance.map(|v| match v & 0xff {
        0 => 0.0,
        vid => (245 + vid * 5) as f32 / 1000.0,
    });
    IntegratedGpuSensors {
        state: if temperature_celsius.is_some() || clock_mhz.is_some() || vid_volts.is_some() {
            SensorState::Ready
        } else {
            SensorState::NoData
        },
        temperature_celsius,
        clock_mhz,
        vid_volts,
        slice_clock_stopped,
        unslice_clock_mhz,
        minimum_clock_mhz,
        maximum_clock_mhz,
        memory_clock_mhz,
        limits: raw
            .limits
            .filter(|v| *v != u32::MAX)
            .map(limits)
            .unwrap_or_default(),
        ..Default::default()
    }
}

fn limits(raw: u32) -> Vec<IntegratedGpuLimit> {
    // Current and latched bits are separate; reading must not clear history.
    [
        (Reason::Power, (1 << 10) | (1 << 11)),
        (Reason::Thermal, (1 << 1) | (1 << 5) | (1 << 6)),
        (Reason::Prochot, 1),
        (Reason::Current, 1 << 7),
        (Reason::Other, 1 << 8),
        (Reason::Inefficient, 1 << 12),
    ]
    .into_iter()
    .map(|(reason, mask)| IntegratedGpuLimit {
        reason,
        active: raw & mask != 0,
        occurred: raw & (mask << 16) != 0,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPS: u32 = (7 << 16) | (7 << 8) | 24;

    #[test]
    fn comet_lake_recorded_memory_status_decodes_without_an_undocumented_reference() {
        // Original i7-10700K: SA_PERF_STATUS=2B000A1Bh, RP_STATE_CAP=00070718h.
        // Intel 615212-003 sections 9.31 and 9.41 define QCLK and DCLK=QCLK/2.
        let raw = Registers {
            temperature: Some(35),
            performance: Some(0),
            capabilities: Some(0x0007_0718),
            memory: Some(0x2b00_0a1b),
            limits: Some(0),
        };
        assert_eq!(super::decode(&raw).memory_clock_mhz, Some(1800.0));
    }

    fn decode(
        temperature: Option<u32>,
        performance: Option<u32>,
        capabilities: Option<u32>,
    ) -> IntegratedGpuSensors {
        super::decode(&Registers {
            temperature,
            performance,
            capabilities,
            ..Default::default()
        })
    }

    #[test]
    fn nominal_clock_steps_are_distinct_from_reference_measurement_and_idle_state() {
        let mut raw = Registers {
            temperature: Some(41),
            performance: Some(72 << 17),
            capabilities: Some(CAPS),
            memory: Some(27),
            ..Default::default()
        };
        let busy = super::decode(&raw);
        assert_eq!(busy.clock_mhz, Some(1200.0));
        assert_eq!(busy.memory_clock_mhz, Some(1800.0));
        assert_eq!(busy.reference_clock_mhz, None);
        assert_eq!(busy.slice_clock_stopped, Some(false));
        raw.performance = Some(0);
        let idle = super::decode(&raw);
        assert_eq!(idle.clock_mhz, Some(0.0));
        assert_eq!(idle.slice_clock_stopped, Some(true));
        assert_eq!(idle.minimum_clock_mhz, Some(350.0));
        raw.memory = Some((1 << 7) | 36);
        assert_eq!(super::decode(&raw).memory_clock_mhz, Some(1800.0));
        raw.memory = Some(0);
        assert_eq!(super::decode(&raw).memory_clock_mhz, None);
        raw.memory = Some(u32::MAX);
        assert_eq!(super::decode(&raw).memory_clock_mhz, None);
    }

    #[test]
    fn activity_uses_24mhz_counters_and_restarts_after_failures_resets_and_gaps() {
        let now = Instant::now();
        let mut counter = ActivityCounter::default();
        assert_eq!(counter.sample(Some(0), now), None);
        assert_eq!(
            counter.sample(Some(48_000), now + Duration::from_secs(1)),
            Some(0.2)
        );
        assert_eq!(
            counter.sample(Some(48_000), now + Duration::from_secs(2)),
            Some(0.0)
        );
        assert_eq!(counter.sample(None, now + Duration::from_secs(3)), None);
        assert_eq!(
            counter.sample(Some(1_000_000), now + Duration::from_secs(4)),
            None
        );
        assert_eq!(
            counter.sample(Some(1_024_000), now + Duration::from_secs(5)),
            Some(0.1)
        );
        assert_eq!(counter.sample(Some(0), now + Duration::from_secs(6)), None);
        assert_eq!(counter.sample(Some(1), now + Duration::from_secs(40)), None);
        assert_eq!(
            counter.sample(Some(u64::MAX), now + Duration::from_secs(41)),
            None
        );
    }

    #[test]
    fn limits_do_not_confuse_current_status_with_latched_history_or_reserved_bits() {
        let values = limits((1 << 11) | (1 << (1 + 16)) | (1 << 7) | (1 << 13));
        let get = |reason| values.iter().find(|v| v.reason == reason).unwrap();
        assert!(get(Reason::Power).active);
        assert!(get(Reason::Current).active);
        assert!(!get(Reason::Thermal).active && get(Reason::Thermal).occurred);
        assert!(!get(Reason::Other).active);
        assert!(limits(0).iter().all(|v| !v.active && !v.occurred));
    }

    #[test]
    fn gen9_active_and_rc6_readings_keep_clock_and_vid_separate() {
        let active = decode(Some(35), Some((21 << 17) | (27 << 8) | 0x6f), Some(CAPS));
        assert_eq!(active.state, SensorState::Ready);
        assert_eq!(active.temperature_celsius, Some(35.0));
        assert!((active.clock_mhz.unwrap() - 350.0).abs() < 0.001);
        assert!((active.unslice_clock_mhz.unwrap() - 450.0).abs() < 0.001);
        assert_eq!(active.vid_volts, Some(0.8));
        let idle = decode(Some(35), Some(0), Some(CAPS));
        assert_eq!((idle.clock_mhz, idle.vid_volts), (Some(0.0), Some(0.0)));
    }

    #[test]
    fn missing_and_unmapped_registers_never_become_idle_readings() {
        for (temp, perf, caps) in [
            (None, None, None),
            (Some(0), Some(0), Some(0)),
            (Some(u32::MAX), Some(u32::MAX), Some(u32::MAX)),
        ] {
            let result = decode(temp, perf, caps);
            assert_eq!(result.state, SensorState::NoData);
            assert!(
                result.temperature_celsius.is_none()
                    && result.clock_mhz.is_none()
                    && result.vid_volts.is_none()
            );
        }
        let partial = decode(Some(36), None, Some(CAPS));
        assert_eq!(partial.temperature_celsius, Some(36.0));
        assert_eq!(partial.clock_mhz, None);
    }

    #[test]
    fn newer_generations_and_other_vendors_do_not_load_the_gen9_module() {
        let mut cpu = CpuInfo::default();
        cpu.vendor = "GenuineIntel".into();
        cpu.family = 6;
        cpu.model = 0xa5;
        cpu.sockets = 1;
        assert!(Reader::new(&cpu).supported);
        for model in [0x97, 0xb7, 0x7e, 0xff] {
            cpu.model = model;
            assert!(!Reader::new(&cpu).supported);
        }
        cpu.model = 0xa5;
        cpu.sockets = 2;
        assert!(!Reader::new(&cpu).supported);
        cpu.sockets = 1;
        cpu.vendor = "AuthenticAMD".into();
        assert!(!Reader::new(&cpu).supported);
    }

    #[test]
    fn older_service_snapshots_and_new_optional_fields_are_compatible() {
        let old: CpuSensors =
            serde_json::from_str(r#"{"state":"ready","packageCelsius":60.0}"#).unwrap();
        assert!(old.integrated_gpu.is_none());
        let current = CpuSensors {
            integrated_gpu: Some(decode(Some(35), Some(0), Some(CAPS))),
            ..Default::default()
        };
        let roundtrip: CpuSensors =
            serde_json::from_slice(&serde_json::to_vec(&current).unwrap()).unwrap();
        let gpu = roundtrip.integrated_gpu.unwrap();
        assert_eq!(
            (gpu.temperature_celsius, gpu.clock_mhz, gpu.vid_volts),
            (Some(35.0), Some(0.0), Some(0.0))
        );
    }
}
