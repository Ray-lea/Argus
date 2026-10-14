//! Read-only ADL2 ABI from GPUOpen's display-library headers. PMLog covers
//! modern Radeon GPUs; Overdrive 5/6 supplies the older-driver fallback.

use super::{GpuPowerSource, Library, Pci, Reading, bounded};
use std::ffi::c_void;
use windows::Win32::System::Memory::{GetProcessHeap, HEAP_FLAGS, HeapAlloc};
use windows::core::{s, w};

type Context = *mut c_void;
type Destroy = unsafe extern "C" fn(Context) -> i32;
type PmLog = unsafe extern "C" fn(Context, i32, *mut Log) -> i32;
type Power = unsafe extern "C" fn(Context, i32, i32, *mut i32) -> i32;
type Temperature = unsafe extern "C" fn(Context, i32, i32, *mut Temp) -> i32;
type Current = unsafe extern "C" fn(Context, i32, *mut Activity) -> i32;

#[repr(C)]
struct Adapter {
    size: i32,
    index: i32,
    udid: [u8; 256],
    bus: i32,
    device: i32,
    function: i32,
    vendor: i32,
    name: [u8; 256],
    display: [u8; 256],
    present: i32,
    exists: i32,
    driver: [u8; 256],
    driver_ext: [u8; 256],
    pnp: [u8; 256],
    display_index: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Sensor {
    supported: i32,
    value: i32,
}
#[repr(C)]
struct Log {
    size: i32,
    sensors: [Sensor; 256],
}
#[repr(C)]
struct Temp {
    size: i32,
    millidegrees: i32,
}
#[repr(C)]
struct Activity {
    size: i32,
    clock: i32,
    memory: i32,
    voltage: i32,
    rest: [i32; 6],
}

pub(super) struct Adl {
    _library: Library,
    context: Context,
    destroy: Destroy,
    log: Option<PmLog>,
    power: Option<Power>,
    temperature: Option<Temperature>,
    activity: Option<Current>,
    adapters: Vec<(Pci, i32)>,
}

unsafe extern "C" fn allocate(size: i32) -> *mut c_void {
    if !(1..=16_777_216).contains(&size) {
        return std::ptr::null_mut();
    }
    unsafe {
        GetProcessHeap()
            .map(|h| HeapAlloc(h, HEAP_FLAGS(0), size as usize))
            .unwrap_or_default()
    }
}

impl Adl {
    pub fn load() -> Option<Self> {
        let library = Library::load(w!("atiadlxx.dll"))?;
        unsafe {
            let create: unsafe extern "C" fn(
                unsafe extern "C" fn(i32) -> *mut c_void,
                i32,
                *mut Context,
            ) -> i32 = library.export(s!("ADL2_Main_Control_Create"))?;
            let destroy: Destroy = library.export(s!("ADL2_Main_Control_Destroy"))?;
            let count: unsafe extern "C" fn(Context, *mut i32) -> i32 =
                library.export(s!("ADL2_Adapter_NumberOfAdapters_Get"))?;
            let info: unsafe extern "C" fn(Context, *mut Adapter, i32) -> i32 =
                library.export(s!("ADL2_Adapter_AdapterInfo_Get"))?;
            let mut context = std::ptr::null_mut();
            if create(allocate, 1, &mut context) != 0 || context.is_null() {
                return None;
            }
            let mut result = Self {
                context,
                destroy,
                log: library.export(s!("ADL2_New_QueryPMLogData_Get")),
                power: library.export(s!("ADL2_Overdrive6_CurrentPower_Get")),
                temperature: library.export(s!("ADL2_Overdrive5_Temperature_Get")),
                activity: library.export(s!("ADL2_Overdrive5_CurrentActivity_Get")),
                _library: library,
                adapters: Vec::new(),
            };
            let mut n = 0;
            if count(context, &mut n) != 0 || !(1..=64).contains(&n) {
                return None;
            }
            let mut adapters: Vec<Adapter> = (0..n).map(|_| std::mem::zeroed()).collect();
            for adapter in &mut adapters {
                adapter.size = size_of::<Adapter>() as i32;
            }
            if info(
                context,
                adapters.as_mut_ptr(),
                (adapters.len() * size_of::<Adapter>()) as i32,
            ) != 0
            {
                return None;
            }
            for adapter in adapters {
                if adapter.present == 0
                    || !matches!(adapter.vendor, 1002 | 0x1002)
                    || !(0..=255).contains(&adapter.bus)
                    || !(0..=31).contains(&adapter.device)
                    || !(0..=7).contains(&adapter.function)
                {
                    continue;
                }
                let pci = (
                    adapter.bus as u32,
                    adapter.device as u32,
                    adapter.function as u32,
                );
                // ADL can expose two display indices for the same physical GPU.
                if !result.adapters.iter().any(|(p, _)| *p == pci) {
                    result.adapters.push((pci, adapter.index));
                }
            }
            Some(result)
        }
    }

