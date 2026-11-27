use super::state::Result;
use crate::security::token_user;
use crate::util::{Handle, wide};
use argus_core::tr;
use std::ffi::c_void;
use std::path::Path;
use std::time::{Duration, Instant};
use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows::Win32::Foundation::{
    E_ACCESSDENIED, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_EVENT, WAIT_OBJECT_0,
};
use windows::Win32::Security::{
    GetTokenInformation, TOKEN_ACCESS_MASK, TOKEN_DUPLICATE, TOKEN_ELEVATION_TYPE, TOKEN_QUERY,
    TokenElevationType, TokenElevationTypeFull, TokenElevationTypeLimited,
};
use windows::Win32::System::Diagnostics::Debug::{
    DebugActiveProcessStop, DebugSetProcessKillOnExit,
};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};
use windows::core::{PCWSTR, PWSTR};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
}

pub fn quote(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        result.push_str(&"\\".repeat(if c == '"' { slashes * 2 + 1 } else { slashes }));
        slashes = 0;
        result.push(c);
    }
    result.push_str(&"\\".repeat(slashes * 2));
    result.push('"');
    result
}

pub fn system_directory() -> Result<std::path::PathBuf> {
    let mut buffer = [0u16; 1024];
    let n = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    if n == 0 || n >= buffer.len() {
        return Err(tr!("taskManager.noSystemDirectory"));
    }
    Ok(String::from_utf16_lossy(&buffer[..n]).into())
}

/// The caller supplies a fixed executable, never an IFEO-appended command.
/// The debug connection is detached before resuming, on the same OS thread.
pub fn spawn(exe: &Path, args: &[String], bypass: bool) -> Result<Handle> {
    create(exe, args, bypass, None, false)
}

/// Starts the interface with the account's standard rights. A launcher that
/// is not elevated, or whose account has no split token (UAC off, the
/// built-in Administrator), starts it directly; an elevated one starts it
/// under the desktop shell. The environment is rebuilt for this account: the
/// launcher may have inherited an administrator's (see `probe_as_caller`).
pub fn standard_spawn(exe: &Path, args: &[String]) -> Result<Handle> {
    let shell = if elevation_type(own_token(TOKEN_QUERY)?.0)? == TokenElevationTypeFull {
        Some(shell()?)
    } else {
        None
    };
    create(exe, args, false, shell.as_ref().map(|s| s.0), true)
}

/// Starts the readiness probe as the account that asked for the replacement,
/// never as the administrator who approved it (UAC may take another account's
/// credentials): the probe is a child of the caller's process and inherits its
/// token, whatever kind it is.
pub fn probe_as_caller(
    exe: &Path,
    args: &[String],
    pid: u32,
    created: u64,
    owner: &str,
) -> Result<Handle> {
    let process = open_parent(pid)?;
    if identity(process.0)? != created
        || unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0
    {
        return Err(tr!("taskManager.callerExited"));
    }
    let token = process_token(process.0, TOKEN_QUERY)?;
    if token_user(token.0).map_err(crate::util::message)? != owner {
        return Err(tr!("taskManager.callerAccount"));
    }
    let mut caller_session = 0;
    let mut session = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut caller_session) } == 0
        || unsafe { ProcessIdToSessionId(std::process::id(), &mut session) } == 0
    {
        return Err(windows::core::Error::from_thread().to_string());
    }
    if caller_session != session {
        return Err(tr!("taskManager.callerSession"));
    }
    create(exe, args, false, Some(process.0), false)
}

/// Opens a process to create children under it. When UAC took another
/// administrator's credentials, the DACL of the caller's processes grants this
/// helper nothing; administrators may still open them with SeDebugPrivilege.
fn open_parent(pid: u32) -> Result<Handle> {
    let open = || unsafe {
        OpenProcess(
            PROCESS_CREATE_PROCESS | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        )
    };
    match open() {
        Err(e) if e.code() == E_ACCESSDENIED && argus_core::privilege::enable_debug_privilege() => {
            open()
        }
        result => result,
    }
    .map(Handle)
    .map_err(|e| e.to_string())
}

