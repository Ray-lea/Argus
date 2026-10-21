//! The signed 2.0 drivers have no VERSIONINFO. Identify only exact official
//! payloads; neither a filename nor a missing resource grants migration rights.
use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};
use windows::Win32::System::Registry::*;
use windows::core::{PCWSTR, PWSTR};

use super::{FOREIGN_DRIVER, Inspection, LEGACY_LAYOUT};
use crate::security;
use crate::util::{Result, io, wide};

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO";
const REGISTRY_DELETE: REG_SAM_FLAGS = REG_SAM_FLAGS(0x10000);

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtDeleteKey(key: HKEY) -> windows::Win32::Foundation::NTSTATUS;
}

fn delete_key(key: HKEY) -> Result<()> {
    let status = unsafe { NtDeleteKey(key) };
    if status.0 >= 0 {
        Ok(())
    } else {
        Err(unsafe { windows::Win32::Foundation::RtlNtStatusToDosError(status) })
    }
}

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

pub(super) fn verify_image(path: &Path, trace: &mut Inspection) -> Result<()> {
    let file = File::open(path).map_err(io)?;
    let size = file.metadata().map_err(io)?.len();
    trace.legacy_image_bytes = Some(size);
    // Bound failure diagnostics too; known official images are below 1 MiB.
    if size > 1024 * 1024 {
        return Err(FOREIGN_DRIVER);
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(size + 1).read_to_end(&mut bytes).map_err(io)?;
    let hash = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    trace.legacy_image_sha256 = Some(hash.clone());
    if bytes.len() as u64 != size
        || !IMAGES
            .iter()
            .any(|(length, expected)| *length == size && *expected == hash)
    {
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

fn validate_key(key: HKEY, programs: &Path) -> Result<()> {
    // Default Uninstall ACLs include Terminal Server User write access. The
    // elevated helper replaces this record with a protected copy before use.
    let mut trace = security::PathInspection::default();
    match security::inspect_registry_key(key, &mut trace) {
        Ok(()) | Err(security::PathError::Writable) => {}
        Err(error) => {
            argus_core::diagnostics::warn(
                "PawnIO legacy uninstall key security",
                format!("reason={error:?}; key={UNINSTALL:?}; check={trace:?}"),
            );
            return Err(LEGACY_LAYOUT);
        }
    }
    let location = crate::util::stage(
        "PawnIO legacy InstallLocation read",
        string(key, "InstallLocation"),
    )
    .map_err(|_| LEGACY_LAYOUT)?;
    let version = crate::util::stage(
        "PawnIO legacy DisplayVersion read",
        string(key, "DisplayVersion"),
    )
    .map_err(|_| LEGACY_LAYOUT)?;
    if !installation_matches(&location, &version, &programs.join("PawnIO")) {
        let version = if version.len() <= 32
            && version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            version.as_str()
        } else {
            "invalid version format"
        };
        argus_core::diagnostics::warn(
            "PawnIO legacy installation identity",
            format!(
                "key={UNINSTALL:?}; version={version}; location={}; expected_location={}",
                argus_core::diagnostics::path(Path::new(&location)),
                argus_core::diagnostics::path(&programs.join("PawnIO"))
            ),
        );
        return Err(LEGACY_LAYOUT);
    }
    Ok(())
}

struct Value {
    name: Vec<u16>,
    kind: REG_VALUE_TYPE,
    data: Vec<u8>,
}

fn values(key: HKEY) -> Result<Vec<Value>> {
    let mut children = 0;
    unsafe {
        RegQueryInfoKeyW(
            key,
            None,
            None,
            None,
            Some(&mut children),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }
    .ok()
    .map_err(crate::util::win)?;
    if children != 0 {
        argus_core::diagnostics::warn(
            "PawnIO legacy uninstall record",
            format!("unexpected_subkeys={children}"),
        );
        return Err(LEGACY_LAYOUT);
    }
    let mut values = Vec::new();
    for index in 0..128 {
        let mut name = [0u16; 256];
        let mut name_len = name.len() as u32;
        let mut data = vec![0u8; 8192];
        let mut bytes = data.len() as u32;
        let mut kind = 0;
        let status = unsafe {
            RegEnumValueW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut bytes),
            )
        };
        if status.0 == 259 {
            return Ok(values);
        }
        status.ok().map_err(crate::util::win)?;
        if kind == REG_LINK.0 || name_len >= name.len() as u32 || bytes > data.len() as u32 {
            return Err(LEGACY_LAYOUT);
        }
        data.truncate(bytes as usize);
        let mut name = name[..name_len as usize].to_vec();
        name.push(0);
        values.push(Value {
            name,
            kind: REG_VALUE_TYPE(kind),
            data,
        });
    }
    Err(LEGACY_LAYOUT)
}

/// Changing a DACL does not revoke existing write handles. Replace the key
/// object instead, keeping the original for rollback until the copy is complete.
fn replace_record(
    root: HKEY,
    path: &str,
    view: REG_SAM_FLAGS,
    descriptor: &security::Descriptor,
    validate: impl Fn(HKEY, bool) -> Result<()>,
) -> Result<bool> {
    let path_wide = wide(path);
    let mut old = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(path_wide.as_ptr()),
            Some(REG_OPTION_OPEN_LINK.0),
            KEY_READ | KEY_WRITE | REGISTRY_DELETE | view,
            &mut old,
        )
    };
    if status.0 == 2 {
        return Ok(false);
    }
    status.ok().map_err(crate::util::win)?;
    let old = Key(old);
    validate(old.0, false)?;
    let values = values(old.0)?;
    let backup = wide(&format!(
        "Argus-PawnIO-{:?}",
        unsafe { windows::Win32::System::Com::CoCreateGuid() }.map_err(crate::util::win)?
    ));
    unsafe { RegRenameKey(old.0, None, PCWSTR(backup.as_ptr())) }
        .ok()
        .map_err(crate::util::win)?;
    let mut replacement = None;
    let result = (|| {
        let mut new = HKEY::default();
        let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
        unsafe {
            RegCreateKeyExW(
                root,
                PCWSTR(path_wide.as_ptr()),
                None,
                None,
                REG_OPTION_OPEN_LINK,
                KEY_ALL_ACCESS | view,
                Some(&descriptor.attributes()),
                &mut new,
                Some(&mut disposition),
            )
        }
        .ok()
        .map_err(crate::util::win)?;
        let new = Key(new);
        // Never change a key that another process inserted during the rename.
        if disposition != REG_CREATED_NEW_KEY {
            return Err(LEGACY_LAYOUT);
        }
        replacement = Some(new);
        let new = replacement.as_ref().unwrap().0;
        for value in &values {
            unsafe {
                RegSetValueExW(
                    new,
                    PCWSTR(value.name.as_ptr()),
                    None,
                    value.kind,
                    Some(&value.data),
                )
            }
            .ok()
            .map_err(crate::util::win)?;
        }
        validate(new, true)?;
        delete_key(old.0)?;
        Ok(true)
    })();
    if result.is_err() {
        if let Some(new) = replacement.take() {
            delete_key(new.0)?;
            drop(new);
        }
        let original = wide(path.rsplit('\\').next().ok_or(LEGACY_LAYOUT)?);
        unsafe { RegRenameKey(old.0, None, PCWSTR(original.as_ptr())) }
            .ok()
            .map_err(crate::util::win)?;
    }
    result
}

