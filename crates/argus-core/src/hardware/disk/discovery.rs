//! Present disk interfaces complement drive letters and legacy device names.
use super::{DeviceHandle, ioctl, native_code};
use crate::util::{AlignedBuf, from_wide};
use std::collections::HashMap;
use std::mem::{offset_of, size_of};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows::Win32::System::Ioctl::{GUID_DEVINTERFACE_DISK, IOCTL_STORAGE_GET_DEVICE_NUMBER};

struct Devices(HDEVINFO);
impl Drop for Devices {
    fn drop(&mut self) {
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

fn warn(stage: &'static str, ordinal: u32, code: u32) {
    crate::diagnostics::warn(
        stage,
        format!("interface={ordinal}; {}", crate::diagnostics::code(code)),
    );
}

pub(super) fn disks() -> HashMap<u32, DeviceHandle> {
    let mut out = HashMap::new();
    let set = unsafe {
        SetupDiGetClassDevsW(
            Some(&GUID_DEVINTERFACE_DISK),
            None,
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    let set = match set {
        Ok(set) => Devices(set),
        Err(error) => {
            warn("disk.enumeration", 0, native_code(error));
            return out;
        }
    };
    for ordinal in 0..4096 {
        let mut interface = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if let Err(error) = unsafe {
            SetupDiEnumDeviceInterfaces(
                set.0,
                None,
                &GUID_DEVINTERFACE_DISK,
                ordinal,
                &mut interface,
            )
        } {
            let code = native_code(error);
            if code != 259 {
                warn("disk.enumeration", ordinal, code);
            }
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
            warn("disk.interface.size", ordinal, 13);
            continue;
        }
        let mut data = AlignedBuf::with_capacity(needed as usize);
        let detail = data
            .as_mut_ptr()
            .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
        unsafe {
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        }
        if let Err(error) = unsafe {
            SetupDiGetDeviceInterfaceDetailW(set.0, &interface, Some(detail), needed, None, None)
        } {
            warn("disk.interface.detail", ordinal, native_code(error));
            continue;
        }
        let offset = offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);
        let path = unsafe {
            std::slice::from_raw_parts(
                data.as_ptr().add(offset).cast::<u16>(),
                (needed as usize - offset) / 2,
            )
        };
        if !path.contains(&0) {
            warn("disk.interface.path", ordinal, 13);
            continue;
        }
        let handle = match DeviceHandle::open(&from_wide(path)) {
            Ok(handle) => handle,
            Err(code) => {
                warn("disk.interface.open", ordinal, code);
                continue;
            }
        };
        let mut number = AlignedBuf::with_capacity(12);
        match handle.read(|raw, _| ioctl(raw, IOCTL_STORAGE_GET_DEVICE_NUMBER, None, &mut number)) {
            Ok(12) => {
                let raw = unsafe { std::slice::from_raw_parts(number.as_ptr(), 12) };
                let device = u32::from_le_bytes(raw[0..4].try_into().unwrap());
                let index = u32::from_le_bytes(raw[4..8].try_into().unwrap());
                if device == 7 && index <= 4095 {
                    out.entry(index).or_insert(handle);
                } else {
                    warn("disk.interface.number", ordinal, 13);
                }
            }
            Ok(_) => warn("disk.interface.number", ordinal, 13),
            Err(code) => warn("disk.interface.number", ordinal, code),
        }
    }
    out
}
