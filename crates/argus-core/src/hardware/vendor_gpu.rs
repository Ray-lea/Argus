//! Optional vendor telemetry. SDK calls run on one bounded worker; the sampler
//! only reads a fresh cache. All libraries come from System32, all exports are
//! read-only, and adapters are matched by the full PCI bus/device/function.

mod adl;
mod igcl;

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{PCSTR, PCWSTR};

pub(super) type Pci = (u32, u32, u32);

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GpuPowerSource {
    Nvml,
    AdlBoard,
    AdlChip,
    Igcl,
    IgclChip,
    Rapl,
}

#[derive(Clone, Default)]
pub(super) struct Reading {
    pub watts: Option<f32>,
    pub limit: Option<f32>,
    pub source: Option<GpuPowerSource>,
    pub temperature: Option<f32>,
    pub clock: Option<u32>,
    pub memory_clock: Option<u32>,
    pub voltage: Option<u32>,
}

type Cache = Arc<Mutex<Option<(Instant, Vec<(Pci, Reading)>)>>>;

pub(super) struct VendorGpu {
    tx: mpsc::SyncSender<()>,
    cache: Cache,
}

impl VendorGpu {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let cache: Cache = Arc::default();
        let output = cache.clone();
        let _ = std::thread::Builder::new()
            .name("argus-gpu-sensors".into())
            .spawn(move || {
                let mut amd = adl::Adl::load();
                let mut intel = igcl::Igcl::load();
                let mut last: Option<Instant> = None;
                while rx.recv().is_ok() {
                    if last.is_some_and(|at| at.elapsed() < Duration::from_millis(500)) {
                        continue;
                    }
                    let started = Instant::now();
                    last = Some(started);
                    let mut readings = Vec::new();
                    if let Some(amd) = &mut amd {
                        readings.extend(amd.sample());
                    }
                    if let Some(intel) = &mut intel {
                        readings.extend(intel.sample());
                    }
                    if let Ok(mut cache) = output.lock() {
                        *cache = Some((started, readings));
                    }
                }
            });
        Self { tx, cache }
    }

    pub fn sample(&self, interval: Duration) -> Vec<(Pci, Reading)> {
        let _ = self.tx.try_send(());
        self.cache
            .try_lock()
            .ok()
            .and_then(|cache| {
                cache.as_ref().and_then(|(at, values)| {
                    (at.elapsed()
                        < interval.clamp(Duration::from_secs(1), Duration::from_secs(5)) * 3)
                        .then(|| values.clone())
                })
            })
            .unwrap_or_default()
    }
}

struct Library(HMODULE);

impl Library {
    fn load(name: PCWSTR) -> Option<Self> {
        unsafe { LoadLibraryExW(name, None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .ok()
            .map(Self)
    }

    /// Keeps the DLL in the process until it exits, whatever `FreeLibrary`
    /// does later, for libraries that leave callbacks registered behind them.
    fn pin(&self) {
        let mut module = HMODULE::default();
        unsafe {
            let _ = GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                PCWSTR(self.0.0 as *const u16),
                &mut module,
            );
        }
    }

    unsafe fn export<F: Copy>(&self, name: PCSTR) -> Option<F> {
        let function = unsafe { GetProcAddress(self.0, name) }?;
        assert_eq!(size_of::<F>(), size_of_val(&function));
        Some(unsafe { std::mem::transmute_copy(&function) })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

fn bounded(value: f64, min: f64, max: f64) -> Option<f32> {
    (value.is_finite() && (min..=max).contains(&value)).then_some(value as f32)
}

/// A reset, duplicate timestamp or discontinuity invalidates one interval.
fn energy_watts(previous: &mut Option<(u64, u64)>, energy: u64, time: u64) -> Option<f32> {
    let before = previous.replace((energy, time))?;
    let elapsed = time.checked_sub(before.1)?;
    if !(1_000..=30_000_000).contains(&elapsed) {
        return None;
    }
    bounded(
        energy.checked_sub(before.0)? as f64 / elapsed as f64,
        0.0,
        2000.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_deltas_use_device_time_and_discard_resets() {
        let mut previous = None;
        assert_eq!(energy_watts(&mut previous, 1_000_000, 1_000_000), None);
        assert_eq!(
            energy_watts(&mut previous, 26_000_000, 2_000_000),
            Some(25.0)
        );
        assert_eq!(
            energy_watts(&mut previous, 26_000_000, 3_000_000),
            Some(0.0)
        );
        assert_eq!(energy_watts(&mut previous, 26_000_000, 3_000_000), None);
        assert_eq!(energy_watts(&mut previous, 0, 4_000_000), None);
        assert_eq!(
            energy_watts(&mut previous, 30_000_000, 5_000_000),
            Some(30.0)
        );
        assert_eq!(energy_watts(&mut previous, 31_000_000, 40_000_000), None);
    }
}
