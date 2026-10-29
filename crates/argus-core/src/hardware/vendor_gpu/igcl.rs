//! Read-only IGCL telemetry and power APIs (Intel's igcl_api.h). Temperature,
//! clocks and voltage do not require a power domain; no package-power guess.

mod telemetry;

use super::{GpuPowerSource, Library, Pci, Reading, bounded, energy_watts};
use crate::hardware::query::Retry;
use std::ffi::c_void;
use std::time::Instant;
use telemetry::Telemetry;
use windows::core::{s, w};

type Handle = *mut c_void;
type Enum = unsafe extern "C" fn(Handle, *mut u32, *mut Handle) -> i32;
type Close = unsafe extern "C" fn(Handle) -> i32;
type EnergyQuery = unsafe extern "C" fn(Handle, *mut Energy) -> i32;
type LimitsQuery = unsafe extern "C" fn(Handle, *mut Limits) -> i32;
type TelemetryQuery = unsafe extern "C" fn(Handle, *mut Telemetry) -> i32;

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
    handle: Handle,
    previous: Option<(u64, u64)>,
    energy_query: Retry,
    limits_query: Retry,
}
struct Device {
    pci: Pci,
    handle: Handle,
    domain: Option<Domain>,
    telemetry_query: Retry,
    telemetry_version: u8,
    previous: Option<(f64, f64)>,
}
#[derive(Default)]
struct Queries {
    energy: Option<EnergyQuery>,
    limits: Option<LimitsQuery>,
    telemetry: Option<TelemetryQuery>,
}
pub(super) struct Igcl {
    _library: Library,
    api: Handle,
    close: Close,
    queries: Queries,
    devices: Vec<Device>,
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
            let domains: Option<Enum> = library.export(s!("ctlEnumPowerDomains"));
            let pci: unsafe extern "C" fn(Handle, *mut Properties) -> i32 =
                library.export(s!("ctlPciGetProperties"))?;
            let queries = Queries {
                energy: library.export(s!("ctlPowerGetEnergyCounter")),
                limits: library.export(s!("ctlPowerGetLimits")),
                telemetry: library.export(s!("ctlPowerTelemetryGet")),
            };
            if queries.energy.is_none() && queries.telemetry.is_none() {
                return None;
            }
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
                queries,
                _library: library,
                devices: Vec::new(),
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
                // IGCL 1.0 does not identify subdomain scopes. Only use a single
                // domain; missing/ambiguous domains do not suppress telemetry.
                let domain = domains
                    .and_then(|query| enumerate(query, device))
                    .filter(|domains| domains.len() == 1)
                    .map(|domains| Domain {
                        handle: domains[0],
                        previous: None,
                        energy_query: Retry::default(),
                        limits_query: Retry::default(),
                    });
                let address = (
                    props.address.bus,
                    props.address.device,
                    props.address.function,
                );
                if result.devices.iter().any(|d| d.pci == address) {
                    continue;
                }
                result.devices.push(Device {
                    pci: address,
                    handle: device,
                    domain,
                    telemetry_query: Retry::default(),
                    telemetry_version: 1,
                    previous: None,
                });
            }
            Some(result)
        }
    }

    pub fn sample(&mut self) -> Vec<(Pci, Reading)> {
        let now = Instant::now();
        self.devices
            .iter_mut()
            .map(|device| (device.pci, self.queries.sample(device, now)))
            .collect()
    }
}

impl Queries {
    fn sample(&self, device: &mut Device, now: Instant) -> Reading {
        let telemetry = self.telemetry.and_then(|query| {
            device.telemetry_query.query(now, || {
                telemetry::query(&mut device.telemetry_version, |data| unsafe {
                    query(device.handle, data)
                })
            })
        });
        let mut reading = if let Some(data) = telemetry {
            data.reading(&mut device.previous)
        } else {
            device.previous = None;
            Reading::default()
        };
        if let Some(domain) = &mut device.domain {
            let energy = self.energy.and_then(|query| {
                domain.energy_query.query(now, || {
                    let mut energy = Energy {
                        header: Header::of::<Energy>(),
                        energy: 0,
                        timestamp: 0,
                    };
                    (unsafe { query(domain.handle, &mut energy) } == 0).then_some(energy)
                })
            });
            let watts = if let Some(energy) = energy {
                energy_watts(&mut domain.previous, energy.energy, energy.timestamp)
            } else {
                domain.previous = None;
                None
            };
            if let Some(watts) = watts {
                reading.watts = Some(watts);
                reading.source = Some(GpuPowerSource::Igcl);
                reading.limit = self.limits.and_then(|query| {
                    domain.limits_query.query(now, || {
                        let mut limits = Limits {
                            header: Header::of::<Limits>(),
                            sustained: Sustained::default(),
                            burst: Burst::default(),
                            ac: 0,
                            dc: 0,
                        };
                        if unsafe { query(domain.handle, &mut limits) } != 0
                            || limits.sustained.enabled == 0
                        {
                            return None;
                        }
                        bounded(limits.sustained.power as f64 / 1000.0, 0.001, 2000.0)
                    })
                });
            }
        }
        reading
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
    fn power_domains_remain_available_without_the_telemetry_export() {
        unsafe extern "C" fn energy(handle: Handle, output: *mut Energy) -> i32 {
            let counters = unsafe { &mut *handle.cast::<(u64, u64)>() };
            counters.0 += 25_000_000;
            counters.1 += 1_000_000;
            let output = unsafe { &mut *output };
            output.energy = counters.0;
            output.timestamp = counters.1;
            0
        }
        unsafe extern "C" fn limits(_: Handle, output: *mut Limits) -> i32 {
            let output = unsafe { &mut *output };
            output.sustained.enabled = 1;
            output.sustained.power = 75_000;
            0
        }
        let queries = Queries {
            energy: Some(energy),
            limits: Some(limits),
            telemetry: None,
        };
        let mut counters = (0u64, 0u64);
        let mut device = Device {
            pci: (0, 2, 0),
            handle: std::ptr::null_mut(),
            domain: Some(Domain {
                handle: (&raw mut counters).cast(),
                previous: None,
                energy_query: Retry::default(),
                limits_query: Retry::default(),
            }),
            telemetry_query: Retry::default(),
            telemetry_version: 1,
            previous: None,
        };
        let now = Instant::now();
        assert_eq!(queries.sample(&mut device, now).watts, None);
        let reading = queries.sample(&mut device, now);
        assert_eq!((reading.watts, reading.limit), (Some(25.0), Some(75.0)));
        assert!(matches!(reading.source, Some(GpuPowerSource::Igcl)));
        assert_eq!(reading.temperature, None);
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
