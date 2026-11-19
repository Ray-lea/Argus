//! The driver that serves a disk below the class driver, read from the PnP
//! tree. It decides which requests may reach it; the bus type a disk reports
//! does not, because a vendor driver chooses that value itself.
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_PropertyW, CM_Get_Parent, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::{DEVPROP_TYPE_STRING, DEVPROPTYPE};
use windows::Win32::Foundation::DEVPROPKEY;
use windows::core::GUID;

// Windows SDK devpkey.h, DEVPKEY_Device_Service.
const SERVICE: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 6,
};

/// Service name of the driver of a disk's parent device: the storage port or
/// miniport driver its requests go to. None when PnP does not tell.
pub(in crate::hardware) fn of_disk(disk: u32) -> Option<String> {
    let mut parent = 0;
    if unsafe { CM_Get_Parent(&mut parent, disk, 0) } != CR_SUCCESS || parent == disk {
        return None;
    }
    // A service name is a registry key name, at most 255 characters.
    let mut value = [0u8; 512];
    let mut size = value.len() as u32;
    let mut kind = DEVPROPTYPE::default();
    let status = unsafe {
        CM_Get_DevNode_PropertyW(
            parent,
            &SERVICE,
            &mut kind,
            Some(value.as_mut_ptr()),
            &mut size,
            0,
        )
    };
    if status != CR_SUCCESS || kind != DEVPROP_TYPE_STRING {
        return None;
    }
    name(value.get(..size as usize)?)
}

fn name(utf16: &[u8]) -> Option<String> {
    let units: Vec<u16> = utf16
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    let name = String::from_utf16(&units).ok()?;
    (!name.is_empty()).then_some(name)
}

/// The name as it goes into the diagnostic log: a registry key name may hold
/// anything, the log takes only what a driver name is made of.
pub(in crate::hardware) fn label(controller: Option<&str>) -> String {
    let Some(name) = controller else {
        return "unknown".into();
    };
    name.chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '?'
            }
        })
        .collect()
}

fn named(controller: Option<&str>, drivers: &[&str]) -> bool {
    controller.is_some_and(|name| {
        drivers
            .iter()
            .any(|driver| name.eq_ignore_ascii_case(driver))
    })
}

/// Storport forwards `StorageDeviceTemperatureProperty` to the miniport, and
/// the ATA port driver answers it itself. Microsoft's own drivers answer or
/// decline at once; an unknown driver, or none, is not asked.
pub(in crate::hardware) fn answers_temperature(controller: Option<&str>) -> bool {
    named(controller, &["stornvme", "storahci", "atapi"])
}

/// NVMe protocol queries on a native NVMe disk: Microsoft's NVMe miniport.
pub(in crate::hardware) fn answers_nvme(controller: Option<&str>) -> bool {
    named(controller, &["stornvme"])
}

/// Microsoft's USB storage drivers, below a bridge that may hold an NVMe disk.
pub(in crate::hardware) fn bridges_usb(controller: Option<&str>) -> bool {
    named(controller, &["usbstor", "uaspstor"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_microsofts_own_storage_drivers_are_asked() {
        for (driver, temperature, nvme, usb) in [
            ("stornvme", true, true, false),
            ("StorNVMe", true, true, false),
            ("storahci", true, false, false),
            ("atapi", true, false, false),
            ("USBSTOR", false, false, true),
            ("UASPStor", false, false, true),
            // Intel RST and VMD, whatever bus type their disks report.
            ("iaStorVD", false, false, false),
            ("iaStorAC", false, false, false),
            ("iaStorAVC", false, false, false),
            // Vendor NVMe and RAID miniports, virtual and card-reader stacks.
            ("secnvme", false, false, false),
            ("rcbottom", false, false, false),
            ("storvsc", false, false, false),
            ("RTSPER", false, false, false),
            ("stornvme2", false, false, false),
            ("", false, false, false),
        ] {
            let controller = Some(driver);
            assert_eq!(answers_temperature(controller), temperature, "{driver}");
            assert_eq!(answers_nvme(controller), nvme, "{driver}");
            assert_eq!(bridges_usb(controller), usb, "{driver}");
        }
        assert!(!answers_temperature(None) && !answers_nvme(None) && !bridges_usb(None));
    }

    #[test]
    fn service_names_stop_at_the_terminator() {
        let wide = |text: &str| -> Vec<u8> {
            text.encode_utf16()
                .chain([0, b'x' as u16])
                .flat_map(u16::to_le_bytes)
                .collect()
        };
        assert_eq!(name(&wide("iaStorVD")).as_deref(), Some("iaStorVD"));
        assert_eq!(name(&wide("")), None);
        assert_eq!(name(&[]), None);
        // An odd trailing byte is not part of a name.
        assert_eq!(name(&[b'a', 0, b'b']).as_deref(), Some("a"));
    }

    #[test]
    fn logged_names_hold_only_driver_name_characters() {
        assert_eq!(label(Some("iaStorVD")), "iaStorVD");
        assert_eq!(label(Some("a b;c=d\n")), "a?b?c?d?");
        assert_eq!(label(Some(&"x".repeat(200))).len(), 64);
        assert_eq!(label(None), "unknown");
    }
}
