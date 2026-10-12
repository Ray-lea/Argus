//! Physical disks: identity via storage IOCTLs, activity via IOCTL_DISK_PERFORMANCE.
//! Every IOCTL used here is FILE_ANY_ACCESS, so no elevation is needed.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    BusTypeNvme, BusTypeSata, BusTypeSd, BusTypeUsb, CreateFileW, FILE_FLAGS_AND_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, GetDriveTypeW, GetLogicalDrives, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    DEVICE_SEEK_PENALTY_DESCRIPTOR, DISK_GEOMETRY_EX, DISK_PERFORMANCE,
    IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_DISK_PERFORMANCE, IOCTL_STORAGE_QUERY_PROPERTY,
    PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
    STORAGE_TEMPERATURE_DATA_DESCRIPTOR, StorageDeviceProperty, StorageDeviceSeekPenaltyProperty,
    StorageDeviceTemperatureProperty, VOLUME_DISK_EXTENTS,
};
use windows::core::PCWSTR;

use crate::util::{AlignedBuf, OwnedHandle, wide};

const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x0056_0000;
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const RESCAN_EVERY: Duration = Duration::from_secs(15);
/// The drive reads its sensor for every query; temperature moves slowly anyway.
const TEMPERATURE_EVERY: Duration = Duration::from_secs(5);
const MAX_DISKS: u32 = 32;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub index: u32,
    /// Empty when the drive does not report one.
    pub model: String,
    pub size: u64,
    /// `nvme` | `sata` | `usb` | `sd` | `other`
    pub bus: &'static str,
    /// False for spinning disks; None when the drive does not say.
    pub ssd: Option<bool>,
    pub removable: bool,
    /// Drive letters on this disk, e.g. `["C:", "D:"]`.
    pub volumes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskStats {
    pub info: Arc<DiskInfo>,
    /// Percent of time the disk was busy, 0–100.
    pub active: f32,
    /// Bytes per second.
    pub read: u64,
    pub write: u64,
    /// Average time per transfer, ms.
    pub response_ms: f32,
    /// °C, for drives that report it (most NVMe and SATA SSDs).
    pub temperature: Option<f32>,
    /// The drive's own warning threshold, °C.
    pub temperature_warning: Option<f32>,
}

struct Disk {
    handle: OwnedHandle,
    info: Arc<DiskInfo>,
    prev: Option<DISK_PERFORMANCE>,
    /// Last reading as (°C, warning °C); None once the drive declines.
    temperature: Option<(f32, Option<f32>)>,
    temperature_at: Option<Instant>,
    temperature_supported: bool,
}

pub(crate) struct DiskTracker {
    disks: Vec<Disk>,
    scanned_at: Instant,
}

impl DiskTracker {
    pub fn new() -> Self {
        let mut t = Self {
            disks: Vec::new(),
            scanned_at: Instant::now(),
        };
        t.rescan();
        t
    }

    fn rescan(&mut self) {
        let volumes = volume_map();
        let mut old: HashMap<u32, Disk> = self.disks.drain(..).map(|d| (d.info.index, d)).collect();
        for index in 0..MAX_DISKS {
            let Some(handle) = open(&format!(r"\\.\PhysicalDrive{index}")) else {
                continue;
            };
            let vols = volumes.get(&index).cloned().unwrap_or_default();
            // Keep counters (and identity) of disks we already know.
            if let Some(mut d) = old.remove(&index) {
                if d.info.volumes != vols {
                    let mut info = DiskInfo {
                        volumes: vols,
                        ..clone_info(&d.info)
                    };
                    info.volumes.sort();
                    d.info = Arc::new(info);
                }
                self.disks.push(d);
                continue;
            }
            let info = describe(handle.0, index, vols);
            self.disks.push(Disk {
                handle,
                info: Arc::new(info),
                prev: None,
                temperature: None,
                temperature_at: None,
                temperature_supported: true,
            });
        }
        self.scanned_at = Instant::now();
    }

