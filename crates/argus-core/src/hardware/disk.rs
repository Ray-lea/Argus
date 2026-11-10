//! Physical disks: identity via storage IOCTLs, activity via IOCTL_DISK_PERFORMANCE.
//! Device presence is independent of performance counter support.
mod access;
mod discovery;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    DEVICE_SEEK_PENALTY_DESCRIPTOR, DISK_GEOMETRY_EX, DISK_PERFORMANCE,
    IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_DISK_PERFORMANCE, IOCTL_STORAGE_QUERY_PROPERTY,
    PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
    STORAGE_TEMPERATURE_DATA_DESCRIPTOR, StorageDeviceProperty, StorageDeviceSeekPenaltyProperty,
    StorageDeviceTemperatureProperty, VOLUME_DISK_EXTENTS,
};
use windows::core::PCWSTR;

use crate::util::{AlignedBuf, wide};
use access::DeviceHandle;

const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x0056_0000;

pub(super) fn bus_name(code: u32) -> &'static str {
    match code {
        1 => "scsi",
        3 => "ata",
        7 => "usb",
        8 => "raid",
        10 => "sas",
        11 => "sata",
        12 => "sd",
        13 => "mmc",
        14 | 15 => "virtual",
        16 => "spaces",
        17 => "nvme",
        _ => "other",
    }
}
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const RESCAN_EVERY: Duration = Duration::from_secs(15);
/// The drive reads its sensor for every query; temperature moves slowly anyway.
const TEMPERATURE_EVERY: Duration = Duration::from_secs(5);
const LEGACY_PROBE_DISKS: u32 = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// Percent of time the disk was busy, 0–100; unknown without a valid interval.
    pub active: Option<f32>,
    /// Bytes per second; unavailable counters remain unknown.
    pub read: Option<u64>,
    pub write: Option<u64>,
    /// Average time per transfer, ms.
    pub response_ms: Option<f32>,
    /// °C, for drives that report it (most NVMe and SATA SSDs).
    pub temperature: Option<f32>,
    /// The drive's own warning threshold, °C.
    pub temperature_warning: Option<f32>,
}

struct Disk {
    handle: Option<DeviceHandle>,
    generation: u64,
    info: Arc<DiskInfo>,
    prev: Option<DISK_PERFORMANCE>,
    /// Last reading as (°C, warning °C); None once the drive declines.
    temperature: Option<(f32, Option<f32>)>,
    temperature_at: Option<Instant>,
    temperature_supported: bool,
}

