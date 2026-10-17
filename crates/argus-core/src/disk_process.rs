//! Per-process storage counters, separate from general file/network/device I/O.
//! Newer Windows exposes a numeric trailer in the ordinary process query;
//! older systems use SystemFullProcessInformation through the broker.

use std::collections::HashMap;
use std::mem::{offset_of, size_of};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use windows::Wdk::System::SystemInformation::SYSTEM_INFORMATION_CLASS;
use windows::Win32::System::SystemInformation::{GetSystemTimePreciseAsFileTime, GetTickCount64};

use crate::nt::{SystemProcessInformation, SystemThreadInformation, query_system_information};
use crate::process::ProcessRow;
use crate::util::{AlignedBuf, filetime_ticks, ticks_to_unix_ms};

pub const MAX_PROCESSES: usize = 8192;
// SYSTEM_EXTENDED_THREAD_INFORMATION adds seven pointer-sized fields.
const THREAD_BYTES: usize = size_of::<SystemThreadInformation>() + 7 * size_of::<usize>();
const HEADER_BYTES: usize = size_of::<SystemProcessInformation>();
const DISK_BYTES: usize = 5 * size_of::<u64>();
/// Longer gaps re-establish the baseline instead of averaging over them.
const MAX_INTERVAL_MS: u64 = 30_000;

/// Bytes per second between two cumulative (read, written) readings of one
/// process instance, `secs` apart. None without both readings, across a
/// counter reset or over an interval too long to trust.
pub(crate) fn interval_rates(
    previous: Option<(u64, u64)>,
    current: Option<(u64, u64)>,
    secs: Option<f64>,
) -> Option<(u64, u64)> {
    let ((r, w), (read, written)) = previous.zip(current)?;
    let secs = secs.filter(|s| *s > 0.0 && *s * 1000.0 <= MAX_INTERVAL_MS as f64)?;
    Some((
        (read.checked_sub(r)? as f64 / secs) as u64,
        (written.checked_sub(w)? as f64 / secs) as u64,
    ))
}

pub(crate) fn standard_supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        #[repr(C)]
        struct Version {
            size: u32,
            major: u32,
            minor: u32,
            build: u32,
            platform: u32,
            service_pack: [u16; 128],
        }
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn RtlGetVersion(version: *mut Version) -> i32;
        }
        let mut version = Version {
            size: size_of::<Version>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform: 0,
            service_pack: [0; 128],
        };
        (unsafe { RtlGetVersion(&mut version) }) >= 0
            && version.major == 10
            && version.build >= 26100
    })
}

/// The Windows 11 24H2+ trailer precedes ImageName, after ordinary 80-byte
/// thread records. Require numeric space before the string, not just spare
/// allocation capacity: on older layouts that space contains the name itself.
pub(crate) fn standard_counters(
    buf: &AlignedBuf,
    returned: usize,
    process: &SystemProcessInformation,
) -> Option<(u64, u64)> {
    if returned > buf.len() {
        return None;
    }
    let base = buf.as_ptr() as usize;
    let offset = (process as *const _ as usize).checked_sub(base)?;
    let length = if process.next_entry_offset == 0 {
        returned.checked_sub(offset)?
    } else {
        process.next_entry_offset as usize
    };
    let end = offset.checked_add(length)?;
    if end > returned {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().add(offset), length) };
    standard_trailer(
        bytes,
        base + offset,
        process.number_of_threads as usize,
        process.image_name.buffer as usize,
        process.image_name.maximum_length as usize,
    )
}

fn standard_trailer(
    record: &[u8],
    base: usize,
    threads: usize,
    name: usize,
    name_bytes: usize,
) -> Option<(u64, u64)> {
    let offset =
        HEADER_BYTES.checked_add(threads.checked_mul(size_of::<SystemThreadInformation>())?)?;
    let name_offset = name.checked_sub(base)?;
    if name_offset < offset.checked_add(DISK_BYTES)?
        || name_offset.checked_add(name_bytes)? > record.len()
    {
        return None;
    }
    let counters = record.get(offset..offset + DISK_BYTES)?;
    Some((
        u64::from_le_bytes(counters[..8].try_into().ok()?),
        u64::from_le_bytes(counters[8..16].try_into().ok()?),
    ))
}

