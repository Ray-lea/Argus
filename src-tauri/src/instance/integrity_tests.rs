use super::*;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows::Win32::Security::{
    DuplicateTokenEx, GetLengthSid, PSID, SID_AND_ATTRIBUTES, SecurityImpersonation,
    SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenIntegrityLevel, TokenPrimary,
};
use windows::Win32::System::SystemServices::SE_GROUP_INTEGRITY;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, GetCurrentProcess,
    GetExitCodeProcess, OpenProcessToken, PROCESS_INFORMATION, STARTUPINFOW, TerminateProcess,
};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW};
use windows::core::PWSTR;

struct Child(Handle);
impl Drop for Child {
    fn drop(&mut self) {
        unsafe {
            if WaitForSingleObject(self.0.0, 0) != WAIT_OBJECT_0 {
                let _ = TerminateProcess(self.0.0, 1);
                WaitForSingleObject(self.0.0, 1000);
            }
        }
    }
}

fn sender(id: &str) -> Child {
    let access = TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT;
    let mut own = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut own) }.unwrap();
    let own = Handle(own);
    let mut low = HANDLE::default();
    unsafe {
        DuplicateTokenEx(
            own.0,
            access,
            None,
            SecurityImpersonation,
            TokenPrimary,
            &mut low,
        )
    }
    .unwrap();
    let low = Handle(low);
    let mut sid = PSID::default();
    unsafe { ConvertStringSidToSidW(PCWSTR(wide("S-1-16-4096").as_ptr()), &mut sid) }.unwrap();
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid,
            Attributes: SE_GROUP_INTEGRITY as u32,
        },
    };
    let lowered = unsafe {
        SetTokenInformation(
            low.0,
            TokenIntegrityLevel,
            (&raw const label).cast(),
            size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(sid),
        )
    };
    unsafe {
        LocalFree(Some(HLOCAL(sid.0)));
    }
    lowered.unwrap();

    let key = "ARGUS_INSTANCE_TEST_ID";
    let mut variables: Vec<_> = std::env::vars_os()
        .filter(|(name, _)| !name.eq_ignore_ascii_case(key))
        .chain(Some((key.into(), id.into())))
        .collect();
    variables.sort_by_key(|(name, _)| name.to_string_lossy().to_ascii_lowercase());
    let mut environment = Vec::new();
    for (name, value) in variables {
        environment.extend(name.encode_wide());
        environment.push(b'=' as u16);
        environment.extend(value.encode_wide());
        environment.push(0);
    }
    environment.push(0);
    let exe = std::env::current_exe().unwrap();
    let mut command = wide(&format!(
        "\"{}\" --exact instance::integrity_tests::lower_integrity_sender --ignored",
        exe.display()
    ));
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessAsUserW(
            Some(low.0),
            PCWSTR(wide(&exe.to_string_lossy()).as_ptr()),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            Some(environment.as_ptr().cast()),
            PCWSTR::null(),
            &startup,
            &mut info,
        )
    }
    .unwrap();
    drop(Handle(info.hThread));
    Child(Handle(info.hProcess))
}

#[test]
#[ignore = "low-integrity subprocess for lower_integrity_launch_can_forward_to_existing_owner"]
fn lower_integrity_sender() {
    let id = std::env::var("ARGUS_INSTANCE_TEST_ID").unwrap();
    let result = acquire_with_timeout(&id, &["--hidden".into()], Duration::from_secs(3));
    std::process::exit(match result {
        Ok(None) => 0,
        Ok(Some(_)) => 87,
        Err(error) => error.code().0,
    });
}

#[test]
fn lower_integrity_launch_can_forward_to_existing_owner() {
    struct Window(Receiver);
    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(HWND(self.0.0 as *mut _));
            }
        }
    }
    let id = format!(
        "argus-instance-integrity-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
    );
    let _owner = acquire(&id, &[]).unwrap().unwrap();
    let (sent, received) = std::sync::mpsc::channel();
    let _window = Window(
        receiver(
            &id,
            Box::new(move |args| {
                sent.send(args).unwrap();
            }),
        )
        .unwrap(),
    );
    let child = sender(&id);
    let deadline = Instant::now() + Duration::from_secs(10);
    while unsafe { WaitForSingleObject(child.0.0, 0) } != WAIT_OBJECT_0 && Instant::now() < deadline
    {
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut code = 0;
    unsafe { GetExitCodeProcess(child.0.0, &mut code) }.unwrap();
    assert_eq!(code, 0, "cross-integrity handover failed: {code:#010x}");
    let messages: Vec<_> = received.try_iter().collect();
    assert_eq!(messages, [vec!["--hidden".to_owned()]]);
}