    pub fn sample(&mut self) -> Vec<DiskStats> {
        if self.scanned_at.elapsed() > RESCAN_EVERY {
            self.rescan();
        }
        let mut out = Vec::with_capacity(self.disks.len());
        for d in &mut self.disks {
            let Some(now) = performance(d.handle.0) else {
                continue;
            };
            if d.temperature_supported
                && d.temperature_at
                    .is_none_or(|t| t.elapsed() > TEMPERATURE_EVERY)
            {
                d.temperature = temperature(d.handle.0);
                d.temperature_supported = d.temperature.is_some();
                d.temperature_at = Some(Instant::now());
            }
            let mut stats = DiskStats {
                info: d.info.clone(),
                active: 0.0,
                read: 0,
                write: 0,
                response_ms: 0.0,
                temperature: d.temperature.map(|t| t.0),
                temperature_warning: d.temperature.and_then(|t| t.1),
            };
            if let Some(prev) = d.prev {
                let elapsed = (now.QueryTime - prev.QueryTime) as f64; // 100 ns ticks
                if elapsed > 0.0 {
                    let idle = (now.IdleTime - prev.IdleTime) as f64;
                    stats.active = ((1.0 - idle / elapsed) * 100.0).clamp(0.0, 100.0) as f32;
                    let secs = elapsed / 1e7;
                    stats.read = ((now.BytesRead - prev.BytesRead).max(0) as f64 / secs) as u64;
                    stats.write =
                        ((now.BytesWritten - prev.BytesWritten).max(0) as f64 / secs) as u64;
                    let transfers = now.ReadCount.wrapping_sub(prev.ReadCount) as u64
                        + now.WriteCount.wrapping_sub(prev.WriteCount) as u64;
                    let busy =
                        ((now.ReadTime - prev.ReadTime) + (now.WriteTime - prev.WriteTime)).max(0);
                    if transfers > 0 {
                        stats.response_ms = (busy as f64 / transfers as f64 / 10_000.0) as f32;
                    }
                }
            }
            d.prev = Some(now);
            out.push(stats);
        }
        out
    }
}

fn clone_info(i: &DiskInfo) -> DiskInfo {
    DiskInfo {
        index: i.index,
        model: i.model.clone(),
        size: i.size,
        bus: i.bus,
        ssd: i.ssd,
        removable: i.removable,
        volumes: i.volumes.clone(),
    }
}

