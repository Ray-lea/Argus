//! Native visibility tests run on an inactive desktop, without SwitchDesktop.
use super::{Motion, Op, SUBCLASS, Target, install, perform, procedure};
use std::{cell::Cell, sync::mpsc, time::Duration};
use windows::{
    Win32::{
        Foundation::*,
        System::{LibraryLoader::GetModuleHandleW, StationsAndDesktops::*},
        UI::{Shell::GetWindowSubclass, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};

thread_local! { static COMMANDS: Cell<(u32,u32)> = const { Cell::new((0,0)) }; }
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_SYSCOMMAND {
        let (minimize, restore) = COMMANDS.get();
        COMMANDS.set((
            minimize + u32::from(w.0 as u32 & 0xfff0 == SC_MINIMIZE),
            restore + u32::from(w.0 as u32 & 0xfff0 == SC_RESTORE),
        ));
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

#[test]
#[ignore = "creates windows on an inactive desktop"]
fn disabling_window_motion_releases_native_commands_and_cleans_an_active_fade() {
    let name: Vec<u16> = format!("ArgusWindowMotion-{}", std::process::id())
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
        let class = w!("ArgusWindowMotionTest");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Argus window motion test"),
            WS_OVERLAPPEDWINDOW,
            80,
            80,
            800,
            600,
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
        install(hwnd);
        perform(hwnd, Op::Enable(false));
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
        let mut raw = 0;
        assert!(GetWindowSubclass(hwnd, Some(procedure), SUBCLASS, Some(&mut raw)).as_bool());
        let state = &*(raw as *const Motion);
        for command in [SC_MINIMIZE, SC_RESTORE] {
            SendMessageW(
                hwnd,
                WM_SYSCOMMAND,
                Some(WPARAM(command as usize)),
                Some(LPARAM(0)),
            );
            assert!(state.fade.get().is_none());
            assert!(state.ticker.borrow().is_none());
        }
        assert_eq!(COMMANDS.get(), (1, 1));
        assert!(!IsIconic(hwnd).as_bool());
        SendMessageW(
            hwnd,
            WM_SYSCOMMAND,
            Some(WPARAM(SC_MAXIMIZE as usize)),
            Some(LPARAM(0)),
        );
        assert!(IsZoomed(hwnd).as_bool());
        SendMessageW(
            hwnd,
            WM_SYSCOMMAND,
            Some(WPARAM(SC_RESTORE as usize)),
            Some(LPARAM(0)),
        );
        assert!(!IsZoomed(hwnd).as_bool());
        perform(hwnd, Op::Enable(true));
        SendMessageW(
            hwnd,
            WM_SYSCOMMAND,
            Some(WPARAM(SC_MINIMIZE as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(COMMANDS.get(), (1, 2));
        assert!(state.fade.get().is_some());
        assert!(state.ticker.borrow().is_some());
        assert_ne!(
            GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_LAYERED.0,
            0
        );
        perform(hwnd, Op::Enable(false));
        assert!(IsIconic(hwnd).as_bool());
        assert!(state.fade.get().is_none());
        assert!(state.ticker.borrow().is_none());
        assert!(!super::is_leaving());
        assert_eq!(
            GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_LAYERED.0,
            0
        );
        SendMessageW(
            hwnd,
            WM_SYSCOMMAND,
            Some(WPARAM(SC_RESTORE as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(COMMANDS.get(), (1, 3));
        perform(hwnd, Op::Go(Target::Hidden));
        assert!(!IsWindowVisible(hwnd).as_bool());
        perform(hwnd, Op::Go(Target::Shown));
        assert!(IsWindowVisible(hwnd).as_bool());
        assert!(state.fade.get().is_none());
        // An animation tool's layered style and opacity remain its own.
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 180, LWA_ALPHA).unwrap();
        perform(hwnd, Op::Enable(true));
        perform(hwnd, Op::Go(Target::Hidden));
        perform(hwnd, Op::Enable(false));
        let mut alpha = 0;
        GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), None).unwrap();
        assert_eq!(alpha, 180);
        assert_ne!(
            GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_LAYERED.0,
            0
        );
    })
    .join();
    stop.send(()).unwrap();
    monitor.join().unwrap();
    // Windows may retain helper threads until this short-lived test process exits.
    let _ = unsafe { CloseDesktop(desktop) };
    result.unwrap();
}
