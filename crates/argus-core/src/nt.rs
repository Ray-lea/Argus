//! Native API structures not (fully) exposed by the `windows` crate.
//! Layouts follow the phnt headers (System Informer) for x64.

use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS};
use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
use windows::Win32::Foundation::{HANDLE, NTSTATUS};

use crate::util::AlignedBuf;

#[link(name = "ntdll")]
unsafe extern "system" {
    pub fn NtQuerySystemInformationEx(
        class: SYSTEM_INFORMATION_CLASS,
        input: *const core::ffi::c_void,
        input_len: u32,
        output: *mut core::ffi::c_void,
        output_len: u32,
        returned: *mut u32,
    ) -> NTSTATUS;
    /// Suspends every thread of a process (raises each suspend count by one).
    pub fn NtSuspendProcess(process: HANDLE) -> NTSTATUS;
    /// Resumes every thread of a process (lowers each suspend count by one).
    pub fn NtResumeProcess(process: HANDLE) -> NTSTATUS;
    pub fn NtSetInformationProcess(
        process: HANDLE,
        class: windows::Wdk::System::Threading::PROCESSINFOCLASS,
        information: *const core::ffi::c_void,
        length: u32,
    ) -> NTSTATUS;
}

pub const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
pub const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023_u32 as i32;
pub const STATUS_BUFFER_OVERFLOW: i32 = 0x8000_0005_u32 as i32;

pub const SYSTEM_PROCESS_ID_INFORMATION: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(88);
pub const SYSTEM_BASIC_PROCESS_INFORMATION: SYSTEM_INFORMATION_CLASS =
    SYSTEM_INFORMATION_CLASS(252);
pub const SYSTEM_EXTENDED_HANDLE_INFORMATION: SYSTEM_INFORMATION_CLASS =
    SYSTEM_INFORMATION_CLASS(64);
pub const SYSTEM_MEMORY_LIST_INFORMATION: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(80);

#[repr(C)]
#[derive(Default)]
pub struct SystemMemoryListInformation {
    pub zero_page_count: usize,
    pub free_page_count: usize,
    pub modified_page_count: usize,
    pub modified_no_write_page_count: usize,
    pub bad_page_count: usize,
    pub page_count_by_priority: [usize; 8],
    pub repurposed_pages_by_priority: [usize; 8],
    pub modified_page_count_page_file: usize,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SystemMemoryListInformation>() == 176);

pub const SYSTEM_STORE_INFORMATION: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(109);
pub const SYSTEM_STORE_INFORMATION_VERSION: u32 = 1;
/// `STORE_INFORMATION_CLASS::MemCompressionInfoRequest`; standard user
/// rights can query it, unlike the store's other commands.
pub const MEM_COMPRESSION_INFO_REQUEST: u32 = 22;
/// The request layout every Windows 10 and later build accepts (phnt's
/// `SYSTEM_STORE_COMPRESSION_INFORMATION_VERSION_V1`).
pub const STORE_COMPRESSION_INFORMATION_VERSION: u32 = 3;

#[repr(C)]
pub struct SystemStoreInformation {
    pub version: u32,
    pub command: u32,
    pub data: *mut core::ffi::c_void,
    pub length: u32,
}

/// `SM_STORE_COMPRESSION_INFORMATION_REQUEST` through
/// `TotalUniqueDataCompressed`, the version 3 size; sizes are in bytes.
#[repr(C)]
#[derive(Default)]
pub struct StoreCompressionInformation {
    /// Version in the low 8 bits.
    pub version: u32,
    /// The Memory Compression process that holds the store.
    pub pid: u32,
    /// The store's working set; 32 bits wide, so it wraps above 4 GiB.
    pub working_set: u32,
    /// Uncompressed size of the data in the store.
    pub total_data_compressed: usize,
    /// The data's size once compressed.
    pub total_compressed_size: usize,
    pub total_unique_data_compressed: usize,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<StoreCompressionInformation>() == 40);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SystemHandleEntry {
    pub object: usize,
    pub pid: usize,
    pub handle: usize,
    pub granted_access: u32,
    pub creator_backtrace: u16,
    pub object_type: u16,
    pub attributes: u32,
    pub reserved: u32,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SystemHandleEntry>() == 40);