    pub fn sample(&mut self) -> Vec<(Pci, Reading)> {
        self.adapters
            .iter()
            .map(|&(pci, index)| {
                let mut reading = Reading::default();
                if let Some(query) = self.log {
                    let mut log = Log {
                        size: size_of::<Log>() as i32,
                        sensors: [Sensor::default(); 256],
                    };
                    if unsafe { query(self.context, index, &mut log) } == 0 {
                        reading = decode(&log);
                    }
                }
                if reading.watts.is_none()
                    && let Some(query) = self.power
                {
                    let mut power = 0;
                    // OD6 TOTAL_POWER is ASIC power in Q8 watts, not board power.
                    if unsafe { query(self.context, index, 0, &mut power) } == 0 {
                        reading.watts = bounded(power as f64 / 256.0, 0.0, 2000.0);
                        reading.source = reading.watts.map(|_| GpuPowerSource::AdlChip);
                    }
                }
                if reading.temperature.is_none()
                    && let Some(query) = self.temperature
                {
                    let mut temp = Temp {
                        size: size_of::<Temp>() as i32,
                        millidegrees: 0,
                    };
                    if unsafe { query(self.context, index, 0, &mut temp) } == 0 {
                        reading.temperature =
                            bounded(temp.millidegrees as f64 / 1000.0, -20.0, 150.0);
                    }
                }
                if let Some(query) = self.activity {
                    let mut activity = Activity {
                        size: size_of::<Activity>() as i32,
                        clock: 0,
                        memory: 0,
                        voltage: 0,
                        rest: [0; 6],
                    };
                    if unsafe { query(self.context, index, &mut activity) } == 0 {
                        reading.clock = reading.clock.or_else(|| {
                            bounded(activity.clock as f64 / 100.0, 1.0, 10_000.0).map(|v| v as u32)
                        });
                        reading.memory_clock = reading.memory_clock.or_else(|| {
                            bounded(activity.memory as f64 / 100.0, 1.0, 20_000.0).map(|v| v as u32)
                        });
                        reading.voltage = reading.voltage.or_else(|| {
                            bounded(activity.voltage as f64, 1.0, 3000.0).map(|v| v as u32)
                        });
                    }
                }
                (pci, reading)
            })
            .collect()
    }
}

fn decode(log: &Log) -> Reading {
    let read = |id: usize, low, high| {
        let sensor = log.sensors[id];
        (sensor.supported != 0)
            .then(|| bounded(sensor.value as f64, low, high))
            .flatten()
    };
    let board = read(73, 0.0, 2000.0);
    let watts = board.or_else(|| read(23, 0.0, 2000.0));
    Reading {
        watts,
        source: watts.map(|_| {
            if board.is_some() {
                GpuPowerSource::AdlBoard
            } else {
                GpuPowerSource::AdlChip
            }
        }),
        temperature: read(8, -20.0, 150.0),
        clock: read(1, 1.0, 10_000.0).map(|v| v as u32),
        memory_clock: read(2, 1.0, 20_000.0).map(|v| v as u32),
        voltage: read(21, 1.0, 3000.0).map(|v| v as u32),
        ..Reading::default()
    }
}

impl Drop for Adl {
    fn drop(&mut self) {
        unsafe {
            (self.destroy)(self.context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_board_from_chip_and_missing_from_zero() {
        assert_eq!(size_of::<Adapter>(), 1572);
        assert_eq!(size_of::<Log>(), 2052);
        let mut log = Log {
            size: 2052,
            sensors: [Sensor::default(); 256],
        };
        assert!(decode(&log).watts.is_none());
        log.sensors[23] = Sensor {
            supported: 1,
            value: 23,
        };
        assert_eq!(decode(&log).watts, Some(23.0));
        assert!(matches!(decode(&log).source, Some(GpuPowerSource::AdlChip)));
        log.sensors[73] = Sensor {
            supported: 1,
            value: 0,
        };
        assert_eq!(decode(&log).watts, Some(0.0));
        assert!(matches!(
            decode(&log).source,
            Some(GpuPowerSource::AdlBoard)
        ));
        log.sensors[73].value = -1;
        assert_eq!(decode(&log).watts, Some(23.0));
    }
}
