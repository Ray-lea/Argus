//! Stable tray identity for one application profile at one executable path.
use sha2::{Digest, Sha256};
use std::{os::windows::ffi::OsStrExt, path::Path};
use windows::core::GUID;

/// Windows binds a tray GUID to the executable path. Scoping by that path also
/// keeps portable copies and hot-reload executables from reusing a bound GUID.
/// The domain and encoding are persistent identity: keep them stable across releases.
pub(super) fn guid(identifier: &str, executable: &Path) -> GUID {
    let mut path: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .map(|unit| match unit {
            47 => 92,
            65..=90 => unit + 32,
            _ => unit,
        })
        .collect();
    // GetModuleFileName may return the extended-length spelling of the same path.
    if path.starts_with(&[92, 92, 63, 92, 117, 110, 99, 92]) {
        path.drain(..6);
        path[0] = 92;
    } else if path.starts_with(&[92, 92, 63, 92]) {
        path.drain(..4);
    }
    let mut hash = Sha256::new();
    hash.update(b"argus.tray-icon.v1\0");
    hash.update(identifier.as_bytes());
    hash.update([0]);
    for unit in path {
        hash.update(unit.to_le_bytes());
    }
    let mut bytes: [u8; 16] = hash.finalize()[..16].try_into().expect("SHA-256 prefix");
    // UUID version 8, RFC variant: the payload is this application's stable hash.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    GUID::from_u128(u128::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::guid;
    use std::path::Path;
    use windows::core::GUID;

    #[test]
    fn the_same_installation_uses_one_guid_across_path_spellings() {
        let identifier = "dev.argus.taskmanager";
        let expected = guid(identifier, Path::new(r"C:\Program Files\Argus\argus.exe"));
        // Freeze the identity format so a later release cannot silently reset it.
        assert_eq!(
            expected,
            GUID::from_u128(0x0caa9b7e_74ed_8890_a7b4_8161052a40f6)
        );
        for path in [
            r"c:\program files\argus\ARGUS.EXE",
            "C:/Program Files/Argus/argus.exe",
            r"\\?\C:\Program Files\Argus\argus.exe",
        ] {
            assert_eq!(guid(identifier, Path::new(path)), expected);
        }
        assert_eq!(
            guid(
                identifier,
                Path::new(r"\\?\UNC\SERVER\share\Argus\argus.exe")
            ),
            guid(identifier, Path::new(r"\\server\share\argus\argus.exe"))
        );
    }

    #[test]
    fn profiles_and_binary_locations_have_separate_tray_identities() {
        let path = Path::new(r"C:\Program Files\Argus\argus.exe");
        let release = guid("dev.argus.taskmanager", path);
        assert_ne!(
            release,
            guid("dev.argus.taskmanager.devbuild.worktree", path)
        );
        assert_ne!(release, guid("dev.argus.taskmanager.test.tray", path));
        assert_ne!(
            release,
            guid(
                "dev.argus.taskmanager",
                Path::new(r"D:\Portable\Argus\argus.exe")
            )
        );
        assert_ne!(
            guid(
                "dev.argus.taskmanager.devbuild.worktree",
                Path::new(r"C:\Argus\runs\run-1\argus.exe")
            ),
            guid(
                "dev.argus.taskmanager.devbuild.worktree",
                Path::new(r"C:\Argus\runs\run-2\argus.exe")
            )
        );
    }
}
