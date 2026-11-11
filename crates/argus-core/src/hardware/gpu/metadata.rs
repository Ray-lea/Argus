//! Static GPU metadata, matched by adapter LUID and collected once at enumeration.
use std::{collections::HashMap, mem::size_of};

use serde::Serialize;
use windows::Win32::{
    Devices::{
        DeviceAndDriverInstallation::{
            CM_Get_DevNode_PropertyW, CR_SUCCESS, DIGCF_PRESENT, GUID_DEVCLASS_DISPLAY, HDEVINFO,
            SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
            SetupDiGetClassDevsW,
        },
        Display::DEVPKEY_Device_AdapterLuid,
        Properties::{
            DEVPKEY_Device_DriverDate, DEVPKEY_Device_DriverVersion, DEVPROP_TYPE_BINARY,
            DEVPROP_TYPE_FILETIME, DEVPROP_TYPE_INT64, DEVPROP_TYPE_STRING, DEVPROP_TYPE_UINT64,
            DEVPROPTYPE,
        },
    },
    Foundation::{DEVPROPKEY, FILETIME, HMODULE, LUID, SYSTEMTIME},
    Graphics::{
        Direct3D::{
            D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_9_1,
            D3D_FEATURE_LEVEL_9_2, D3D_FEATURE_LEVEL_9_3, D3D_FEATURE_LEVEL_10_0,
            D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
            D3D_FEATURE_LEVEL_12_0, D3D_FEATURE_LEVEL_12_1, D3D_FEATURE_LEVEL_12_2,
        },
        Direct3D11::{D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDevice},
        Direct3D12::{D3D12CreateDevice, ID3D12Device},
        Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory4},
    },
    System::Time::FileTimeToSystemTime,
};

use windows::core::GUID;

// Native display-stack LUID property, also used by DXGI diagnostic tools:
// https://github.com/fastfetch-cli/fastfetch/blob/dev/src/detection/gpu/gpu_windows.c
const DISPLAY_ADAPTER_LUID: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x60b193cb_5276_4d0f_96fc_f173abad3ec6),
    pid: 2,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectXInfo {
    /// Highest Direct3D API successfully validated on this adapter and driver.
    pub version: u8,
    pub feature_level: String,
}

#[derive(Default)]
pub(super) struct Driver {
    pub version: Option<String>,
    pub date: Option<String>,
}

struct DeviceSet(HDEVINFO);
impl Drop for DeviceSet {
    fn drop(&mut self) {
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

pub(super) struct Metadata {
    drivers: HashMap<u64, Driver>,
    factory: Option<IDXGIFactory4>,
}

impl Metadata {
    pub fn new() -> Self {
        Self {
            drivers: crate::diagnostics::startup_phase("gpu.metadata.pnp", drivers),
            factory: crate::diagnostics::startup_phase("gpu.metadata.dxgi-factory", || unsafe {
                CreateDXGIFactory1()
            })
            .ok(),
        }
    }

    pub fn driver(&mut self, luid: LUID) -> Driver {
        self.drivers.remove(&luid_bits(luid)).unwrap_or_default()
    }

    pub fn direct_x(&self, luid: LUID) -> Option<DirectXInfo> {
        let adapter: IDXGIAdapter =
            unsafe { self.factory.as_ref()?.EnumAdapterByLuid(luid) }.ok()?;
        // A null output tests capability without creating/retaining a D3D12 device.
        for level in [
            D3D_FEATURE_LEVEL_12_2,
            D3D_FEATURE_LEVEL_12_1,
            D3D_FEATURE_LEVEL_12_0,
            D3D_FEATURE_LEVEL_11_1,
            D3D_FEATURE_LEVEL_11_0,
        ] {
            if crate::diagnostics::startup_query(
                "gpu.metadata.d3d12",
                luid_bits(luid),
                level.0 as u32,
                || unsafe {
                    D3D12CreateDevice::<_, ID3D12Device>(&adapter, level, std::ptr::null_mut())
                },
            )
            .is_ok()
            {
                return capability(12, level);
            }
        }
        let levels = [
            D3D_FEATURE_LEVEL_11_1,
            D3D_FEATURE_LEVEL_11_0,
            D3D_FEATURE_LEVEL_10_1,
            D3D_FEATURE_LEVEL_10_0,
            D3D_FEATURE_LEVEL_9_3,
            D3D_FEATURE_LEVEL_9_2,
            D3D_FEATURE_LEVEL_9_1,
        ];
        let mut level = D3D_FEATURE_LEVEL::default();
        crate::diagnostics::startup_query("gpu.metadata.d3d11", luid_bits(luid), 0, || unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&levels),
                D3D11_SDK_VERSION,
                None,
                Some(&mut level),
                None,
            )
        })
        .ok()?;
        capability(11, level)
    }
}

