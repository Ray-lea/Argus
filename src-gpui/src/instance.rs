//! Single-instance ownership is a held mutex, never the existence of its name.
//! Keep the old window/message names so older installed builds can be activated.
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::native_runtime::Host;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_DATA, ERROR_INVALID_PARAMETER, ERROR_TIMEOUT, GetLastError, HANDLE,
    HWND, LPARAM, LRESULT, SetLastError, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT, WIN32_ERROR,
    WPARAM,
};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
    TOKEN_QUERY, TokenIntegrityLevel,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateMutexExW, GetCurrentProcess, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION, ReleaseMutex, SYNCHRONIZATION_SYNCHRONIZE,
    WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW,
    DestroyWindow, FindWindowW, GWLP_USERDATA, GetWindowLongPtrW, GetWindowThreadProcessId,
    MSGFLT_ALLOW, RegisterClassW, SMTO_ABORTIFHUNG, SMTO_BLOCK, SMTO_ERRORONEXIT,
    SendMessageTimeoutW, SetWindowLongPtrW, WM_COPYDATA, WM_NCDESTROY, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{Error, PCWSTR, Result};

const MESSAGE: usize = 1542;
const MAX_MESSAGE: usize = 64 * 1024;

#[cfg(test)]
mod integrity_tests;

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

#[derive(Debug)]
pub(crate) struct Failure {
    operation: &'static str,
    error: Error,
    sender_integrity: Option<u32>,
    receiver_integrity: Option<u32>,
    waited_ms: u64,
}

impl Failure {
    pub fn code(&self) -> windows::core::HRESULT {
        self.error.code()
    }
    pub fn report(&self) {
        argus_service::taskmgr::ready::handover_failed(
            self.operation,
            self.error.code().0,
            self.sender_integrity,
            self.receiver_integrity,
            self.waited_ms,
        );
    }
}

fn integrity(process: HANDLE) -> Option<u32> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
    let token = Handle(token);
    let mut data = [0u64; 16];
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            Some(data.as_mut_ptr().cast()),
            size_of_val(&data) as u32,
            &mut size,
        )
    }
    .ok()?;
    let sid = unsafe { (*data.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
    let count = unsafe { *GetSidSubAuthorityCount(sid) };
    (count != 0).then(|| unsafe { *GetSidSubAuthority(sid, u32::from(count) - 1) })
}

fn failure(id: &str, operation: &'static str, error: Error, started: Instant) -> Failure {
    let receiver_integrity = (|| {
        let class = wide(&format!("{id}-sic"));
        let title = wide(&format!("{id}-siw"));
        let window = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR(title.as_ptr())) }.ok()?;
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut pid));
        }
        let process =
            Handle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
        integrity(process.0)
    })();
    Failure {
        operation,
        error,
        receiver_integrity,
        sender_integrity: integrity(unsafe { GetCurrentProcess() }),
        waited_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    }
}

pub(crate) fn acquire(id: &str, args: &[String]) -> std::result::Result<Option<Owner>, Failure> {
    acquire_with_timeout(id, args, Duration::from_secs(12))
}

fn acquire_with_timeout(
    id: &str,
    args: &[String],
    timeout: Duration,
) -> std::result::Result<Option<Owner>, Failure> {
    let started = Instant::now();
    let failed = |operation, error| failure(id, operation, error, started);
    let name = wide(&format!("{id}-sim"));
    // Waiting and releasing ownership need only SYNCHRONIZE. ALL_ACCESS
    // would deny a standard process opening an elevated instance's mutex.
    let handle = Handle(
        unsafe {
            CreateMutexExW(
                None,
                PCWSTR(name.as_ptr()),
                Default::default(),
                SYNCHRONIZATION_SYNCHRONIZE.0,
            )
        }
        .map_err(|error| failed("CreateMutexExW", error))?,
    );
    let class = wide(&format!("{id}-sic"));
    let title = wide(&format!("{id}-siw"));
    let cwd = std::env::current_dir().unwrap_or_default();
    let payload = format!("{}|{}\0", cwd.to_string_lossy(), args.join("|"));
    if payload.len() > MAX_MESSAGE {
        return Err(failed(
            "activation payload size",
            Error::from_hresult(ERROR_INVALID_PARAMETER.to_hresult()),
        ));
    }
    loop {
        match unsafe { WaitForSingleObject(handle.0, 0) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => {
                return Ok(Some(Owner {
                    handle,
                    _thread: PhantomData,
                }));
            }
            WAIT_TIMEOUT => {}
            _ => return Err(failed("WaitForSingleObject", Error::from_thread())),
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
                SetLastError(WIN32_ERROR(0));
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
            let error = if sent.0 != 0 {
                ERROR_INVALID_DATA
            } else {
                let code = unsafe { GetLastError() };
                if code.0 == 0 { ERROR_TIMEOUT } else { code }
            };
            // A timed-out send can still be delivered later. Never replay it;
            // only retry ownership if the receiving process has exited.
            match unsafe { WaitForSingleObject(handle.0, 0) } {
                WAIT_OBJECT_0 | WAIT_ABANDONED => {
                    return Ok(Some(Owner {
                        handle,
                        _thread: PhantomData,
                    }));
                }
                _ => {
                    return Err(failed(
                        "SendMessageTimeoutW",
                        Error::from_hresult(error.to_hresult()),
                    ));
                }
            }
        }
        if started.elapsed() >= timeout {
            return Err(failed(
                "receiving window discovery",
                Error::from_hresult(ERROR_TIMEOUT.to_hresult()),
            ));
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
    // Only this bounded activation protocol crosses UIPI, not other messages
    // or the main window. Task-manager requests still authenticate the launcher.
    if let Err(error) =
        unsafe { ChangeWindowMessageFilterEx(window, WM_COPYDATA, MSGFLT_ALLOW, None) }
    {
        unsafe {
            let _ = DestroyWindow(window);
        }
        return Err(error);
    }
    Ok(Receiver(window.0 as isize))
}

pub(crate) fn install(
    app: &Host,
    callback: impl Fn(&Host, Vec<String>) + Send + Sync + 'static,
) -> windows::core::Result<()> {
    let handle = app.clone();
    let window = receiver(
        &app.config().identifier,
        Box::new(move |args| callback(&handle, args)),
    )?;
    app.manage(window);
    Ok(())
}
pub(crate) fn shutdown(app: &Host) {
    if let Some(receiver) = app.try_state::<Receiver>() {
        unsafe {
            let _ = DestroyWindow(HWND(receiver.0 as *mut _));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::CreateMutexW;
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
