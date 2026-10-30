//! Process image paths with cached DOS device mappings.
//!
//! `SystemProcessIdInformation` returns the NT image path for any PID with no
//! access check, so it works for protected and other users' processes even when
//! we are not elevated. Missing mappings use the limited-query Win32 path,
//! or the GLOBALROOT namespace when a process cannot be opened.

use std::time::{Duration, Instant};

use windows::Wdk::System::SystemInformation::NtQuerySystemInformation;
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, GetLastError, HANDLE};
use windows::Win32::Storage::FileSystem::{GetLogicalDrives, QueryDosDeviceW};
use windows::core::PCWSTR;

use crate::nt::{
    STATUS_INFO_LENGTH_MISMATCH, SYSTEM_PROCESS_ID_INFORMATION, SystemProcessIdInformation,
    UnicodeString,
};
use crate::util::{from_wide, wide};

const REFRESH_COOLDOWN: Duration = Duration::from_secs(10);

pub(crate) struct PathResolver {
    /// (`\Device\HarddiskVolume3`, `C:`), longest device name first.
    devices: Vec<(String, String)>,
    refreshed_at: Instant,
    system_root: String,
}

impl PathResolver {
    pub fn new() -> Self {
        let mut r = Self {
            devices: Vec::new(),
            refreshed_at: Instant::now(),
            system_root: std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()),
        };
        r.refresh_devices();
        r
    }

    fn refresh_devices(&mut self) {
        self.devices.clear();
        let mask = unsafe { GetLogicalDrives() };
        if mask == 0 {
            crate::diagnostics::warn(
                "paths.logical_drives",
                crate::diagnostics::code(unsafe { GetLastError() }.0),
            );
        }
        let mut target = vec![0u16; 1024];
        for i in 0..26u8 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = format!("{}:", (b'A' + i) as char);
            let name = wide(&letter);
            loop {
                let n = unsafe { QueryDosDeviceW(PCWSTR(name.as_ptr()), Some(&mut target)) };
                if n > 0 {
                    self.devices
                        .push((from_wide(&target[..n as usize]), letter));
                    break;
                }
                let error = unsafe { GetLastError() };
                if error == ERROR_INSUFFICIENT_BUFFER && target.len() < 32_768 {
                    target.resize(target.len() * 2, 0);
                    continue;
                }
                crate::diagnostics::warn(
                    "paths.device_mapping",
                    format!("drive={letter}; {}", crate::diagnostics::code(error.0)),
                );
                break;
            }
        }
        self.devices
            .sort_by_key(|(dev, _)| std::cmp::Reverse(dev.len()));
        self.refreshed_at = Instant::now();
        // Seed the application's volume even if drive enumeration failed.
        if let Some(native) = query_nt_image_name(std::process::id())
            && self.to_dos(&native).is_none()
            && let Some(path) = crate::details::image_path_of(std::process::id())
        {
            self.remember_drive(&native, &path);
        }
    }

    /// Win32 path of the process image, if the process still exists and has one.
    pub fn image_path(&mut self, pid: u32) -> Option<String> {
        if pid == 0 || pid == 4 {
            return None;
        }
        let Some(nt) = query_nt_image_name(pid) else {
            return crate::details::image_path_of(pid);
        };
        if nt.is_empty() {
            return None;
        }
        if let Some(p) = self.to_dos(&nt) {
            return Some(p);
        }
        if self.refreshed_at.elapsed() > REFRESH_COOLDOWN {
            // A volume may have been mounted since we built the map.
            self.refresh_devices();
            if let Some(p) = self.to_dos(&nt) {
                return Some(p);
            }
        }
        if let Some(path) = crate::details::image_path_of(pid) {
            self.remember_drive(&nt, &path);
            return Some(path);
        }
        // Win32 file APIs can read this namespace without opening the process.
        Some(win32_device_path(&nt))
    }

    fn remember_drive(&mut self, native: &str, path: &str) {
        let bytes = path.as_bytes();
        if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || &bytes[1..3] != b":\\" {
            return;
        }
        let suffix = &path[2..];
        let Some(end) = native.len().checked_sub(suffix.len()) else {
            return;
        };
        let Some(device) = native.get(..end) else {
            return;
        };
        if strip_prefix_ci(device, r"\Device\").is_none()
            || !native[end..].eq_ignore_ascii_case(suffix)
        {
            return;
        }
        self.devices
            .retain(|(name, _)| !name.eq_ignore_ascii_case(device));
        self.devices.push((device.into(), path[..2].into()));
        self.devices
            .sort_by_key(|(dev, _)| std::cmp::Reverse(dev.len()));
    }

    pub(crate) fn to_dos(&self, nt: &str) -> Option<String> {
        if let Some(rest) = strip_prefix_ci(nt, r"\??\UNC\") {
            return Some(format!(r"\\{rest}"));
        }
        if let Some(rest) = strip_prefix_ci(nt, r"\??\") {
            return Some(rest.to_owned());
        }
        if let Some(rest) = strip_prefix_ci(nt, r"\SystemRoot\") {
            return Some(format!(r"{}\{}", self.system_root, rest));
        }
        if let Some(rest) = strip_prefix_ci(nt, r"\Device\Mup\") {
            return Some(format!(r"\\{rest}"));
        }
        for (dev, letter) in &self.devices {
            if let Some(rest) = strip_prefix_ci(nt, dev)
                && (rest.is_empty() || rest.starts_with('\\'))
            {
                return Some(format!("{letter}{rest}"));
            }
        }
        None
    }
}

