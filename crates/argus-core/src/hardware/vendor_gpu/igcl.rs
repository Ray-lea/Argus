//! Read-only IGCL 1.0 power API (Intel's igcl_api.h). Integrated/older GPUs
//! without a power domain keep their WDDM telemetry; no package-power guess.

use super::{GpuPowerSource, Library, Pci, Reading, bounded, energy_watts};
use std::ffi::c_void;
use windows::core::{s, w};

type Handle = *mut c_void;
type Enum = unsafe extern "C" fn(Handle, *mut u32, *mut Handle) -> i32;
type Close = unsafe extern "C" fn(Handle) -> i32;
type EnergyQuery = unsafe extern "C" fn(Handle, *mut Energy) -> i32;
type LimitsQuery = unsafe extern "C" fn(Handle, *mut Limits) -> i32;

#[repr(C)]
#[derive(Default)]
struct Header {
    size: u32,
    version: u8,
}
impl Header {
    fn of<T>() -> Self {
        Self {
            size: size_of::<T>() as u32,
            version: 0,
        }
    }
}
#[repr(C)]
struct Init {
    header: Header,
    app_version: u32,
    flags: u32,
    supported: u32,
    uid: [u32; 4],
}
#[repr(C)]
#[derive(Default)]
struct Address {
    header: Header,
    domain: u32,
    bus: u32,
    device: u32,
    function: u32,
}
#[repr(C)]
#[derive(Default)]
struct Speed {
    header: Header,
    generation: i32,
    width: i32,
    bandwidth: i64,
}
#[repr(C)]
struct Properties {
    header: Header,
    address: Address,
    speed: Speed,
    bar_supported: u8,
    bar_enabled: u8,
}
#[repr(C)]
struct Energy {
    header: Header,
    energy: u64,
    timestamp: u64,
}
#[repr(C)]
#[derive(Default)]
struct Sustained {
    enabled: u8,
    power: i32,
    interval: i32,
}
#[repr(C)]
#[derive(Default)]
struct Burst {
    enabled: u8,
    power: i32,
}
#[repr(C)]
struct Limits {
    header: Header,
    sustained: Sustained,
    burst: Burst,
    ac: i32,
    dc: i32,
}

struct Domain {
    pci: Pci,
    handle: Handle,
    previous: Option<(u64, u64)>,
}
pub(super) struct Igcl {
    _library: Library,
    api: Handle,
    close: Close,
    energy: EnergyQuery,
    limits: Option<LimitsQuery>,
    domains: Vec<Domain>,
}

unsafe fn enumerate(query: Enum, parent: Handle) -> Option<Vec<Handle>> {
    let mut count = 0;
    if unsafe { query(parent, &mut count, std::ptr::null_mut()) } != 0 || !(1..=64).contains(&count)
    {
        return None;
    }
    let mut handles = vec![std::ptr::null_mut(); count as usize];
    if unsafe { query(parent, &mut count, handles.as_mut_ptr()) } != 0
        || count as usize > handles.len()
    {
        return None;
    }
    handles.truncate(count as usize);
    handles.retain(|h| !h.is_null());
    Some(handles)
}

