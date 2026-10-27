//! Read-only Comet Lake GT sensors through the signed IntelMCHBAR module.
//! Gen9 layout: Intel 332688 Vol. 2, sections 7.74 / 7.78 / 7.80.
//! Newer Xe layouts use different ratio bits and must not enter this reader.

use std::time::{Duration, Instant};

use argus_core::hardware::{
    CpuInfo, CpuSensors, IntegratedGpuSensors, SensorState, intel_mchbar_supported,
};

use super::{Module, PciGuard};

const BLOB: &[u8] = include_bytes!("../../../vendor/pawnio/IntelMCHBAR.bin");

#[derive(Clone, Copy)]
enum Register {
    Performance = 0x5948,
    Temperature = 0x5980,
    Capabilities = 0x5998,
}

pub(super) struct Reader {
    supported: bool,
    module: Option<Module>,
    attempt: Option<Instant>,
    state: SensorState,
}

impl Reader {
    pub fn new(cpu: &CpuInfo) -> Self {
        Self {
            supported: intel_mchbar_supported(cpu),
            module: None,
            attempt: None,
            state: SensorState::Pending,
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
            let temperature = read(Register::Temperature);
            let performance = read(Register::Performance);
            let capabilities = read(Register::Capabilities);
            let result = decode(temperature, performance, capabilities);
            if result.temperature_celsius.is_none()
                || result.clock_mhz.is_none()
                || result.vid_volts.is_none()
            {
                argus_core::diagnostics::warn(
                    "Intel iGPU MCHBAR sensors unavailable",
                    format!(
                        "model=0xa5; temperature={temperature:?}; performance={performance:?}; capabilities={capabilities:?}; state={:?}",
                        result.state
                    ),
                );
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

fn decode(
    temperature: Option<u32>,
    performance: Option<u32>,
    capabilities: Option<u32>,
) -> IntegratedGpuSensors {
    let temperature_celsius = temperature
        .filter(|v| v & !0xff == 0 && (1..=150).contains(v))
        .map(|v| v as f32);
    // A nonzero RP0 capability distinguishes an enabled GT from zero-filled
    // unsupported registers. Do not clamp the live ratio to RP0: OC can exceed it.
    let supported = capabilities.is_some_and(|v| {
        let max = v & 0xff;
        v & 0xff00_0000 == 0 && (1..=60).contains(&max) && ((v >> 16) & 0xff) <= max
    });
    let performance = performance.filter(|v| supported && v & 0xfc00_0000 == 0);
    let clock_mhz = performance
        .map(|v| ((v >> 17) & 0x1ff) as f32 * (50.0 / 3.0))
        .filter(|v| *v <= 3000.0);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPS: u32 = (7 << 16) | (7 << 8) | 24;

    #[test]
    fn gen9_active_and_rc6_readings_keep_clock_and_vid_separate() {
        let active = decode(Some(35), Some((21 << 17) | (27 << 8) | 0x6f), Some(CAPS));
        assert_eq!(active.state, SensorState::Ready);
        assert_eq!(active.temperature_celsius, Some(35.0));
        assert!((active.clock_mhz.unwrap() - 350.0).abs() < 0.001);
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
