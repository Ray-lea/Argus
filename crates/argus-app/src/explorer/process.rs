//! Prepare a replacement shell without running its main thread until the old one exits.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    ACL, ACL_REVISION, AddMandatoryAce, GetLengthSid, InitializeAcl, InitializeSecurityDescriptor,
    PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR, SYSTEM_MANDATORY_LABEL_ACE,
    SetSecurityDescriptorDacl, SetSecurityDescriptorSacl, TOKEN_DEFAULT_DACL,
    TOKEN_MANDATORY_LABEL, TokenDefaultDacl, TokenIntegrityLevel,
};
use windows::Win32::System::Environment::CreateEnvironmentBlock;
use windows::Win32::System::StationsAndDesktops::{
    GetProcessWindowStation, GetThreadDesktop, GetUserObjectInformationW, UOI_NAME,
};
use windows::Win32::System::SystemServices::{
    SECURITY_DESCRIPTOR_REVISION, SYSTEM_MANDATORY_LABEL_NO_READ_UP,
    SYSTEM_MANDATORY_LABEL_NO_WRITE_UP,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
    GetCurrentThreadId, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    OpenProcess, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, PROCESS_CREATE_PROCESS, PROCESS_INFORMATION,
    ResumeThread, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

use super::{ErrorKind, RestartError, Shell, Stage, token, token_data, verify_tokens};
use crate::tasks::{Environment, Handle, quote, wide};

struct ParentAttribute {
    list: Vec<usize>,
    parent: Box<HANDLE>,
}

impl ParentAttribute {
    fn new(parent: HANDLE) -> windows::core::Result<Self> {
        let mut bytes = 0;
        let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut bytes) };
        let mut list = vec![0; bytes.div_ceil(size_of::<usize>())];
        let pointer = LPPROC_THREAD_ATTRIBUTE_LIST(list.as_mut_ptr().cast());
        unsafe { InitializeProcThreadAttributeList(Some(pointer), 1, None, &mut bytes) }?;
        let mut value = Self {
            list,
            parent: Box::new(parent),
        };
        unsafe {
            UpdateProcThreadAttribute(
                value.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
                Some((&raw const *value.parent).cast()),
                size_of::<HANDLE>(),
                None,
                None,
            )
        }?;
        Ok(value)
    }

    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        LPPROC_THREAD_ATTRIBUTE_LIST(self.list.as_mut_ptr().cast())
    }
}

impl Drop for ParentAttribute {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.pointer()) };
    }
}

fn desktop() -> windows::core::Result<Vec<u16>> {
    fn name(handle: HANDLE) -> windows::core::Result<String> {
        let mut buffer = [0u16; 256];
        unsafe {
            GetUserObjectInformationW(
                handle,
                UOI_NAME,
                Some(buffer.as_mut_ptr().cast()),
                size_of_val(&buffer) as u32,
                None,
            )
        }?;
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Ok(String::from_utf16_lossy(&buffer[..end]))
    }
    let station = unsafe { GetProcessWindowStation() }?;
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) }?;
    Ok(wide(&format!(
        "{}\\{}",
        name(HANDLE(station.0))?,
        name(HANDLE(desktop.0))?
    )))
}

pub(super) struct Prepared {
    process: Handle,
    thread: Handle,
    pending: bool,
}

