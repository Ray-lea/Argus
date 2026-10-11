//! Batteries through the battery class driver (the IOCTLs behind
//! `powercfg /batteryreport`): charge, power draw, voltage, wear and cycles.
//! Standard users may open battery devices for reading; no WMI involved.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Power::{
    BATTERY_CAPACITY_RELATIVE, BATTERY_CHARGING, BATTERY_CRITICAL, BATTERY_DISCHARGING,
    BATTERY_INFORMATION, BATTERY_MANUFACTURE_DATE, BATTERY_POWER_ON_LINE,
    BATTERY_QUERY_INFORMATION, BATTERY_QUERY_INFORMATION_LEVEL, BATTERY_STATUS,
    BATTERY_UNKNOWN_CAPACITY, BATTERY_UNKNOWN_RATE, BATTERY_UNKNOWN_VOLTAGE, BATTERY_WAIT_STATUS,
    BatteryDeviceName, BatteryInformation, BatteryManufactureDate, BatteryManufactureName,
    BatterySerialNumber, BatteryTemperature, GUID_DEVICE_BATTERY, GetSystemPowerStatus,
    IOCTL_BATTERY_QUERY_INFORMATION, IOCTL_BATTERY_QUERY_STATUS, IOCTL_BATTERY_QUERY_TAG,
    SYSTEM_POWER_STATUS,
};
use windows::core::PCWSTR;

use crate::util::{AlignedBuf, OwnedHandle};

