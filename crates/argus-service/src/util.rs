use windows::Win32::Foundation::{CloseHandle, HANDLE};

pub type Result<T> = std::result::Result<T, u32>;
pub const UNSAFE_PATH: u32 = 0x2000_0001;
pub const OTHER_OWNER: u32 = 0x2000_0002;
pub const INCOMPATIBLE: u32 = 0x2000_0003;
pub const FOREIGN_SERVICE: u32 = 0x2000_0004;

pub fn message(code: u32) -> String {
    match code {
        UNSAFE_PATH => "服务目录的所有者、权限或链接不安全，已停止操作".into(),
        OTHER_OWNER => "特权服务已授权给另一个 Windows 账户，请先卸载再安装".into(),
        INCOMPATIBLE => "特权服务版本不兼容，请在设置中更新服务".into(),
        FOREIGN_SERVICE => "同名服务的配置不属于 Argus，已停止操作".into(),
        0x2000_0005 => {
            "任务管理器替换尚未可靠恢复，已停止服务更新或卸载；请先在设置中恢复 Windows 任务管理器"
                .into()
        }
        1223 => "已取消管理员授权，未更改服务".into(),
        1460 => "特权服务响应超时，请刷新状态后确认操作结果".into(),
        _ => format!(
            "{}（{code}）",
            std::io::Error::from_raw_os_error(code as i32)
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
