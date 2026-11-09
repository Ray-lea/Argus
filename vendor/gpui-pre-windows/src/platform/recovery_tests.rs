use super::{TrackedWindow, for_each_recovery_window};
use gpui::PlatformFrameSignal;
use parking_lot::RwLock;
use smallvec::SmallVec;
use std::{sync::Arc, time::Duration};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
                PostThreadMessageW, SendMessageW, WM_APP, WM_QUIT, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

const NOTIFY: u32 = WM_APP + 0x73;

unsafe extern "system" fn lifecycle_callback(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    if message == NOTIFY {
        let windows = unsafe { &*(data as *const RwLock<SmallVec<[TrackedWindow; 4]>>) };
        // Closing or opening a GPUI window takes this same write lock. Bound
        // the wait so the regression reports the deadlock instead of hanging.
        if let Some(mut windows) = windows.try_write_for(Duration::from_millis(200)) {
            windows.clear();
            return LRESULT(1);
        }
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, message, w, l) }
}

#[test]
fn synchronous_recovery_allows_window_lifecycle_on_the_receiving_thread() {
    let windows = Arc::new(RwLock::new(SmallVec::new()));
    let receiver_windows = windows.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let receiver = std::thread::spawn(move || unsafe {
        let hwnd = CreateWindowExW(
            Default::default(),
            w!("STATIC"),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(
            SetWindowSubclass(
                hwnd,
                Some(lifecycle_callback),
                1,
                Arc::as_ptr(&receiver_windows) as usize
            )
            .as_bool()
        );
        tx.send((hwnd.0 as usize, GetCurrentThreadId())).unwrap();
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            DispatchMessageW(&message);
        }
        let _ = RemoveWindowSubclass(hwnd, Some(lifecycle_callback), 1);
        DestroyWindow(hwnd).unwrap();
    });
    let (hwnd, thread) = rx.recv().unwrap();
    windows.write().push(TrackedWindow {
        handle: HWND(hwnd as _).into(),
        frame_signal: Arc::new(PlatformFrameSignal::new()),
    });
    let mut writable_in_callback = false;
    for_each_recovery_window(&windows, |window| {
        writable_in_callback =
            unsafe { SendMessageW(window.as_raw(), NOTIFY, Some(WPARAM(0)), Some(LPARAM(0))) }.0
                == 1;
    });
    unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) }.unwrap();
    receiver.join().unwrap();
    assert!(
        writable_in_callback,
        "recovery held the window registry while waiting for its UI thread"
    );
    assert!(windows.read().is_empty());
}