/// The desktop shell's process, for an elevated launcher to start the
/// interface under: Explorer runs with the account's standard token. The
/// linked token of an elevated token is no substitute: without SeTcbPrivilege
/// it comes at identification level, and Windows refuses to make a primary
/// token from it (ERROR_BAD_IMPERSONATION_LEVEL).
fn shell() -> Result<Handle> {
    let window = unsafe { GetShellWindow() };
    let mut pid = 0;
    if window.0.is_null()
        || unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) } == 0
        || pid == 0
    {
        return Err(tr!("taskManager.noShell"));
    }
    let process = open_parent(pid)?;
    let token = process_token(process.0, TOKEN_QUERY)?;
    let own = own_token(TOKEN_QUERY)?;
    if token_user(token.0).map_err(crate::util::message)?
        != token_user(own.0).map_err(crate::util::message)?
        || elevation_type(token.0)? == TokenElevationTypeFull
    {
        return Err(tr!("taskManager.noStandardToken"));
    }
    Ok(process)
}

fn process_token(process: HANDLE, access: TOKEN_ACCESS_MASK) -> Result<Handle> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, access, &mut token) }.map_err(|e| e.to_string())?;
    Ok(Handle(token))
}

fn own_token(access: TOKEN_ACCESS_MASK) -> Result<Handle> {
    process_token(unsafe { GetCurrentProcess() }, access)
}

fn elevation_type(token: HANDLE) -> Result<TOKEN_ELEVATION_TYPE> {
    let mut kind = TOKEN_ELEVATION_TYPE::default();
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token,
            TokenElevationType,
            Some((&raw mut kind).cast()),
            size_of::<TOKEN_ELEVATION_TYPE>() as u32,
            &mut size,
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(kind)
}

/// An attribute list that makes `parent` the new process's parent: the child
/// inherits its token, session and job instead of the creator's.
struct ParentAttribute {
    list: Vec<u64>,
    parent: Box<HANDLE>,
}

impl ParentAttribute {
    fn new(parent: HANDLE) -> Result<Self> {
        let mut size = 0;
        // Fails with ERROR_INSUFFICIENT_BUFFER and reports the size.
        let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut size) };
        let mut list = vec![0u64; size.div_ceil(8)];
        unsafe {
            InitializeProcThreadAttributeList(
                Some(LPPROC_THREAD_ATTRIBUTE_LIST(list.as_mut_ptr().cast())),
                1,
                None,
                &mut size,
            )
        }
        .map_err(|e| e.to_string())?;
        let mut attribute = Self {
            list,
            parent: Box::new(parent),
        };
        let value = (&raw const *attribute.parent).cast::<c_void>();
        unsafe {
            UpdateProcThreadAttribute(
                attribute.list(),
                0,
                PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
                Some(value),
                size_of::<HANDLE>(),
                None,
                None,
            )
        }
        .map_err(|e| e.to_string())?;
        Ok(attribute)
    }

    fn list(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        LPPROC_THREAD_ATTRIBUTE_LIST(self.list.as_mut_ptr().cast())
    }
}

impl Drop for ParentAttribute {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.list()) };
    }
}

/// The environment Windows builds for this process's account.
struct Environment(*mut c_void);

impl Environment {
    fn own() -> Result<Self> {
        let token = own_token(TOKEN_QUERY | TOKEN_DUPLICATE)?;
        let mut block = std::ptr::null_mut();
        unsafe { CreateEnvironmentBlock(&mut block, Some(token.0), false) }
            .map_err(|e| e.to_string())?;
        Ok(Self(block))
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyEnvironmentBlock(self.0);
        }
    }
}

fn create(
    exe: &Path,
    args: &[String],
    bypass: bool,
    parent: Option<HANDLE>,
    own_environment: bool,
) -> Result<Handle> {
    start(exe, args, bypass, parent, own_environment, |_| Ok(())).map(|(process, ())| process)
}

