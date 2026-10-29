//! The read-only ctlPowerTelemetryGet ABI from Intel's igcl_api.h. Each item
//! carries its own support flag, units and scalar type; a zero is not missing.

use super::Header;
use crate::hardware::vendor_gpu::{GpuPowerSource, Reading, bounded};

const MHZ: i32 = 0;
const VOLTS: i32 = 3;
const CELSIUS: i32 = 5;
const JOULES: i32 = 6;
const SECONDS: i32 = 7;
const MILLIVOLTS: i32 = 13;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Item {
    supported: u8,
    units: i32,
    kind: i32,
    // Storage for ctl_data_value_t; Windows uses a little-endian scalar ABI.
    value: u64,
}

impl Item {
    fn scalar(&self, units: i32) -> Option<f64> {
        if self.supported == 0 || self.units != units {
            return None;
        }
        let value = match self.kind {
            0 => self.value as i8 as f64,
            1 => self.value as u8 as f64,
            2 => self.value as i16 as f64,
            3 => self.value as u16 as f64,
            4 => self.value as i32 as f64,
            5 => self.value as u32 as f64,
            6 => self.value as i64 as f64,
            7 => self.value as f64,
            8 => f32::from_bits(self.value as u32) as f64,
            9 => f64::from_bits(self.value),
            _ => return None,
        };
        value.is_finite().then_some(value)
    }
}

#[repr(C)]
#[derive(Default)]
struct Psu {
    supported: u8,
    kind: i32,
    energy: Item,
    voltage: Item,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct Telemetry {
    header: Header,
    timestamp: Item,
    gpu_energy: Item,
    gpu_voltage: Item,
    gpu_clock: Item,
    gpu_temperature: Item,
    activity: [Item; 3],
    gpu_limits: [u8; 5],
    vram_energy: Item,
    vram_voltage: Item,
    vram_clock: Item,
    vram_effective_clock: Item,
    vram_read: Item,
    vram_write: Item,
    vram_temperature: Item,
    vram_limits: [u8; 5],
    card_energy: Item,
    psu: [Psu; 5],
    fan: [Item; 5],
    // Nine additional items in struct version 1, unused by this reader.
    extension: [Item; 9],
}

impl Telemetry {
    fn new(version: u8) -> Self {
        Self {
            header: Header {
                size: if version == 0 {
                    std::mem::offset_of!(Self, extension) as u32
                } else {
                    size_of::<Self>() as u32
                },
                version,
            },
            ..Default::default()
        }
    }

    pub(super) fn reading(&self, previous: &mut Option<(f64, f64)>) -> Reading {
        let watts = self.power(previous);
        Reading {
            watts,
            source: watts.map(|_| GpuPowerSource::IgclChip),
            temperature: self
                .gpu_temperature
                .scalar(CELSIUS)
                .and_then(|v| bounded(v, -100.0, 200.0)),
            clock: self
                .gpu_clock
                .scalar(MHZ)
                .and_then(|v| bounded(v, 0.0, 20_000.0))
                .map(|v| v as u32),
            memory_clock: self
                .vram_clock
                .scalar(MHZ)
                .and_then(|v| bounded(v, 0.0, 100_000.0))
                .map(|v| v as u32),
            voltage: self
                .gpu_voltage
                .scalar(VOLTS)
                .map(|v| v * 1000.0)
                .or_else(|| self.gpu_voltage.scalar(MILLIVOLTS))
                .and_then(|v| bounded(v, 0.0, 5000.0))
                .map(|v| v.round() as u32),
            ..Reading::default()
        }
    }