/// SYSTEM_HANDLE_INFORMATION_EX starts with two ULONG_PTRs. Never trust the
/// count independently of the returned buffer length, including on newer OSes.
pub fn handle_entries(buf: &AlignedBuf, returned: usize) -> Option<&[SystemHandleEntry]> {
    let header = 2 * std::mem::size_of::<usize>();
    let available = returned.min(buf.len());
    if available < header {
        return None;
    }
    let count = unsafe { *buf.as_ptr().cast::<usize>() };
    if count > (available - header) / std::mem::size_of::<SystemHandleEntry>() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(buf.as_ptr().add(header).cast(), count) })
}

/// `ProcessHandleInformation` (Windows 8 and later): one process's handle
/// table. The process handle needs `PROCESS_QUERY_INFORMATION` and
/// `PROCESS_DUP_HANDLE`; the limited query right is refused.
pub const PROCESS_HANDLE_INFORMATION: PROCESSINFOCLASS = PROCESSINFOCLASS(51);

/// `PROCESS_HANDLE_TABLE_ENTRY_INFO`. `object_type` is the same type index as
/// `SystemHandleEntry::object_type`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcessHandleEntry {
    pub handle: usize,
    pub handle_count: usize,
    pub pointer_count: usize,
    pub granted_access: u32,
    pub object_type: u32,
    pub attributes: u32,
    pub reserved: u32,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ProcessHandleEntry>() == 40);

