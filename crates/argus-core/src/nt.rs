//! Native API structures not (fully) exposed by the `windows` crate.
//! Layouts follow the phnt headers (System Informer) for x64.

use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS};
use windows::Win32::Foundation::{HANDLE, NTSTATUS};

use crate::util::AlignedBuf;

pub const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
pub const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023_u32 as i32;
pub const STATUS_BUFFER_OVERFLOW: i32 = 0x8000_0005_u32 as i32;

pub const SYSTEM_PROCESS_ID_INFORMATION: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(88);
pub const SYSTEM_EXTENDED_HANDLE_INFORMATION: SYSTEM_INFORMATION_CLASS =
    SYSTEM_INFORMATION_CLASS(64);

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
}