/// `create`, running `before` with the new process's ID once it exists: with
/// `bypass`, before it runs. A failure there ends the new process.
fn start<T>(
    exe: &Path,
    args: &[String],
    bypass: bool,
    parent: Option<HANDLE>,
    own_environment: bool,
    before: impl FnOnce(u32) -> Result<T>,
) -> Result<(Handle, T)> {
    let file = wide(&exe.to_string_lossy());
    let mut command = wide(
        &std::iter::once(exe.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(|a| quote(&a))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let directory = wide(&system_directory()?.to_string_lossy());
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOW>() as u32;
    let mut flags = CREATE_NO_WINDOW
        | if bypass {
            DEBUG_ONLY_THIS_PROCESS | CREATE_SUSPENDED
        } else {
            Default::default()
        };
    let mut attribute = parent.map(ParentAttribute::new).transpose()?;
    if let Some(attribute) = attribute.as_mut() {
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attribute.list();
        flags |= EXTENDED_STARTUPINFO_PRESENT;
    }
    let environment = own_environment.then(Environment::own).transpose()?;
    if environment.is_some() {
        flags |= CREATE_UNICODE_ENVIRONMENT;
    }
    let mut info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(file.as_ptr()),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            false,
            flags,
            environment.as_ref().map(|e| e.0.cast_const()),
            PCWSTR(directory.as_ptr()),
            &startup.StartupInfo,
            &mut info,
        )
    }
    .map_err(|e| e.to_string())?;
    let process = Handle(info.hProcess);
    let thread = Handle(info.hThread);
    let end = || unsafe {
        let _ = TerminateProcess(process.0, 1);
        WaitForSingleObject(process.0, 1000);
    };
    let value = before(info.dwProcessId).inspect_err(|_| end())?;
    if bypass {
        let result = (|| {
            unsafe {
                DebugSetProcessKillOnExit(false)?;
                DebugActiveProcessStop(info.dwProcessId)?;
            }
            if unsafe { ResumeThread(thread.0) } == u32::MAX {
                return Err(windows::core::Error::from_thread());
            }
            Ok(())
        })();
        if let Err(error) = result {
            end();
            return Err(tr!("taskManager.detachFailed", error = error));
        }
    }
    Ok((process, value))
}

pub fn exit_code(process: &Handle, timeout: Duration) -> Result<u32> {
    if unsafe { WaitForSingleObject(process.0, timeout.as_millis().min(u32::MAX as u128) as u32) }
        != WAIT_OBJECT_0
    {
        unsafe {
            let _ = TerminateProcess(process.0, 1);
            WaitForSingleObject(process.0, 1000);
        }
        return Err(tr!("taskManager.probeTimeout"));
    }
    let mut code = 0;
    unsafe { GetExitCodeProcess(process.0, &mut code) }.map_err(|e| e.to_string())?;
    Ok(code)
}

/// Windows Task Manager started past IFEO, with what its elevated copy needs.
///
/// Started with an administrator's standard (limited) token, Task Manager asks
/// Windows for an elevated copy of itself (it is `autoElevate`) and exits with
/// E_ABORT. Windows creates that copy with the first one as its parent and
/// without the debug flag, so IFEO hands it to the launcher, which would open
/// Argus. The starter therefore leaves a marker named after the first one's
/// process ID (`relaunched_native` answers it) and keeps that process's handle,
/// so that no other process takes the ID meanwhile: the first one has exited
/// by the time the launcher runs, and an exited process reports no image.
pub struct Native {
    process: Handle,
    marker: Option<Handle>,
}

/// How long a Task Manager may run before its elevated copy (a consent prompt
/// may wait for the user), and how long the launcher may take after it exits.
const RELAUNCH_WAIT: Duration = Duration::from_secs(60);
const ANSWER_WAIT: Duration = Duration::from_secs(15);

pub fn native() -> Result<Native> {
    // Neither an elevated token nor an account without a split token gets a
    // copy. Without the answer, Task Manager still opens, unmarked.
    let limited = own_token(TOKEN_QUERY)
        .and_then(|token| elevation_type(token.0))
        .is_ok_and(|kind| kind == TokenElevationTypeLimited);
    let (process, marker) = start(
        &system_directory()?.join("taskmgr.exe"),
        &[],
        true,
        None,
        false,
        |pid| Ok(limited.then(|| mark(pid)).flatten()),
    )?;
    Ok(Native { process, marker })
}

impl Native {
    /// Keeps the marker until the launcher answers it, until `ANSWER_WAIT`
    /// after Task Manager exits, or until it has run for `RELAUNCH_WAIT`.
    pub fn wait(self) {
        let Some(marker) = self.marker else { return };
        let exited = WAIT_EVENT(WAIT_OBJECT_0.0 + 1);
        let millis = |d: Duration| d.as_millis() as u32;
        let handles = [marker.0, self.process.0];
        if unsafe { WaitForMultipleObjects(&handles, false, millis(RELAUNCH_WAIT)) } == exited {
            unsafe { WaitForSingleObject(marker.0, millis(ANSWER_WAIT)) };
        }
    }
}

