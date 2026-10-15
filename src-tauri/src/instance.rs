//! Single-instance ownership is a held mutex, never the existence of its name.
//! Keep the old window/message names so older installed builds can be activated.
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tauri::{
    Manager, Runtime,
    plugin::{self, TauriPlugin},
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, ERROR_TIMEOUT, HANDLE, HWND, LPARAM, LRESULT,
    WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW,
    GWLP_USERDATA, GetWindowLongPtrW, GetWindowThreadProcessId, RegisterClassW, SMTO_ABORTIFHUNG,
    SMTO_BLOCK, SMTO_ERRORONEXIT, SendMessageTimeoutW, SetWindowLongPtrW, WM_COPYDATA,
    WM_NCDESTROY, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{Error, PCWSTR, Result};

const MESSAGE: usize = 1542;
const MAX_MESSAGE: usize = 64 * 1024;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Ownership stays on the main thread until the event loop has exited.
pub(crate) struct Owner {
    handle: Handle,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.handle.0);
        }
    }
}

pub(crate) fn acquire(id: &str, args: &[String]) -> Result<Option<Owner>> {
    acquire_with_timeout(id, args, Duration::from_secs(12))
}

fn acquire_with_timeout(id: &str, args: &[String], timeout: Duration) -> Result<Option<Owner>> {
    let name = wide(&format!("{id}-sim"));
    let handle = Handle(unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }?);
    let class = wide(&format!("{id}-sic"));
    let title = wide(&format!("{id}-siw"));
    let cwd = std::env::current_dir().unwrap_or_default();
    let payload = format!("{}|{}\0", cwd.to_string_lossy(), args.join("|"));
    if payload.len() > MAX_MESSAGE {
        return Err(Error::from_hresult(ERROR_INVALID_PARAMETER.to_hresult()));
    }
    let started = Instant::now();
    loop {
        match unsafe { WaitForSingleObject(handle.0, 0) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => {
                return Ok(Some(Owner {
                    handle,
                    _thread: PhantomData,
                }));
            }
            WAIT_TIMEOUT => {}
            _ => return Err(Error::from_thread()),
        }
        if let Ok(window) = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR(title.as_ptr())) } {
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(window, Some(&mut pid));
            }
            if pid != 0 {
                unsafe {
                    let _ = AllowSetForegroundWindow(pid);
                }
            }
            let packet = COPYDATASTRUCT {
                dwData: MESSAGE,
                cbData: payload.len() as u32,
                lpData: payload.as_ptr().cast_mut().cast(),
            };
            let mut reply = 0usize;
            let sent = unsafe {
                SendMessageTimeoutW(
                    window,
                    WM_COPYDATA,
                    WPARAM(0),
                    LPARAM((&packet as *const COPYDATASTRUCT) as isize),
                    SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
                    250,
                    Some(&mut reply),
                )
            };
            if sent.0 != 0 && reply == 1 {
                return Ok(None);
            }
            // A timed-out send can still be delivered later. Never replay it;
            // only retry ownership if the receiving process has exited.
            match unsafe { WaitForSingleObject(handle.0, 0) } {
                WAIT_OBJECT_0 | WAIT_ABANDONED => {
                    return Ok(Some(Owner {
                        handle,
                        _thread: PhantomData,
                    }));
                }
                _ => return Err(Error::from_hresult(ERROR_TIMEOUT.to_hresult())),
            }
        }
        if started.elapsed() >= timeout {
            return Err(Error::from_hresult(ERROR_TIMEOUT.to_hresult()));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn decode(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.is_empty() || bytes.len() > MAX_MESSAGE || bytes.last() != Some(&0) {
        return None;
    }
    let text = std::str::from_utf8(&bytes[..bytes.len() - 1]).ok()?;
    if text.contains('\0') {
        return None;
    }
    let (_, args) = text.split_once('|')?;
    let args: Vec<_> = args.split('|').take(129).map(str::to_owned).collect();
    (args.len() <= 128).then_some(args)
}

type Callback = Box<dyn Fn(Vec<String>)>;
struct Receiver(isize);

unsafe extern "system" fn receive(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let data = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *mut Callback;
    if message == WM_COPYDATA && !data.is_null() && lparam.0 != 0 {
        let packet = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
        if packet.dwData != MESSAGE
            || packet.lpData.is_null()
            || packet.cbData as usize > MAX_MESSAGE
        {
            return LRESULT(0);
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(packet.lpData.cast(), packet.cbData as usize) };
        if let Some(args) = decode(bytes) {
            unsafe {
                (*data)(args);
            }
            return LRESULT(1);
        }
        return LRESULT(0);
    }
    if message == WM_NCDESTROY && !data.is_null() {
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            drop(Box::from_raw(data));
        }
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn receiver(id: &str, callback: Callback) -> Result<Receiver> {
    let class = wide(&format!("{id}-sic"));
    let title = wide(&format!("{id}-siw"));
    let module = unsafe { GetModuleHandleW(None) }?;
    let definition = WNDCLASSW {
        lpfnWndProc: Some(receive),
        hInstance: module.into(),
        lpszClassName: PCWSTR(class.as_ptr()),
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&definition);
    }
    let window = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        )
    }?;
    unsafe {
        SetWindowLongPtrW(
            window,
            GWLP_USERDATA,
            Box::into_raw(Box::new(callback)) as isize,
        );
    }
    Ok(Receiver(window.0 as isize))
}

