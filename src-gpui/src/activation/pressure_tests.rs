//! Foreground/z-order checks stay on an inactive desktop; no SwitchDesktop.
use crate::activation;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        System::{LibraryLoader::GetModuleHandleW, StationsAndDesktops::*, Threading::*},
        UI::WindowsAndMessaging::*,
    },
    core::{PCWSTR, w},
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
struct Desktop {
    current: HDESK,
}
impl Desktop {
    fn new() -> Self {
        // Windows can retain GUI helper threads after the last window closes.
        // One desktop belongs to this short-lived test process and exits with it.
        static DESKTOP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let raw = *DESKTOP.get_or_init(|| {
            let name = wide(&format!(
                "ArgusPressure-{}-{}",
                std::process::id(),
                unsafe { GetCurrentThreadId() }
            ));
            unsafe {
                let current = CreateDesktopW(
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
                .unwrap();
                current.0 as usize
            }
        });
        Self {
            current: HDESK(raw as _),
        }
    }
}

fn on_desktop(body: impl FnOnce(usize) + Send + 'static) {
    let desktop = Desktop::new();
    let raw = desktop.current.0 as usize;
    let (finish, stop) = mpsc::channel();
    let monitor = std::thread::spawn(move || {
        while stop.recv_timeout(Duration::from_millis(50)).is_err() {
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
            }
            assert_ne!(
                pid,
                std::process::id(),
                "inactive desktop changed the user's foreground"
            );
        }
    });
    let result = std::thread::spawn(move || {
        unsafe {
            SetThreadDesktop(HDESK(raw as _)).unwrap();
        }
        body(raw);
    })
    .join();
    finish.send(()).unwrap();
    monitor.join().unwrap();
    result.unwrap();
}

struct Window(HWND);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0).unwrap();
        }
    }
}
unsafe extern "system" fn procedure(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, w, l) }
}
fn window(topmost: bool) -> Window {
    unsafe {
        let class = w!("ArgusPressureTestWindow");
        let module = GetModuleHandleW(None).unwrap();
        let definition = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&definition);
        let hwnd = CreateWindowExW(
            if topmost {
                WS_EX_TOPMOST | WS_EX_NOACTIVATE
            } else {
                WINDOW_EX_STYLE(0)
            },
            class,
            w!("Owned pressure window"),
            WS_OVERLAPPEDWINDOW,
            80,
            80,
            1280,
            820,
            None,
            None,
            Some(module.into()),
            None,
        )
        .unwrap();
        SetWindowPos(
            hwnd,
            Some(if topmost { HWND_TOPMOST } else { HWND_BOTTOM }),
            0,
            0,
            0,
            0,
            SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap();
        Window(hwnd)
    }
}

