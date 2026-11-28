//! Argus patch: `TaskDialogIndirect` is looked up when a prompt opens instead
//! of being imported. Only comctl32.dll 6 has it. A process without a Common
//! Controls 6 activation context binds the system's version 5.82, and an
//! executable importing the function (by name, or by the SDK's ordinal 345,
//! which 5.82 lacks as well) then fails to load at all.
use std::sync::OnceLock;

use windows::{
    Win32::{
        Foundation::{ERROR_PROC_NOT_FOUND, HANDLE},
        System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        UI::Controls::TASKDIALOGCONFIG,
    },
    core::{BOOL, HRESULT, Result, s, w},
};

type TaskDialogIndirectFn = unsafe extern "system" fn(
    config: *const TASKDIALOGCONFIG,
    button: *mut i32,
    radio_button: *mut i32,
    verification_checked: *mut BOOL,
) -> HRESULT;

fn resolve() -> Option<TaskDialogIndirectFn> {
    static FUNCTION: OnceLock<Option<TaskDialogIndirectFn>> = OnceLock::new();
    *FUNCTION.get_or_init(|| {
        // The activation context redirects the name to comctl32.dll 6 when
        // the executable's manifest asks for it; the module stays loaded.
        let module = unsafe {
            LoadLibraryExW(
                w!("comctl32.dll"),
                None::<HANDLE>,
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        }
        .ok()?;
        let address = unsafe { GetProcAddress(module, s!("TaskDialogIndirect")) }?;
        // SAFETY: the export has the documented `TaskDialogIndirect` signature.
        Some(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, TaskDialogIndirectFn>(
                address,
            )
        })
    })
}

/// `TaskDialogIndirect` with only the pressed button returned.
/// `ERROR_PROC_NOT_FOUND` when this process has no comctl32.dll 6.
pub(crate) unsafe fn task_dialog_indirect(
    config: &TASKDIALOGCONFIG,
    button: &mut i32,
) -> Result<()> {
    let Some(function) = resolve() else {
        return Err(ERROR_PROC_NOT_FOUND.to_hresult().into());
    };
    unsafe { function(config, button, std::ptr::null_mut(), std::ptr::null_mut()) }.ok()
}
