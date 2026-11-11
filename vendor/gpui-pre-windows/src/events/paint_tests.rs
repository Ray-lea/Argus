//! Native paint tests run on an inactive desktop, without SwitchDesktop.
use super::acknowledge_hidden_paint;
use std::{cell::Cell, sync::mpsc, time::Duration};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::{GetUpdateRect, ValidateRect},
        System::{LibraryLoader::GetModuleHandleW, StationsAndDesktops::*},
        UI::WindowsAndMessaging::*,
    },
    core::{PCWSTR, w},
};

thread_local! {
    static PAINTS: Cell<u32> = const { Cell::new(0) };
    static ACKNOWLEDGE_HIDDEN: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        PAINTS.set(PAINTS.get() + 1);
        if !(ACKNOWLEDGE_HIDDEN.get() && acknowledge_hidden_paint(hwnd)) {
            // What a paint of a visible window does before drawing.
            unsafe {
                let _ = ValidateRect(Some(hwnd), None);
            }
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

/// Dispatches until the queue is empty, for at most `limit` messages.
fn pump(limit: u32) -> u32 {
    let mut message = MSG::default();
    let mut dispatched = 0;
    while dispatched < limit
        && unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool()
    {
        unsafe {
            DispatchMessageW(&message);
        }
        dispatched += 1;
    }
    dispatched
}

#[test]
#[ignore = "creates windows on an inactive desktop"]
fn a_hidden_window_with_an_update_region_is_painted_once() {
    let name: Vec<u16> = format!("ArgusHiddenPaint-{}", std::process::id())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let desktop = unsafe {
        CreateDesktopW(
            PCWSTR(name.as_ptr()),
            PCWSTR::null(),
            None,
            Default::default(),
            DESKTOP_CREATEWINDOW.0
                | DESKTOP_ENUMERATE.0
                | DESKTOP_READOBJECTS.0
                | DESKTOP_WRITEOBJECTS.0,
            None,
        )
    }
    .unwrap();
    let desktop_raw = desktop.0 as usize;
    let (stop, wait) = mpsc::channel();
    let monitor = std::thread::spawn(move || {
        while wait.recv_timeout(Duration::from_millis(50)).is_err() {
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
            }
            assert_ne!(pid, std::process::id(), "test took the user's foreground");
        }
    });
    let result = std::thread::spawn(move || unsafe {
        SetThreadDesktop(HDESK(desktop_raw as _)).unwrap();
        let module = GetModuleHandleW(None).unwrap();
        let class = w!("ArgusHiddenPaintTest");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        });
        // The surface of a GPUI window: composition without a redirection bitmap.
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("Argus hidden paint test"),
            WS_POPUP,
            80,
            80,
            320,
            240,
            None,
            None,
            Some(module.into()),
            None,
        )
        .unwrap();
        struct Owned(HWND);
        impl Drop for Owned {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let _window = Owned(hwnd);
        SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap();
        pump(1000);
        assert!(!GetUpdateRect(hwnd, None, false).as_bool());

        // A fade-out: transparent, hidden, then opaque and no longer layered.
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA).unwrap();
        let _ = ShowWindow(hwnd, SW_HIDE);
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA).unwrap();
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
        assert!(!IsWindowVisible(hwnd).as_bool());
        assert!(
            GetUpdateRect(hwnd, None, false).as_bool(),
            "restoring the opacity no longer leaves an update region on a hidden window"
        );

        // Validation alone keeps the region: every empty queue paints again.
        PAINTS.set(0);
        pump(64);
        assert!(PAINTS.get() > 8, "paints={}", PAINTS.get());
        assert!(GetUpdateRect(hwnd, None, false).as_bool());

        ACKNOWLEDGE_HIDDEN.set(true);
        PAINTS.set(0);
        pump(64);
        assert_eq!(PAINTS.get(), 1);
        assert!(!GetUpdateRect(hwnd, None, false).as_bool());
        assert_eq!(pump(64), 0);

        // A visible window keeps the ordinary path.
        SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap();
        assert!(!acknowledge_hidden_paint(hwnd));
    })
    .join();
    stop.send(()).unwrap();
    monitor.join().unwrap();
    // Windows may retain helper threads until this short-lived test process exits.
    let _ = unsafe { CloseDesktop(desktop) };
    result.unwrap();
}
