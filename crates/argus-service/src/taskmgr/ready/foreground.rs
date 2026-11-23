//! The process that signals ready owns the UI; the launcher's child may
//! already have forwarded the request and exited. Windows supplies the
//! connected PID. The pipe carries no data and never waits for a peer.
use crate::{security::Descriptor, util::Handle};
use windows::Win32::{
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_WRITE_DATA, OPEN_EXISTING,
        PIPE_ACCESS_INBOUND, SECURITY_ANONYMOUS, SECURITY_SQOS_PRESENT,
    },
    System::Pipes::{CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_REJECT_REMOTE_CLIENTS},
    UI::WindowsAndMessaging::AllowSetForegroundWindow,
};
use windows::core::HSTRING;

fn name(id: &str) -> HSTRING {
    HSTRING::from(format!(r"\\.\pipe\Argus.Taskmgr.{id}.foreground"))
}

pub(super) fn listen(id: &str, security: &Descriptor) -> Option<Handle> {
    let pipe = unsafe {
        CreateNamedPipeW(
            &name(id),
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            0,
            0,
            Some(&security.attributes()),
        )
    };
    (!pipe.is_invalid()).then_some(Handle(pipe))
}

pub(super) fn connect(id: &str) -> Option<Handle> {
    unsafe {
        CreateFileW(
            &name(id),
            FILE_WRITE_DATA.0,
            Default::default(),
            None,
            OPEN_EXISTING,
            SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS,
            None,
        )
    }
    .ok()
    .map(Handle)
}

pub(super) fn client(pipe: &Handle) -> Option<u32> {
    let mut pid = 0;
    unsafe { GetNamedPipeClientProcessId(pipe.0, &mut pid) }.ok()?;
    (pid != 0).then_some(pid)
}

pub(super) fn grant(pipe: &Handle) {
    if let Some(pid) = client(pipe) {
        let _ = unsafe { AllowSetForegroundWindow(pid) };
    }
}