fn luid_bits(luid: LUID) -> u64 {
    (u64::from(luid.HighPart as u32) << 32) | u64::from(luid.LowPart)
}

fn capability(version: u8, level: D3D_FEATURE_LEVEL) -> Option<DirectXInfo> {
    let feature_level = match level {
        D3D_FEATURE_LEVEL_12_2 => "12.2",
        D3D_FEATURE_LEVEL_12_1 => "12.1",
        D3D_FEATURE_LEVEL_12_0 => "12.0",
        D3D_FEATURE_LEVEL_11_1 => "11.1",
        D3D_FEATURE_LEVEL_11_0 => "11.0",
        D3D_FEATURE_LEVEL_10_1 => "10.1",
        D3D_FEATURE_LEVEL_10_0 => "10.0",
        D3D_FEATURE_LEVEL_9_3 => "9.3",
        D3D_FEATURE_LEVEL_9_2 => "9.2",
        D3D_FEATURE_LEVEL_9_1 => "9.1",
        _ => return None,
    };
    Some(DirectXInfo {
        version,
        feature_level: feature_level.into(),
    })
}

fn drivers() -> HashMap<u64, Driver> {
    let mut drivers = HashMap::new();
    let Ok(set) =
        (unsafe { SetupDiGetClassDevsW(Some(&GUID_DEVCLASS_DISPLAY), None, None, DIGCF_PRESENT) })
    else {
        return drivers;
    };
    let set = DeviceSet(set);
    for index in 0..256 {
        let mut device = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut device) }.is_err() {
            break;
        }
        let luid = [&DISPLAY_ADAPTER_LUID, &DEVPKEY_Device_AdapterLuid]
            .into_iter()
            .find_map(|key| {
                property(device.DevInst, key).and_then(|(kind, bytes)| adapter_luid(kind, &bytes))
            });
        let Some(luid) = luid else {
            continue;
        };
        let version = property(device.DevInst, &DEVPKEY_Device_DriverVersion)
            .filter(|(kind, _)| *kind == DEVPROP_TYPE_STRING)
            .and_then(|(_, data)| string(&data));
        let date = property(device.DevInst, &DEVPKEY_Device_DriverDate)
            .filter(|(kind, _)| *kind == DEVPROP_TYPE_FILETIME)
            .and_then(|(_, data)| date(&data));
        drivers.entry(luid).or_insert(Driver { version, date });
    }
    drivers
}

fn property(node: u32, key: &DEVPROPKEY) -> Option<(DEVPROPTYPE, Vec<u8>)> {
    let mut bytes = vec![0; 4096];
    let mut size = bytes.len() as u32;
    let mut kind = DEVPROPTYPE::default();
    let status = unsafe {
        CM_Get_DevNode_PropertyW(node, key, &mut kind, Some(bytes.as_mut_ptr()), &mut size, 0)
    };
    if status != CR_SUCCESS || size as usize > bytes.len() {
        return None;
    }
    bytes.truncate(size as usize);
    Some((kind, bytes))
}

fn adapter_luid(kind: DEVPROPTYPE, bytes: &[u8]) -> Option<u64> {
    // Native display nodes use a 64-bit property; indirect adapters use binary.
    if !matches!(
        kind,
        DEVPROP_TYPE_BINARY | DEVPROP_TYPE_INT64 | DEVPROP_TYPE_UINT64
    ) {
        return None;
    }
    let luid = u64::from_le_bytes(bytes.try_into().ok()?);
    (luid != 0).then_some(luid)
}