    fn power(&self, previous: &mut Option<(f64, f64)>) -> Option<f32> {
        let current = self
            .gpu_energy
            .scalar(JOULES)
            .zip(self.timestamp.scalar(SECONDS))
            .filter(|&(energy, time)| energy >= 0.0 && time >= 0.0);
        let before = std::mem::replace(previous, current);
        let ((energy, time), (before, at)) = current.zip(before)?;
        let elapsed = time - at;
        if !(0.001..=30.0).contains(&elapsed) {
            return None;
        }
        bounded((energy - before) / elapsed, 0.0, 2000.0)
    }
}

pub(super) fn query(
    version: &mut u8,
    mut read: impl FnMut(&mut Telemetry) -> i32,
) -> Option<Telemetry> {
    let mut telemetry = Telemetry::new(*version);
    let mut status = read(&mut telemetry);
    // Older drivers may only know the version-0 prefix of the struct. The
    // backing allocation still covers version 1; only version-0 fields are read.
    if *version != 0 && matches!(status, 0x40000009 | 0x4000000f) {
        *version = 0;
        telemetry = Telemetry::new(0);
        status = read(&mut telemetry);
    }
    (status == 0).then_some(telemetry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(units: i32, value: f64) -> Item {
        Item {
            supported: 1,
            units,
            kind: 9,
            value: value.to_bits(),
        }
    }

    #[test]
    fn ffi_matches_x64_sdk_telemetry_layout() {
        assert_eq!(size_of::<Item>(), 24);
        assert_eq!(align_of::<Item>(), 8);
        assert_eq!(std::mem::offset_of!(Item, value), 16);
        assert_eq!(size_of::<Psu>(), 56);
        assert_eq!(size_of::<Telemetry>(), 1024);
        assert_eq!(std::mem::offset_of!(Telemetry, gpu_voltage), 56);
        assert_eq!(std::mem::offset_of!(Telemetry, gpu_clock), 80);
        assert_eq!(std::mem::offset_of!(Telemetry, gpu_temperature), 104);
        assert_eq!(std::mem::offset_of!(Telemetry, vram_clock), 256);
        assert_eq!(std::mem::offset_of!(Telemetry, extension), 808);
    }

    #[test]
    fn supported_idle_readings_survive_without_fabricating_unsupported_fields() {
        let mut telemetry = Telemetry::new(1);
        let empty = telemetry.reading(&mut None);
        assert_eq!(
            (empty.temperature, empty.clock, empty.voltage),
            (None, None, None)
        );
        telemetry.gpu_temperature = item(CELSIUS, 35.0);
        telemetry.gpu_clock = item(MHZ, 349.4);
        telemetry.gpu_voltage = item(VOLTS, 0.8);
        telemetry.vram_clock = item(MHZ, 1797.0);
        let active = telemetry.reading(&mut None);
        assert_eq!(
            (active.temperature, active.clock, active.voltage),
            (Some(35.0), Some(349), Some(800))
        );
        assert_eq!(active.memory_clock, Some(1797));
        telemetry.gpu_clock = item(MHZ, 0.0);
        telemetry.gpu_voltage = item(VOLTS, 0.0);
        let idle = telemetry.reading(&mut None);
        assert_eq!((idle.clock, idle.voltage), (Some(0), Some(0)));
        telemetry.gpu_clock.supported = 0;
        telemetry.gpu_voltage = item(MILLIVOLTS, 725.0);
        let partial = telemetry.reading(&mut None);
        assert_eq!((partial.clock, partial.voltage), (None, Some(725)));
    }

    #[test]
    fn all_scalar_types_are_decoded_with_units_and_validity_checks() {
        for (kind, value, expected) in [
            (0, 0xff, -1.0),
            (1, 0xff, 255.0),
            (2, 0xffff, -1.0),
            (3, 0xffff, 65535.0),
            (4, 0xffff_ffff, -1.0),
            (5, 0xffff_ffff, u32::MAX as f64),
            (6, u64::MAX, -1.0),
            (7, u64::MAX, u64::MAX as f64),
            (8, 35.5f32.to_bits() as u64, 35.5),
            (9, 35.5f64.to_bits(), 35.5),
        ] {
            assert_eq!(
                Item {
                    supported: 1,
                    units: CELSIUS,
                    kind,
                    value
                }
                .scalar(CELSIUS),
                Some(expected)
            );
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(item(CELSIUS, value).scalar(CELSIUS), None);
        }
        assert_eq!(item(VOLTS, 35.0).scalar(CELSIUS), None);
        assert_eq!(
            Item {
                kind: 10,
                ..item(CELSIUS, 35.0)
            }
            .scalar(CELSIUS),
            None
        );
        let mut telemetry = Telemetry::new(1);
        telemetry.gpu_temperature = item(CELSIUS, 1000.0);
        telemetry.gpu_clock = item(MHZ, -1.0);
        telemetry.gpu_voltage = item(VOLTS, 10.0);
        let invalid = telemetry.reading(&mut None);
        assert_eq!(
            (invalid.temperature, invalid.clock, invalid.voltage),
            (None, None, None)
        );
    }

    #[test]
    fn chip_power_uses_gpu_energy_and_device_time_without_crossing_gaps() {
        let mut telemetry = Telemetry::new(1);
        let mut previous = None;
        let mut sample = |energy, time| {
            telemetry.gpu_energy = item(JOULES, energy);
            telemetry.timestamp = item(SECONDS, time);
            telemetry.power(&mut previous)
        };
        assert_eq!(sample(100.0, 1000.0), None);
        assert_eq!(sample(100.014, 1001.0), Some(0.014));
        assert_eq!(sample(100.014, 1002.0), Some(0.0));
        assert_eq!(sample(100.014, 1002.0), None);
        assert_eq!(sample(0.0, 1003.0), None);
        assert_eq!(sample(1.0, 1040.0), None);
        assert_eq!(sample(f64::NAN, 1041.0), None);
        assert_eq!(sample(2.0, 1042.0), None);
        assert_eq!(sample(3.0, 1043.0), Some(1.0));
    }

    #[test]
    fn old_driver_layout_is_negotiated_only_for_version_or_size_errors() {
        for error in [0x40000009, 0x4000000f] {
            let mut version = 1;
            let mut calls = Vec::new();
            assert!(
                query(&mut version, |data| {
                    calls.push((data.header.version, data.header.size));
                    if data.header.version == 1 { error } else { 0 }
                })
                .is_some()
            );
            assert_eq!(calls, [(1, 1024), (0, 808)]);
            assert_eq!(version, 0);
        }
        let mut calls = 0;
        assert!(
            query(&mut 1, |_| {
                calls += 1;
                0x4000000a
            })
            .is_none()
        );
        assert_eq!(calls, 1);
    }

    #[test]
    fn telemetry_without_power_domains_recovers_and_drops_failed_energy_intervals() {
        use super::super::{Device, Queries};
        use crate::hardware::query::Retry;
        use std::cell::Cell;
        use std::time::{Duration, Instant};

        struct Driver {
            calls: Cell<u32>,
            failed: Cell<bool>,
            energy: Cell<f64>,
            time: Cell<f64>,
        }
        unsafe extern "C" fn read(handle: *mut std::ffi::c_void, data: *mut Telemetry) -> i32 {
            let driver = unsafe { &*handle.cast::<Driver>() };
            driver.calls.set(driver.calls.get() + 1);
            if driver.failed.get() {
                return 0x4000000a;
            }
            let data = unsafe { &mut *data };
            data.gpu_clock = item(MHZ, 0.0);
            data.gpu_temperature = item(CELSIUS, 35.0);
            data.gpu_energy = item(JOULES, driver.energy.get());
            data.timestamp = item(SECONDS, driver.time.get());
            0
        }
        let driver = Driver {
            calls: Cell::new(0),
            failed: Cell::new(false),
            energy: Cell::new(10.0),
            time: Cell::new(100.0),
        };
        let queries = Queries {
            telemetry: Some(read),
            ..Default::default()
        };
        let mut device = Device {
            pci: (0, 2, 0),
            handle: (&raw const driver).cast_mut().cast(),
            domain: None,
            telemetry_query: Retry::default(),
            telemetry_version: 1,
            previous: None,
        };
        let now = Instant::now();
        let first = queries.sample(&mut device, now);
        assert_eq!(
            (first.temperature, first.clock, first.watts),
            (Some(35.0), Some(0), None)
        );
        driver.energy.set(driver.energy.get() + 1.0);
        driver.time.set(driver.time.get() + 1.0);
        let second = queries.sample(&mut device, now + Duration::from_secs(1));
        assert_eq!(second.watts, Some(1.0));
        assert!(matches!(second.source, Some(GpuPowerSource::IgclChip)));
        driver.failed.set(true);
        let failed = queries.sample(&mut device, now + Duration::from_secs(2));
        assert_eq!(
            (failed.temperature, failed.clock, failed.watts),
            (None, None, None)
        );
        driver.failed.set(false);
        assert_eq!(
            queries
                .sample(&mut device, now + Duration::from_secs(3))
                .temperature,
            None
        );
        assert_eq!(driver.calls.get(), 3);
        driver.time.set(driver.time.get() + 6.0);
        let recovered = queries.sample(&mut device, now + Duration::from_secs(7));
        assert_eq!((recovered.temperature, recovered.watts), (Some(35.0), None));
        driver.time.set(driver.time.get() + 1.0);
        assert_eq!(
            queries
                .sample(&mut device, now + Duration::from_secs(8))
                .watts,
            Some(0.0)
        );
    }
}