fn win32_device_path(native: &str) -> String {
    if strip_prefix_ci(native, r"\Device\").is_some() {
        format!(r"\\?\GLOBALROOT{native}")
    } else {
        native.to_owned()
    }
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

fn query_nt_image_name(pid: u32) -> Option<String> {
    let mut chars = 512usize;
    for _ in 0..2 {
        let mut buf = vec![0u16; chars];
        let mut info = SystemProcessIdInformation {
            process_id: HANDLE(pid as usize as *mut _),
            image_name: UnicodeString {
                length: 0,
                maximum_length: (chars * 2).min(u16::MAX as usize) as u16,
                buffer: buf.as_mut_ptr(),
            },
        };
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESS_ID_INFORMATION,
                (&mut info as *mut SystemProcessIdInformation).cast(),
                std::mem::size_of::<SystemProcessIdInformation>() as u32,
                std::ptr::null_mut(),
            )
        };
        if status.0 == STATUS_INFO_LENGTH_MISMATCH {
            // MaximumLength now holds the required size in bytes.
            chars = (info.image_name.maximum_length as usize / 2).max(chars * 2);
            continue;
        }
        if status.0 < 0 {
            return None;
        }
        return Some(unsafe { info.image_name.to_string_lossy() });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_device_mapping_falls_back_to_the_process_win32_path() {
        let mut resolver = PathResolver::new();
        resolver.devices.clear();
        resolver.refreshed_at = Instant::now();
        let expected = std::env::current_exe().unwrap().canonicalize().unwrap();
        let actual = resolver.image_path(std::process::id()).unwrap();
        assert_eq!(std::fs::canonicalize(&actual).unwrap(), expected);
        let native = query_nt_image_name(std::process::id()).unwrap();
        assert!(
            resolver
                .to_dos(&native)
                .unwrap()
                .eq_ignore_ascii_case(&actual)
        );
    }

    #[test]
    fn learned_drive_mapping_checks_suffix_and_device_boundaries() {
        let mut resolver = PathResolver::new();
        resolver.devices.clear();
        resolver.remember_drive(
            r"\Device\HarddiskVolume3\Program Files\Argus\argus.exe",
            r"C:\Program Files\Argus\argus.exe",
        );
        assert_eq!(
            resolver.to_dos(r"\device\harddiskvolume3\Windows\explorer.exe"),
            Some(r"C:\Windows\explorer.exe".into())
        );
        assert!(
            resolver
                .to_dos(r"\Device\HarddiskVolume30\Windows\explorer.exe")
                .is_none()
        );
        resolver.remember_drive(
            r"\Device\HarddiskVolume4\Other\argus.exe",
            r"D:\Different\argus.exe",
        );
        assert!(
            resolver
                .to_dos(r"\Device\HarddiskVolume4\Other\argus.exe")
                .is_none()
        );
        assert_eq!(
            resolver.to_dos(r"\??\UNC\server\share\app.exe"),
            Some(r"\\server\share\app.exe".into())
        );
        assert_eq!(
            win32_device_path(r"\Device\HarddiskVolume3\app.exe"),
            r"\\?\GLOBALROOT\Device\HarddiskVolume3\app.exe"
        );
    }
}