/// Reads the handle table of `process` into `buf` (`PROCESS_HANDLE_SNAPSHOT_INFORMATION`:
/// two ULONG_PTRs, then the entries). `expected` handles size the first
/// attempt; a table that needs more than `max_bytes` fails with
/// `STATUS_INFO_LENGTH_MISMATCH`, like a table that keeps outgrowing the buffer.
pub fn process_handles(
    process: HANDLE,
    expected: usize,
    max_bytes: usize,
    buf: &mut AlignedBuf,
) -> Result<&[ProcessHandleEntry], NTSTATUS> {
    let header = 2 * std::mem::size_of::<usize>();
    let entry = std::mem::size_of::<ProcessHandleEntry>();
    let sized = |handles: usize| header + (handles + handles / 8 + 64) * entry;
    let mut wanted = sized(expected).min(max_bytes);
    for _ in 0..4 {
        if wanted > buf.len() {
            // The old contents are not needed: release them first, so a
            // large table is never held twice.
            *buf = AlignedBuf::with_capacity(0);
            *buf = AlignedBuf::with_capacity(wanted);
        }
        let mut returned = 0u32;
        let status = unsafe {
            NtQueryInformationProcess(
                process,
                PROCESS_HANDLE_INFORMATION,
                buf.as_mut_ptr(),
                buf.len().min(u32::MAX as usize) as u32,
                &mut returned,
            )
        };
        match status.0 {
            STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_TOO_SMALL => {
                let needed = returned as usize;
                if needed > max_bytes {
                    break;
                }
                // The table can grow between calls; leave headroom.
                wanted = if needed > 0 {
                    sized(needed.saturating_sub(header) / entry).max(needed)
                } else {
                    buf.len() * 2
                }
                .min(max_bytes);
            }
            s if s < 0 => return Err(status),
            _ => {
                let available = (returned as usize).min(buf.len());
                if available < header {
                    return Err(NTSTATUS(STATUS_INFO_LENGTH_MISMATCH));
                }
                let count = unsafe { *buf.as_ptr().cast::<usize>() };
                if count > (available - header) / entry {
                    return Err(NTSTATUS(STATUS_INFO_LENGTH_MISMATCH));
                }
                return Ok(unsafe {
                    std::slice::from_raw_parts(buf.as_ptr().add(header).cast(), count)
                });
            }
        }
    }
    Err(NTSTATUS(STATUS_INFO_LENGTH_MISMATCH))
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UnicodeString {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: *mut u16,
}

impl UnicodeString {
    /// # Safety
    /// `buffer` must point to at least `length` valid bytes.
    pub unsafe fn to_string_lossy(self) -> String {
        if self.buffer.is_null() || self.length == 0 {
            return String::new();
        }
        let s = unsafe { std::slice::from_raw_parts(self.buffer, self.length as usize / 2) };
        String::from_utf16_lossy(s)
    }
}

#[repr(C)]
pub struct SystemBasicProcessInformation {
    pub next: u32,
    pub pid: HANDLE,
    pub parent: HANDLE,
    pub sequence: u64,
    pub name: UnicodeString,
}

const _: () = assert!(size_of::<SystemBasicProcessInformation>() == 48);

#[repr(C)]
pub struct SystemProcessInformation {
    pub next_entry_offset: u32,
    pub number_of_threads: u32,
    pub working_set_private_size: i64,
    pub hard_fault_count: u32,
    pub number_of_threads_high_watermark: u32,
    pub cycle_time: u64,
    pub create_time: i64,
    pub user_time: i64,
    pub kernel_time: i64,
    pub image_name: UnicodeString,
    pub base_priority: i32,
    pub unique_process_id: HANDLE,
    pub inherited_from_unique_process_id: HANDLE,
    pub handle_count: u32,
    pub session_id: u32,
    pub unique_process_key: usize,
    pub peak_virtual_size: usize,
    pub virtual_size: usize,
    pub page_fault_count: u32,
    pub peak_working_set_size: usize,
    pub working_set_size: usize,
    pub quota_peak_paged_pool_usage: usize,
    pub quota_paged_pool_usage: usize,
    pub quota_peak_non_paged_pool_usage: usize,
    pub quota_non_paged_pool_usage: usize,
    pub pagefile_usage: usize,
    pub peak_pagefile_usage: usize,
    pub private_page_count: usize,
    pub read_operation_count: i64,
    pub write_operation_count: i64,
    pub other_operation_count: i64,
    pub read_transfer_count: i64,
    pub write_transfer_count: i64,
    pub other_transfer_count: i64,
    // SYSTEM_THREAD_INFORMATION[number_of_threads] follows.
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SystemProcessInformation>() == 256);

#[repr(C)]
pub struct SystemThreadInformation {
    pub kernel_time: i64,
    pub user_time: i64,
    pub create_time: i64,
    pub wait_time: u32,
    pub start_address: *mut core::ffi::c_void,
    pub unique_process: HANDLE,
    pub unique_thread: HANDLE,
    pub priority: i32,
    pub base_priority: i32,
    pub context_switches: u32,
    pub thread_state: u32,
    pub wait_reason: u32,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SystemThreadInformation>() == 80);

pub const THREAD_STATE_WAITING: u32 = 5;
pub const WAIT_REASON_SUSPENDED: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SystemProcessorPerformanceInformation {
    pub idle_time: i64,
    pub kernel_time: i64,
    pub user_time: i64,
    pub dpc_time: i64,
    pub interrupt_time: i64,
    pub interrupt_count: u32,
}

#[repr(C)]
pub struct SystemProcessIdInformation {
    pub process_id: HANDLE,
    pub image_name: UnicodeString,
}

/// Upper bound for variable-size queries; the process list on a busy machine is
/// a few MB, so anything beyond this means the call is failing for another reason.
const MAX_QUERY_BYTES: usize = 64 * 1024 * 1024;

/// Runs `NtQuerySystemInformation` for a variable-size class, growing `buf`
/// to the size the kernel reports. Returns the number of bytes written.
pub fn query_system_information(
    class: SYSTEM_INFORMATION_CLASS,
    buf: &mut AlignedBuf,
) -> Result<usize, NTSTATUS> {
    for _ in 0..4 {
        let mut needed = 0u32;
        let status = unsafe {
            NtQuerySystemInformation(class, buf.as_mut_ptr(), buf.len() as u32, &mut needed)
        };
        match status.0 {
            STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_TOO_SMALL
                if needed as usize > buf.len() && (needed as usize) < MAX_QUERY_BYTES =>
            {
                // The process list can grow between calls; leave headroom.
                buf.grow_to(needed as usize + needed as usize / 8 + 16 * 1024);
            }
            s if s < 0 => return Err(status),
            _ => return Ok(needed as usize),
        }
    }
    Err(NTSTATUS(STATUS_INFO_LENGTH_MISMATCH))
}

/// Iterates the variable-length records of a `SystemProcessInformation` buffer.
pub struct ProcessIter<'a> {
    buf: &'a AlignedBuf,
    offset: Option<usize>,
}