/// PID, creation time (Unix ms), cumulative disk bytes read/written. No paths,
/// command lines, SIDs or other variable-length process metadata cross IPC.
pub type DiskCounter = (u32, i64, u64, u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskSnapshot {
    /// Monotonic boot time, shared across callers and service restarts.
    pub sampled_at_ms: u64,
    pub counters: Vec<DiskCounter>,
}

pub struct DiskQuery {
    buf: AlignedBuf,
}

impl Default for DiskQuery {
    fn default() -> Self {
        Self {
            buf: AlignedBuf::with_capacity(2 * 1024 * 1024),
        }
    }
}

impl DiskQuery {
    pub fn read(&mut self) -> Option<DiskSnapshot> {
        let sampled_at_ms = unsafe { GetTickCount64() };
        let len = query_system_information(SYSTEM_INFORMATION_CLASS(148), &mut self.buf).ok()?;
        if len > self.buf.len() {
            return None;
        }
        // Only bytes returned by the kernel are inspected; each record and
        // extension is bounded again below before reading fixed numeric fields.
        let bytes = unsafe { std::slice::from_raw_parts(self.buf.as_ptr(), len) };
        Some(DiskSnapshot {
            sampled_at_ms,
            counters: decode(bytes)?,
        })
    }
}

fn decode(bytes: &[u8]) -> Option<Vec<DiskCounter>> {
    let u32_at =
        |b: &[u8], at: usize| Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?));
    let u64_at =
        |b: &[u8], at: usize| Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?));
    let mut counters = Vec::new();
    let mut offset = 0;
    loop {
        let remaining = bytes.get(offset..)?;
        let next = u32_at(remaining, 0)? as usize;
        let record = if next == 0 {
            remaining
        } else {
            if !next.is_multiple_of(8) {
                return None;
            }
            remaining.get(..next)?
        };
        let threads = u32_at(record, 4)? as usize;
        let extension = HEADER_BYTES.checked_add(threads.checked_mul(THREAD_BYTES)?)?;
        record.get(extension..extension.checked_add(DISK_BYTES)?)?;
        let pid = u32::try_from(u64_at(
            record,
            offset_of!(SystemProcessInformation, unique_process_id),
        )?)
        .ok()?;
        if pid != 0 {
            if counters.len() == MAX_PROCESSES {
                return None;
            }
            let created = u64_at(record, offset_of!(SystemProcessInformation, create_time))? as i64;
            counters.push((
                pid,
                ticks_to_unix_ms(created),
                u64_at(record, extension)?,
                u64_at(record, extension + 8)?,
            ));
        }
        if next == 0 {
            return Some(counters);
        }
        offset = offset.checked_add(next)?;
    }
}

#[derive(Default)]
pub struct DiskRates {
    previous: Option<DiskSnapshot>,
    values: HashMap<(u32, i64), (u64, u64)>,
}

impl DiskRates {
    pub fn apply(&mut self, frame: &DiskSnapshot, processes: &mut [ProcessRow]) {
        // The tick count is system-wide, so the offset maps a snapshot's
        // clock onto the wall clock that creation times use.
        let wall_ms = ticks_to_unix_ms(filetime_ticks(unsafe { GetSystemTimePreciseAsFileTime() }));
        let offset = wall_ms - unsafe { GetTickCount64() } as i64;
        self.apply_with(frame, processes, offset);
    }

