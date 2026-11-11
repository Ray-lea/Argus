use std::mem::{offset_of, size_of};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    DISK_GEOMETRY_EX, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_STORAGE_QUERY_PROPERTY,
    NVMeDataTypeIdentify, NVMeDataTypeLogPage, PropertyStandardQuery, ProtocolTypeNvme,
    STORAGE_PROPERTY_ID, STORAGE_PROPERTY_QUERY, STORAGE_PROTOCOL_DATA_DESCRIPTOR,
    STORAGE_PROTOCOL_SPECIFIC_DATA, StorageAccessAlignmentProperty,
    StorageAdapterProtocolSpecificProperty, StorageDeviceProperty,
    StorageDeviceProtocolSpecificProperty, StorageDeviceSeekPenaltyProperty,
    StorageDeviceTrimProperty,
};
use windows::core::PCWSTR;

use crate::util::{OwnedHandle, wide};

use super::{DiskDetails, DiskIdentity, DiskQuery, DiskQueryState, DiskQueryStatus, nvme};

const PROPERTY_LIMIT: usize = 4096;
const PROTOCOL_OFFSET: usize = offset_of!(STORAGE_PROPERTY_QUERY, AdditionalParameters);
const PROTOCOL_SIZE: usize = size_of::<STORAGE_PROTOCOL_SPECIFIC_DATA>();
const DESCRIPTOR_SIZE: usize = size_of::<STORAGE_PROTOCOL_DATA_DESCRIPTOR>();

#[derive(Debug)]
pub(super) struct Failure {
    pub state: DiskQueryState,
    pub code: Option<u32>,
}

impl Failure {
    pub fn native(error: windows::core::Error) -> Self {
        let hresult = error.code().0 as u32;
        let code = if hresult >> 16 == 0x8007 {
            hresult & 0xffff
        } else {
            hresult
        };
        Self {
            state: match code {
                1 | 50 => DiskQueryState::Unsupported,
                5 | 1314 => DiskQueryState::AccessDenied,
                _ => DiskQueryState::Unavailable,
            },
            code: Some(code),
        }
    }

    pub fn invalid() -> Self {
        Self {
            state: DiskQueryState::InvalidData,
            code: None,
        }
    }
}

pub(super) type Result<T> = std::result::Result<T, Failure>;

pub(super) fn query(index: u32) -> DiskDetails {
    let mut result = DiskDetails::empty(index);
    let path = wide(&format!(r"\\.\PhysicalDrive{index}"));
    // Base queries work with a zero-access handle. The service supplements them.
    let opened = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map(OwnedHandle)
    .map_err(Failure::native);
    let Some(handle) = record(&mut result, DiskQuery::Open, opened) else {
        return result;
    };
    result.volumes = super::super::disk::volume_map()
        .remove(&index)
        .unwrap_or_default();
    result.volumes.sort();
    let identity = property(handle.0, StorageDeviceProperty).and_then(|data| identity(&data));
    result.identity = record(&mut result, DiskQuery::Identity, identity);
    let capacity = control(handle.0, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &[], 256).and_then(|data| {
        let value = i64::from_le_bytes(bytes(&data, offset_of!(DISK_GEOMETRY_EX, DiskSize))?);
        (value > 0)
            .then_some(value as u64)
            .ok_or_else(Failure::invalid)
    });
    result.capacity_bytes = record(&mut result, DiskQuery::Capacity, capacity);
    let seek = property(handle.0, StorageDeviceSeekPenaltyProperty)
        .and_then(|data| descriptor(&data, 9).and_then(|d| boolean(d[8])));
    result.solid_state = record(&mut result, DiskQuery::SeekPenalty, seek).map(|seek| !seek);
    let alignment = property(handle.0, StorageAccessAlignmentProperty).and_then(|data| {
        let data = descriptor(&data, 28)?;
        let logical = u32_at(data, 16)?;
        let physical = u32_at(data, 20)?;
        if !logical.is_power_of_two() || physical < logical || !physical.is_power_of_two() {
            return Err(Failure::invalid());
        }
        Ok((logical, physical))
    });
    if let Some((logical, physical)) = record(&mut result, DiskQuery::Alignment, alignment) {
        result.logical_sector_bytes = Some(logical);
        result.physical_sector_bytes = Some(physical);
    }
    let trim = property(handle.0, StorageDeviceTrimProperty)
        .and_then(|data| descriptor(&data, 9).and_then(|d| boolean(d[8])));
    result.trim_enabled = record(&mut result, DiskQuery::Trim, trim);
    let bus = result.identity.as_ref().map_or("", |i| i.bus.as_str());
    let native_nvme = bus == "nvme";
    if probe_nvme(bus, result.solid_state) {
        let identity = nvme_query(native_nvme, |scope| {
            protocol(handle.0, scope, NVMeDataTypeIdentify.0 as u32, 1, 4096)
                .and_then(|data| nvme::identify(&data).ok_or_else(Failure::invalid))
        })
        .map(|(identity, _)| identity);
        result.nvme_identity = record(&mut result, DiskQuery::NvmeIdentity, identity);
        let version = result.nvme_identity.as_ref().and_then(|i| i.version);
        let health = nvme_query(native_nvme, |scope| {
            protocol(handle.0, scope, NVMeDataTypeLogPage.0 as u32, 2, 512)
                .and_then(|data| nvme::health(&data, version).ok_or_else(Failure::invalid))
        })
        .map(|(mut health, scope)| {
            health.scope = if scope == StorageAdapterProtocolSpecificProperty {
                "controller"
            } else {
                "device"
            }
            .into();
            health
        });
        result.nvme_health = record(&mut result, DiskQuery::NvmeHealth, health);
        if native_nvme || result.nvme_identity.is_some() || result.nvme_health.is_some() {
            result.solid_state = Some(true);
            let link = super::pcie::query(index).ok_or(Failure {
                state: DiskQueryState::Unsupported,
                code: None,
            });
            result.pcie_link = record(&mut result, DiskQuery::PcieLink, link);
        } else {
            record::<()>(
                &mut result,
                DiskQuery::PcieLink,
                Err(Failure {
                    state: DiskQueryState::Unsupported,
                    code: None,
                }),
            );
        }
    } else {
        for query in [
            DiskQuery::PcieLink,
            DiskQuery::NvmeIdentity,
            DiskQuery::NvmeHealth,
        ] {
            result.queries.push(DiskQueryStatus {
                query,
                state: DiskQueryState::Unsupported,
                win32_error: None,
            });
        }
    }
    if result.needs_smart_supplement() {
        super::legacy::prediction(handle.0, &mut result);
    }
    result
}

