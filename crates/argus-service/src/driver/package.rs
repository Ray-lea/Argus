//! Immutable upstream package, downloaded only by the explicit UAC helper.
//! Whole-file SHA-256 also covers bytes outside Authenticode's signed ranges.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use super::{DOWNLOAD_FAILED, PACKAGE_INVALID, SIGNATURE_INVALID};
use crate::util::{Result, io, wide};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Security::WinTrust::*;
use windows::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
use windows::core::PCWSTR;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    version: String,
    url: String,
    size: u64,
    sha256: String,
}

pub(super) fn manifest() -> &'static Manifest {
    static VALUE: OnceLock<Manifest> = OnceLock::new();
    VALUE.get_or_init(|| {
        serde_json::from_str(include_str!("../../vendor/pawnio/installer.json"))
            .expect("embedded PawnIO installer manifest")
    })
}

pub(super) fn legacy_manifest() -> &'static Manifest {
    static VALUE: OnceLock<Manifest> = OnceLock::new();
    VALUE.get_or_init(|| {
        serde_json::from_str(include_str!("../../vendor/pawnio/legacy-installer.json"))
            .expect("embedded legacy PawnIO installer manifest")
    })
}

fn check_bytes(bytes: &[u8], expected: &Manifest) -> Result<()> {
    if bytes.len() as u64 != expected.size
        || format!("{:x}", Sha256::digest(bytes)) != expected.sha256
    {
        return Err(PACKAGE_INVALID);
    }
    Ok(())
}

pub(super) fn download(expected: &Manifest) -> Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("redirect limit");
            }
            if attempt.url().scheme() == "https"
                && matches!(
                    attempt.url().host_str(),
                    Some(
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.follow()
            } else {
                attempt.error("unexpected download host")
            }
        }))
        .user_agent(concat!("Argus/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| DOWNLOAD_FAILED)?;
    let response = client
        .get(&expected.url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|_| DOWNLOAD_FAILED)?;
    if response
        .content_length()
        .is_some_and(|size| size != expected.size)
    {
        return Err(PACKAGE_INVALID);
    }
    let mut bytes = Vec::with_capacity(expected.size as usize);
    response
        .take(expected.size + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DOWNLOAD_FAILED)?;
    check_bytes(&bytes, expected)?;
    Ok(bytes)
}

/// Pin the verified executable against replacement until its process exits.
pub(super) fn verify(path: &Path, expected: &Manifest) -> Result<File> {
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(io)?;
    if file.metadata().map_err(io)?.len() != expected.size {
        return Err(PACKAGE_INVALID);
    }
    let mut bytes = Vec::with_capacity(expected.size as usize);
    (&file)
        .take(expected.size + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    check_bytes(&bytes, expected)?;
    file.rewind().map_err(io)?;
    let name = wide(&path.to_string_lossy());
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(name.as_ptr()),
        hFile: HANDLE(file.as_raw_handle()),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut info },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let result = unsafe {
        WinVerifyTrust(
            HWND(std::ptr::null_mut()),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        )
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(
            HWND(std::ptr::null_mut()),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        );
    }
    if result != 0 {
        return Err(SIGNATURE_INVALID);
    }
    Ok(file)
}

pub(super) fn version() -> &'static str {
    &manifest().version
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncation_and_same_size_replacement() {
        for expected in [manifest(), legacy_manifest()] {
            assert!(check_bytes(&[], expected).is_err());
            assert!(check_bytes(&vec![0; expected.size as usize], expected).is_err());
            assert_eq!(expected.sha256.len(), 64);
            assert!(expected.size < 16 * 1024 * 1024);
            assert_eq!(
                reqwest::Url::parse(&expected.url).unwrap().host_str(),
                Some("github.com")
            );
        }
    }
}
