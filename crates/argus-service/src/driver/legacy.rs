//! The signed 2.0 drivers have no VERSIONINFO. Identify only exact official
//! payloads; neither a filename nor a missing resource grants migration rights.
use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};
use windows::Win32::System::Registry::*;
use windows::core::{PCWSTR, w};

use super::{FOREIGN_DRIVER, LEGACY_LAYOUT};
use crate::security;
use crate::util::{Result, io, wide};

// Signed ARM64 and x64 payloads extracted from the official 2.0.1 installer.
// The installer itself is pinned separately in legacy-installer.json.
const IMAGES: [(u64, &str); 2] = [
    (
        600864,
        "815b457d490d4f18ffd2d7e98669f195940ab76e418f0f066e8c9e0746eac216",
    ),
    (
        85344,
        "e19f274b9f3c917445bff19a80bbe2ee34a54995e62be50073122b20d86af900",
    ),
];

pub(super) fn verify_image(path: &Path) -> Result<()> {
    let file = File::open(path).map_err(io)?;
    let size = file.metadata().map_err(io)?.len();
    let Some((_, expected)) = IMAGES.iter().find(|(length, _)| *length == size) else {
        return Err(FOREIGN_DRIVER);
    };
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(size + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 != size || format!("{:x}", Sha256::digest(&bytes)) != *expected {
        return Err(FOREIGN_DRIVER);
    }
    Ok(())
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn string(key: HKEY, name: &str) -> Result<String> {
    let mut data = [0u16; 1024];
    let mut size = size_of_val(&data) as u32;
    unsafe {
        RegGetValueW(
            key,
            None,
            PCWSTR(wide(name).as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .ok()
    .map_err(crate::util::win)?;
    if size < 2 || size as usize > size_of_val(&data) || !size.is_multiple_of(2) {
        return Err(LEGACY_LAYOUT);
    }
    let text = String::from_utf16(&data[..size as usize / 2 - 1]).map_err(|_| LEGACY_LAYOUT)?;
    if text.contains('\0') {
        return Err(LEGACY_LAYOUT);
    }
    Ok(text)
}

fn installation_matches(location: &str, version: &str, expected: &Path) -> bool {
    matches!(version, "2.0.0" | "2.0.0.0" | "2.0.1" | "2.0.1.0")
        && Path::new(location)
            .as_os_str()
            .eq_ignore_ascii_case(expected.as_os_str())
}

/// The legacy installer recursively removes its registered InstallLocation.
/// Check both views so it cannot select a newer installation or another path.
/// No uninstall command or executable path is read from the registry.
pub(super) fn validate_installation(programs: &Path) -> Result<()> {
    let mut found = false;
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        let mut key = HKEY::default();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                w!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO"),
                None,
                KEY_QUERY_VALUE | REG_SAM_FLAGS(0x20000) | view,
                &mut key,
            )
        };
        if status.0 == 2 {
            continue;
        }
        status.ok().map_err(crate::util::win)?;
        let key = Key(key);
        security::inspect_registry_key(key.0).map_err(|_| LEGACY_LAYOUT)?;
        let location = string(key.0, "InstallLocation").map_err(|_| LEGACY_LAYOUT)?;
        let version = string(key.0, "DisplayVersion").map_err(|_| LEGACY_LAYOUT)?;
        if !installation_matches(&location, &version, &programs.join("PawnIO")) {
            return Err(LEGACY_LAYOUT);
        }
        found = true;
    }
    if found { Ok(()) } else { Err(LEGACY_LAYOUT) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_version_and_expected_file_size_do_not_establish_identity() {
        let path = std::env::temp_dir().join(format!("argus-legacy-image-{:?}.sys", unsafe {
            windows::Win32::System::Com::CoCreateGuid().unwrap()
        }));
        std::fs::write(&path, vec![0u8; IMAGES[1].0 as usize]).unwrap();
        let result = verify_image(&path);
        std::fs::remove_file(path).unwrap();
        assert_eq!(result, Err(FOREIGN_DRIVER));
    }

    #[test]
    fn migration_never_targets_a_newer_version_or_another_directory() {
        let directory = Path::new(r"C:\Program Files\PawnIO");
        for version in ["2.0.0.0", "2.0.1.0"] {
            assert!(installation_matches(
                r"C:\PROGRAM FILES\PawnIO",
                version,
                directory
            ));
        }
        for version in ["2.1.0.0", "2.2.0.0", "", "2.0.9.0"] {
            assert!(!installation_matches(
                r"C:\Program Files\PawnIO",
                version,
                directory
            ));
        }
        for location in [
            r"C:\Program Files",
            r"C:\Program Files\PawnIO\..",
            r"C:\Users\Public\PawnIO",
        ] {
            assert!(!installation_matches(location, "2.0.1.0", directory));
        }
    }
}
