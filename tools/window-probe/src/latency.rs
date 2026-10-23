//! A message-only window distinguishes desktop-wide waits from renderer work.
use std::{
    sync::{Mutex, OnceLock, mpsc},
    time::{Duration, Instant},
};
use windows::{
    Win32::{Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*},
    core::*,
};
static EVENTS: OnceLock<Mutex<Vec<serde_json::Value>>> = OnceLock::new();
const PING: u32 = WM_APP + 100;
unsafe extern "system" fn procedure(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        PING => {
            let now = crate::unix_ms();
            EVENTS.get().unwrap().lock().unwrap().push(serde_json::json!({"kind":"receive","startUnixMs":l.0,"unixMs":now,"durationMs":now-l.0 as i64}));
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, w, l) },
    }
}
pub fn run(path: &str, seconds: u64) -> anyhow::Result<()> {
    EVENTS.get_or_init(|| Mutex::new(Vec::new()));
    let (send, receive) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = w!("ArgusPrototypeMessageLatency");
        let definition = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: class,
            ..Default::default()
        };
        unsafe {
            if RegisterClassW(&definition) == 0 {
                return Err(Error::from_thread());
            }
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )?
        };
        let _ = send.send(hwnd.0 as usize);
        let mut message = MSG::default();
        unsafe {
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        unsafe {
            let _ = UnregisterClassW(class, Some(instance));
        }
        Ok(())
    });
    let hwnd = HWND(receive.recv_timeout(Duration::from_secs(10))? as *mut _);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(seconds) {
        let at = Instant::now();
        let unix = crate::unix_ms();
        unsafe {
            PostMessageW(Some(hwnd), PING, WPARAM(0), LPARAM(unix as isize))?;
        }
        EVENTS.get().unwrap().lock().unwrap().push(serde_json::json!({"kind":"post","startUnixMs":unix,"durationMs":at.elapsed().as_secs_f64()*1000.}));
        std::thread::sleep(Duration::from_millis(10));
    }
    unsafe {
        PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))?;
    }
    thread
        .join()
        .map_err(|_| anyhow::anyhow!("latency window thread failed"))??;
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&*EVENTS.get().unwrap().lock().unwrap())?,
    )?;
    Ok(())
}
