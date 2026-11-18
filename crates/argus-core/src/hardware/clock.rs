//! Windows 10 Task Manager's WdcCpuMonitor reads the Processor Information
//! numerator (33) and base (34) by ID even when PDH has no counter name.

use super::CpuInfo;
use super::sensors::valid_mhz;
use crate::nt::{STATUS_BUFFER_OVERFLOW, STATUS_BUFFER_TOO_SMALL, STATUS_INFO_LENGTH_MISMATCH};
use crate::util::{AlignedBuf, OwnedHandle};
use std::ffi::c_void;
use windows::Win32::Foundation::{HANDLE, NTSTATUS, UNICODE_STRING};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::core::{PCWSTR, PWSTR, s, w};

const FREQUENCY: u16 = 33;
const BASE: u16 = 34;
const MASK: u64 = (1 << FREQUENCY) | (1 << BASE);
type CreateQuery = unsafe extern "system" fn(*mut HANDLE, *const c_void) -> NTSTATUS;
type AddItem = unsafe extern "system" fn(
    *mut u32,
    HANDLE,
    u8,
    *const UNICODE_STRING,
    *const UNICODE_STRING,
    u32,
    u64,
    *const c_void,
) -> NTSTATUS;
type Collect = unsafe extern "system" fn(HANDLE, *mut c_void, u32, *mut u32) -> NTSTATUS;

#[derive(Clone, Copy)]
struct Count {
    instance: u32,
    numerator: u64,
    base: u32,
}

impl Count {
    fn mhz_since(self, prev: Self) -> Option<u32> {
        // The 32-bit base wraps during ordinary uptime. A replaced instance
        // or a reset 64-bit numerator starts a new interval.
        let base = self.base.wrapping_sub(prev.base);
        if self.instance != prev.instance || base == 0 {
            return None;
        }
        valid_mhz(self.numerator.checked_sub(prev.numerator)? as f64 / f64::from(base))
    }
}

pub(super) struct Query {
    handle: OwnedHandle,
    collect: Collect,
    buffer: AlignedBuf,
    // Dense CPU thread order, followed by the provider's separate _Total.
    previous: Vec<Option<Count>>,
    current: Vec<Option<Count>>,
}

impl Query {
    pub fn open(cpu: &CpuInfo) -> Option<Self> {
        // These user-mode PCW exports back the api-ms-win-core-pcw contract.
        // KernelBase is already loaded for the lifetime of the process.
        let module = unsafe { GetModuleHandleW(w!("kernelbase.dll")) }.ok()?;
        let create: CreateQuery =
            unsafe { std::mem::transmute(GetProcAddress(module, s!("PcwCreateQuery"))?) };
        let add: AddItem =
            unsafe { std::mem::transmute(GetProcAddress(module, s!("PcwAddQueryItem"))?) };
        let collect: Collect =
            unsafe { std::mem::transmute(GetProcAddress(module, s!("PcwCollectData"))?) };
        let mut handle = HANDLE::default();
        if unsafe { create(&mut handle, std::ptr::null()) }.is_err() {
            return None;
        }
        let handle = OwnedHandle(handle);
        let unicode = |value: PCWSTR| {
            let len = unsafe { value.as_wide() }.len() as u16 * 2;
            UNICODE_STRING {
                Length: len,
                MaximumLength: len + 2,
                Buffer: PWSTR(value.0.cast_mut()),
            }
        };
        let mut item = 0;
        let status = unsafe {
            add(
                &mut item,
                handle.0,
                1,
                &unicode(w!("Processor Information")),
                &unicode(w!("*")),
                u32::MAX,
                MASK,
                std::ptr::null(),
            )
        };
        if status.is_err() {
            return None;
        }
        let slots = cpu.threads.len() + 1;
        let mut query = Self {
            handle,
            collect,
            buffer: AlignedBuf::with_capacity(4096),
            previous: vec![None; slots],
            current: vec![None; slots],
        };
        // Keep the query on transient read failures; a later pair can resume.
        query.advance(cpu);
        Some(query)
    }

    fn advance(&mut self, cpu: &CpuInfo) {
        std::mem::swap(&mut self.current, &mut self.previous);
        self.current.fill(None);
        for _ in 0..3 {
            let mut used = 0;
            let status = unsafe {
                (self.collect)(
                    self.handle.0,
                    self.buffer.as_mut_ptr(),
                    self.buffer.len() as u32,
                    &mut used,
                )
            };
            let size = used as usize;
            if status.is_ok() && size <= self.buffer.len() {
                // SAFETY: PCW wrote at most `size` bytes to this aligned buffer.
                let bytes = unsafe { std::slice::from_raw_parts(self.buffer.as_ptr(), size) };
                if decode(bytes, |name, count| {
                    let slot = if name == "_Total" {
                        Some(cpu.threads.len())
                    } else {
                        cpu.counter_index(name)
                    };
                    if let Some(slot) = slot {
                        self.current[slot] = Some(count);
                    }
                })
                .is_none()
                {
                    self.current.fill(None);
                }
                return;
            }
            if !matches!(
                status.0,
                STATUS_BUFFER_OVERFLOW | STATUS_BUFFER_TOO_SMALL | STATUS_INFO_LENGTH_MISMATCH
            ) || size <= self.buffer.len()
                || size > 4 * 1024 * 1024
            {
                return;
            }
            self.buffer.grow_to(size);
        }
    }

    pub fn sample(&mut self, cpu: &CpuInfo) -> (Option<u32>, Vec<u32>) {
        self.advance(cpu);
        let mut mhz: Vec<_> = self
            .current
            .iter()
            .zip(&self.previous)
            .map(|(now, before)| {
                now.zip(*before)
                    .and_then(|(n, p)| n.mhz_since(p))
                    .unwrap_or(0)
            })
            .collect();
        let total = mhz.pop().filter(|v| *v > 0);
        (total, mhz)
    }

