//! Board power and GPU-core activity from NVML (`nvml.dll`, which the
//! driver installs in System32). WDDM's adapter perf data leaves power empty
//! on NVIDIA cards and only ever gives it as a share of the budget. Loaded from
//! System32 only, never from the application or working directory; without an
//! NVIDIA driver it is simply absent. NVML needs no administrator rights.

use crate::diagnostics::{startup_native, startup_phase};
use std::ffi::c_void;

use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{PCSTR, s, w};

type Device = *mut c_void;
type Return = i32;
const SUCCESS: Return = 0;

/// `nvmlPciInfo_t` as filled by `nvmlDeviceGetPciInfo_v3`.
#[repr(C)]
#[derive(Default)]
struct PciInfo {
    bus_id_legacy: [u8; 16],
    domain: u32,
    bus: u32,
    device: u32,
    pci_device_id: u32,
    pci_sub_system_id: u32,
    bus_id: [u8; 32],
}

type Query = unsafe extern "C" fn(Device, *mut u32) -> Return;
#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: u32,
    memory: u32,
}
type UtilizationQuery = unsafe extern "C" fn(Device, *mut Utilization) -> Return;

pub(crate) struct Nvml {
    power_usage: Query,
    power_limit: Query,
    utilization: Option<UtilizationQuery>,
    /// PCI bus and device number of each GPU NVML manages.
    devices: Vec<(u32, u32, u32, Device)>,
}

// NVML is thread-safe and its device handles stay valid for the process.
unsafe impl Send for Nvml {}

/// Looks up an export and casts it to the function type it is documented with.
unsafe fn export<F: Copy>(module: windows::Win32::Foundation::HMODULE, name: PCSTR) -> Option<F> {
    let f = unsafe { GetProcAddress(module, name) }?;
    debug_assert_eq!(size_of::<F>(), size_of_val(&f));
    Some(unsafe { std::mem::transmute_copy(&f) })
}

