//! Which processes own an app window: the test Task Manager uses to list a
//! process under "Apps" rather than "Background processes".

use std::collections::HashSet;

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DWM_CLOAKED_SHELL, DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetWindow, GetWindowLongPtrW,
    GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, w};

/// Shell surfaces that are visible top-level windows but not apps.
const SHELL_CLASSES: [&str; 4] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

/// IDs of the processes that own at least one window shown on the taskbar.
pub fn owners() -> HashSet<u32> {
    let mut pids = HashSet::new();
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&raw mut pids as isize));
    }
    pids
}

unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `owners` passes its set and outlives the enumeration.
    let pids = unsafe { &mut *(lparam.0 as *mut HashSet<u32>) };
    if let Some(pid) = app_window_owner(hwnd) {
        pids.insert(pid);
    }
    true.into()
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..len])
}

fn process_id(hwnd: HWND) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn cloaked(hwnd: HWND) -> u32 {
    let mut value = 0u32;
    let ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut value).cast(),
            size_of::<u32>() as u32,
        )
    };
    if ok.is_ok() { value } else { 0 }
}

/// The process a taskbar window belongs to; None for every other window.
fn app_window_owner(hwnd: HWND) -> Option<u32> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        // Same rule as the taskbar: unowned and not a tool window, unless the
        // window asks for a taskbar button.
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid());
        if ex & WS_EX_APPWINDOW.0 == 0 && (ex & WS_EX_TOOLWINDOW.0 != 0 || owned) {
            return None;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err()
            || rect.right <= rect.left
            || rect.bottom <= rect.top
        {
            return None;
        }
    }
    let class = class_name(hwnd);
    if SHELL_CLASSES.contains(&class.as_str()) {
        return None;
    }
    let cloak = cloaked(hwnd);
    if class == "ApplicationFrameWindow" {
        // A packaged app's frame belongs to ApplicationFrameHost; the app is
        // the process behind the core window inside it. A cloaked frame is a
        // suspended app that is no longer on screen.
        if cloak != 0 {
            return None;
        }
        let core =
            unsafe { FindWindowExW(Some(hwnd), None, w!("Windows.UI.Core.CoreWindow"), None) }
                .ok()?;
        let pid = process_id(core);
        return (pid != 0 && pid != process_id(hwnd)).then_some(pid);
    }
    // DWM hides shell surfaces and background windows by cloaking them; the
    // shell also cloaks windows on other virtual desktops, which still count.
    // A packaged app's core window shows inside a frame, so one at the top
    // level that the shell cloaks is not on screen (a suspended Settings, the
    // input experience host).
    if cloak != 0 && (cloak != DWM_CLOAKED_SHELL || class == "Windows.UI.Core.CoreWindow") {
        return None;
    }
    let pid = process_id(hwnd);
    (pid != 0).then_some(pid)
}
