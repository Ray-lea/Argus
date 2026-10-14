//! CPU telemetry through embedded, signed PawnIO modules. Nothing here
//! installs/starts a driver, loads a user-supplied module, or changes MSRs.
//! Register maps: AMD OSRR 56255; Linux k10temp; Intel SDM Vol. 4;
//! LibreHardwareMonitor (SVI2 layouts). See vendor/pawnio/manifest.json.

use crate::util::Handle;
use argus_core::hardware::{CcdTemperature, CoreTemperature, CpuSensors, SensorState};
use std::time::Instant;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::SystemInformation::GROUP_AFFINITY;
use windows::Win32::System::Threading::{
    CreateMutexW, GetActiveProcessorCount, GetActiveProcessorGroupCount, GetCurrentThread,
    ReleaseMutex, SetThreadGroupAffinity, WaitForSingleObject,
};
use windows::core::w;

const LOAD: u32 = (41394 << 16) | (0x821 << 2);
const EXECUTE: u32 = (41394 << 16) | (0x841 << 2);
const INTEL: &[u8] = include_bytes!("../../vendor/pawnio/IntelMSR.bin");
const AMD_OLD: &[u8] = include_bytes!("../../vendor/pawnio/AMDFamily10.bin");
const AMD_ZEN: &[u8] = include_bytes!("../../vendor/pawnio/AMDFamily17.bin");

struct Module(Handle);
impl Module {
    fn open(blob: &[u8]) -> Result<Self, SensorState> {
        let handle = unsafe {
            CreateFileW(
                w!(r"\\?\GLOBALROOT\Device\PawnIO"),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .map(Handle)
        .map_err(|_| SensorState::DriverUnavailable)?;
        let mut returned = 0;
        unsafe {
            DeviceIoControl(
                handle.0,
                LOAD,
                Some(blob.as_ptr().cast()),
                blob.len() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )
        }
        .map_err(|_| SensorState::ModuleRejected)?;
        Ok(Self(handle))
    }

    fn read(&self, function: &[u8], input: &[u64]) -> Option<u64> {
        // Fixed-size ABI: 32-byte NUL-padded function name, then 64-bit cells.
        if function.len() >= 32 || input.len() > 2 {
            return None;
        }
        let mut request = [0u64; 6];
        let name = unsafe { std::slice::from_raw_parts_mut(request.as_mut_ptr().cast::<u8>(), 32) };
        name[..function.len()].copy_from_slice(function);
        request[4..4 + input.len()].copy_from_slice(input);
        let mut value = 0u64;
        let mut returned = 0;
        unsafe {
            DeviceIoControl(
                self.0.0,
                EXECUTE,
                Some(request.as_ptr().cast()),
                (32 + input.len() * 8) as u32,
                Some((&mut value as *mut u64).cast()),
                8,
                Some(&mut returned),
                None,
            )
        }
        .ok()?;
        (returned == 8).then_some(value)
    }

    fn msr(&self, register: u32) -> Option<u64> {
        self.read(b"ioctl_read_msr", &[register as u64])
    }
    fn smn(&self, register: u32) -> Option<u32> {
        self.read(b"ioctl_read_smn", &[register as u64])
            .map(|v| v as u32)
    }
}

struct Affinity(GROUP_AFFINITY);
impl Affinity {
    fn set(group: u16, processor: u8) -> Option<Self> {
        let next = GROUP_AFFINITY {
            Mask: 1usize.checked_shl(processor as u32)?,
            Group: group,
            Reserved: [0; 3],
        };
        let mut old = GROUP_AFFINITY::default();
        unsafe { SetThreadGroupAffinity(GetCurrentThread(), &next, Some(&mut old)) }
            .as_bool()
            .then(|| Self(old))
    }
}
impl Drop for Affinity {
    fn drop(&mut self) {
        unsafe {
            let _ = SetThreadGroupAffinity(GetCurrentThread(), &self.0, None);
        }
    }
}

/// Coordinate the SMN selector with other hardware monitors. Never wait on
/// another tool's PCI lock for more than 20 ms; a skipped reading is harmless.
struct PciGuard(Handle);
impl PciGuard {
    fn acquire() -> Option<Self> {
        let handle = Handle(unsafe { CreateMutexW(None, false, w!(r"Global\Access_PCI")) }.ok()?);
        matches!(
            unsafe { WaitForSingleObject(handle.0, 20) },
            WAIT_OBJECT_0 | WAIT_ABANDONED
        )
        .then(|| Self(handle))
    }
}
impl Drop for PciGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0.0);
        }
    }
}