fn marker_name(pid: u32) -> Vec<u16> {
    wide(&format!("Local\\Argus.Taskmgr.Native.{pid}"))
}

/// A name some other process already holds is not this starter's marker; the
/// copy then takes the launcher's usual path.
fn mark(pid: u32) -> Option<Handle> {
    let name = marker_name(pid);
    let event = Handle(unsafe { CreateEventW(None, true, false, PCWSTR(name.as_ptr())) }.ok()?);
    (unsafe { GetLastError() } != ERROR_ALREADY_EXISTS).then_some(event)
}

/// Whether this process is the elevated copy that a Task Manager from `native`
/// asked for. Answers the marker of the starter, which may then let it go.
pub fn relaunched_native() -> bool {
    let Some(parent) = parent_id() else {
        return false;
    };
    let name = marker_name(parent);
    let Ok(marker) = (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) })
    else {
        return false;
    };
    let marker = Handle(marker);
    // A parent ID outlives the parent: the process now holding it must have
    // been started before this one to be the one that asked.
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, parent) })
    else {
        return false;
    };
    let process = Handle(process);
    if !matches!(
        (identity(process.0), identity(unsafe { GetCurrentProcess() })),
        (Ok(started), Ok(own)) if started <= own
    ) {
        return false;
    }
    let _ = unsafe { SetEvent(marker.0) };
    true
}

/// `PROCESS_BASIC_INFORMATION`, which the `windows` crate gates behind a
/// feature this workspace does not enable.
#[repr(C)]
#[derive(Default)]
struct BasicInformation {
    exit_status: i32,
    peb: usize,
    affinity_mask: usize,
    base_priority: i32,
    id: usize,
    parent: usize,
}

fn parent_id() -> Option<u32> {
    let mut info = BasicInformation::default();
    unsafe {
        NtQueryInformationProcess(
            GetCurrentProcess(),
            ProcessBasicInformation,
            (&raw mut info).cast(),
            size_of::<BasicInformation>() as u32,
            std::ptr::null_mut(),
        )
    }
    .is_ok()
    .then_some(info.parent as u32)
}