#[test]
#[ignore = "creates windows on an inactive desktop"]
fn an_unresponsive_topmost_window_does_not_cover_an_explicitly_activated_window() {
    on_desktop(|raw| {
        let target = window(false);
        activation::install(target.0);
        let (ready, shown) = mpsc::channel();
        let (release, release_wait) = mpsc::channel();
        let blocker = std::thread::spawn(move || unsafe {
            SetThreadDesktop(HDESK(raw as _)).unwrap();
            let window = window(true);
            ready.send(window.0.0 as usize).unwrap();
            // No message pumping until the test releases this thread.
            let _ = release_wait.recv_timeout(Duration::from_secs(12));
            drop(window);
        });
        let blocker_hwnd = HWND(shown.recv_timeout(Duration::from_secs(2)).unwrap() as _);
        std::thread::sleep(Duration::from_millis(5100));
        let hung = unsafe { IsHungAppWindow(blocker_hwnd).as_bool() };
        let start = Instant::now();
        activation::focus(target.0);
        let elapsed = start.elapsed();
        let raised =
            unsafe { GetWindowLongPtrW(target.0, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 };
        let mut top = unsafe { GetTopWindow(None) }.ok();
        let mut ahead = false;
        while let Some(hwnd) = top {
            if hwnd == target.0 {
                ahead = true;
                break;
            }
            if hwnd == blocker_hwnd {
                break;
            }
            top = unsafe { GetWindow(hwnd, GW_HWNDNEXT) }.ok();
        }
        let mut gui = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            GetGUIThreadInfo(GetCurrentThreadId(), &mut gui).unwrap();
        }
        println!(
            "hung={hung} raised={raised} ahead={ahead} focus={} activation_us={}",
            gui.hwndFocus == target.0,
            elapsed.as_micros()
        );
        unsafe {
            SendMessageW(
                target.0,
                WM_ACTIVATE,
                Some(WPARAM(WA_INACTIVE as usize)),
                Some(LPARAM(0)),
            );
        }
        let restored =
            unsafe { GetWindowLongPtrW(target.0, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0 };
        release.send(()).unwrap();
        blocker.join().unwrap();
        let focused = gui.hwndFocus == target.0;
        drop(target);
        assert!(hung, "the obstruction did not reach the unresponsive state");
        assert!(focused);
        assert!(raised, "activated window stayed below the topmost blocker");
        assert!(ahead);
        assert!(restored);
        assert!(elapsed < Duration::from_millis(200));
    });
}

#[test]
#[ignore = "creates windows on an inactive desktop"]
fn normal_and_previously_pinned_windows_keep_their_placement_policy() {
    on_desktop(|_| {
        let target = window(false);
        activation::install(target.0);
        activation::focus(target.0);
        assert_eq!(
            unsafe { GetWindowLongPtrW(target.0, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0,
            0
        );
        unsafe {
            SetWindowPos(
                target.0,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            )
            .unwrap();
        }
        activation::focus(target.0);
        unsafe {
            SendMessageW(
                target.0,
                WM_ACTIVATE,
                Some(WPARAM(WA_INACTIVE as usize)),
                Some(LPARAM(0)),
            );
        }
        assert_ne!(
            unsafe { GetWindowLongPtrW(target.0, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0,
            0
        );
        drop(target);
    });
}

#[test]
#[ignore = "creates windows on an inactive desktop"]
fn deactivation_keeps_the_newly_active_normal_window_above_argus() {
    on_desktop(|_| {
        let target = window(false);
        activation::install(target.0);
        let blocker = window(true);
        let next = window(false);
        for _ in 0..4 {
            activation::focus(target.0);
            assert_ne!(
                unsafe { GetWindowLongPtrW(target.0, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0,
                0
            );
            unsafe {
                // A different input queue can finish activation before the old
                // window handles WM_ACTIVATE/WA_INACTIVE.
                SetWindowPos(
                    next.0,
                    Some(HWND_TOP),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                )
                .unwrap();
                SendMessageW(
                    target.0,
                    WM_ACTIVATE,
                    Some(WPARAM(WA_INACTIVE as usize)),
                    Some(LPARAM(next.0.0 as isize)),
                );
            }
            assert_eq!(
                unsafe { GetWindowLongPtrW(target.0, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0,
                0
            );
            let mut previous = unsafe { GetWindow(target.0, GW_HWNDPREV) }.ok();
            let mut next_is_above = false;
            while let Some(hwnd) = previous {
                if hwnd == next.0 {
                    next_is_above = true;
                    break;
                }
                previous = unsafe { GetWindow(hwnd, GW_HWNDPREV) }.ok();
            }
            assert!(
                next_is_above,
                "deactivation covered the newly active window"
            );
        }
        drop(blocker);
    });
}

#[test]
#[ignore = "creates windows on an inactive desktop"]
fn switching_to_a_topmost_window_and_hiding_both_end_temporary_elevation() {
    on_desktop(|_| {
        let target = window(false);
        activation::install(target.0);
        let blocker = window(true);
        for hide in [false, true] {
            activation::focus(target.0);
            assert_ne!(
                unsafe { GetWindowLongPtrW(target.0, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0,
                0
            );
            unsafe {
                if hide {
                    let _ = ShowWindow(target.0, SW_HIDE);
                } else {
                    SendMessageW(
                        target.0,
                        WM_ACTIVATE,
                        Some(WPARAM(WA_INACTIVE as usize)),
                        Some(LPARAM(blocker.0.0 as isize)),
                    );
                }
            }
            assert_eq!(
                unsafe { GetWindowLongPtrW(target.0, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0,
                0
            );
            assert_ne!(
                unsafe { GetWindowLongPtrW(blocker.0, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0,
                0
            );
        }
    });
}
