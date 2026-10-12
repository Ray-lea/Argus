//! Process image paths without opening the process.
//!
//! `SystemProcessIdInformation` returns the NT image path for any PID with no
//! access check, so it works for protected and other users' processes even when
//! we are not elevated. The NT path is then mapped back to a DOS path.

use std::time::{Duration, Instant};

use windows::Wdk::System::SystemInformation::NtQuerySystemInformation;
use windows::Win32::Foundation::HANDLE;
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
        let mut target = vec![0u16; 1024];
        for i in 0..26u8 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = format!("{}:", (b'A' + i) as char);
            let name = wide(&letter);
            let n = unsafe { QueryDosDeviceW(PCWSTR(name.as_ptr()), Some(&mut target)) };
            if n > 0 {
                self.devices.push((from_wide(&target), letter));
            }
        }
        self.devices
            .sort_by_key(|(dev, _)| std::cmp::Reverse(dev.len()));
        self.refreshed_at = Instant::now();
    }

    /// Win32 path of the process image, if the process still exists and has one.
    pub fn image_path(&mut self, pid: u32) -> Option<String> {
        if pid == 0 || pid == 4 {
            return None;
        }
        let nt = query_nt_image_name(pid)?;
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
        Some(nt)
    }

    pub(crate) fn to_dos(&self, nt: &str) -> Option<String> {
        if let Some(rest) = nt.strip_prefix(r"\??\") {
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