#[derive(Clone, Copy)]
struct Identity {
    intel: bool,
    family: u32,
    model: u32,
    thermal: u32,
}
impl Identity {
    fn current() -> Option<Self> {
        use std::arch::x86_64::__cpuid;
        let vendor = __cpuid(0);
        let intel = (vendor.ebx, vendor.edx, vendor.ecx) == (0x756e6547, 0x49656e69, 0x6c65746e);
        let amd = (vendor.ebx, vendor.edx, vendor.ecx) == (0x68747541, 0x69746e65, 0x444d4163);
        if !intel && !amd {
            return None;
        }
        let id = __cpuid(1).eax;
        let base_family = (id >> 8) & 15;
        let family = base_family
            + if base_family == 15 {
                (id >> 20) & 255
            } else {
                0
            };
        let model = ((id >> 4) & 15)
            | if matches!(base_family, 6 | 15) {
                (id >> 12) & 0xf0
            } else {
                0
            };
        Some(Self {
            intel,
            family,
            model,
            thermal: if vendor.eax >= 6 { __cpuid(6).eax } else { 0 },
        })
    }
}

pub(super) struct CpuReader {
    module: Module,
    id: Identity,
    processors: Vec<(u16, u8)>,
    amd_offset: f32,
    energy: Option<(u32, Instant)>,
}

impl CpuReader {
    pub fn open() -> Result<Self, SensorState> {
        let id = Identity::current().ok_or(SensorState::Unsupported)?;
        let blob = match (id.intel, id.family) {
            (true, 6) => INTEL,
            (false, 0x10..=0x16) => AMD_OLD,
            (false, 0x17..=0x1a) => AMD_ZEN,
            _ => return Err(SensorState::Unsupported),
        };
        let info = argus_core::hardware::cpu_info();
        // AMD's SMN module addresses node 0. Do not mislabel a socket as the
        // whole machine on configurations outside the consumer single socket.
        if info.sockets != 1 {
            return Err(SensorState::Unsupported);
        }
        let module = Module::open(blob)?;
        let processors = (0..unsafe { GetActiveProcessorGroupCount() })
            .flat_map(|group| {
                (0..unsafe { GetActiveProcessorCount(group) }.min(64))
                    .map(move |processor| (group, processor as u8))
            })
            .take(256)
            .collect();
        let amd_offset = if !id.intel && id.family == 0x17 {
            if ["1600X", "1700X", "1800X"]
                .iter()
                .any(|s| info.name.contains(s))
            {
                20.0
            } else if info.name.contains("2700X") {
                10.0
            } else if info.name.contains("Threadripper 19") || info.name.contains("Threadripper 29")
            {
                27.0
            } else {
                0.0
            }
        } else {
            0.0
        };
        Ok(Self {
            module,
            id,
            processors,
            amd_offset,
            energy: None,
        })
    }

    pub fn sample(&mut self) -> CpuSensors {
        let mut frame = CpuSensors::default();
        let Some(_affinity) = Affinity::set(0, 0) else {
            return CpuSensors::unavailable(SensorState::NoData);
        };
        if self.id.intel {
            self.intel(&mut frame);
        } else {
            self.amd(&mut frame);
        }
        let (unit_register, energy_register) = if self.id.intel {
            (0x606, 0x611)
        } else {
            (0xc0010299, 0xc001029b)
        };
        if self.id.intel || self.id.family >= 0x17 {
            if let (Some(unit), Some(energy)) = (
                self.module.msr(unit_register),
                self.module.msr(energy_register),
            ) {
                let scale = energy_unit(self.id, unit as u32);
                let now = Instant::now();
                if let Some((before, at)) = self.energy.replace((energy as u32, now)) {
                    frame.package_watts = energy_delta(
                        before,
                        energy as u32,
                        now.duration_since(at).as_secs_f64(),
                        scale,
                    );
                }
            } else {
                self.energy = None;
            }
        }
        frame.state = if frame.package_celsius.is_some()
            || frame.control_celsius.is_some()
            || !frame.core_temperatures.is_empty()
            || !frame.ccd_temperatures.is_empty()
            || frame.core_voltage.is_some()
            || frame.core_vid.is_some()
            || frame.package_watts.is_some()
        {
            SensorState::Ready
        } else {
            SensorState::NoData
        };
        frame
    }

