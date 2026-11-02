//! Read the matched disk controller's PnP link properties, never PCI registers.

use std::mem::{offset_of, size_of};

use serde::Serialize;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_PropertyW, CM_Get_Device_IDW, CM_Get_Parent, CR_SUCCESS, DIGCF_DEVICEINTERFACE,
    DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows::Win32::Devices::Properties::{DEVPROP_TYPE_UINT32, DEVPROPTYPE};
use windows::Win32::Foundation::DEVPROPKEY;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::IOCTL_STORAGE_GET_DEVICE_NUMBER;
use windows::core::{GUID, PCWSTR};

use crate::util::{AlignedBuf, OwnedHandle};

const DISK_INTERFACE: GUID = GUID::from_u128(0x53f56307_b6bf_11d0_94f2_00a0c91efb8b);
// Windows SDK pciprop.h, DEVPKEY_PciDevice_{Current,Max}Link{Speed,Width}.
const PCI_PROPERTIES: GUID = GUID::from_u128(0x3ab22e31_8264_4b4e_9af5_a8d2d8e33e62);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PcieLink {
    pub current: Option<PcieLinkRate>,
    /// Controller endpoint capability; this is not the motherboard slot maximum.
    pub maximum: Option<PcieLinkRate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PcieLinkRate {
    /// PCIe Link Speed encoding: 1 = 2.5 GT/s, 2 = 5, 3 = 8, 4 = 16, etc.
    pub generation: u32,
    pub lanes: u32,
}

struct DeviceSet(HDEVINFO);

impl Drop for DeviceSet {
    fn drop(&mut self) {
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

pub(super) fn query(index: u32) -> Option<PcieLink> {
    let mut node = disk_node(index)?;
    for _ in 0..16 {
        let mut id = [0u16; 512];
        if unsafe { CM_Get_Device_IDW(node, &mut id, 0) } != CR_SUCCESS {
            return None;
        }
        if id.starts_with(&[b'P' as u16, b'C' as u16, b'I' as u16, b'\\' as u16]) {
            // Stop at the endpoint even if it exposes no link properties; a bridge's
            // link would describe another connection in the device tree.
            let current = rate(property(node, 9), property(node, 10));
            let maximum = rate(property(node, 11), property(node, 12));
            return (current.is_some() || maximum.is_some())
                .then_some(PcieLink { current, maximum });
        }
        let mut parent = 0;
        if unsafe { CM_Get_Parent(&mut parent, node, 0) } != CR_SUCCESS || parent == node {
            return None;
        }
        node = parent;
    }
    None
}

fn property(node: u32, pid: u32) -> Option<u32> {
    let key = DEVPROPKEY {
        fmtid: PCI_PROPERTIES,
        pid,
    };
    let mut value = [0u8; 4];
    let mut size = value.len() as u32;
    let mut kind = DEVPROPTYPE::default();
    let status = unsafe {
        CM_Get_DevNode_PropertyW(
            node,
            &key,
            &mut kind,
            Some(value.as_mut_ptr()),
            &mut size,
            0,
        )
    };
    (status == CR_SUCCESS && size == 4 && kind == DEVPROP_TYPE_UINT32)
        .then(|| u32::from_le_bytes(value))
}

fn rate(generation: Option<u32>, lanes: Option<u32>) -> Option<PcieLinkRate> {
    let generation = generation?;
    let lanes = lanes?;
    ((1..=7).contains(&generation) && matches!(lanes, 1 | 2 | 4 | 8 | 12 | 16 | 32))
        .then_some(PcieLinkRate { generation, lanes })
}

fn disk_node(index: u32) -> Option<u32> {
    let set = DeviceSet(unsafe {
        SetupDiGetClassDevsW(
            Some(&DISK_INTERFACE),
            None,
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
        .ok()?
    });
    for ordinal in 0..256 {
        let mut interface = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if unsafe {
            SetupDiEnumDeviceInterfaces(set.0, None, &DISK_INTERFACE, ordinal, &mut interface)
        }
        .is_err()
        {
            break;
        }
        let mut needed = 0;
        unsafe {
            let _ = SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &interface,
                None,
                0,
                Some(&mut needed),
                None,
            );
        }
        if !(size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32..=64 * 1024).contains(&needed) {
            continue;
        }
        let mut storage = AlignedBuf::with_capacity(needed as usize);
        let detail = storage
            .as_mut_ptr()
            .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
        let mut info = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        unsafe {
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        }
        if unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &interface,
                Some(detail),
                needed,
                None,
                Some(&mut info),
            )
        }
        .is_err()
        {
            continue;
        }
        let path_offset = offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);
        // Check the terminator within the returned allocation before calling CreateFile.
        let path = unsafe {
            std::slice::from_raw_parts(
                storage.as_ptr().add(path_offset).cast::<u16>(),
                (needed as usize - path_offset) / 2,
            )
        };
        if !path.contains(&0) {
            continue;
        }
        let handle = unsafe {
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
        .ok()
        .map(OwnedHandle);
        let Some(handle) = handle else {
            continue;
        };
        let Ok(number) = super::io::control(handle.0, IOCTL_STORAGE_GET_DEVICE_NUMBER, &[], 12)
        else {
            continue;
        };
        if number.len() == 12
            && u32::from_le_bytes(number[..4].try_into().ok()?) == 7 // FILE_DEVICE_DISK
            && u32::from_le_bytes(number[4..8].try_into().ok()?) == index
        {
            return Some(info.DevInst);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_requires_both_valid_speed_and_width() {
        assert_eq!(
            rate(Some(4), Some(4)),
            Some(PcieLinkRate {
                generation: 4,
                lanes: 4
            })
        );
        assert!(rate(Some(0), Some(4)).is_none());
        assert!(rate(Some(4), Some(0)).is_none());
        assert!(rate(Some(4), None).is_none());
        assert!(rate(None, Some(4)).is_none());
    }
}