pub(super) fn record<T>(
    details: &mut DiskDetails,
    query: DiskQuery,
    result: Result<T>,
) -> Option<T> {
    match result {
        Ok(value) => {
            details.queries.push(DiskQueryStatus {
                query,
                state: DiskQueryState::Ready,
                win32_error: None,
            });
            Some(value)
        }
        Err(failure) => {
            details.queries.push(DiskQueryStatus {
                query,
                state: failure.state,
                win32_error: failure.code,
            });
            None
        }
    }
}

fn property(handle: HANDLE, property: STORAGE_PROPERTY_ID) -> Result<Vec<u8>> {
    let mut input = [0u8; size_of::<STORAGE_PROPERTY_QUERY>()];
    input[..4].copy_from_slice(&property.0.to_le_bytes());
    input[4..8].copy_from_slice(&PropertyStandardQuery.0.to_le_bytes());
    control(handle, IOCTL_STORAGE_QUERY_PROPERTY, &input, PROPERTY_LIMIT)
}

pub(super) fn control(handle: HANDLE, code: u32, input: &[u8], capacity: usize) -> Result<Vec<u8>> {
    let mut output = vec![0; capacity];
    let mut returned = 0;
    unsafe {
        DeviceIoControl(
            handle,
            code,
            (!input.is_empty()).then_some(input.as_ptr().cast()),
            input.len() as u32,
            Some(output.as_mut_ptr().cast()),
            output.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .map_err(Failure::native)?;
    if returned as usize > output.len() {
        return Err(Failure::invalid());
    }
    output.truncate(returned as usize);
    Ok(output)
}

/// NVMe protocol queries are answered by the adapter's miniport driver, like
/// the periodic temperature query (see `disk::temperature_query_allowed`).
/// A RAID-class controller serves the system disk too, and an Intel RST
/// release that leaves such a request pending stalls it for all of Windows,
/// so disks behind one keep to the queries the class driver answers.
fn probe_nvme(bus: &str, solid_state: Option<bool>) -> bool {
    bus == "nvme"
        || solid_state != Some(false) && matches!(bus, "scsi" | "usb")
        || bus == "other" && solid_state == Some(true)
}

/// Try the existing controller query first. Some drivers only implement device
/// properties. On bridges/RAID, keep requests device-scoped to avoid another LUN.
fn nvme_query<T>(
    native: bool,
    mut query: impl FnMut(STORAGE_PROPERTY_ID) -> Result<T>,
) -> Result<(T, STORAGE_PROPERTY_ID)> {
    if !native {
        return query(StorageDeviceProtocolSpecificProperty)
            .map(|value| (value, StorageDeviceProtocolSpecificProperty));
    }
    let first = match query(StorageAdapterProtocolSpecificProperty) {
        Ok(value) => return Ok((value, StorageAdapterProtocolSpecificProperty)),
        Err(error) => error,
    };
    match query(StorageDeviceProtocolSpecificProperty) {
        Ok(value) => Ok((value, StorageDeviceProtocolSpecificProperty)),
        // Preserve a meaningful driver/data failure if fallback is unsupported.
        Err(second) if second.state == DiskQueryState::Unsupported => Err(first),
        Err(second) => Err(second),
    }
}

fn protocol(
    handle: HANDLE,
    scope: STORAGE_PROPERTY_ID,
    data_type: u32,
    request: u32,
    length: usize,
) -> Result<Vec<u8>> {
    let mut buffer = vec![0u8; PROTOCOL_OFFSET + PROTOCOL_SIZE + length];
    for (offset, value) in [
        (0, scope.0 as u32),
        (4, PropertyStandardQuery.0 as u32),
        (PROTOCOL_OFFSET, ProtocolTypeNvme.0 as u32),
        (PROTOCOL_OFFSET + 4, data_type),
        (PROTOCOL_OFFSET + 8, request),
        (PROTOCOL_OFFSET + 16, PROTOCOL_SIZE as u32),
        (PROTOCOL_OFFSET + 20, length as u32),
    ] {
        buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut returned = 0;
    // The Windows storage property API supports a shared input/output buffer.
    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(buffer.as_ptr().cast()),
            buffer.len() as u32,
            Some(buffer.as_mut_ptr().cast()),
            buffer.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .map_err(Failure::native)?;
    let returned = buffer
        .get(..returned as usize)
        .ok_or_else(Failure::invalid)?;
    protocol_payload(returned, data_type, length).map(<[u8]>::to_vec)
}

fn protocol_payload(data: &[u8], data_type: u32, length: usize) -> Result<&[u8]> {
    if data.len() < DESCRIPTOR_SIZE
        || u32_at(data, 0)? as usize != DESCRIPTOR_SIZE
        || u32_at(data, 4)? as usize != DESCRIPTOR_SIZE
        || u32_at(data, PROTOCOL_OFFSET)? != ProtocolTypeNvme.0 as u32
        || u32_at(data, PROTOCOL_OFFSET + 4)? != data_type
    {
        return Err(Failure::invalid());
    }
    let offset = u32_at(data, PROTOCOL_OFFSET + 16)? as usize;
    let returned_length = u32_at(data, PROTOCOL_OFFSET + 20)? as usize;
    if offset < PROTOCOL_SIZE || returned_length < length {
        return Err(Failure::invalid());
    }
    let start = PROTOCOL_OFFSET
        .checked_add(offset)
        .ok_or_else(Failure::invalid)?;
    let end = start
        .checked_add(returned_length)
        .ok_or_else(Failure::invalid)?;
    data.get(start..end)
        .and_then(|payload| payload.get(..length))
        .ok_or_else(Failure::invalid)
}

fn descriptor(data: &[u8], minimum: usize) -> Result<&[u8]> {
    let size = u32_at(data, 4)? as usize;
    if size < minimum || u32_at(data, 0)? < minimum as u32 {
        return Err(Failure::invalid());
    }
    data.get(..size).ok_or_else(Failure::invalid)
}

fn identity(data: &[u8]) -> Result<DiskIdentity> {
    let data = descriptor(data, 36)?;
    let string_at = |offset| -> Result<Option<String>> {
        let offset = u32_at(data, offset)? as usize;
        if offset == 0 {
            return Ok(None);
        }
        if offset < 36 {
            return Err(Failure::invalid());
        }
        let tail = data.get(offset..).ok_or_else(Failure::invalid)?;
        let end = tail
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(Failure::invalid)?;
        Ok(nvme::text(&tail[..end]))
    };
    let vendor = string_at(12)?;
    let product = string_at(16)?;
    let model = match (vendor, product) {
        (Some(v), Some(p)) if !p.starts_with(&v) => Some(format!("{v} {p}")),
        (_, Some(p)) => Some(p),
        (v, _) => v,
    };
    let bus_code = u32_at(data, 28)?;
    Ok(DiskIdentity {
        model,
        serial_number: string_at(24)?,
        firmware_revision: string_at(20)?,
        bus: super::super::disk::bus_name(bus_code).into(),
        bus_code,
        removable: boolean(data[10])?,
    })
}

fn boolean(value: u8) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Failure::invalid()),
    }
}

