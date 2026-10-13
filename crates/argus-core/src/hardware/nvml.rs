//! Board power of NVIDIA GPUs, in watts, from NVML (`nvml.dll`, which the
//! driver installs in System32). WDDM's adapter perf data leaves power empty
//! on NVIDIA cards and only ever gives it as a share of the budget. Loaded from
//! System32 only, never from the application or working directory; without an
//! NVIDIA driver it is simply absent. NVML needs no administrator rights.

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

pub(crate) struct Nvml {
    power_usage: Query,
    power_limit: Query,
    /// PCI bus and device number of each GPU NVML manages.
    devices: Vec<(u32, u32, Device)>,
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
        let module =
            unsafe { LoadLibraryExW(w!("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()?;
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
            // Initialized for the rest of the process; NVML is never shut down.
            if init() != SUCCESS {
                return None;
            }
            let mut n = 0;
            if count(&mut n) != SUCCESS {
                return None;
            }
            let devices = (0..n)
                .filter_map(|i| {
                    let mut device = std::ptr::null_mut();
                    let mut pci = PciInfo::default();
                    (by_index(i, &mut device) == SUCCESS && pci_info(device, &mut pci) == SUCCESS)
                        .then_some((pci.bus, pci.device, device))
                })
                .collect::<Vec<_>>();
            (!devices.is_empty()).then_some(Self {
                power_usage,
                power_limit,
                devices,
            })
        }
    }

    /// Board power draw and the limit the driver enforces, in watts.
    pub fn power(&self, bus: u32, device: u32) -> (Option<f32>, Option<f32>) {
        let Some(&(_, _, handle)) = self
            .devices
            .iter()
            .find(|&&(b, d, _)| (b, d) == (bus, device))
        else {
            return (None, None);
        };
        let read = |query: Query| {
            let mut milliwatts = 0;
            (unsafe { query(handle, &mut milliwatts) } == SUCCESS && milliwatts > 0)
                .then(|| milliwatts as f32 / 1000.0)
        };
        (read(self.power_usage), read(self.power_limit))
    }
}