    fn intel(&self, frame: &mut CpuSensors) {
        let started = Instant::now();
        for &(group, processor) in &self.processors {
            if started.elapsed().as_millis() > 500 {
                break;
            }
            let Some(_affinity) = Affinity::set(group, processor) else {
                continue;
            };
            let target = self.module.msr(0x1a2).map(|v| ((v >> 16) & 255) as u32);
            if let Some(target) = target
                && self.id.thermal & 1 != 0
                && let Some(celsius) = self
                    .module
                    .msr(0x19c)
                    .and_then(|v| intel_temperature(v, target))
            {
                frame.core_temperatures.push(CoreTemperature {
                    group,
                    processor,
                    celsius,
                });
            }
            if group == 0 && processor == 0 {
                if self.id.thermal & (1 << 6) != 0 {
                    frame.package_celsius = target.and_then(|target| {
                        self.module
                            .msr(0x1b1)
                            .and_then(|v| intel_temperature(v, target))
                    });
                }
                // PERF_STATUS[47:32] VID is model specific. Never reinterpret
                // its frequency fields or use this encoding on future models.
                if matches!(
                    self.id.model,
                    0x2a | 0x2d
                        | 0x3a
                        | 0x3c
                        | 0x3f
                        | 0x45
                        | 0x46
                        | 0x3d
                        | 0x47
                        | 0x4f
                        | 0x56
                        | 0x4e
                        | 0x5e
                        | 0x8e
                        | 0x9e
                ) {
                    frame.core_vid = self
                        .module
                        .msr(0x198)
                        .and_then(|v| valid(((v >> 32) & 0xffff) as f64 / 8192.0, 0.1, 2.0));
                }
            }
        }
    }

    fn amd(&self, frame: &mut CpuSensors) {
        let Some(_pci) = PciGuard::acquire() else {
            return;
        };
        if self.id.family >= 0x17 {
            frame.control_celsius = self
                .module
                .smn(0x59800)
                .and_then(|v| amd_temperature(v, true));
            frame.package_celsius = frame
                .control_celsius
                .and_then(|v| valid((v - self.amd_offset) as f64, -20.0, 125.0));
            if let Some((offset, count)) = ccd_profile(self.id.family, self.id.model) {
                for index in 0..count {
                    if let Some(celsius) = self
                        .module
                        .smn(0x59800 + offset + index as u32 * 4)
                        .and_then(ccd_temperature)
                    {
                        frame
                            .ccd_temperatures
                            .push(CcdTemperature { index, celsius });
                    }
                }
            }
            if let Some(offset) = svi_profile(self.id.family, self.id.model)
                && let Some(flags) = self.module.smn(0x5a008)
                && flags != u32::MAX
                && flags & 1 == 0
            {
                frame.core_voltage = self
                    .module
                    .smn(offset)
                    .filter(|v| *v != 0 && *v != u32::MAX)
                    .and_then(|v| valid(1.55 - ((v >> 16) & 255) as f64 * 0.00625, 0.1, 1.55));
            }
        } else {
            let raw = if self.id.family == 0x15 && matches!(self.id.model & 0xf0, 0x60 | 0x70) {
                self.module.read(b"ioctl_read_smu", &[0xd8200ca4])
            } else {
                self.module.read(b"ioctl_read_miscctl", &[0, 0xa4])
            };
            frame.control_celsius = raw.and_then(|v| amd_temperature(v as u32, false));
            // Legacy Tctl is a control scale and may differ from die °C.
            if let Some(value) = self.module.msr(0xc0010071) {
                let svi2 =
                    (self.id.family == 0x15 && self.id.model >= 0x10) || self.id.family == 0x16;
                let vid = ((value >> 9) & 127) | if svi2 { (value >> 13) & 128 } else { 0 };
                frame.core_vid = valid(
                    1.55 - vid as f64 * if svi2 { 0.00625 } else { 0.0125 },
                    0.1,
                    1.55,
                );
            }
        }
    }
}