pub(super) fn protect_installation(programs: &Path) -> Result<()> {
    validate_installation(programs)?;
    let descriptor = security::Descriptor::new("O:BAG:BAD:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;KR;;;BU)")?;
    let mut found = false;
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        found |= replace_record(
            HKEY_LOCAL_MACHINE,
            UNINSTALL,
            view,
            &descriptor,
            |key, copy| {
                if copy {
                    let mut trace = security::PathInspection::default();
                    security::inspect_registry_key(key, &mut trace).map_err(|error| {
                        argus_core::diagnostics::warn(
                            "PawnIO migrated uninstall key security",
                            format!("reason={error:?}; key={UNINSTALL:?}; check={trace:?}"),
                        );
                        LEGACY_LAYOUT
                    })?;
                }
                validate_key(key, programs)
            },
        )?;
    }
    if found { Ok(()) } else { Err(LEGACY_LAYOUT) }
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
                PCWSTR(wide(UNINSTALL).as_ptr()),
                Some(REG_OPTION_OPEN_LINK.0),
                KEY_QUERY_VALUE | REG_SAM_FLAGS(0x20000) | view,
                &mut key,
            )
        };
        if status.0 == 2 {
            continue;
        }
        status.ok().map_err(crate::util::win)?;
        let key = Key(key);
        validate_key(key.0, programs)?;
        found = true;
    }
    if found { Ok(()) } else { Err(LEGACY_LAYOUT) }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RegistryFixture {
        path: Vec<u16>,
        root: Key,
    }
    impl RegistryFixture {
        fn new() -> Self {
            let path = wide(&format!(
                r"Software\Argus-PawnIO-Test-{:?}",
                unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap()
            ));
            let root = Self::create(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()));
            Self { path, root }
        }
        fn create(root: HKEY, path: PCWSTR) -> Key {
            let mut key = HKEY::default();
            unsafe {
                RegCreateKeyExW(
                    root,
                    path,
                    None,
                    None,
                    REG_OPTION_NON_VOLATILE,
                    KEY_ALL_ACCESS,
                    None,
                    &mut key,
                    None,
                )
            }
            .ok()
            .unwrap();
            Key(key)
        }
        fn child(&self, path: &str) -> Key {
            Self::create(self.root.0, PCWSTR(wide(path).as_ptr()))
        }
        fn descriptor() -> security::Descriptor {
            let sid = security::current_sid().unwrap();
            security::Descriptor::new(&format!("O:{sid}D:P(A;;KA;;;{sid})")).unwrap()
        }
    }
    impl Drop for RegistryFixture {
        fn drop(&mut self) {
            unsafe {
                let _ = RegDeleteTreeW(self.root.0, None);
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(self.path.as_ptr()));
            }
        }
    }
    fn set_text(key: HKEY, text: &str) -> u32 {
        let data = wide(text);
        unsafe {
            RegSetValueExW(
                key,
                PCWSTR(wide("InstallLocation").as_ptr()),
                None,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    data.as_ptr().cast(),
                    data.len() * 2,
                )),
            )
        }
        .0
    }

    #[test]
    fn replacing_uninstall_metadata_revokes_old_handles_without_losing_values() {
        let fixture = RegistryFixture::new();
        let old = fixture.child("PawnIO");
        assert_eq!(set_text(old.0, "original"), 0);
        assert_eq!(
            replace_record(
                fixture.root.0,
                "PawnIO",
                REG_SAM_FLAGS(0),
                &RegistryFixture::descriptor(),
                |copy, replaced| {
                    if replaced {
                        assert_eq!(set_text(old.0, "changed through an earlier handle"), 0);
                        assert_eq!(string(copy, "InstallLocation").unwrap(), "original");
                    }
                    Ok(())
                }
            ),
            Ok(true)
        );
        assert_eq!(set_text(old.0, "late write"), 1018);
        let current = fixture.child("PawnIO");
        assert_eq!(string(current.0, "InstallLocation").unwrap(), "original");
    }

    #[test]
    fn failed_metadata_copy_restores_the_original_record() {
        let fixture = RegistryFixture::new();
        let old = fixture.child("PawnIO");
        assert_eq!(set_text(old.0, "original"), 0);
        assert_eq!(
            replace_record(
                fixture.root.0,
                "PawnIO",
                REG_SAM_FLAGS(0),
                &RegistryFixture::descriptor(),
                |_, copy| { if copy { Err(5) } else { Ok(()) } }
            ),
            Err(5)
        );
        assert_eq!(set_text(old.0, "still the original object"), 0);
        let current = fixture.child("PawnIO");
        assert_eq!(
            string(current.0, "InstallLocation").unwrap(),
            "still the original object"
        );
    }

    #[test]
    fn unexpected_metadata_subkeys_are_not_replaced() {
        let fixture = RegistryFixture::new();
        let old = fixture.child("PawnIO");
        let _child = fixture.child(r"PawnIO\Unexpected");
        assert_eq!(
            replace_record(
                fixture.root.0,
                "PawnIO",
                REG_SAM_FLAGS(0),
                &RegistryFixture::descriptor(),
                |_, _| Ok(())
            ),
            Err(LEGACY_LAYOUT)
        );
        assert_eq!(set_text(old.0, "untouched"), 0);
    }

    #[test]
    fn missing_version_and_expected_file_size_do_not_establish_identity() {
        let path = std::env::temp_dir().join(format!("argus-legacy-image-{:?}.sys", unsafe {
            windows::Win32::System::Com::CoCreateGuid().unwrap()
        }));
        std::fs::write(&path, vec![0u8; IMAGES[1].0 as usize]).unwrap();
        let result = verify_image(&path, &mut Inspection::default());
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