impl<'a> ProcessIter<'a> {
    pub fn new(buf: &'a AlignedBuf) -> Self {
        Self {
            buf,
            offset: Some(0),
        }
    }
}

pub struct ProcessEntry<'a> {
    pub info: &'a SystemProcessInformation,
    pub threads: &'a [SystemThreadInformation],
}

impl<'a> Iterator for ProcessIter<'a> {
    type Item = ProcessEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let offset = self.offset?;
        let header = std::mem::size_of::<SystemProcessInformation>();
        if offset + header > self.buf.len() {
            self.offset = None;
            return None;
        }
        // SAFETY: the kernel wrote a chain of 8-byte aligned records into the buffer,
        // and every access below is bounds-checked against it.
        let base = unsafe { self.buf.as_ptr().add(offset) };
        let info = unsafe { &*(base as *const SystemProcessInformation) };
        let max_threads =
            (self.buf.len() - offset - header) / std::mem::size_of::<SystemThreadInformation>();
        let n = (info.number_of_threads as usize).min(max_threads);
        let threads = unsafe {
            std::slice::from_raw_parts(base.add(header) as *const SystemThreadInformation, n)
        };
        self.offset = match info.next_entry_offset {
            0 => None,
            n => Some(offset + n as usize),
        };
        Some(ProcessEntry { info, threads })
    }
}

#[cfg(test)]
mod handle_tests {
    use super::*;
    #[test]
    fn bounds_checks_the_native_handle_count() {
        let mut buffer = AlignedBuf::with_capacity(16 + 40);
        assert!(handle_entries(&buffer, 8).is_none());
        unsafe {
            *buffer.as_mut_ptr().cast::<usize>() = 2;
        }
        assert!(handle_entries(&buffer, 56).is_none());
        unsafe {
            *buffer.as_mut_ptr().cast::<usize>() = 1;
        }
        assert_eq!(handle_entries(&buffer, 56).unwrap().len(), 1);
    }

    #[test]
    fn lists_this_process_file_and_directory_handles_as_one_type() {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        // FILE_FLAG_BACKUP_SEMANTICS opens a directory.
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(0x0200_0000)
            .open(std::env::temp_dir())
            .unwrap();
        let process = unsafe { windows::Win32::System::Threading::GetCurrentProcess() };
        // No size hint: the first attempt is too small and must grow.
        let mut buffer = AlignedBuf::with_capacity(0);
        let entries = process_handles(process, 0, 64 << 20, &mut buffer).unwrap();
        let kind = |handle: usize| {
            entries
                .iter()
                .find(|e| e.handle == handle)
                .map(|e| e.object_type)
        };
        let file_type = kind(file.as_raw_handle() as usize);
        assert!(file_type.is_some());
        assert_eq!(kind(directory.as_raw_handle() as usize), file_type);
        let mut small = AlignedBuf::with_capacity(0);
        assert_eq!(
            process_handles(process, 0, 64, &mut small).err(),
            Some(NTSTATUS(STATUS_INFO_LENGTH_MISMATCH))
        );
    }
}