fn string(bytes: &[u8]) -> Option<String> {
    let (pairs, rest) = bytes.as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let words = pairs
        .iter()
        .copied()
        .map(u16::from_le_bytes)
        .collect::<Vec<_>>();
    let end = words.iter().position(|word| *word == 0)?;
    let value = String::from_utf16(&words[..end]).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn date(bytes: &[u8]) -> Option<String> {
    let ticks = u64::from_le_bytes(bytes.try_into().ok()?);
    if ticks == 0 || ticks == u64::MAX {
        return None;
    }
    let filetime = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut date = SYSTEMTIME::default();
    // Driver INF dates are calendar dates in UTC, never shifted to local time.
    unsafe { FileTimeToSystemTime(&filetime, &mut date) }.ok()?;
    Some(format!(
        "{:04}-{:02}-{:02}",
        date.wYear, date.wMonth, date.wDay
    ))
}

#[cfg(test)]
mod tests {
    use super::{adapter_luid, capability, date, luid_bits, string};
    use windows::Win32::{
        Devices::Properties::{
            DEVPROP_TYPE_BINARY, DEVPROP_TYPE_INT64, DEVPROP_TYPE_STRING, DEVPROP_TYPE_UINT64,
        },
        Foundation::LUID,
        Graphics::Direct3D::{D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_12_2},
    };

    #[test]
    fn adapter_matching_preserves_both_luid_halves_and_rejects_wrong_types() {
        let luid = LUID {
            HighPart: -1,
            LowPart: 42,
        };
        let value = luid_bits(luid);
        assert_eq!(
            adapter_luid(DEVPROP_TYPE_INT64, &value.to_le_bytes()),
            Some(value)
        );
        assert_eq!(
            adapter_luid(DEVPROP_TYPE_UINT64, &value.to_le_bytes()),
            Some(value)
        );
        assert_eq!(
            adapter_luid(DEVPROP_TYPE_BINARY, &value.to_le_bytes()),
            Some(value)
        );
        assert_eq!(
            adapter_luid(DEVPROP_TYPE_STRING, &value.to_le_bytes()),
            None
        );
        assert_eq!(adapter_luid(DEVPROP_TYPE_BINARY, &[0; 7]), None);
        assert_eq!(adapter_luid(DEVPROP_TYPE_BINARY, &[0; 8]), None);
    }

    #[test]
    fn driver_properties_validate_lengths_and_preserve_the_inf_calendar_date() {
        assert_eq!(
            date(&116444736000000000u64.to_le_bytes()).as_deref(),
            Some("1970-01-01")
        );
        assert_eq!(date(&[0; 8]), None);
        assert_eq!(date(&[255; 8]), None);
        assert_eq!(date(&[1; 7]), None);
        let encoded = "32.0.15.9999\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(string(&encoded).as_deref(), Some("32.0.15.9999"));
        assert_eq!(string(&encoded[..encoded.len() - 2]), None);
        assert_eq!(string(&[0]), None);
    }

    #[test]
    fn directx_api_and_hardware_feature_level_stay_distinct() {
        let info = capability(12, D3D_FEATURE_LEVEL_11_0).unwrap();
        assert_eq!(info.version, 12);
        assert_eq!(info.feature_level, "11.0");
        assert_eq!(
            capability(12, D3D_FEATURE_LEVEL_12_2)
                .unwrap()
                .feature_level,
            "12.2"
        );
        assert!(capability(12, D3D_FEATURE_LEVEL(0)).is_none());
    }

    #[test]
    #[ignore = "Requires live Windows graphics adapters"]
    fn live_adapter_metadata() {
        let started = std::time::Instant::now();
        let mut metadata = super::Metadata::new();
        let enumeration_ms = started.elapsed().as_secs_f64() * 1000.;
        let ids = metadata.drivers.keys().copied().collect::<Vec<_>>();
        assert!(
            !ids.is_empty(),
            "no display device exposed its adapter LUID"
        );
        for id in ids {
            let started = std::time::Instant::now();
            let luid = LUID {
                LowPart: id as u32,
                HighPart: (id >> 32) as i32,
            };
            let driver = metadata.driver(luid);
            let direct_x = metadata.direct_x(luid);
            println!(
                "{}",
                serde_json::json!({"id":id.to_string(), "driverVersion":driver.version,
                "driverDate":driver.date, "directX":direct_x, "enumerationMs":enumeration_ms,
                "capabilityMs":started.elapsed().as_secs_f64()*1000.})
            );
        }
    }
}
