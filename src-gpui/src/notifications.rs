//! Windows notifications under Argus's own name and icon.
//!
//! The vendored GPUI shows them (`App::show_system_notification`) for the
//! process's AppUserModelID, which `frontend::run` sets to the app identifier
//! as the installer's shortcuts carry it, and registers that ID under
//! `HKCU\Software\Classes\AppUserModelId` with a display name. An app without
//! package identity also names its icon there, or its notifications show
//! none: `register` keeps the icon in the app's local data folder and points
//! `IconUri` at it. The uninstaller removes the key.
use std::fs;

use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
use windows::core::PCWSTR;

use crate::native_runtime::Host;

/// The tag of the notification that says an update waits.
pub const UPDATE: &str = "update";

const ICON_FILE: &str = "notification-icon.png";
const APP_ICON: &[u8] = include_bytes!("../../resources/icons/icon.ico");

/// Registers the name and icon notifications show. Writes a file and the
/// registry: not on the interface thread.
pub fn register(host: &Host) {
    let Ok(dir) = host.path().app_local_data_dir() else {
        return;
    };
    let Some(png) = largest_png(APP_ICON) else {
        return;
    };
    let icon = dir.join(ICON_FILE);
    if fs::read(&icon).ok().as_deref() != Some(png)
        && let Err(error) = fs::create_dir_all(&dir).and_then(|()| fs::write(&icon, png))
    {
        argus_core::diagnostics::file_failure("notification icon write", &icon, &error);
        return;
    }
    let key = wide(&format!(
        r"Software\Classes\AppUserModelId\{}",
        host.config().identifier
    ));
    let values = [
        ("DisplayName", host.package_info().name.clone()),
        ("IconUri", icon.display().to_string()),
    ];
    for (name, value) in values {
        let name_wide = wide(name);
        let data = wide(&value);
        let result = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name_wide.as_ptr()),
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                (data.len() * 2) as u32,
            )
        };
        if result.is_err() {
            argus_core::diagnostics::warn(
                "notification registration",
                format!("value={name}; error={}", result.0),
            );
            return;
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The largest PNG image in an icon file.
fn largest_png(ico: &[u8]) -> Option<&[u8]> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    let count = u16::from_le_bytes(ico.get(4..6)?.try_into().ok()?) as usize;
    (0..count)
        .filter_map(|index| {
            let entry = ico.get(6 + 16 * index..6 + 16 * (index + 1))?;
            // A width of 0 stands for 256.
            let width = match entry[0] {
                0 => 256,
                width => u32::from(width),
            };
            let size = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
            let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
            let image = ico.get(offset..offset.checked_add(size)?)?;
            image.starts_with(SIGNATURE).then_some((width, image))
        })
        .max_by_key(|(width, _)| *width)
        .map(|(_, image)| image)
}

#[cfg(test)]
mod tests {
    use super::{APP_ICON, largest_png};

    #[test]
    fn the_notification_icon_is_the_largest_image_of_the_app_icon() {
        let png = largest_png(APP_ICON).expect("a PNG image in the app icon");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        // The IHDR chunk's width: the 256-pixel image.
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 256);
    }

    #[test]
    fn a_damaged_icon_has_no_image() {
        assert_eq!(largest_png(&[]), None);
        // One entry whose image lies past the end of the file.
        let mut ico = vec![0, 0, 1, 0, 1, 0];
        ico.extend([0; 8]);
        ico.extend(16u32.to_le_bytes());
        ico.extend(u32::MAX.to_le_bytes());
        assert_eq!(largest_png(&ico), None);
    }
}
