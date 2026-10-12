use argus_core::tr;
use windows::Win32::Foundation::{CloseHandle, HANDLE};

pub type Result<T> = std::result::Result<T, u32>;
pub const UNSAFE_PATH: u32 = 0x2000_0001;
pub const OTHER_OWNER: u32 = 0x2000_0002;
pub const INCOMPATIBLE: u32 = 0x2000_0003;
pub const FOREIGN_SERVICE: u32 = 0x2000_0004;

pub fn message(code: u32) -> String {
    match code {
        UNSAFE_PATH => tr!("service.unsafePath"),
        OTHER_OWNER => tr!("service.otherOwner"),
        INCOMPATIBLE => tr!("service.incompatible"),
        FOREIGN_SERVICE => tr!("service.foreignService"),
        0x2000_0005 => tr!("service.taskManagerActive"),
        1223 => tr!("service.canceled"),
        1460 => tr!("service.timeout"),
        _ => tr!(
            "service.osError",
            error = std::io::Error::from_raw_os_error(code as i32),
            code = code
        ),
    }
}

pub fn win(e: windows::core::Error) -> u32 {
    e.code().0 as u32 & 0xffff
}

pub fn io(e: std::io::Error) -> u32 {
    e.raw_os_error().unwrap_or(31) as u32
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub struct Handle(pub HANDLE);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}