    fn apply_with(&mut self, frame: &DiskSnapshot, processes: &mut [ProcessRow], wall_offset: i64) {
        if self
            .previous
            .as_ref()
            .is_none_or(|p| p.sampled_at_ms != frame.sampled_at_ms)
        {
            self.values.clear();
            if let Some(previous) = &self.previous
                && let Some(elapsed) = frame.sampled_at_ms.checked_sub(previous.sampled_at_ms)
                && (1..=MAX_INTERVAL_MS).contains(&elapsed)
            {
                let old: HashMap<_, _> = previous
                    .counters
                    .iter()
                    .map(|&(pid, created, read, written)| ((pid, created), (read, written)))
                    .collect();
                // The clock is read before each query: a process missing from
                // the previous snapshot but created after that read did all of
                // its I/O since. Coarse ticks can only defer one to the next.
                let taken = previous.sampled_at_ms as i64 + wall_offset;
                for &(pid, created, read, written) in &frame.counters {
                    let (r, w) = match old.get(&(pid, created)) {
                        Some(&counters) => counters,
                        None if created > taken => (0, 0),
                        None => continue,
                    };
                    let (Some(read), Some(written)) = (read.checked_sub(r), written.checked_sub(w))
                    else {
                        continue;
                    };
                    let rate = |bytes: u64| {
                        ((u128::from(bytes) * 1000) / u128::from(elapsed)).min(u64::MAX as u128)
                            as u64
                    };
                    self.values
                        .insert((pid, created), (rate(read), rate(written)));
                }
            }
            self.previous = Some(frame.clone());
        }
        for process in processes {
            let value = self.values.get(&(process.pid, process.create_time));
            process.disk_read = value.map(|v| v.0);
            process.disk_write = value.map(|v| v.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_trailer_cannot_read_image_names_or_unreturned_allocation_space() {
        let offset = HEADER_BYTES + 2 * size_of::<SystemThreadInformation>();
        let mut bytes = vec![0; offset + DISK_BYTES + 24];
        bytes[offset..offset + 8].copy_from_slice(&4096_u64.to_le_bytes());
        bytes[offset + 8..offset + 16].copy_from_slice(&8192_u64.to_le_bytes());
        let base = 0x1000;
        assert_eq!(
            standard_trailer(&bytes, base, 2, base + offset + DISK_BYTES, 24),
            Some((4096, 8192))
        );
        assert_eq!(standard_trailer(&bytes, base, 2, base + offset, 24), None);
        assert_eq!(
            standard_trailer(
                &bytes[..offset + 20],
                base,
                2,
                base + offset + DISK_BYTES,
                24
            ),
            None
        );
        assert_eq!(standard_trailer(&bytes, base, usize::MAX, 0, 24), None);
        assert_eq!(
            standard_trailer(&bytes, base, 2, base + offset + DISK_BYTES, 1024),
            None
        );
    }

    fn record(pid: u64, threads: usize, read: u64, written: u64) -> Vec<u8> {
        let extension = HEADER_BYTES + threads * THREAD_BYTES;
        let mut bytes = vec![0; extension + DISK_BYTES];
        bytes[4..8].copy_from_slice(&(threads as u32).to_le_bytes());
        let at = offset_of!(SystemProcessInformation, unique_process_id);
        bytes[at..at + 8].copy_from_slice(&pid.to_le_bytes());
        let at = offset_of!(SystemProcessInformation, create_time);
        bytes[at..at + 8].copy_from_slice(&116_444_736_010_000_000_i64.to_le_bytes());
        // General I/O deliberately differs from disk I/O.
        let at = offset_of!(SystemProcessInformation, read_transfer_count);
        bytes[at..at + 8].copy_from_slice(&999_999_u64.to_le_bytes());
        bytes[extension..extension + 8].copy_from_slice(&read.to_le_bytes());
        bytes[extension + 8..extension + 16].copy_from_slice(&written.to_le_bytes());
        bytes
    }

    #[test]
    fn reads_disk_extension_after_extended_threads_and_keeps_zero_counters() {
        assert_eq!(THREAD_BYTES, 136);
        assert_eq!(HEADER_BYTES, 256);
        let mut bytes = record(17, 3, 4096, 8192);
        let length = bytes.len() as u32;
        bytes[..4].copy_from_slice(&length.to_le_bytes());
        bytes.extend(record(19, 0, 0, 0));
        assert_eq!(
            decode(&bytes).unwrap(),
            [(17, 1000, 4096, 8192), (19, 1000, 0, 0)]
        );
        assert!(
            !serde_json::to_string(&DiskSnapshot {
                sampled_at_ms: 1,
                counters: decode(&bytes).unwrap()
            })
            .unwrap()
            .contains("999999")
        );
    }

    #[test]
    fn rejects_truncated_extensions_invalid_offsets_and_thread_counts() {
        let valid = record(17, 2, 4, 8);
        for length in [0, 4, HEADER_BYTES - 1, valid.len() - 1] {
            assert!(decode(&valid[..length]).is_none());
        }
        for next in [8, 255, u32::MAX] {
            let mut bytes = valid.clone();
            bytes[..4].copy_from_slice(&next.to_le_bytes());
            assert!(decode(&bytes).is_none());
        }
        let mut bytes = valid;
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bytes).is_none());
    }

    #[test]
    fn standard_counters_end_the_last_record_where_the_returned_data_ends() {
        // Two records: header, one thread, the trailer, then an 8-byte name.
        let record = HEADER_BYTES + size_of::<SystemThreadInformation>() + DISK_BYTES + 8;
        let mut buf = AlignedBuf::with_capacity(2 * record + 64);
        let base = buf.as_ptr() as usize;
        let name = offset_of!(SystemProcessInformation, image_name);
        for (i, read) in [4096_u64, 8192].into_iter().enumerate() {
            let at = i * record;
            let trailer = at + HEADER_BYTES + size_of::<SystemThreadInformation>();
            let next = if i == 0 { record as u32 } else { 0 };
            let mut write = |offset: usize, value: &[u8]| unsafe {
                std::ptr::copy_nonoverlapping(
                    value.as_ptr(),
                    buf.as_mut_ptr().cast::<u8>().add(offset),
                    value.len(),
                );
            };
            write(at, &next.to_le_bytes());
            write(at + 4, &1_u32.to_le_bytes());
            write(trailer, &read.to_le_bytes());
            write(at + name + 2, &8_u16.to_le_bytes());
            write(
                at + name + 8,
                &((base + trailer + DISK_BYTES) as u64).to_le_bytes(),
            );
        }
        let info = |i: usize| unsafe {
            &*(buf.as_ptr().add(i * record) as *const SystemProcessInformation)
        };
        assert_eq!(
            standard_counters(&buf, 2 * record, info(0)),
            Some((4096, 0))
        );
        assert_eq!(
            standard_counters(&buf, 2 * record, info(1)),
            Some((8192, 0))
        );
        // The name would extend past the bytes the kernel returned.
        assert_eq!(standard_counters(&buf, 2 * record - 1, info(1)), None);
        assert_eq!(standard_counters(&buf, buf.len() + 8, info(0)), None);
    }

    #[test]
    fn interval_rates_need_two_readings_and_a_trustworthy_interval() {
        // First sample, or a process the trailer does not cover.
        assert_eq!(interval_rates(None, Some((4096, 0)), Some(1.0)), None);
        assert_eq!(interval_rates(Some((0, 0)), None, Some(1.0)), None);
        assert_eq!(interval_rates(Some((0, 0)), Some((4096, 0)), None), None);
        // A process started since the last sample counts from zero.
        assert_eq!(
            interval_rates(Some((0, 0)), Some((4096, 8192)), Some(2.0)),
            Some((2048, 4096))
        );
        assert_eq!(
            interval_rates(Some((4096, 8192)), Some((4096, 8192)), Some(0.5)),
            Some((0, 0))
        );
        // A counter reset, no time between readings, or too long a gap.
        assert_eq!(
            interval_rates(Some((4096, 0)), Some((0, 0)), Some(1.0)),
            None
        );
        assert_eq!(interval_rates(Some((0, 0)), Some((1, 1)), Some(0.0)), None);
        assert_eq!(interval_rates(Some((0, 0)), Some((1, 1)), Some(30.5)), None);
        assert_eq!(
            interval_rates(Some((0, 0)), Some((30, 30)), Some(30.0)),
            Some((1, 1))
        );
    }

    #[test]
    fn rates_use_source_time_and_reprime_on_pid_reuse_reset_and_gaps() {
        let mut rates = DiskRates::default();
        let mut apply = |ms, created, read, written| {
            rates.apply_with(
                &DiskSnapshot {
                    sampled_at_ms: ms,
                    counters: vec![(17, created, read, written)],
                },
                &mut [],
                0,
            );
            rates.values.get(&(17, created)).copied()
        };
        assert_eq!(apply(1000, 10, 5000, 7000), None);
        assert_eq!(apply(3000, 10, 9000, 8000), Some((2000, 500)));
        assert_eq!(apply(3000, 10, 9000, 8000), Some((2000, 500)));
        assert_eq!(apply(4000, 11, 100, 200), None);
        assert_eq!(apply(5000, 11, 100, 200), Some((0, 0)));
        assert_eq!(apply(6000, 11, 0, 0), None);
        assert_eq!(apply(100_000, 11, 8000, 9000), None);
        assert_eq!(apply(99_000, 11, 8000, 9000), None);
    }

    #[test]
    fn processes_started_after_the_previous_snapshot_count_from_zero() {
        let mut rates = DiskRates::default();
        let frame = |ms, counters| DiskSnapshot {
            sampled_at_ms: ms,
            counters,
        };
        // Snapshot clock 1000 ms is wall time 5000 ms.
        rates.apply_with(&frame(1000, vec![(17, 4000, 100, 100)]), &mut [], 4000);
        rates.apply_with(
            &frame(
                3000,
                vec![
                    (17, 4000, 2100, 100),
                    (18, 5500, 4096, 0),
                    (19, 4500, 4096, 0),
                ],
            ),
            &mut [],
            4000,
        );
        assert_eq!(rates.values.get(&(17, 4000)), Some(&(1000, 0)));
        // Created after the previous snapshot: all of its bytes are new.
        assert_eq!(rates.values.get(&(18, 5500)), Some(&(2048, 0)));
        // Created before it yet missing from it: no baseline to trust.
        assert_eq!(rates.values.get(&(19, 4500)), None);
    }
}