/// Opens a device for IOCTLs only (no read/write access, so no admin needed).
fn open(path: &str) -> Option<OwnedHandle> {
    let w = wide(path);
    unsafe {
        CreateFileW(
            PCWSTR(w.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .ok()
    .map(OwnedHandle)
}

fn ioctl(handle: HANDLE, code: u32, input: Option<&[u8]>, out: &mut AlignedBuf) -> Option<u32> {
    let mut returned = 0u32;
    unsafe {
        DeviceIoControl(
            handle,
            code,
            input.map(|i| i.as_ptr().cast()),
            input.map_or(0, |i| i.len() as u32),
            Some(out.as_mut_ptr()),
            out.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()
    .map(|_| returned)
}

fn performance(handle: HANDLE) -> Option<DISK_PERFORMANCE> {
    let mut buf = AlignedBuf::with_capacity(std::mem::size_of::<DISK_PERFORMANCE>());
    ioctl(handle, IOCTL_DISK_PERFORMANCE, None, &mut buf)?;
    Some(unsafe { std::ptr::read(buf.as_ptr() as *const DISK_PERFORMANCE) })
}

fn query_property(
    handle: HANDLE,
    id: windows::Win32::System::Ioctl::STORAGE_PROPERTY_ID,
) -> Option<AlignedBuf> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: id,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    let input = unsafe {
        std::slice::from_raw_parts(
            (&query as *const STORAGE_PROPERTY_QUERY).cast::<u8>(),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>(),
        )
    };
    let mut buf = AlignedBuf::with_capacity(1024);
    ioctl(handle, IOCTL_STORAGE_QUERY_PROPERTY, Some(input), &mut buf)?;
    Some(buf)
}

/// The drive's composite temperature and warning threshold (°C).
fn temperature(handle: HANDLE) -> Option<(f32, Option<f32>)> {
    let buf = query_property(handle, StorageDeviceTemperatureProperty)?;
    // SAFETY: the buffer holds the descriptor the IOCTL wrote (1 KB, aligned).
    let d = unsafe { &*(buf.as_ptr() as *const STORAGE_TEMPERATURE_DATA_DESCRIPTOR) };
    if d.InfoCount == 0 {
        return None;
    }
    let t = d.TemperatureInfo[0].Temperature;
    // Unreported values come back as 0 or wildly out of range.
    (t > 0 && t < 120).then(|| {
        (
            t as f32,
            (d.WarningTemperature > 0).then_some(d.WarningTemperature as f32),
        )
    })
}

fn describe(handle: HANDLE, index: u32, mut volumes: Vec<String>) -> DiskInfo {
    volumes.sort();
    let mut info = DiskInfo {
        index,
        model: String::new(),
        size: 0,
        bus: "other",
        ssd: None,
        removable: false,
        volumes,
    };

    if let Some(buf) = query_property(handle, StorageDeviceProperty) {
        let desc = unsafe { &*(buf.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
        let raw = unsafe { std::slice::from_raw_parts(buf.as_ptr(), buf.len()) };
        let string_at = |offset: u32| -> String {
            let o = offset as usize;
            if o == 0 || o >= raw.len() {
                return String::new();
            }
            let end = raw[o..]
                .iter()
                .position(|&b| b == 0)
                .map_or(raw.len(), |p| o + p);
            String::from_utf8_lossy(&raw[o..end]).trim().to_owned()
        };
        let vendor = string_at(desc.VendorIdOffset);
        let product = string_at(desc.ProductIdOffset);
        let model = if vendor.is_empty() || product.starts_with(&vendor) {
            product
        } else {
            format!("{vendor} {product}")
        };
        if !model.is_empty() {
            info.model = model;
        }
        info.removable = desc.RemovableMedia;
        info.bus = match desc.BusType {
            b if b == BusTypeNvme => "nvme",
            b if b == BusTypeSata => "sata",
            b if b == BusTypeUsb => "usb",
            b if b == BusTypeSd => "sd",
            _ => "other",
        };
    }

    if let Some(buf) = query_property(handle, StorageDeviceSeekPenaltyProperty) {
        let desc = unsafe { &*(buf.as_ptr() as *const DEVICE_SEEK_PENALTY_DESCRIPTOR) };
        info.ssd = Some(!desc.IncursSeekPenalty);
    }
    if info.bus == "nvme" {
        info.ssd = Some(true);
    }

    let mut geo = AlignedBuf::with_capacity(256);
    if ioctl(handle, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, None, &mut geo).is_some() {
        let g = unsafe { &*(geo.as_ptr() as *const DISK_GEOMETRY_EX) };
        info.size = g.DiskSize.max(0) as u64;
    }
    info
}

/// Maps physical disk number to the drive letters that live on it.
fn volume_map() -> HashMap<u32, Vec<String>> {
    let mut map: HashMap<u32, Vec<String>> = HashMap::new();
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u8 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = format!("{}:", (b'A' + i) as char);
        // Only local disks: probing optical or network drives can block.
        let root = wide(&format!(r"{letter}\"));
        let kind = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
        if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE {
            continue;
        }
        let Some(h) = open(&format!(r"\\.\{letter}")) else {
            continue;
        };
        let mut buf = AlignedBuf::with_capacity(512);
        if ioctl(h.0, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, None, &mut buf).is_none() {
            continue;
        }
        let ext = unsafe { &*(buf.as_ptr() as *const VOLUME_DISK_EXTENTS) };
        let n = ext.NumberOfDiskExtents as usize;
        let extents = unsafe { std::slice::from_raw_parts(ext.Extents.as_ptr(), n.min(16)) };
        for e in extents {
            let list = map.entry(e.DiskNumber).or_default();
            if !list.contains(&letter) {
                list.push(letter.clone());
            }
        }
    }
    map
}
