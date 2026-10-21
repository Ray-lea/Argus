use argus_core::tr;
use windows::Win32::Foundation::{CloseHandle, HANDLE};

pub type Result<T> = std::result::Result<T, u32>;
pub const UNSAFE_PATH: u32 = 0x2000_0001;
pub const OTHER_OWNER: u32 = 0x2000_0002;
pub const INCOMPATIBLE: u32 = 0x2000_0003;
pub const FOREIGN_SERVICE: u32 = 0x2000_0004;
/// The task manager replacement held up a service update or removal: Argus's
/// own value is not restored yet.
pub const TASK_MANAGER_ACTIVE: u32 = 0x2000_0005;
/// The same, because another program's configuration holds taskmgr.exe's
/// entry and Argus does not overwrite it.
pub const TASK_MANAGER_CONFLICT: u32 = 0x2000_0006;
/// An enabled replacement could not be brought to the new build, so the
/// service update stopped before changing the service.
pub const TASK_MANAGER_FOLLOW: u32 = 0x2000_0007;

pub fn message(code: u32) -> String {
    match code {
        UNSAFE_PATH => tr!("service.unsafePath"),
        OTHER_OWNER => tr!("service.otherOwner"),
        INCOMPATIBLE => tr!("service.incompatible"),
        FOREIGN_SERVICE => tr!("service.foreignService"),
        TASK_MANAGER_ACTIVE => tr!("service.taskManagerActive"),
        TASK_MANAGER_CONFLICT => tr!("service.taskManagerConflict"),
        TASK_MANAGER_FOLLOW => tr!("service.taskManagerFollow"),
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
    let code = e.code().0 as u32;
    if code >> 16 == 0x8007 {
        code & 0xffff
    } else {
        code
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn non_win32_hresult_is_not_misreported_as_access_denied() {
        for (hresult, expected) in [
            (0x80070005u32, 5),
            (0x80070715, 1813),
            (0x80004005, 0x80004005),
            (0xc0000022, 0xc0000022),
        ] {
            assert_eq!(
                win(windows::core::Error::from_hresult(windows::core::HRESULT(
                    hresult as i32
                ))),
                expected
            );
        }
    }
}
