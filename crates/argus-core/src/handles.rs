//! On-demand file-handle scanning. Run only in a disposable helper process:
//! querying or closing a foreign file handle can block inside a filesystem.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Wdk::Foundation::{NtQueryObject, OBJECT_ATTRIBUTES, OBJECT_INFORMATION_CLASS};
use windows::Wdk::Storage::FileSystem::{
    FILE_OPEN, FILE_OPEN_FOR_BACKUP_INTENT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
};
use windows::Wdk::System::SystemInformation::SystemProcessInformation;
use windows::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, OBJECT_ATTRIBUTE_FLAGS, UNICODE_STRING,
};
use windows::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ID_INFO,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK,
    FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, GetFileType,
    OPEN_EXISTING, QueryDosDeviceW, SYNCHRONIZE,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_DUP_HANDLE,
    PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::core::{PCWSTR, PWSTR};

use crate::actions::ActionError;
use crate::nt::{self, ProcessIter, UnicodeString};
use crate::paths::PathResolver;
use crate::tr;
use crate::util::{AlignedBuf, OwnedHandle, filetime_ticks, from_wide, ticks_to_unix_ms, wide};

pub const MAX_LOCKERS: usize = 128;
pub const MAX_PATH_BYTES: usize = 8192;
pub const MAX_HANDLES: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Cursor {
    pub pid: u32,
    pub handle: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandleLocker {
    pub pid: u32,
    pub create_time: i64,
    pub name: String,
    pub path: String,
    pub count: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandleSummary {
    pub available: bool,
    pub via_service: bool,
    /// The scan ran locally after the privileged service was asked and did
    /// not answer; `error` says why. Set by the host only, never sent over
    /// the service pipe.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub service_asked: bool,
    pub elevated: bool,
    pub checked: usize,
    pub denied_processes: usize,
    pub skipped: usize,
    pub timeouts: usize,
    pub truncated: bool,
    pub error: Option<String>,
}

impl HandleSummary {
    pub fn incomplete(&self) -> bool {
        !self.available
            || self.denied_processes > 0
            || self.skipped > 0
            || self.timeouts > 0
            || self.truncated
            || self.error.is_some()
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandleReport {
    pub lockers: Vec<HandleLocker>,
    pub summary: HandleSummary,
}

/// A cursor is emitted before any potentially blocking foreign-handle call,
/// including DuplicateHandle/GetFileType and the final CloseHandle, and before
/// each process is opened and its handles listed (with `handle: u64::MAX`,
/// so a stall there skips the process). The host watchdog can retain hits and
/// resume strictly after an interrupted cursor.
#[derive(Debug, Serialize, Deserialize)]
pub enum ScanEvent {
    Ready,
    Progress {
        cursor: Cursor,
        checked: usize,
        denied: usize,
        skipped: usize,
    },
    Found(HandleLocker),
    Done(HandleSummary),
    Error(ActionError),
}

pub fn validate_paths(paths: &[String]) -> Result<(), ActionError> {
    if paths.is_empty()
        || paths.len() > 32
        || paths.iter().map(String::len).sum::<usize>() > MAX_PATH_BYTES
    {
        return Err(ActionError::Os(tr!("core.handles.tooMany")));
    }
    for path in paths {
        let path = path.strip_prefix(r"\\?\").unwrap_or(path);
        let b = path.as_bytes();
        if b.len() < 3
            || !b[0].is_ascii_alphabetic()
            || b[1] != b':'
            || !matches!(b[2], b'\\' | b'/')
            || path.chars().any(char::is_control)
            || path[3..].contains([':', '"', '<', '>', '|', '?', '*'])
            || path[3..].split(['\\', '/']).any(|p| p == "." || p == "..")
        {
            return Err(ActionError::Os(tr!("core.handles.localPathsOnly")));
        }
    }
    Ok(())
}

fn nt_error(status: i32) -> ActionError {
    match status as u32 {
        0xc000_0022 => ActionError::AccessDenied,
        0xc000_000f | 0xc000_0034 | 0xc000_003a => ActionError::NotFound,
        0xc000_050b => ActionError::Os(tr!("core.handles.reparsePoint")),
        _ => ActionError::Os(tr!(
            "core.handles.ntStatus",
            status = format_args!("0x{:08X}", status as u32)
        )),
    }
}

/// Maps only local disk DOS devices. NtCreateFile's OBJ_DONT_REPARSE then
/// prevents a junction from turning a SYSTEM query into an SMB connection.
fn open_target(path: &str) -> Result<OwnedHandle, ActionError> {
    let path = path
        .strip_prefix(r"\\?\")
        .unwrap_or(path)
        .replace('/', "\\");
    let mut device = [0u16; 1024];
    let drive = wide(&path[..2]);
    if unsafe { QueryDosDeviceW(PCWSTR(drive.as_ptr()), Some(&mut device)) } == 0 {
        return Err(windows::core::Error::from_thread().into());
    }
    let device = from_wide(&device);
    if !device
        .strip_prefix(r"\Device\HarddiskVolume")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err(ActionError::Os(tr!("core.handles.localDisksOnly")));
    }
    let mut name = wide(&format!("{device}{}", &path[2..]));
    let mut unicode = UNICODE_STRING {
        Length: ((name.len() - 1) * 2) as u16,
        MaximumLength: (name.len() * 2) as u16,
        Buffer: PWSTR(name.as_mut_ptr()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        ObjectName: &mut unicode,
        Attributes: OBJECT_ATTRIBUTE_FLAGS(0x40 | 0x1000),
        ..Default::default()
    };
    let mut handle = HANDLE::default();
    let mut io = IO_STATUS_BLOCK::default();
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &attributes,
            &mut io,
            None,
            Default::default(),
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            FILE_OPEN_FOR_BACKUP_INTENT | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    if status.0 < 0 {
        return Err(nt_error(status.0));
    }
    Ok(OwnedHandle(handle))
}

fn object_text(handle: HANDLE, class: i32) -> Option<String> {
    let mut buffer = AlignedBuf::with_capacity(1024);
    for _ in 0..3 {
        let mut needed = 0;
        let status = unsafe {
            NtQueryObject(
                Some(handle),
                OBJECT_INFORMATION_CLASS(class),
                Some(buffer.as_mut_ptr()),
                buffer.len() as u32,
                Some(&mut needed),
            )
        };
        if matches!(
            status.0,
            nt::STATUS_INFO_LENGTH_MISMATCH
                | nt::STATUS_BUFFER_TOO_SMALL
                | nt::STATUS_BUFFER_OVERFLOW
        ) {
            if needed as usize <= buffer.len() || needed > 128 * 1024 {
                return None;
            }
            buffer.grow_to(needed as usize);
            continue;
        }
        if status.0 < 0 {
            return None;
        }
        let text = unsafe { *(buffer.as_ptr().cast::<UnicodeString>()) };
        if text.length == 0 {
            return Some(String::new());
        }
        let start = text.buffer as usize;
        let base = buffer.as_ptr() as usize;
        if !text.length.is_multiple_of(2)
            || !start.is_multiple_of(2)
            || start < base
            || start.checked_add(text.length as usize)? > base + buffer.len()
        {
            return None;
        }
        return Some(unsafe { text.to_string_lossy() });
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileIdentity {
    Wide(u64, [u8; 16]),
    Legacy(u32, u64),
}

fn file_identity(handle: HANDLE) -> Option<FileIdentity> {
    let mut id = FILE_ID_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    }
    .is_ok()
        && id.FileId.Identifier != [0; 16]
    {
        return Some(FileIdentity::Wide(
            id.VolumeSerialNumber,
            id.FileId.Identifier,
        ));
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle, &mut info) }.ok()?;
    let id = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    (id != 0).then_some(FileIdentity::Legacy(info.dwVolumeSerialNumber, id))
}

fn eq_path(a: &str, b: &str) -> bool {
    let a: Vec<_> = a.encode_utf16().collect();
    let b: Vec<_> = b.encode_utf16().collect();
    (unsafe { CompareStringOrdinal(&a, &b, true) }) == CSTR_EQUAL
}

fn beneath(name: &str, directory: &str) -> bool {
    let name: Vec<_> = name.encode_utf16().collect();
    let directory: Vec<_> = directory.encode_utf16().collect();
    let Some(prefix) = name.get(..directory.len()) else {
        return false;
    };
    (unsafe { CompareStringOrdinal(prefix, &directory, true) }) == CSTR_EQUAL
        && (name.len() == directory.len() || name.get(directory.len()) == Some(&(b'\\' as u16)))
}

struct Target {
    _handle: OwnedHandle,
    name: String,
    volume: String,
    directory: bool,
    identity: Option<FileIdentity>,
}

fn targets(paths: &[String]) -> Result<Vec<Target>, ActionError> {
    validate_paths(paths)?;
    paths
        .iter()
        .map(|path| {
            let handle = open_target(path)?;
            let name = object_text(handle.0, 1)
                .ok_or_else(|| ActionError::Os(tr!("core.handles.unresolvedName")))?;
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            unsafe { GetFileInformationByHandle(handle.0, &mut info) }?;
            let identity = file_identity(handle.0);
            let volume = name.split('\\').take(3).collect::<Vec<_>>().join("\\");
            Ok(Target {
                _handle: handle,
                name: name.trim_end_matches('\\').into(),
                volume,
                directory: info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
                identity,
            })
        })
        .collect()
}

struct Instance {
    /// FILETIME ticks.
    created: i64,
    name: String,
    handles: usize,
}

/// Where the scan finds each process's file handles.
enum Listing {
    /// The system-wide handle table, grouped by process in handle order.
    Table(BTreeMap<u32, Vec<usize>>),
    /// The system-wide table is too large to read at once (a process leaking
    /// millions of handles has made it hundreds of MB), so each process's own
    /// table is read once the process is open. Processes that cannot be
    /// opened are skipped without reading anything.
    PerProcess { file_type: u32, buffer: AlignedBuf },
}

/// Bounds one process's table in `Listing::PerProcess`, inside the helper's
/// 256 MB job limit. A process with more handles (about 3 million) is left
/// out and the result marked truncated.
const MAX_PROCESS_TABLE_BYTES: usize = 128 * 1024 * 1024;

/// `anchor` is a File handle of this process; its type index is the one
/// file handles have.
fn listing(me: u32, anchor: HANDLE, after: Option<Cursor>) -> Result<Listing, ActionError> {
    let mut table = AlignedBuf::with_capacity(1024 * 1024);
    let returned =
        match nt::query_system_information(nt::SYSTEM_EXTENDED_HANDLE_INFORMATION, &mut table) {
            Ok(returned) => returned,
            Err(status) if status.0 == nt::STATUS_INFO_LENGTH_MISMATCH => {
                drop(table);
                let mut buffer = AlignedBuf::with_capacity(64 * 1024);
                let own = nt::process_handles(
                    unsafe { GetCurrentProcess() },
                    0,
                    MAX_PROCESS_TABLE_BYTES,
                    &mut buffer,
                )
                .map_err(|s| nt_error(s.0))?;
                let file_type = own
                    .iter()
                    .find(|e| e.handle == anchor.0 as usize)
                    .map(|e| e.object_type)
                    .ok_or_else(|| ActionError::Os(tr!("core.handles.unknownFileType")))?;
                return Ok(Listing::PerProcess { file_type, buffer });
            }
            Err(status) => return Err(nt_error(status.0)),
        };
    let entries = nt::handle_entries(&table, returned)
        .ok_or_else(|| ActionError::Os(tr!("core.handles.unknownTable")))?;
    let file_type = entries
        .iter()
        .find(|e| e.pid == me as usize && e.handle == anchor.0 as usize)
        .map(|e| e.object_type)
        .ok_or_else(|| ActionError::Os(tr!("core.handles.unknownFileType")))?;
    let mut processes: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for e in entries {
        if e.object_type == file_type
            && e.pid <= u32::MAX as usize
            && e.pid != me as usize
            && after.is_none_or(|after| {
                Cursor {
                    pid: e.pid as u32,
                    handle: e.handle as u64,
                } > after
            })
        {
            processes.entry(e.pid as u32).or_default().push(e.handle);
        }
    }
    for handles in processes.values_mut() {
        handles.sort_unstable();
    }
    Ok(Listing::Table(processes))
}

/// Opens `pid` if it is still the instance created at `created` (FILETIME ticks).
fn open_instance(pid: u32, created: i64, access: PROCESS_ACCESS_RIGHTS) -> Option<OwnedHandle> {
    let process = OwnedHandle(unsafe { OpenProcess(access, false, pid) }.ok()?);
    let (mut c, mut e, mut k, mut u) = Default::default();
    (unsafe { GetProcessTimes(process.0, &mut c, &mut e, &mut k, &mut u) }.is_ok()
        && filetime_ticks(c) == created)
        .then_some(process)
}

pub fn scan(
    paths: &[String],
    after: Option<Cursor>,
    mut emit: impl FnMut(ScanEvent) -> bool,
) -> Result<(), ActionError> {
    let start = Instant::now();
    let targets = targets(paths)?;
    let me = std::process::id();
    // NUL is a File object too, and exists even when a target is a directory.
    let anchor = OwnedHandle(unsafe {
        CreateFileW(
            PCWSTR(wide("NUL").as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            Default::default(),
            None,
        )
    }?);
    let mut processes = AlignedBuf::with_capacity(1024 * 1024);
    nt::query_system_information(SystemProcessInformation, &mut processes)
        .map_err(|s| nt_error(s.0))?;
    let instances: HashMap<u32, Instance> = ProcessIter::new(&processes)
        .map(|p| {
            (
                p.info.unique_process_id.0 as usize as u32,
                Instance {
                    created: p.info.create_time,
                    name: unsafe { p.info.image_name.to_string_lossy() },
                    handles: p.info.handle_count as usize,
                },
            )
        })
        .collect();
    drop(processes);
    let mut listing = listing(me, anchor.0, after)?;
    let (pids, access): (Vec<u32>, _) = match &listing {
        Listing::Table(processes) => (
            processes.keys().copied().collect(),
            PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION,
        ),
        Listing::PerProcess { .. } => {
            let mut pids: Vec<u32> = instances
                .keys()
                .copied()
                .filter(|&pid| {
                    pid != 0 && pid != me && after.is_none_or(|after| whole_process(pid) > after)
                })
                .collect();
            pids.sort_unstable();
            (pids, PROCESS_DUP_HANDLE | PROCESS_QUERY_INFORMATION)
        }
    };
    let mut summary = HandleSummary {
        available: true,
        elevated: crate::privilege::is_elevated(),
        ..Default::default()
    };
    let resolver = PathResolver::new();
    let mut hit_pids = HashSet::new();
    let over_budget = |summary: &HandleSummary| {
        start.elapsed() > Duration::from_secs(12)
            || summary.checked + summary.skipped >= MAX_HANDLES
    };
    'processes: for pid in pids {
        if over_budget(&summary) {
            summary.truncated = true;
            break;
        }
        let Some(instance) = instances.get(&pid) else {
            continue;
        };
        // Reading a table with millions of entries takes a while: report
        // progress first, and should it stall, a restart skips the process.
        if !emit(ScanEvent::Progress {
            cursor: whole_process(pid),
            checked: summary.checked,
            denied: summary.denied_processes,
            skipped: summary.skipped,
        }) {
            return Ok(());
        }
        let Some(process) = open_instance(pid, instance.created, access) else {
            summary.denied_processes += 1;
            continue;
        };
        let handles = match &mut listing {
            Listing::Table(processes) => processes.remove(&pid).unwrap_or_default(),
            Listing::PerProcess { file_type, buffer } => {
                match nt::process_handles(
                    process.0,
                    instance.handles,
                    MAX_PROCESS_TABLE_BYTES,
                    buffer,
                ) {
                    Ok(entries) => {
                        let mut handles: Vec<usize> = entries
                            .iter()
                            .filter(|e| {
                                e.object_type == *file_type
                                    && after.is_none_or(|after| {
                                        Cursor {
                                            pid,
                                            handle: e.handle as u64,
                                        } > after
                                    })
                            })
                            .map(|e| e.handle)
                            .collect();
                        handles.sort_unstable();
                        handles
                    }
                    Err(status) if status.0 == nt::STATUS_INFO_LENGTH_MISMATCH => {
                        summary.truncated = true;
                        continue;
                    }
                    Err(_) => {
                        summary.denied_processes += 1;
                        continue;
                    }
                }
            }
        };
        for handle in handles {
            if over_budget(&summary) {
                summary.truncated = true;
                break 'processes;
            }
            if !emit(ScanEvent::Progress {
                cursor: Cursor {
                    pid,
                    handle: handle as u64,
                },
                checked: summary.checked,
                denied: summary.denied_processes,
                skipped: summary.skipped,
            }) {
                return Ok(());
            }
            let mut duplicate = HANDLE::default();
            if unsafe {
                DuplicateHandle(
                    process.0,
                    HANDLE(handle as *mut _),
                    GetCurrentProcess(),
                    &mut duplicate,
                    0,
                    false,
                    DUPLICATE_SAME_ACCESS,
                )
            }
            .is_err()
            {
                summary.skipped += 1;
                continue;
            }
            let duplicate = OwnedHandle(duplicate);
            // A handle can be reused after enumeration. Recheck its type before any
            // file query, and avoid pipes/devices whose name queries can block.
            let Some(kind) = object_text(duplicate.0, 2) else {
                summary.skipped += 1;
                continue;
            };
            if kind != "File" {
                continue;
            }
            let file_type = unsafe { GetFileType(duplicate.0) };
            if file_type != FILE_TYPE_DISK {
                if file_type.0 == 0 {
                    summary.skipped += 1;
                }
                continue;
            }
            let Some(path) = object_text(duplicate.0, 1) else {
                summary.skipped += 1;
                continue;
            };
            summary.checked += 1;
            let mut identity = None;
            let mut identity_read = false;
            let matched = targets.iter().any(|target| {
                if target.directory {
                    return beneath(&path, &target.name);
                }
                // Only query file IDs on the target's volume, so hard-link aliases
                // match without probing unrelated network filesystem handles.
                if !beneath(&path, &target.volume) {
                    return false;
                }
                if !identity_read {
                    identity = file_identity(duplicate.0);
                    identity_read = true;
                }
                match (target.identity, identity) {
                    (Some(a), Some(b)) => a == b,
                    _ => eq_path(&path, &target.name),
                }
            });
            if matched {
                if !hit_pids.contains(&pid) && hit_pids.len() >= MAX_LOCKERS {
                    summary.truncated = true;
                    break 'processes;
                }
                hit_pids.insert(pid);
                let path = resolver.to_dos(&path).unwrap_or(path);
                if !emit(ScanEvent::Found(HandleLocker {
                    pid,
                    create_time: ticks_to_unix_ms(instance.created),
                    name: instance.name.chars().take(256).collect(),
                    path: path.chars().take(1024).collect(),
                    count: 1,
                })) {
                    return Ok(());
                }
            }
        }
    }
    emit(ScanEvent::Done(summary));
    Ok(())
}

/// The cursor after every handle of `pid`: resuming after it skips the process.
fn whole_process(pid: u32) -> Cursor {
    Cursor {
        pid,
        handle: u64::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_remote_device_relative_and_traversal_targets() {
        for path in [
            r"\\host\share\file",
            r"\\?\UNC\host\share\file",
            r"\\.\PhysicalDrive0",
            r"C:relative",
            r"C:\a\..\b",
            "C:\\a\0b",
            r"C:\a:stream",
        ] {
            assert!(validate_paths(&[path.into()]).is_err(), "{path:?}");
        }
        assert!(validate_paths(&[r"D:\工作\文件.txt".into()]).is_ok());
        assert!(validate_paths(&[r"\\?\D:\工作".into()]).is_ok());
    }
    #[test]
    fn directory_matching_has_a_component_boundary() {
        assert!(beneath(
            r"\Device\HarddiskVolume3\Test\a",
            r"\Device\HarddiskVolume3\test"
        ));
        assert!(!beneath(
            r"\Device\HarddiskVolume3\Test-other",
            r"\Device\HarddiskVolume3\Test"
        ));
        assert!(!beneath(
            r"\Device\HarddiskVolume30\Test",
            r"\Device\HarddiskVolume3"
        ));
    }
}