    #[cfg(test)]
    pub(super) fn total_pair(&self) -> Option<[(u64, u32); 2]> {
        let prev = (*self.previous.last()?)?;
        let now = (*self.current.last()?)?;
        (now.instance == prev.instance)
            .then_some([(prev.numerator, prev.base), (now.numerator, now.base)])
    }
}

fn dword(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(*b.get(at..)?.first_chunk()?))
}

/// Each PCW block starts with its size. Offsets below are relative to the
/// containing block, including when a provider returns fewer counters.
fn block(b: &[u8], minimum: usize) -> Option<(&[u8], &[u8])> {
    let size = dword(b, 0)? as usize;
    (size >= minimum).then_some(())?;
    b.split_at_checked(size)
}

fn decode(bytes: &[u8], mut record: impl FnMut(&str, Count)) -> Option<()> {
    let (root, _) = block(bytes, 12)?;
    if dword(root, 8)? != 1 {
        return None;
    }
    let offset = dword(root, 4)? as usize;
    if offset < 12 {
        return None;
    }
    let (set, _) = block(root.get(offset..)?, 12)?;
    let offset = dword(set, 4)? as usize;
    if offset < 12 {
        return None;
    }
    let mut rows = set.get(offset..)?;
    let mut name = String::with_capacity(20);
    for _ in 0..dword(set, 8)? {
        let row;
        (row, rows) = block(rows, 16)?;
        let offset = dword(row, 8)? as usize;
        let wide = row.get(16..offset)?;
        name.clear();
        let mut units = wide
            .as_chunks::<2>()
            .0
            .iter()
            .copied()
            .map(u16::from_le_bytes);
        // The fixed counterset names are ASCII decimal pairs and _Total.
        loop {
            let unit = units.next()?;
            if unit == 0 {
                break;
            }
            if unit > 127 || name.len() >= 32 {
                name.clear();
                break;
            }
            name.push(char::from(unit as u8));
        }
        let mut fields = row.get(offset..)?;
        let mut numerator = None;
        let mut base = None;
        for _ in 0..dword(row, 12)? {
            let field;
            (field, fields) = block(fields, 8)?;
            let id = u16::from_le_bytes([field[4], field[5]]);
            let size = u16::from_le_bytes([field[6], field[7]]) as usize;
            let data = field.get(8..)?.get(..size)?;
            match (id, size) {
                (FREQUENCY, 8) => numerator = Some(u64::from_le_bytes(data.try_into().ok()?)),
                (BASE, 4) => base = Some(u32::from_le_bytes(data.try_into().ok()?)),
                _ => {}
            }
        }
        if let Some((numerator, base)) = numerator.zip(base) {
            record(
                &name,
                Count {
                    instance: dword(row, 4)?,
                    numerator,
                    base,
                },
            );
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A captured PCW result shortened to CPU 0 and the provider's _Total.
    fn frame() -> Vec<u8> {
        [
            176u32, 16, 1, 0, 160, 32, 2, 0, 0, 6, 0, 0, 64, 0, 32, 2, 0x2c0030, 0x30, 0, 0, 16,
            0x80021, 0x1ece68a4, 0x3c722, 16, 0x40022, 0xa6534228, 0, 64, 21, 32, 2, 0x54005f,
            0x74006f, 0x6c0061, 0, 16, 0x80021, 0xaeb3173d, 0x2cb9b, 16, 0x40022, 0x784f0f03, 0,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect()
    }

    #[test]
    fn reads_raw_numerator_and_base_by_id_for_each_instance() {
        let mut data = frame();
        // Counter records can be returned in either order.
        data[80..112].rotate_left(16);
        let mut results = Vec::new();
        decode(&data, |name, v| {
            results.push((name.to_owned(), v.numerator, v.base))
        })
        .unwrap();
        assert_eq!(
            results,
            [
                ("0,0".into(), 0x3c7221ece68a4, 0xa6534228),
                ("_Total".into(), 0x2cb9baeb3173d, 0x784f0f03)
            ]
        );
        data[84..86].copy_from_slice(&40u16.to_le_bytes());
        let mut valid = Vec::new();
        decode(&data, |name, _| valid.push(name.to_owned())).unwrap();
        assert_eq!(valid, ["_Total"]);
    }

    #[test]
    fn rejects_inconsistent_sizes_and_counts_before_using_a_frame() {
        for (at, value) in [
            (0, 177u32),
            (4, 8),
            (8, 2),
            (16, 161),
            (20, 160),
            (24, 3),
            (48, 0),
            (56, 65),
            (60, 3),
            (80, 4),
            (96, 1000),
        ] {
            let mut data = frame();
            data[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(decode(&data, |_, _| {}).is_none(), "bad field at {at}");
        }
    }

    #[test]
    fn delta_handles_base_wrap_resets_zero_and_changed_instances() {
        let before = Count {
            instance: 7,
            numerator: 10_000,
            base: u32::MAX - 2,
        };
        let after = Count {
            numerator: 30_000,
            base: 2,
            ..before
        };
        assert_eq!(after.mhz_since(before), Some(4000));
        for invalid in [
            Count {
                instance: 8,
                ..after
            },
            Count {
                base: before.base,
                ..after
            },
            Count {
                numerator: 0,
                ..after
            },
            Count {
                numerator: 100_000,
                ..after
            },
        ] {
            assert_eq!(invalid.mhz_since(before), None);
        }
    }
}