fn valid(value: f64, low: f64, high: f64) -> Option<f32> {
    (value.is_finite() && (low..=high).contains(&value)).then_some(value as f32)
}
fn intel_temperature(value: u64, target: u32) -> Option<f32> {
    if value & (1 << 31) == 0 || !(50..=125).contains(&target) {
        return None;
    }
    valid(target as f64 - ((value >> 16) & 127) as f64, -20.0, 125.0)
}
fn amd_temperature(value: u32, zen: bool) -> Option<f32> {
    if value == 0 || value == u32::MAX {
        return None;
    }
    let offset = if (zen && value & (1 << 19) != 0) || value & 0x30000 == 0x30000 {
        49.0
    } else {
        0.0
    };
    valid((value >> 21) as f64 * 0.125 - offset, -20.0, 150.0)
}
fn ccd_temperature(value: u32) -> Option<f32> {
    if value == u32::MAX || value & (1 << 11) == 0 {
        return None;
    }
    valid((value & 0x7ff) as f64 * 0.125 - 49.0, -20.0, 125.0)
}
fn ccd_profile(family: u32, model: u32) -> Option<(u32, u8)> {
    match (family, model) {
        (0x17 | 0x18, 0x1 | 0x8 | 0x11 | 0x18) => Some((0x154, 4)),
        (0x17 | 0x18, 0x31 | 0x47 | 0x60 | 0x68 | 0x71)
        | (0x19, 0x0 | 0x1 | 0x8 | 0x21 | 0x50..=0x5f) => Some((0x154, 8)),
        (0x17 | 0x18, 0xa0..=0xaf) | (0x19, 0x40..=0x4f) => Some((0x300, 8)),
        (0x19, 0x60..=0x7f) | (0x1a, 0x40..=0x4f) => Some((0x308, 8)),
        (0x19, 0x10..=0x1f | 0xa0..=0xaf) => Some((0x300, 12)),
        (0x1a, 0x00..=0x1f) => Some((0x1f0, 16)),
        _ => None,
    }
}
fn svi_profile(family: u32, model: u32) -> Option<u32> {
    match (family, model) {
        (0x17, 0x1 | 0x8 | 0x11 | 0x18) => Some(0x5a00c),
        (0x17, 0x31) => Some(0x5a014),
        (0x17, 0x71) | (0x19, 0x21) => Some(0x5a010),
        _ => None,
    }
}
fn energy_unit(id: Identity, unit: u32) -> f64 {
    let exponent = (unit >> 8) & 31;
    if id.intel && matches!(id.model, 0x37 | 0x4a | 0x4d | 0x5a | 0x4c) {
        1e-6 * (1u64 << exponent) as f64
    } else {
        1.0 / (1u64 << exponent) as f64
    }
}
fn energy_delta(before: u32, after: u32, seconds: f64, unit: f64) -> Option<f32> {
    if !(0.05..=30.0).contains(&seconds) {
        return None;
    }
    valid(
        after.wrapping_sub(before) as f64 * unit / seconds,
        0.0,
        2000.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn affinity_is_restored_without_administrator_access() {
        use windows::Win32::System::Threading::GetThreadGroupAffinity;
        let mut before = GROUP_AFFINITY::default();
        unsafe { GetThreadGroupAffinity(GetCurrentThread(), &mut before) }.unwrap();
        {
            let _guard = Affinity::set(before.Group, before.Mask.trailing_zeros() as u8).unwrap();
            let mut current = GROUP_AFFINITY::default();
            unsafe { GetThreadGroupAffinity(GetCurrentThread(), &mut current) }.unwrap();
            assert_eq!(current.Mask.count_ones(), 1);
        }
        let mut after = GROUP_AFFINITY::default();
        unsafe { GetThreadGroupAffinity(GetCurrentThread(), &mut after) }.unwrap();
        assert_eq!((before.Group, before.Mask), (after.Group, after.Mask));
    }
    #[test]
    fn validates_sensor_bits_and_zen5_mapping() {
        assert_eq!(intel_temperature((1 << 31) | (30 << 16), 100), Some(70.0));
        assert_eq!(intel_temperature(30 << 16, 100), None);
        assert_eq!(intel_temperature(1 << 31, 0), None);
        assert_eq!(amd_temperature((992 << 21) | (1 << 19), true), Some(75.0));
        assert_eq!(amd_temperature(u32::MAX, true), None);
        assert_eq!(ccd_temperature((1 << 11) | 992), Some(75.0));
        assert_eq!(ccd_temperature(992), None);
        assert_eq!(ccd_profile(0x1a, 0x44), Some((0x308, 8)));
        assert_eq!(ccd_profile(0x1a, 0xff), None);
        assert_eq!(svi_profile(0x19, 0x61), None);
        assert_eq!(svi_profile(0x1a, 0x44), None);
        assert_eq!(svi_profile(0x19, 0x21), Some(0x5a010));
    }
    #[test]
    fn energy_counter_wrap_and_sleep_are_bounded() {
        assert_eq!(energy_delta(u32::MAX - 99, 100, 1.0, 0.125), Some(25.0));
        assert_eq!(energy_delta(1, 1, 1.0, 1.0), Some(0.0));
        assert_eq!(energy_delta(1000, 0, 1.0, 0.125), None);
        assert_eq!(energy_delta(0, 1, 31.0, 1.0), None);
        let core = Identity {
            intel: true,
            family: 6,
            model: 0x9e,
            thermal: 0,
        };
        let atom = Identity {
            model: 0x37,
            ..core
        };
        assert_eq!(energy_unit(core, 16 << 8), 1.0 / 65536.0);
        assert_eq!(energy_unit(atom, 16 << 8), 0.065536);
    }
}