fn bytes<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N]> {
    data.get(offset..offset.checked_add(N).ok_or_else(Failure::invalid)?)
        .and_then(|value| value.try_into().ok())
        .ok_or_else(Failure::invalid)
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    bytes(data, offset).map(u32::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvme_driver_fallback_keeps_bridge_queries_device_scoped() {
        let mut calls = Vec::new();
        let value = nvme_query(true, |scope| {
            calls.push(scope);
            if scope == StorageAdapterProtocolSpecificProperty {
                Err(Failure::invalid())
            } else {
                Ok(42)
            }
        })
        .unwrap();
        assert_eq!(value, (42, StorageDeviceProtocolSpecificProperty));
        assert_eq!(
            calls,
            [
                StorageAdapterProtocolSpecificProperty,
                StorageDeviceProtocolSpecificProperty
            ]
        );
        calls.clear();
        let value = nvme_query(false, |scope| {
            calls.push(scope);
            Ok(7)
        })
        .unwrap();
        assert_eq!(value, (7, StorageDeviceProtocolSpecificProperty));
        assert_eq!(calls, [StorageDeviceProtocolSpecificProperty]);
        calls.clear();
        assert_eq!(
            nvme_query(true, |scope| {
                calls.push(scope);
                Ok(9)
            })
            .unwrap(),
            (9, StorageAdapterProtocolSpecificProperty)
        );
        assert_eq!(calls, [StorageAdapterProtocolSpecificProperty]);
        let error = nvme_query::<()>(true, |scope| {
            Err(if scope == StorageAdapterProtocolSpecificProperty {
                Failure::invalid()
            } else {
                Failure {
                    state: DiskQueryState::Unsupported,
                    code: Some(50),
                }
            })
        })
        .unwrap_err();
        assert_eq!(error.state, DiskQueryState::InvalidData);
        assert!(probe_nvme("nvme", None));
        assert!(probe_nvme("usb", Some(true)));
        assert!(probe_nvme("scsi", None));
        // A RAID-class miniport must never receive an NVMe request from here.
        assert!(!probe_nvme("raid", Some(true)));
        assert!(!probe_nvme("raid", None));
        assert!(!probe_nvme("scsi", Some(false)));
        assert!(!probe_nvme("ata", None));
        assert!(!probe_nvme("virtual", Some(true)));
    }

    fn response() -> Vec<u8> {
        let mut data = vec![0; DESCRIPTOR_SIZE + 512];
        for (offset, value) in [
            (0, DESCRIPTOR_SIZE as u32),
            (4, DESCRIPTOR_SIZE as u32),
            (8, ProtocolTypeNvme.0 as u32),
            (12, NVMeDataTypeLogPage.0 as u32),
            (24, PROTOCOL_SIZE as u32),
            (28, 512),
        ] {
            data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        data
    }

    #[test]
    fn protocol_uses_returned_length_and_rejects_out_of_bounds_descriptors() {
        let valid = response();
        assert_eq!(protocol_payload(&valid, 2, 512).unwrap().len(), 512);
        for len in 0..valid.len() {
            assert!(protocol_payload(&valid[..len], 2, 512).is_err());
        }
        for (offset, value) in [
            (24, 39),
            (24, u32::MAX),
            (28, 511),
            (28, u32::MAX),
            (8, 2),
            (4, 0),
        ] {
            let mut data = valid.clone();
            data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(protocol_payload(&data, 2, 512).is_err());
        }
    }

    #[test]
    fn identity_offsets_cannot_read_padding_or_descriptor_fields() {
        let mut data = vec![0; 64];
        data[0..4].copy_from_slice(&40u32.to_le_bytes());
        data[4..8].copy_from_slice(&64u32.to_le_bytes());
        data[16..20].copy_from_slice(&40u32.to_le_bytes());
        data[28..32].copy_from_slice(&17u32.to_le_bytes());
        data[40..45].copy_from_slice(b"NVMe\0");
        assert_eq!(identity(&data).unwrap().model.as_deref(), Some("NVMe"));
        assert!(identity(&data[..44]).is_err());
        for offset in [12u32, 64, u32::MAX] {
            data[16..20].copy_from_slice(&offset.to_le_bytes());
            assert!(identity(&data).is_err());
        }
    }
}
