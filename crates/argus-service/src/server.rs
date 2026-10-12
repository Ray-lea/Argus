use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};
use windows::Win32::System::Services::*;
use windows::Win32::System::Threading::SetEvent;
use windows::core::{PCWSTR, PWSTR};

use crate::util::{Handle, Result, wide, win};

static STOP: OnceLock<Arc<Handle>> = OnceLock::new();
static STATUS: AtomicIsize = AtomicIsize::new(0);

pub fn dispatch() -> Result<()> {
    let mut name = wide(crate::SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR(name.as_mut_ptr()),
            lpServiceProc: Some(main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) }.map_err(win)
}

unsafe extern "system" fn control(
    code: u32,
    _: u32,
    _: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
) -> u32 {
    match code {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            let handle = SERVICE_STATUS_HANDLE(STATUS.load(Ordering::Acquire) as *mut _);
            report(handle, SERVICE_STOP_PENDING, 0);
            if let Some(event) = STOP.get() {
                unsafe {
                    let _ = SetEvent(event.0);
                }
            }
            0
        }
        SERVICE_CONTROL_INTERROGATE => 0,
        _ => 120,
    }
}

fn report(handle: SERVICE_STATUS_HANDLE, state: SERVICE_STATUS_CURRENT_STATE, error: u32) {
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        dwWin32ExitCode: error,
        dwCheckPoint: u32::from(state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING),
        dwWaitHint: if state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING {
            30_000
        } else {
            0
        },
        ..Default::default()
    };
    unsafe {
        let _ = SetServiceStatus(handle, &status);
    }
}

unsafe extern "system" fn main(_: u32, _: *mut PWSTR) {
    let Ok(handle) = (unsafe {
        RegisterServiceCtrlHandlerExW(
            PCWSTR(wide(crate::SERVICE_NAME).as_ptr()),
            Some(control),
            None,
        )
    }) else {
        return;
    };
    report(handle, SERVICE_START_PENDING, 0);
    STATUS.store(handle.0 as isize, Ordering::Release);
    let result = (|| {
        let stop = Arc::new(crate::pipe::event()?);
        let _ = STOP.set(stop.clone());
        let dir = crate::install::directory()?;
        let _parent = crate::security::protect_directory(dir.parent().ok_or(87u32)?)?;
        let _guard = crate::security::protect_directory(&dir)?;
        let owner = crate::install::owner(&dir)?;
        // A recovery conflict must not take the unrelated broker offline.
        let _ = crate::taskmgr::reconcile(&dir);
        crate::security::allow_process_query(&owner)?;
        argus_core::privilege::enable_debug_privilege();
        // ETW sessions survive crashes; this build owns only this service name.
        argus_core::net_process::stop_session(crate::network::SESSION);
        crate::pipe::serve(crate::PIPE_NAME, owner, stop, || {
            report(handle, SERVICE_RUNNING, 0)
        })
    })();
    report(handle, SERVICE_STOPPED, result.err().unwrap_or(0));
}
