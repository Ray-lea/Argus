//! `GetWindowSubclass` imported by ordinal, as the Windows SDK's
//! ComCtl32.Lib imports it. Without a Common Controls 6 activation context a
//! process binds the system's comctl32.dll 5.82, which exports this function
//! only as ordinal 411 (version 6 exports the same ordinal by name too). The
//! `windows` crate imports it by name, and an executable that does so fails to
//! load in such a process ("entry point GetWindowSubclass not found"), as the
//! copy of argus.exe that the installer runs from its temporary folder did on
//! a user's machine.
use windows::{
    Win32::{Foundation::HWND, UI::Shell::SUBCLASSPROC},
    core::BOOL,
};

#[link(name = "comctl32.dll", kind = "raw-dylib", modifiers = "+verbatim")]
unsafe extern "system" {
    // Not named `GetWindowSubclass`: rustc merges the raw-dylib imports of a
    // library by symbol name, and the `windows` crate declares that name as a
    // by-name import.
    #[link_ordinal(411)]
    fn argus_get_window_subclass_ordinal(
        hwnd: HWND,
        procedure: SUBCLASSPROC,
        id: usize,
        data: *mut usize,
    ) -> BOOL;
}

/// The reference data of the subclass `(procedure, id)` on `hwnd`, when that
/// subclass is installed.
pub(crate) fn data(hwnd: HWND, procedure: SUBCLASSPROC, id: usize) -> Option<usize> {
    let mut data = 0;
    // SAFETY: the call only reads the window's subclass list; `data` outlives it.
    unsafe { argus_get_window_subclass_ordinal(hwnd, procedure, id, &mut data) }
        .as_bool()
        .then_some(data)
}
