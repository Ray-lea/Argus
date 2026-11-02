//! Windows' file-message transport remains available across integrity levels.
use crate::native_runtime::Host;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::Ole::RevokeDragDrop,
    UI::{
        Shell::{
            DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, HDROP,
            RemoveWindowSubclass, SetWindowSubclass,
        },
        WindowsAndMessaging::{
            ChangeWindowMessageFilterEx, MSGFLT_ALLOW, WM_COPYDATA, WM_DROPFILES, WM_NCDESTROY,
        },
    },
};

const SUBCLASS: usize = 0x4152_4746;
// Used by the shell to marshal the drop's global-memory block.
const WM_COPYGLOBALDATA: u32 = 0x0049;
const MAX_FILES: u32 = 128;
const MAX_PATH_UNITS: usize = 32767;
const MAX_TOTAL_UNITS: usize = 65536;

#[derive(Debug, PartialEq, Eq)]
enum Rejected {
    Count,
    Length,
    Read,
    Encoding,
}

fn paths(drop: HDROP) -> Result<Vec<String>, Rejected> {
    let count = unsafe { DragQueryFileW(drop, u32::MAX, None) };
    if count > MAX_FILES {
        return Err(Rejected::Count);
    }
    let mut result = Vec::with_capacity(count as usize);
    let mut total = 0;
    for index in 0..count {
        let length = unsafe { DragQueryFileW(drop, index, None) } as usize;
        total += length;
        if length == 0 || length > MAX_PATH_UNITS || total > MAX_TOTAL_UNITS {
            return Err(Rejected::Length);
        }
        let mut name = vec![0; length + 1];
        if unsafe { DragQueryFileW(drop, index, Some(&mut name)) } as usize != length {
            return Err(Rejected::Read);
        }
        result.push(String::from_utf16(&name[..length]).map_err(|_| Rejected::Encoding)?);
    }
    Ok(result)
}

pub(super) fn install(host: &Host, hwnd: HWND) {
    let data = Box::into_raw(Box::new(host.clone()));
    if !unsafe { SetWindowSubclass(hwnd, Some(window_proc), SUBCLASS, data as usize) }.as_bool() {
        unsafe {
            drop(Box::from_raw(data));
        }
        argus_core::diagnostics::windows_failure(
            "file drop subclass",
            &windows::core::Error::from_thread(),
        );
        return;
    }
    unsafe {
        DragAcceptFiles(hwnd, true);
    }
    let mut allowed = true;
    for message in [WM_DROPFILES, WM_COPYGLOBALDATA, WM_COPYDATA] {
        if let Err(error) =
            unsafe { ChangeWindowMessageFilterEx(hwnd, message, MSGFLT_ALLOW, None) }
        {
            allowed = false;
            argus_core::diagnostics::windows_failure("file drop message filter", &error);
        }
    }
    if allowed && argus_core::privilege::is_elevated() {
        // An inaccessible OLE target prevents the shell's file-message
        // fallback. Elevated windows use the registered native transport.
        if let Err(error) = unsafe { RevokeDragDrop(hwnd) } {
            argus_core::diagnostics::windows_failure("administrator file drop transport", &error);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    if message == WM_DROPFILES {
        let handle = HDROP(wp.0 as _);
        let result = paths(handle);
        unsafe {
            DragFinish(handle);
        }
        match result {
            Ok(paths) if !paths.is_empty() => {
                let host = unsafe { &*(data as *const Host) };
                let _ = host.emit("argus-files-dropped", paths);
            }
            Err(reason) => {
                argus_core::diagnostics::warn("file drop rejected", format!("reason={reason:?}"))
            }
            _ => {}
        }
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(window_proc), id);
            let result = DefSubclassProc(hwnd, message, wp, lp);
            drop(Box::from_raw(data as *mut Host));
            return result;
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wp, lp) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{
        Foundation::{GlobalFree, HGLOBAL},
        System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
    };

    struct DropData(HGLOBAL);
    impl DropData {
        fn new(names: &[String]) -> Self {
            let mut bytes = Vec::new();
            // DROPFILES: data offset, point, non-client flag, UTF-16 flag.
            for value in [20u32, 0, 0, 0, 1] {
                bytes.extend(value.to_le_bytes());
            }
            for name in names {
                for unit in name.encode_utf16().chain(Some(0)) {
                    bytes.extend(unit.to_le_bytes());
                }
            }
            bytes.extend(0u16.to_le_bytes());
            unsafe {
                let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).unwrap();
                let pointer = GlobalLock(memory);
                assert!(!pointer.is_null());
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
                let _ = GlobalUnlock(memory);
                Self(memory)
            }
        }
        fn read(&self) -> Result<Vec<String>, Rejected> {
            paths(HDROP(self.0.0))
        }
    }
    impl Drop for DropData {
        fn drop(&mut self) {
            unsafe {
                let _ = GlobalFree(Some(self.0));
            }
        }
    }

    #[test]
    fn native_file_message_preserves_unicode_files_and_directories() {
        let expected = vec![
            r"C:\资料\example.txt".into(),
            r"C:\folder with spaces".into(),
        ];
        assert_eq!(DropData::new(&expected).read().unwrap(), expected);
    }
    #[test]
    fn native_file_message_rejects_excessive_payloads() {
        assert_eq!(
            DropData::new(&vec!["C:\\a".into(); 129]).read(),
            Err(Rejected::Count)
        );
        assert_eq!(
            DropData::new(&["a".repeat(32768)]).read(),
            Err(Rejected::Length)
        );
        assert_eq!(
            DropData::new(&vec!["a".repeat(24000); 3]).read(),
            Err(Rejected::Length)
        );
    }
}