impl Prepared {
    pub(super) fn new(shell: &Shell) -> Result<Self, RestartError> {
        let shell_token = token(shell.process.0, true)?;
        let own = token(unsafe { GetCurrentProcess() }, false)?;
        verify_tokens(shell_token.0, own.0, !argus_core::privilege::is_elevated())
            .map_err(|e| e.at(Stage::Token))?;
        let parent = Handle(
            unsafe { OpenProcess(PROCESS_CREATE_PROCESS, false, shell.identity.pid) }
                .map_err(|e| RestartError::win(ErrorKind::Permission, e).at(Stage::Spawn))?,
        );
        let mut attribute = ParentAttribute::new(parent.0)
            .map_err(|e| RestartError::win(ErrorKind::Prepare, e).at(Stage::Spawn))?;
        let mut environment = std::ptr::null_mut();
        unsafe { CreateEnvironmentBlock(&mut environment, Some(shell_token.0), false) }
            .map_err(|e| RestartError::win(ErrorKind::Prepare, e).at(Stage::Environment))?;
        let environment = Environment(environment);
        // Process/thread object security otherwise comes from the elevated
        // creator, preventing the desktop account from managing its new shell.
        let dacl_data = token_data(shell_token.0, TokenDefaultDacl)?;
        let dacl = unsafe { &*dacl_data.as_ptr().cast::<TOKEN_DEFAULT_DACL>() };
        let integrity = token_data(shell_token.0, TokenIntegrityLevel)?;
        let sid = unsafe {
            (*integrity.as_ptr().cast::<TOKEN_MANDATORY_LABEL>())
                .Label
                .Sid
        };
        let bytes = size_of::<ACL>() + size_of::<SYSTEM_MANDATORY_LABEL_ACE>() - size_of::<u32>()
            + unsafe { GetLengthSid(sid) } as usize;
        let mut label_storage = vec![0u32; bytes.div_ceil(size_of::<u32>())];
        let label = label_storage.as_mut_ptr().cast::<ACL>();
        unsafe {
            InitializeAcl(label, bytes as u32, ACL_REVISION).and_then(|()| {
                AddMandatoryAce(
                    label,
                    ACL_REVISION,
                    Default::default(),
                    SYSTEM_MANDATORY_LABEL_NO_WRITE_UP | SYSTEM_MANDATORY_LABEL_NO_READ_UP,
                    sid,
                )
            })
        }
        .map_err(|e| RestartError::win(ErrorKind::Prepare, e).at(Stage::Spawn))?;
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        let descriptor_ptr = PSECURITY_DESCRIPTOR((&raw mut descriptor).cast());
        unsafe {
            InitializeSecurityDescriptor(descriptor_ptr, SECURITY_DESCRIPTOR_REVISION)
                .and_then(|()| {
                    SetSecurityDescriptorDacl(descriptor_ptr, true, Some(dacl.DefaultDacl), false)
                })
                .and_then(|()| SetSecurityDescriptorSacl(descriptor_ptr, true, Some(label), false))
        }
        .map_err(|e| RestartError::win(ErrorKind::Prepare, e).at(Stage::Spawn))?;
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor_ptr.0,
            bInheritHandle: false.into(),
        };
        let file = wide(&shell.path);
        let mut command = wide(&quote(&shell.path));
        let mut desktop = desktop()
            .map_err(|e| RestartError::win(ErrorKind::Prepare, e).at(Stage::Environment))?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.lpDesktop = PWSTR(desktop.as_mut_ptr());
        startup.lpAttributeList = attribute.pointer();
        let mut info = PROCESS_INFORMATION::default();
        // The parent's standard token is inherited even by an elevated caller.
        // Launch the system Explorer directly: a RunAsAdmin compatibility flag
        // on Argus must not prevent preparation of a standard-user replacement.
        unsafe {
            CreateProcessW(
                PCWSTR(file.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                Some(&security),
                Some(&security),
                false,
                CREATE_NO_WINDOW
                    | CREATE_SUSPENDED
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT,
                Some(environment.0),
                PCWSTR::null(),
                &startup.StartupInfo,
                &mut info,
            )
        }
        .map_err(|e| RestartError::win(ErrorKind::Prepare, e).at(Stage::Spawn))?;
        let prepared = Self {
            process: Handle(info.hProcess),
            thread: Handle(info.hThread),
            pending: true,
        };
        let child_token = token(prepared.process.0, false)?;
        verify_tokens(child_token.0, shell_token.0, true).map_err(|e| e.at(Stage::Token))?;
        Ok(prepared)
    }

    pub(super) fn start(mut self) -> Result<(), RestartError> {
        if unsafe { ResumeThread(self.thread.0) } == u32::MAX {
            return Err(
                RestartError::win(ErrorKind::Restore, windows::core::Error::from_thread())
                    .at(Stage::Restart),
            );
        }
        self.pending = false;
        Ok(())
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if self.pending {
            // Only our never-started replacement is cancelled, never the live shell.
            let _ = unsafe { TerminateProcess(self.process.0, 1) };
            let _ = unsafe { WaitForSingleObject(self.process.0, 5000) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, WAIT_OBJECT_0};
    use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, SuspendThread,
    };

    #[test]
    fn abandoning_preparation_terminates_only_the_never_started_child() {
        let mut directory = [0u16; 32768];
        let length = unsafe { GetSystemDirectoryW(Some(&mut directory)) } as usize;
        assert!(length > 0 && length < directory.len());
        let shell = Shell {
            identity: super::super::Identity {
                pid: unsafe { GetCurrentProcessId() },
                create_time: 0,
            },
            process: Handle(
                unsafe {
                    OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                        false,
                        GetCurrentProcessId(),
                    )
                }
                .unwrap(),
            ),
            path: format!(
                "{}\\cmd.exe",
                String::from_utf16_lossy(&directory[..length])
            ),
        };
        let prepared = Prepared::new(&shell);
        if argus_core::privilege::is_elevated() {
            // Elevated parents must be rejected before a replacement is created.
            assert!(matches!(
                prepared,
                Err(RestartError {
                    kind: ErrorKind::Unsupported,
                    ..
                })
            ));
            return;
        }
        let prepared = prepared.unwrap_or_else(|error| panic!("{error:?}"));
        let previous = unsafe { SuspendThread(prepared.thread.0) };
        unsafe { ResumeThread(prepared.thread.0) };
        let mut observed = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                prepared.process.0,
                GetCurrentProcess(),
                &mut observed,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        }
        .unwrap();
        let observed = Handle(observed);
        drop(prepared);
        assert_eq!(previous, 1);
        assert_eq!(
            unsafe { WaitForSingleObject(observed.0, 5000) },
            WAIT_OBJECT_0
        );
    }
}