pub fn identity(process: HANDLE) -> Result<u64> {
    let (mut c, mut e, mut k, mut u) = Default::default();
    unsafe { GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u) }
        .map_err(|e| e.to_string())?;
    Ok((u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
}

pub fn wait(event: HANDLE, parent: HANDLE, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    loop {
        if unsafe { WaitForSingleObject(parent, 0) } == WAIT_OBJECT_0 {
            return false;
        }
        if unsafe { WaitForSingleObject(event, 20) } == WAIT_OBJECT_0 {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Security::Authorization::ConvertStringSidToSidW;
    use windows::Win32::Security::{
        DuplicateTokenEx, GetLengthSid, GetSidSubAuthority, GetSidSubAuthorityCount, PSID,
        SID_AND_ATTRIBUTES, SecurityImpersonation, SetTokenInformation, TOKEN_ADJUST_DEFAULT,
        TOKEN_ASSIGN_PRIMARY, TOKEN_MANDATORY_LABEL, TokenIntegrityLevel, TokenPrimary,
    };
    use windows::Win32::System::SystemServices::SE_GROUP_INTEGRITY;

    fn fixture(name: &str) -> Vec<String> {
        vec![
            "--exact".into(),
            format!("taskmgr::launch::tests::{name}"),
            "--ignored".into(),
        ]
    }

    #[test]
    #[ignore = "child fixture used by detached_debug_start_runs_to_completion"]
    fn native_child() {
        assert_eq!(2 + 2, 4);
    }
    #[test]
    fn detached_debug_start_runs_to_completion() {
        let process = spawn(
            &std::env::current_exe().unwrap(),
            &fixture("native_child"),
            true,
        )
        .unwrap();
        assert_eq!(exit_code(&process, Duration::from_secs(5)).unwrap(), 0);
    }

    #[test]
    #[ignore = "child fixture used by a_relaunch_is_recognized_from_the_marker_of_its_parent"]
    fn relaunch_child() {
        assert!(relaunched_native());
    }
    #[test]
    #[ignore = "child fixture used by a_relaunch_is_recognized_from_the_marker_of_its_parent"]
    fn marked_parent() {
        let child = spawn(
            &std::env::current_exe().unwrap(),
            &fixture("relaunch_child"),
            false,
        )
        .unwrap();
        assert_eq!(exit_code(&child, Duration::from_secs(10)).unwrap(), 0);
    }

    /// The handoff `native` and the launcher rely on: a process started with
    /// a marker named after it is recognized from its child, which answers.
    #[test]
    fn a_relaunch_is_recognized_from_the_marker_of_its_parent() {
        let (parent, marker) = start(
            &std::env::current_exe().unwrap(),
            &fixture("marked_parent"),
            true,
            None,
            false,
            |pid| Ok(mark(pid)),
        )
        .unwrap();
        let marker = marker.unwrap();
        assert_eq!(exit_code(&parent, Duration::from_secs(20)).unwrap(), 0);
        assert_eq!(unsafe { WaitForSingleObject(marker.0, 0) }, WAIT_OBJECT_0);
        // The name is this starter's alone while it holds it.
        assert!(mark(unsafe { GetProcessId(parent.0) }).is_none());
        // No marker carries the name of this test's own parent.
        assert!(!relaunched_native());
    }

    #[test]
    fn windows_arguments_are_quoted_without_shell_interpretation() {
        assert_eq!(quote(r"C:\with space\"), "\"C:\\with space\\\\\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
    }

    fn integrity(token: HANDLE) -> u32 {
        let mut data = [0u64; 16];
        let mut size = 0;
        unsafe {
            GetTokenInformation(
                token,
                TokenIntegrityLevel,
                Some(data.as_mut_ptr().cast()),
                size_of_val(&data) as u32,
                &mut size,
            )
        }
        .unwrap();
        let sid = unsafe { (*data.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
        unsafe { *GetSidSubAuthority(sid, u32::from(*GetSidSubAuthorityCount(sid)) - 1) }
    }

    #[test]
    #[ignore = "child fixture used by children_inherit_the_parent_token"]
    fn waiting_parent() {
        std::thread::sleep(Duration::from_secs(30));
    }
    #[test]
    #[ignore = "child fixture used by children_inherit_the_parent_token"]
    fn low_integrity_child() {
        assert_eq!(integrity(own_token(TOKEN_QUERY).unwrap().0), 0x1000);
    }

    /// The probe and the interface rely on this: a child created under another
    /// process runs with that process's token, not the creator's. The parent
    /// here runs at low integrity, so the child must too.
    #[test]
    fn children_inherit_the_parent_token() {
        let own =
            own_token(TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT)
                .unwrap();
        assert!(integrity(own.0) > 0x1000);
        let mut low = HANDLE::default();
        unsafe {
            DuplicateTokenEx(
                own.0,
                TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
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
        unsafe {
            SetTokenInformation(
                low.0,
                TokenIntegrityLevel,
                (&raw const label).cast(),
                (size_of::<TOKEN_MANDATORY_LABEL>() as u32) + GetLengthSid(sid),
            )
        }
        .unwrap();
        // A lowered copy of the caller's own token needs no privilege.
        let exe = std::env::current_exe().unwrap();
        let mut command = wide(
            &std::iter::once(exe.to_string_lossy().into_owned())
                .chain(fixture("waiting_parent"))
                .map(|a| quote(&a))
                .collect::<Vec<_>>()
                .join(" "),
        );
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
                CREATE_NO_WINDOW,
                None,
                PCWSTR::null(),
                &startup,
                &mut info,
            )
        }
        .unwrap();
        let parent = Handle(info.hProcess);
        drop(Handle(info.hThread));
        let result = (|| {
            let opened = open_parent(info.dwProcessId)?;
            let child = create(
                &exe,
                &fixture("low_integrity_child"),
                false,
                Some(opened.0),
                false,
            )?;
            exit_code(&child, Duration::from_secs(10))
        })();
        unsafe {
            let _ = TerminateProcess(parent.0, 0);
        }
        assert_eq!(result.unwrap(), 0);
    }
}