impl Nvml {
    pub fn load() -> Option<Self> {
        let library = startup_phase("nvml.load-library", || unsafe {
            LoadLibraryExW(w!("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
        });
        log::info!(target: "argus_startup", "phase=nvml.library-result; status=0x{:08X}", library.as_ref().err().map_or(0, |error| error.code().0 as u32));
        let module = library.ok()?;
        unsafe {
            let init: unsafe extern "C" fn() -> Return = export(module, s!("nvmlInit_v2"))?;
            let count: unsafe extern "C" fn(*mut u32) -> Return =
                export(module, s!("nvmlDeviceGetCount_v2"))?;
            let by_index: unsafe extern "C" fn(u32, *mut Device) -> Return =
                export(module, s!("nvmlDeviceGetHandleByIndex_v2"))?;
            let pci_info: unsafe extern "C" fn(Device, *mut PciInfo) -> Return =
                export(module, s!("nvmlDeviceGetPciInfo_v3"))?;
            let power_usage: Query = export(module, s!("nvmlDeviceGetPowerUsage"))?;
            let power_limit: Query = export(module, s!("nvmlDeviceGetEnforcedPowerLimit"))?;
            let utilization = export(module, s!("nvmlDeviceGetUtilizationRates"));
            // Initialized for the rest of the process; NVML is never shut down.
            if startup_native("nvml.init", 0, 0, || init()) != SUCCESS {
                return None;
            }
            let mut n = 0;
            if startup_native("nvml.device-count", 0, 0, || count(&mut n)) != SUCCESS || n > 64 {
                return None;
            }
            let devices = (0..n)
                .filter_map(|i| {
                    let mut device = std::ptr::null_mut();
                    let mut pci = PciInfo::default();
                    if startup_native("nvml.device-handle", u64::from(i), 0, || {
                        by_index(i, &mut device)
                    }) != SUCCESS
                        || startup_native("nvml.pci-info", u64::from(i), 0, || {
                            pci_info(device, &mut pci)
                        }) != SUCCESS
                        || pci.domain != 0
                    {
                        return None;
                    }
                    let end = pci
                        .bus_id
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(pci.bus_id.len());
                    let id = std::str::from_utf8(&pci.bus_id[..end]).ok()?;
                    let function = u32::from_str_radix(id.rsplit_once('.')?.1, 16)
                        .ok()
                        .filter(|v| *v < 8)?;
                    Some((pci.bus, pci.device, function, device))
                })
                .collect::<Vec<_>>();
            (!devices.is_empty()).then_some(Self {
                power_usage,
                power_limit,
                utilization,
                devices,
            })
        }
    }

    pub(super) fn probe_once() -> Option<usize> {
        let nvml = Self::load()?;
        for &(bus, device, function, _) in &nvml.devices {
            nvml.power(bus, device, function);
            nvml.utilization(bus, device, function);
        }
        Some(nvml.devices.len())
    }

    /// Driver GPU-core activity, including CUDA workloads missing from WDDM nodes.
    /// Zero is valid; unsupported or invalid readings remain absent.
    pub fn utilization(&self, bus: u32, device: u32, function: u32) -> Option<f32> {
        let query = self.utilization?;
        let &(_, _, _, handle) = self
            .devices
            .iter()
            .find(|&&(b, d, f, _)| (b, d, f) == (bus, device, function))?;
        let mut value = Utilization::default();
        (startup_native(
            "nvml.utilization",
            pci_key(bus, device, function),
            0,
            || unsafe { query(handle, &mut value) },
        ) == SUCCESS
            && value.gpu <= 100)
            .then_some(value.gpu as f32)
    }

    /// Board power draw and the limit the driver enforces, in watts.
    pub fn power(&self, bus: u32, device: u32, function: u32) -> (Option<f32>, Option<f32>) {
        let Some(&(_, _, _, handle)) = self
            .devices
            .iter()
            .find(|&&(b, d, f, _)| (b, d, f) == (bus, device, function))
        else {
            return (None, None);
        };
        let read = |phase: &'static str, query: Query, allow_zero: bool| {
            let mut milliwatts = 0;
            (startup_native(phase, pci_key(bus, device, function), 0, || unsafe {
                query(handle, &mut milliwatts)
            }) == SUCCESS
                && (allow_zero || milliwatts > 0)
                && milliwatts <= 2_000_000)
                .then(|| milliwatts as f32 / 1000.0)
        };
        (
            read("nvml.power", self.power_usage, true),
            read("nvml.power-limit", self.power_limit, false),
        )
    }
}

fn pci_key(bus: u32, device: u32, function: u32) -> u64 {
    (u64::from(bus) << 32) | (u64::from(device) << 8) | u64::from(function)
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn unavailable(_: Device, _: *mut u32) -> Return {
        3
    }
    unsafe extern "C" fn sample(device: Device, out: *mut Utilization) -> Return {
        let value = device as usize;
        if value == 999 {
            return 3;
        }
        unsafe {
            (*out).gpu = value as u32;
        }
        SUCCESS
    }
    #[test]
    fn driver_utilization_is_optional_bounded_and_matched_by_pci_address() {
        let mut nvml = Nvml {
            power_usage: unavailable,
            power_limit: unavailable,
            utilization: Some(sample),
            devices: vec![(1, 0, 0, 100usize as Device)],
        };
        assert_eq!(nvml.utilization(1, 0, 0), Some(100.0));
        assert_eq!(nvml.utilization(2, 0, 0), None);
        nvml.devices[0].3 = std::ptr::null_mut();
        assert_eq!(nvml.utilization(1, 0, 0), Some(0.0));
        for value in [101usize, 999] {
            nvml.devices[0].3 = value as Device;
            assert_eq!(nvml.utilization(1, 0, 0), None);
        }
        nvml.utilization = None;
        assert_eq!(nvml.utilization(1, 0, 0), None);
    }
}