impl Igcl {
    pub fn load() -> Option<Self> {
        let library = Library::load(w!("ControlLib.dll"))?;
        // ControlLib registers an ETW provider that neither ctlClose nor its
        // unload unregisters. Once the DLL is gone, the next session to enable
        // the provider calls into freed code from a thread-pool thread, which
        // happened minutes to hours after a failed load on a machine with the
        // DLL but no Intel GPU. Pinned, it stays for the process's lifetime.
        library.pin();
        unsafe {
            let init: unsafe extern "C" fn(*mut Init, *mut Handle) -> i32 =
                library.export(s!("ctlInit"))?;
            let close: Close = library.export(s!("ctlClose"))?;
            let devices: Enum = library.export(s!("ctlEnumerateDevices"))?;
            let domains: Enum = library.export(s!("ctlEnumPowerDomains"))?;
            let pci: unsafe extern "C" fn(Handle, *mut Properties) -> i32 =
                library.export(s!("ctlPciGetProperties"))?;
            let energy: EnergyQuery = library.export(s!("ctlPowerGetEnergyCounter"))?;
            let mut args = Init {
                header: Header::of::<Init>(),
                app_version: 1 << 16,
                flags: 1,
                supported: 0,
                uid: [0; 4],
            };
            let mut api = std::ptr::null_mut();
            if init(&mut args, &mut api) != 0 || api.is_null() {
                return None;
            }
            let mut result = Self {
                api,
                close,
                energy,
                limits: library.export(s!("ctlPowerGetLimits")),
                _library: library,
                domains: Vec::new(),
            };
            for device in enumerate(devices, api)? {
                let mut props = Properties {
                    header: Header::of::<Properties>(),
                    address: Address {
                        header: Header::of::<Address>(),
                        ..Default::default()
                    },
                    speed: Speed {
                        header: Header::of::<Speed>(),
                        ..Default::default()
                    },
                    bar_supported: 0,
                    bar_enabled: 0,
                };
                if pci(device, &mut props) != 0 || props.address.domain != 0 {
                    continue;
                }
                let Some(domains) = enumerate(domains, device) else {
                    continue;
                };
                // IGCL 1.0 does not identify subdomain scopes. Only use a single
                // domain; summing unknown domains could double-count power.
                if domains.len() != 1 {
                    continue;
                }
                let address = (
                    props.address.bus,
                    props.address.device,
                    props.address.function,
                );
                if result.domains.iter().any(|d| d.pci == address) {
                    continue;
                }
                result.domains.push(Domain {
                    pci: address,
                    handle: domains[0],
                    previous: None,
                });
            }
            Some(result)
        }
    }

    pub fn sample(&mut self) -> Vec<(Pci, Reading)> {
        self.domains
            .iter_mut()
            .map(|domain| {
                let mut energy = Energy {
                    header: Header::of::<Energy>(),
                    energy: 0,
                    timestamp: 0,
                };
                let watts = if unsafe { (self.energy)(domain.handle, &mut energy) } == 0 {
                    energy_watts(&mut domain.previous, energy.energy, energy.timestamp)
                } else {
                    domain.previous = None;
                    None
                };
                let mut limits = Limits {
                    header: Header::of::<Limits>(),
                    sustained: Sustained::default(),
                    burst: Burst::default(),
                    ac: 0,
                    dc: 0,
                };
                let limit = self.limits.and_then(|query| {
                    (unsafe { query(domain.handle, &mut limits) } == 0
                        && limits.sustained.enabled != 0)
                        .then(|| bounded(limits.sustained.power as f64 / 1000.0, 0.001, 2000.0))
                        .flatten()
                });
                (
                    domain.pci,
                    Reading {
                        watts,
                        limit,
                        source: watts.map(|_| GpuPowerSource::Igcl),
                        ..Reading::default()
                    },
                )
            })
            .collect()
    }
}

impl Drop for Igcl {
    fn drop(&mut self) {
        unsafe {
            (self.close)(self.api);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ffi_matches_x64_sdk_layout() {
        assert_eq!(size_of::<Init>(), 36);
        assert_eq!(size_of::<Properties>(), 64);
        assert_eq!(size_of::<Energy>(), 24);
        assert_eq!(std::mem::offset_of!(Energy, energy), 8);
        assert_eq!(size_of::<Limits>(), 36);
    }

    #[test]
    fn control_lib_stays_loaded() {
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        // Holds a reference while the SDK is used, so that dropping it shows
        // whether anything else keeps the DLL; returns where it is missing.
        let Some(probe) = Library::load(w!("ControlLib.dll")) else {
            return;
        };
        drop(Igcl::load());
        drop(probe);
        assert!(unsafe { GetModuleHandleW(w!("ControlLib.dll")) }.is_ok());
    }
}