impl Disk {
    fn sample_with(
        &mut self,
        read: impl FnOnce(HANDLE) -> Result<DISK_PERFORMANCE, u32>,
    ) -> Option<DISK_PERFORMANCE> {
        let handle = self.handle.as_ref()?;
        let result = handle.read(|raw, generation| {
            if self.generation != generation {
                self.generation = generation;
                self.prev = None;
                self.temperature_at = None;
                self.temperature_supported = true;
            }
            if self.temperature_supported
                && self
                    .temperature_at
                    .is_none_or(|t| t.elapsed() > TEMPERATURE_EVERY)
            {
                self.temperature = temperature(raw);
                self.temperature_supported = self.temperature.is_some();
                self.temperature_at = Some(Instant::now());
            }
            read(raw)
        });
        match result {
            Ok(now) => Some(now),
            Err(code) => {
                if handle.suspended() || handle.removed() {
                    self.temperature = None;
                    self.temperature_at = None;
                } else {
                    failure("disk.performance", self.info.index, code);
                }
                None
            }
        }
    }
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
        // A query-remove is a pause, not permission to reopen the same device
        // through a fresh interface or volume handle during enumeration.
        if self
            .disks
            .iter()
            .any(|d| d.handle.as_ref().is_some_and(DeviceHandle::suspended))
        {
            return;
        }
        let volumes = volume_map();
        let mut discovered = discovery::disks();
        let mut old: HashMap<u32, Disk> = self.disks.drain(..).map(|d| (d.info.index, d)).collect();
        let indices: BTreeSet<_> = (0..LEGACY_PROBE_DISKS)
            .chain(discovered.keys().copied())
            .chain(volumes.keys().copied())
            .filter(|index| *index <= 4095)
            .collect();
        for index in indices {
            let opened = DeviceHandle::open(&format!(r"\\.\PhysicalDrive{index}"))
                .or_else(|code| discovered.remove(&index).ok_or(code));
            let handle = match opened {
                Ok(handle) => Some(handle),
                Err(code) => {
                    if !matches!(code, 2 | 3 | 21 | 1112) {
                        failure("disk.open", index, code);
                    }
                    if let Some(disk) = old.remove(&index).filter(|_| !matches!(code, 2 | 3)) {
                        self.disks.push(disk);
                        continue;
                    }
                    if !volumes.contains_key(&index) {
                        continue;
                    }
                    None
                }
            };
            let vols = volumes.get(&index).cloned().unwrap_or_default();
            let (info, geometry_error) = match &handle {
                Some(handle) => handle
                    .read(|raw, _| Ok(describe(raw, index, vols.clone())))
                    .unwrap_or_else(|_| (unknown_disk(index, vols), None)),
                None => (
                    DiskInfo {
                        index,
                        model: String::new(),
                        size: 0,
                        bus: "other",
                        ssd: None,
                        removable: false,
                        volumes: vols,
                    },
                    None,
                ),
            };
            if no_media(&info, geometry_error) {
                continue;
            }
            // Refresh handles and metadata on hotplug without resetting a
            // healthy counter stream for a disk whose identity is unchanged.
            if let Some(mut d) = old.remove(&index) {
                let identity_changed = d.info.model != info.model
                    || d.info.size != info.size
                    || d.info.bus != info.bus;
                if identity_changed || d.prev.is_none() || d.handle.is_none() {
                    d.handle = handle;
                    d.generation = 0;
                    d.prev = None;
                }
                if *d.info != info {
                    d.info = Arc::new(info);
                }
                d.temperature_supported = true;
                self.disks.push(d);
                continue;
            }
            self.disks.push(Disk {
                handle,
                generation: 0,
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
        let count = self.disks.len();
        self.disks
            .retain(|d| !d.handle.as_ref().is_some_and(DeviceHandle::removed));
        if self.disks.len() != count {
            // Finish removal before the next regular inventory scan. Dropping
            // the notifications here is outside the PnP callback.
            self.scanned_at = Instant::now();
        }
        if self.scanned_at.elapsed() > RESCAN_EVERY {
            self.rescan();
        }
        self.collect(|d| d.sample_with(performance))
    }

    fn collect(
        &mut self,
        mut read: impl FnMut(&mut Disk) -> Option<DISK_PERFORMANCE>,
    ) -> Vec<DiskStats> {
        self.disks
            .iter_mut()
            .map(|d| {
                let now = read(d);
                let mut stats = DiskStats {
                    info: d.info.clone(),
                    active: None,
                    read: None,
                    write: None,
                    response_ms: None,
                    temperature: d.temperature.map(|t| t.0),
                    temperature_warning: d.temperature.and_then(|t| t.1),
                };
                if let (Some(prev), Some(now)) = (d.prev, now) {
                    let elapsed = (now.QueryTime - prev.QueryTime) as f64; // 100 ns ticks
                    if elapsed > 0.0
                        && now.BytesRead >= prev.BytesRead
                        && now.BytesWritten >= prev.BytesWritten
                        && now.IdleTime >= prev.IdleTime
                        && now.ReadTime >= prev.ReadTime
                        && now.WriteTime >= prev.WriteTime
                    {
                        let idle = (now.IdleTime - prev.IdleTime) as f64;
                        stats.active =
                            Some(((1.0 - idle / elapsed) * 100.0).clamp(0.0, 100.0) as f32);
                        let secs = elapsed / 1e7;
                        stats.read = Some(((now.BytesRead - prev.BytesRead) as f64 / secs) as u64);
                        stats.write =
                            Some(((now.BytesWritten - prev.BytesWritten) as f64 / secs) as u64);
                        let transfers = now.ReadCount.wrapping_sub(prev.ReadCount) as u64
                            + now.WriteCount.wrapping_sub(prev.WriteCount) as u64;
                        let busy = (now.ReadTime - prev.ReadTime) as f64
                            + (now.WriteTime - prev.WriteTime) as f64;
                        stats.response_ms = Some(if transfers > 0 {
                            (busy / transfers as f64 / 10_000.0) as f32
                        } else {
                            0.
                        });
                    }
                }
                // Failure resets the baseline, so recovery never averages across
                // a missing interval or reports an old sample as current activity.
                d.prev = now;
                stats
            })
            .collect()
    }
}

fn unknown_disk(index: u32, volumes: Vec<String>) -> DiskInfo {
    DiskInfo {
        index,
        model: String::new(),
        size: 0,
        bus: "other",
        ssd: None,
        removable: false,
        volumes,
    }
}

fn no_media(info: &DiskInfo, geometry_error: Option<u32>) -> bool {
    info.size == 0
        && info.volumes.is_empty()
        && (geometry_error == Some(1112) || (info.removable && geometry_error == Some(21)))
}

fn native_code(error: windows::core::Error) -> u32 {
    let code = error.code().0 as u32;
    if code >> 16 == 0x8007 {
        code & 0xffff
    } else {
        code
    }
}

fn failure(stage: &'static str, index: u32, code: u32) {
    crate::diagnostics::warn(
        stage,
        format!("disk={index}; {}", crate::diagnostics::code(code)),
    );
}

fn ioctl(
    handle: HANDLE,
    code: u32,
    input: Option<&[u8]>,
    out: &mut AlignedBuf,
) -> Result<u32, u32> {
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
    .map_err(native_code)
    .map(|_| returned)
}

fn performance(handle: HANDLE) -> Result<DISK_PERFORMANCE, u32> {
    let mut buf = AlignedBuf::with_capacity(std::mem::size_of::<DISK_PERFORMANCE>());
    let returned = ioctl(handle, IOCTL_DISK_PERFORMANCE, None, &mut buf)?;
    if returned as usize != std::mem::size_of::<DISK_PERFORMANCE>() {
        return Err(13);
    }
    let value = unsafe { std::ptr::read(buf.as_ptr() as *const DISK_PERFORMANCE) };
    if value.QueryTime <= 0
        || value.BytesRead < 0
        || value.BytesWritten < 0
        || value.IdleTime < 0
        || value.ReadTime < 0
        || value.WriteTime < 0
    {
        return Err(13);
    }
    Ok(value)
}

fn query_property(
    handle: HANDLE,
    id: windows::Win32::System::Ioctl::STORAGE_PROPERTY_ID,
) -> Result<(AlignedBuf, usize), u32> {
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
    let returned = ioctl(handle, IOCTL_STORAGE_QUERY_PROPERTY, Some(input), &mut buf)? as usize;
    if returned > buf.len() {
        return Err(13);
    }
    Ok((buf, returned))
}

/// The drive's composite temperature and warning threshold (°C).
fn temperature(handle: HANDLE) -> Option<(f32, Option<f32>)> {
    let (buf, returned) = query_property(handle, StorageDeviceTemperatureProperty).ok()?;
    if returned < std::mem::size_of::<STORAGE_TEMPERATURE_DATA_DESCRIPTOR>() {
        return None;
    }
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

fn describe(handle: HANDLE, index: u32, mut volumes: Vec<String>) -> (DiskInfo, Option<u32>) {
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

    if let Ok((buf, returned)) = query_property(handle, StorageDeviceProperty)
        .and_then(|(buf, returned)| {
            if returned >= std::mem::offset_of!(STORAGE_DEVICE_DESCRIPTOR, RawDeviceProperties) {
                Ok((buf, returned))
            } else {
                Err(13)
            }
        })
        .inspect_err(|code| failure("disk.identity", index, *code))
    {
        let desc = unsafe { &*(buf.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
        let raw = unsafe { std::slice::from_raw_parts(buf.as_ptr(), returned) };
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
        info.bus = bus_name(desc.BusType.0 as u32);
    }

    if let Ok((buf, returned)) = query_property(handle, StorageDeviceSeekPenaltyProperty)
        && returned >= std::mem::size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>()
    {
        let desc = unsafe { &*(buf.as_ptr() as *const DEVICE_SEEK_PENALTY_DESCRIPTOR) };
        info.ssd = Some(!desc.IncursSeekPenalty);
    }
    if info.bus == "nvme" {
        info.ssd = Some(true);
    }

    let mut geo = AlignedBuf::with_capacity(256);
    let geometry =
        ioctl(handle, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, None, &mut geo).and_then(|returned| {
            if (returned as usize) < std::mem::offset_of!(DISK_GEOMETRY_EX, DiskSize) + 8 {
                return Err(13);
            }
            Ok(())
        });
    if geometry.is_ok() {
        let g = unsafe { &*(geo.as_ptr() as *const DISK_GEOMETRY_EX) };
        info.size = g.DiskSize.max(0) as u64;
    } else if let Err(code) = geometry
        && !no_media(&info, Some(code))
    {
        failure("disk.geometry", index, code);
    }
    (info, geometry.err())
}

/// Maps physical disk number to the drive letters that live on it.
pub(super) fn volume_map() -> HashMap<u32, Vec<String>> {
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
        let Ok(h) = DeviceHandle::open(&format!(r"\\.\{letter}")) else {
            continue;
        };
        let mut buf = AlignedBuf::with_capacity(512);
        let Ok(returned) =
            h.read(|raw, _| ioctl(raw, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, None, &mut buf))
        else {
            continue;
        };
        let offset = std::mem::offset_of!(VOLUME_DISK_EXTENTS, Extents);
        let returned = (returned as usize).min(buf.len());
        if returned < offset {
            continue;
        }
        let ext = unsafe { &*(buf.as_ptr() as *const VOLUME_DISK_EXTENTS) };
        let n = ext.NumberOfDiskExtents as usize;
        let available = (returned - offset) / std::mem::size_of_val(&ext.Extents[0]);
        if n > available {
            continue;
        }
        let extents = unsafe { std::slice::from_raw_parts(ext.Extents.as_ptr(), n) };
        for e in extents {
            let list = map.entry(e.DiskNumber).or_default();
            if !list.contains(&letter) {
                list.push(letter.clone());
            }
        }
    }
    map
}