/// Capacity and identity change rarely (full charge capacity drifts with wear).
const INFO_EVERY: Duration = Duration::from_secs(60);
/// Batteries come and go only on docks and some tablets.
const RESCAN_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryInfo {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub serial: String,
    /// Four-letter code, e.g. `LION`, `LiP`, `PbAc`.
    pub chemistry: String,
    /// mWh. None when the battery reports relative units only.
    pub designed_capacity: Option<u32>,
    pub full_capacity: Option<u32>,
    pub cycle_count: Option<u32>,
    /// `YYYY-MM-DD`.
    pub manufactured: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryStats {
    pub info: Arc<BatteryInfo>,
    /// 0–100.
    pub percent: f32,
    /// mWh remaining.
    pub capacity: Option<u32>,
    /// mW into the battery (negative while discharging).
    pub rate: Option<i32>,
    /// mV.
    pub voltage: Option<u32>,
    /// `charging` | `discharging` | `full` | `idle` (plugged in, not charging).
    pub state: &'static str,
    pub plugged_in: bool,
    pub critical: bool,
    /// Seconds until empty, as Windows estimates it (only while discharging).
    pub remaining: Option<u32>,
    /// °C, if the battery reports it.
    pub temperature: Option<f32>,
}

struct Battery {
    handle: OwnedHandle,
    tag: u32,
    info: Arc<BatteryInfo>,
    relative: bool,
    info_at: Instant,
    has_temperature: bool,
}

pub(crate) struct BatteryTracker {
    batteries: Vec<Battery>,
    scanned_at: Instant,
}

impl BatteryTracker {
    pub fn new() -> Self {
        let mut t = Self {
            batteries: Vec::new(),
            scanned_at: Instant::now(),
        };
        t.rescan();
        t
    }

    fn rescan(&mut self) {
        self.batteries = device_paths()
            .into_iter()
            .enumerate()
            .filter_map(|(i, path)| open(i, &path))
            .collect();
        self.scanned_at = Instant::now();
    }

    pub fn sample(&mut self) -> Vec<BatteryStats> {
        if self.scanned_at.elapsed() > RESCAN_EVERY {
            self.rescan();
        }
        let mut power = SYSTEM_POWER_STATUS::default();
        let system = unsafe { GetSystemPowerStatus(&mut power) }.is_ok();
        let mut out = Vec::with_capacity(self.batteries.len());
        for b in &mut self.batteries {
            if b.info_at.elapsed() > INFO_EVERY
                && let Some((info, relative)) = describe(&b.handle, b.tag, &b.info.id)
            {
                b.info = Arc::new(info);
                b.relative = relative;
                b.info_at = Instant::now();
            }
            let Some(status) = status(&b.handle, b.tag) else {
                continue;
            };
            let known = |v: u32| (v != BATTERY_UNKNOWN_CAPACITY).then_some(v);
            let capacity = known(status.Capacity);
            let percent = match (b.relative, capacity, b.info.full_capacity) {
                (true, Some(c), _) => c as f32,
                (false, Some(c), Some(full)) if full > 0 => c as f32 / full as f32 * 100.0,
                _ => continue,
            }
            .clamp(0.0, 100.0);
            let state = status.PowerState;
            let plugged_in = state & BATTERY_POWER_ON_LINE != 0;
            let temperature = if b.has_temperature {
                let t = query_u32(&b.handle, b.tag, BatteryTemperature);
                b.has_temperature = t.is_some();
                // Tenths of a kelvin.
                t.map(|t| t as f32 / 10.0 - 273.15)
                    .filter(|c| *c > 0.0 && *c < 100.0)
            } else {
                None
            };
            out.push(BatteryStats {
                info: b.info.clone(),
                percent,
                capacity: capacity.filter(|_| !b.relative),
                rate: (status.Rate != BATTERY_UNKNOWN_RATE as i32 && !b.relative)
                    .then_some(status.Rate),
                voltage: (status.Voltage != BATTERY_UNKNOWN_VOLTAGE && status.Voltage > 0)
                    .then_some(status.Voltage),
                state: if state & BATTERY_CHARGING != 0 {
                    "charging"
                } else if state & BATTERY_DISCHARGING != 0 {
                    "discharging"
                } else if plugged_in && percent >= 99.5 {
                    "full"
                } else {
                    "idle"
                },
                plugged_in,
                critical: state & BATTERY_CRITICAL != 0,
                remaining: (system
                    && state & BATTERY_DISCHARGING != 0
                    && power.BatteryLifeTime != u32::MAX)
                    .then_some(power.BatteryLifeTime),
                temperature,
            });
        }
        out
    }
}

/// Device interface paths of every present battery.
fn device_paths() -> Vec<Vec<u16>> {
    let mut paths = Vec::new();
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(
            Some(&GUID_DEVICE_BATTERY),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) else {
            return paths;
        };
        for index in 0..8 {
            let mut iface = SP_DEVICE_INTERFACE_DATA {
                cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInterfaces(set, None, &GUID_DEVICE_BATTERY, index, &mut iface)
                .is_err()
            {
                break;
            }
            let mut needed = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(set, &iface, None, 0, Some(&mut needed), None);
            if needed == 0 {
                continue;
            }
            let mut buf = AlignedBuf::with_capacity(needed as usize);
            let detail = buf.as_mut_ptr().cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &iface, Some(detail), needed, None, None)
                .is_ok()
            {
                // DevicePath is a NUL-terminated string running past the struct.
                let start = (*detail).DevicePath.as_ptr();
                let len = (0..).take_while(|&i| *start.add(i) != 0).count();
                let mut path = std::slice::from_raw_parts(start, len).to_vec();
                path.push(0);
                paths.push(path);
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    paths
}

fn open(index: usize, path: &[u16]) -> Option<Battery> {
    let handle = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .ok()
    .map(OwnedHandle)?;
    let wait = 0u32;
    let mut tag = 0u32;
    ioctl(&handle, IOCTL_BATTERY_QUERY_TAG, &wait, &mut tag)?;
    if tag == 0 {
        return None; // slot without a battery
    }
    let (info, relative) = describe(&handle, tag, &index.to_string())?;
    Some(Battery {
        handle,
        tag,
        info: Arc::new(info),
        relative,
        info_at: Instant::now(),
        has_temperature: true,
    })
}

fn describe(handle: &OwnedHandle, tag: u32, id: &str) -> Option<(BatteryInfo, bool)> {
    let mut info = BATTERY_INFORMATION::default();
    ioctl(
        handle,
        IOCTL_BATTERY_QUERY_INFORMATION,
        &query(tag, BatteryInformation),
        &mut info,
    )?;
    let relative = info.Capabilities & BATTERY_CAPACITY_RELATIVE != 0;
    let mwh = |v: u32| (!relative && v != 0 && v != BATTERY_UNKNOWN_CAPACITY).then_some(v);
    let mut date = BATTERY_MANUFACTURE_DATE::default();
    let manufactured = ioctl(
        handle,
        IOCTL_BATTERY_QUERY_INFORMATION,
        &query(tag, BatteryManufactureDate),
        &mut date,
    )
    .filter(|_| date.Year > 1990 && (1..=12).contains(&date.Month))
    .map(|_| format!("{:04}-{:02}-{:02}", date.Year, date.Month, date.Day));
    Some((
        BatteryInfo {
            id: id.to_string(),
            name: query_string(handle, tag, BatteryDeviceName),
            manufacturer: query_string(handle, tag, BatteryManufactureName),
            serial: query_string(handle, tag, BatterySerialNumber),
            chemistry: String::from_utf8_lossy(&info.Chemistry)
                .trim_matches(['\0', ' '])
                .to_string(),
            designed_capacity: mwh(info.DesignedCapacity),
            full_capacity: mwh(info.FullChargedCapacity),
            cycle_count: (info.CycleCount > 0).then_some(info.CycleCount),
            manufactured,
        },
        relative,
    ))
}

fn status(handle: &OwnedHandle, tag: u32) -> Option<BATTERY_STATUS> {
    // Timeout 0: report the current status instead of waiting for a change.
    let wait = BATTERY_WAIT_STATUS {
        BatteryTag: tag,
        ..Default::default()
    };
    let mut status = BATTERY_STATUS::default();
    ioctl(handle, IOCTL_BATTERY_QUERY_STATUS, &wait, &mut status)?;
    Some(status)
}

fn query(tag: u32, level: BATTERY_QUERY_INFORMATION_LEVEL) -> BATTERY_QUERY_INFORMATION {
    BATTERY_QUERY_INFORMATION {
        BatteryTag: tag,
        InformationLevel: level,
        AtRate: 0,
    }
}

fn query_u32(
    handle: &OwnedHandle,
    tag: u32,
    level: BATTERY_QUERY_INFORMATION_LEVEL,
) -> Option<u32> {
    let mut v = 0u32;
    ioctl(
        handle,
        IOCTL_BATTERY_QUERY_INFORMATION,
        &query(tag, level),
        &mut v,
    )?;
    Some(v)
}

fn query_string(handle: &OwnedHandle, tag: u32, level: BATTERY_QUERY_INFORMATION_LEVEL) -> String {
    let mut buf = [0u16; 128];
    match ioctl(
        handle,
        IOCTL_BATTERY_QUERY_INFORMATION,
        &query(tag, level),
        &mut buf,
    ) {
        Some(bytes) => {
            let chars = &buf[..(bytes as usize / 2).min(buf.len())];
            let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
            String::from_utf16_lossy(&chars[..end]).trim().to_string()
        }
        None => String::new(),
    }
}

/// One fixed-size IOCTL; returns the bytes written.
fn ioctl<I, O>(handle: &OwnedHandle, code: u32, input: &I, output: &mut O) -> Option<u32> {
    let mut returned = 0u32;
    unsafe {
        DeviceIoControl(
            handle.0,
            code,
            Some((input as *const I).cast()),
            size_of::<I>() as u32,
            Some((output as *mut O).cast()),
            size_of::<O>() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()
    .map(|_| returned)
}