pub(crate) fn plugin<R: Runtime>(
    callback: impl Fn(&tauri::AppHandle<R>, Vec<String>) + Send + Sync + 'static,
) -> TauriPlugin<R> {
    plugin::Builder::new("argus-instance")
        .setup(move |app, _| {
            let handle = app.clone();
            let window = receiver(
                &app.config().identifier,
                Box::new(move |args| callback(&handle, args)),
            )?;
            app.manage(window);
            Ok(())
        })
        .on_event(|app, event| {
            if matches!(event, tauri::RunEvent::Exit)
                && let Some(receiver) = app.try_state::<Receiver>()
            {
                unsafe {
                    let _ = DestroyWindow(HWND(receiver.0 as *mut _));
                }
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn name() -> String {
        format!(
            "argus-instance-test-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        )
    }
    #[test]
    fn abandoned_owner_is_recovered_even_while_an_old_handle_keeps_the_name_alive() {
        let id = name();
        let named = wide(&format!("{id}-sim"));
        let retained =
            Handle(unsafe { CreateMutexW(None, false, PCWSTR(named.as_ptr())) }.unwrap());
        let worker_id = id.clone();
        std::thread::spawn(move || {
            let named = wide(&format!("{worker_id}-sim"));
            let handle =
                Handle(unsafe { CreateMutexW(None, false, PCWSTR(named.as_ptr())) }.unwrap());
            assert_eq!(unsafe { WaitForSingleObject(handle.0, 0) }, WAIT_OBJECT_0);
            // Closing this handle does not release ownership; thread exit abandons it.
        })
        .join()
        .unwrap();
        let owner = acquire_with_timeout(&id, &[], Duration::from_millis(100))
            .unwrap()
            .unwrap();
        let worker_id = id.clone();
        assert!(
            std::thread::spawn(move || acquire_with_timeout(
                &worker_id,
                &[],
                Duration::from_millis(50)
            )
            .is_err())
            .join()
            .unwrap()
        );
        drop(owner);
        drop(retained);
        assert!(
            acquire_with_timeout(&id, &[], Duration::from_millis(50))
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn activation_payload_is_bounded_and_compatible_with_older_windows_clients() {
        assert_eq!(
            decode(b"C:\\work|argus.exe|--hidden\0").unwrap(),
            vec!["argus.exe", "--hidden"]
        );
        for payload in [
            b"missing terminator".as_slice(),
            b"cwd|a\0b\0",
            b"no separator\0",
        ] {
            assert!(decode(payload).is_none());
        }
        assert!(decode(&vec![0; MAX_MESSAGE + 1]).is_none());
    }
    #[test]
    fn concurrent_launches_wait_for_the_receiving_window_and_forward_once() {
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
        };
        let id = name();
        let primary_id = id.clone();
        let (ready, wait_ready) = std::sync::mpsc::channel();
        let (activated, messages) = std::sync::mpsc::channel();
        let primary = std::thread::spawn(move || {
            let _owner = acquire(&primary_id, &[]).unwrap().unwrap();
            ready.send(()).unwrap();
            // The ownership gate intentionally precedes the window/event loop.
            std::thread::sleep(Duration::from_millis(100));
            let window = receiver(
                &primary_id,
                Box::new(move |args| {
                    activated.send(args).unwrap();
                }),
            )
            .unwrap();
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(800) {
                let mut msg = MSG::default();
                unsafe {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        DispatchMessageW(&msg);
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            unsafe {
                DestroyWindow(HWND(window.0 as *mut _)).unwrap();
            }
        });
        wait_ready.recv().unwrap();
        let launches: Vec<_> = (0..4)
            .map(|i| {
                let id = id.clone();
                std::thread::spawn(move || acquire(&id, &[format!("argus-{i}")]).unwrap().is_none())
            })
            .collect();
        for launch in launches {
            assert!(launch.join().unwrap());
        }
        primary.join().unwrap();
        let mut received: Vec<_> = messages.try_iter().map(|a| a[0].clone()).collect();
        received.sort();
        assert_eq!(received, ["argus-0", "argus-1", "argus-2", "argus-3"]);
        assert!(acquire(&id, &[]).unwrap().is_some());
    }
}
