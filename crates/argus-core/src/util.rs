use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};

/// Closes the wrapped handle on drop.
pub(crate) struct OwnedHandle(pub HANDLE);

// Kernel handles are valid process-wide, so moving one between threads is fine.
unsafe impl Send for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Null-terminated UTF-16 copy of `s`.
pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Lossy UTF-16 to `String`, stopping at the first NUL.
pub(crate) fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Offset between the Windows epoch (1601) and the Unix epoch, in 100 ns ticks.
const EPOCH_DIFF_100NS: i64 = 116_444_736_000_000_000;

/// Converts a Windows timestamp in 100 ns ticks to Unix milliseconds (0 stays 0).
pub(crate) fn ticks_to_unix_ms(ticks: i64) -> i64 {
    if ticks <= EPOCH_DIFF_100NS {
        0
    } else {
        (ticks - EPOCH_DIFF_100NS) / 10_000
    }
}

pub(crate) fn filetime_ticks(ft: FILETIME) -> i64 {
    (((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64) as i64
}

/// A heap buffer with 8-byte alignment, suitable for NT structures.
pub(crate) struct AlignedBuf {
    data: Vec<u64>,
}

impl AlignedBuf {
    pub fn with_capacity(bytes: usize) -> Self {
        Self {
            data: vec![0; bytes.div_ceil(8)],
        }
    }

    pub fn len(&self) -> usize {
        self.data.len() * 8
    }

    pub fn grow_to(&mut self, bytes: usize) {
        if bytes > self.len() {
            self.data.resize(bytes.div_ceil(8), 0);
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut core::ffi::c_void {
        self.data.as_mut_ptr().cast()
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr().cast()
    }
}
