//! Operations that change process state.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, STATUS_ACCESS_DENIED,
};
use windows::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcessId,
    GetPriorityClass, GetProcessTimes, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, IsProcessCritical,
    NORMAL_PRIORITY_CLASS, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_CREATION_FLAGS,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    PROCESS_SUSPEND_RESUME, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, ProcessPowerThrottling,
    REALTIME_PRIORITY_CLASS, SetPriorityClass, SetProcessInformation, TerminateProcess,
    WaitForSingleObject,
};

use crate::process::list_lineage;
use crate::util::{OwnedHandle, filetime_ticks, ticks_to_unix_ms};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum ActionError {
    /// Needs administrator rights (or the target is protected).
    AccessDenied,
    /// The process has already exited (or its PID was reused).
    NotFound,
    /// Terminating it would crash the system; requires explicit confirmation.
    Critical,
    /// The safety check failed; this never permits termination, including force.
    CriticalityUnknown,
    /// Refused: the System process, Idle, or Argus itself.
    Refused,
    /// Denied to Argus's own rights, which acted because the privileged
    /// service speaks another protocol and refused it: updating the service
    /// lets it act again. Only the host reports it; the service never sends it.
    ServiceOutdated,
    /// Resuming could not be confirmed: threads stayed suspended after every
    /// retry, or their state could not be read.
    StillSuspended,
    Os(String),
}

impl From<windows::core::Error> for ActionError {
    fn from(e: windows::core::Error) -> Self {
        if e.code() == ERROR_ACCESS_DENIED.to_hresult() {
            ActionError::AccessDenied
        } else if e.code() == ERROR_INVALID_PARAMETER.to_hresult() {
            ActionError::NotFound
        } else {
            ActionError::Os(e.to_string())
        }
    }
}

impl ActionError {
    pub fn diagnostic(&self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::AccessDenied => "access denied".into(),
            Self::NotFound => "target exited or identity changed".into(),
            Self::Critical => "critical process requires confirmation".into(),
            Self::CriticalityUnknown => "process criticality could not be checked".into(),
            Self::Refused => "operation refused by policy".into(),
            Self::ServiceOutdated => "privileged service protocol incompatible".into(),
            Self::StillSuspended => "threads remain suspended or unreadable after resume".into(),
            Self::Os(message) => crate::diagnostics::failure(message),
        }
    }
}

/// Opens the process instance identified by `pid` + `create_time` (Unix ms;
/// 0 skips the identity check), so a reused PID is never acted on.
fn open_instance(
    pid: u32,
    create_time: i64,
    access: PROCESS_ACCESS_RIGHTS,
) -> Result<OwnedHandle, ActionError> {
    // Negative creation times represent identities still being resolved.
    if create_time < 0 {
        return Err(ActionError::NotFound);
    }
    let handle = unsafe { OpenProcess(access | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .inspect_err(|error| {
            crate::diagnostics::warn(
                "process action OpenProcess",
                format!(
                    "{}; pid={pid}; access=0x{:08x}",
                    crate::diagnostics::code(error.code().0 as u32),
                    (access | PROCESS_QUERY_LIMITED_INFORMATION).0
                ),
            )
        })?;
    let handle = OwnedHandle(handle);
    if create_time != 0 {
        let (mut created, mut exited, mut kernel, mut user) = Default::default();
        unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }
            .inspect_err(|error| {
                crate::diagnostics::windows_failure("process action GetProcessTimes", error)
            })?;
        let created: FILETIME = created;
        if ticks_to_unix_ms(filetime_ticks(created)) != create_time {
            return Err(ActionError::NotFound);
        }
    }
    Ok(handle)
}

/// Terminates the process instance identified by `pid` + `create_time`
/// (Unix ms; 0 skips the identity check).
pub fn terminate(pid: u32, create_time: i64, allow_critical: bool) -> Result<(), ActionError> {
    if pid == 0 || pid == 4 || pid == unsafe { GetCurrentProcessId() } {
        return Err(ActionError::Refused);
    }
    let handle = open_instance(pid, create_time, PROCESS_TERMINATE)?;
    permit_termination(criticality(&handle), allow_critical)?;
    unsafe { TerminateProcess(handle.0, 1) }?;
    Ok(())
}

/// Ends process instances (`pid`, `create_time`) and waits up to `wait` for
/// them to be gone; returns how many were ended. Each is raised to high
/// priority first: a process ended while starved of CPU (an idle-priority
/// WebView2 renderer on a busy machine, say) took half a minute to finish
/// exiting, keeping its files open all along; raised, about 0.1 s.
pub fn end_and_wait(targets: &[(u32, i64)], wait: Duration) -> usize {
    let me = unsafe { GetCurrentProcessId() };
    let access = PROCESS_TERMINATE | PROCESS_SET_INFORMATION | PROCESS_SYNCHRONIZE;
    let ended: Vec<OwnedHandle> = targets
        .iter()
        .filter(|&&(pid, _)| pid != 0 && pid != 4 && pid != me)
        .filter_map(|&(pid, create_time)| {
            let handle = open_instance(pid, create_time, access).ok()?;
            permit_termination(criticality(&handle), false).ok()?;
            let _ = unsafe { SetPriorityClass(handle.0, HIGH_PRIORITY_CLASS) };
            unsafe { TerminateProcess(handle.0, 1) }.ok()?;
            Some(handle)
        })
        .collect();
    let deadline = Instant::now() + wait;
    for handle in &ended {
        let left = deadline.saturating_duration_since(Instant::now());
        unsafe { WaitForSingleObject(handle.0, left.as_millis() as u32) };
    }
    ended.len()
}

/// Query the same instance handle used for termination. IsProcessCritical
/// requires only QUERY_LIMITED_INFORMATION, which open_instance already has.
/// A denied or failed check is distinct from a successful negative answer.
fn criticality(handle: &OwnedHandle) -> Result<bool, ActionError> {
    let mut critical = windows::core::BOOL::default();
    unsafe { IsProcessCritical(handle.0, &mut critical) }
        .inspect_err(|error| {
            crate::diagnostics::windows_failure("process action IsProcessCritical", error)
        })
        .map_err(|_| ActionError::CriticalityUnknown)?;
    Ok(critical.as_bool())
}

fn permit_termination(critical: Result<bool, ActionError>, allow: bool) -> Result<(), ActionError> {
    match critical? {
        true if !allow => Err(ActionError::Critical),
        _ => Ok(()),
    }
}

/// A process priority class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Priority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

impl Priority {
    fn class(self) -> PROCESS_CREATION_FLAGS {
        match self {
            Priority::Idle => IDLE_PRIORITY_CLASS,
            Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
            Priority::Normal => NORMAL_PRIORITY_CLASS,
            Priority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
            Priority::High => HIGH_PRIORITY_CLASS,
            Priority::Realtime => REALTIME_PRIORITY_CLASS,
        }
    }

    fn from_class(class: u32) -> Option<Self> {
        [
            Priority::Idle,
            Priority::BelowNormal,
            Priority::Normal,
            Priority::AboveNormal,
            Priority::High,
            Priority::Realtime,
        ]
        .into_iter()
        .find(|p| p.class().0 == class)
    }
}

/// Sets the priority class of a process instance and returns the class it
/// ended up with: without the privilege to raise base priority (elevation),
/// Windows quietly turns "realtime" into "high".
pub fn set_priority(
    pid: u32,
    create_time: i64,
    priority: Priority,
) -> Result<Priority, ActionError> {
    let handle = open_instance(pid, create_time, PROCESS_SET_INFORMATION)?;
    unsafe { SetPriorityClass(handle.0, priority.class()) }?;
    Ok(Priority::from_class(unsafe { GetPriorityClass(handle.0) }).unwrap_or(priority))
}

/// Turns efficiency mode on or off the way Task Manager does: EcoQoS (the
/// scheduler prefers efficient cores and low clocks) plus the idle priority
/// class; turning it off hands QoS back to Windows and restores normal
/// priority if it is still idle.
pub fn set_efficiency_mode(pid: u32, create_time: i64, on: bool) -> Result<(), ActionError> {
    let handle = open_instance(pid, create_time, PROCESS_SET_INFORMATION)?;
    let mask = if on {
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED
    } else {
        0
    };
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: mask,
        StateMask: mask,
    };
    unsafe {
        SetProcessInformation(
            handle.0,
            ProcessPowerThrottling,
            (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }?;
    if on {
        unsafe { SetPriorityClass(handle.0, IDLE_PRIORITY_CLASS) }?;
    } else if unsafe { GetPriorityClass(handle.0) } == IDLE_PRIORITY_CLASS.0 {
        unsafe { SetPriorityClass(handle.0, NORMAL_PRIORITY_CLASS) }?;
    }
    Ok(())
}

/// Resumes repeated at most this many times: one more than a thread's suspend
/// count can reach (`MAXIMUM_SUSPEND_COUNT`, 127), so any stacking clears.
const MAX_RESUMES: usize = 128;

fn require_suspendable(handle: windows::Win32::Foundation::HANDLE) -> Result<(), ActionError> {
    // Query the already verified instance with limited query rights. A failed
    // critical-process query must stop the operation before any suspension.
    let mut critical = windows::core::BOOL::default();
    unsafe { windows::Win32::System::Threading::IsProcessCritical(handle, &mut critical) }?;
    if critical.as_bool() {
        return Err(ActionError::Critical);
    }
    Ok(())
}

/// Suspends or resumes every thread of a process instance, the way Resource
/// Monitor's "Suspend process" does. Suspending is refused for the System
/// process, Argus itself and critical processes, whose stall hangs Windows.
///
/// Each native call moves every thread's suspend count by one, so both
/// directions are made idempotent against the live thread states rather than
/// the caller's (possibly stale) list: a fully suspended process is not
/// suspended again, and resuming repeats until no thread is left suspended,
/// including threads another tool suspended more often than the rest.
pub fn set_suspended(pid: u32, create_time: i64, suspend: bool) -> Result<(), ActionError> {
    use crate::process::{Suspension, suspension};
    if pid == 0 || pid == 4 || pid == unsafe { GetCurrentProcessId() } {
        return Err(ActionError::Refused);
    }
    if create_time <= 0 {
        return Err(ActionError::NotFound);
    }
    let handle = open_instance(pid, create_time, PROCESS_SUSPEND_RESUME)?;
    let check = |status: windows::Win32::Foundation::NTSTATUS| {
        if status == STATUS_ACCESS_DENIED {
            return Err(ActionError::AccessDenied);
        }
        Ok(status.ok()?)
    };
    if suspend {
        require_suspendable(handle.0)?;
        // An unreadable state suspends anyway: a stacked count is what resume
        // clears, while skipping would leave the process running unasked.
        if suspension(pid, create_time) == Ok(Some(Suspension::All)) {
            return Ok(());
        }
        return check(unsafe { crate::nt::NtSuspendProcess(handle.0) });
    }
    for _ in 0..MAX_RESUMES {
        check(unsafe { crate::nt::NtResumeProcess(handle.0) })?;
        // Done, or gone (the instance left the process list). Still suspended
        // or not readable this time: resuming a thread whose count is already
        // zero changes nothing, so try again.
        if let Ok(None | Some(Suspension::None)) = suspension(pid, create_time) {
            return Ok(());
        }
    }
    Err(ActionError::StillSuspended)
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeKillReport {
    pub terminated: u32,
    pub failed: u32,
    pub criticality_unknown: u32,
    /// A descendant was denied while the privileged service needs an update
    /// (`ActionError::ServiceOutdated`), so the UI offers it. The count alone
    /// would lose that; a denied root fails the whole call with the error.
    pub service_outdated: bool,
}

impl TreeKillReport {
    /// Counts how ending one descendant went.
    fn note(&mut self, result: Result<(), ActionError>) {
        match result {
            Ok(()) => self.terminated += 1,
            Err(e) => {
                self.failed += 1;
                self.criticality_unknown += u32::from(matches!(e, ActionError::CriticalityUnknown));
                self.service_outdated |= matches!(e, ActionError::ServiceOutdated);
            }
        }
    }
}

/// Terminates a process and all of its descendants, deepest first.
/// Enumerates locally while allowing the host to route individual operations
/// through its privileged service. Every target retains its creation time.
pub fn terminate_tree_with(
    pid: u32,
    create_time: i64,
    mut terminate_one: impl FnMut(u32, i64) -> Result<(), ActionError>,
) -> Result<TreeKillReport, ActionError> {
    let procs = list_lineage();
    let mut children: HashMap<u32, Vec<(u32, i64)>> = HashMap::new();
    let created: HashMap<u32, i64> = procs.iter().map(|&(p, _, c)| (p, c)).collect();
    for &(p, parent, c) in &procs {
        // A child must start after its parent; otherwise the parent PID was reused.
        if let Some(&pc) = created.get(&parent)
            && pc <= c
            && p != parent
        {
            children.entry(parent).or_default().push((p, c));
        }
    }
    if create_time != 0 && created.get(&pid) != Some(&create_time) {
        return Err(ActionError::NotFound);
    }

    // Post-order: collect descendants, then reverse so leaves go first.
    let mut order = Vec::new();
    let mut stack = vec![(pid, create_time)];
    while let Some((p, c)) = stack.pop() {
        order.push((p, c));
        if let Some(kids) = children.get(&p) {
            stack.extend(kids.iter().copied());
        }
    }
    let root = order.remove(0);

    let mut report = TreeKillReport::default();
    for (p, c) in order.into_iter().rev() {
        report.note(terminate_one(p, c));
    }
    terminate_one(root.0, root.1)?;
    report.terminated += 1;
    Ok(report)
}

#[cfg(test)]
mod tests {
    /// Waits up to two seconds for the suspend APCs to reach every thread.
    fn settles(pid: u32, created: i64, want: bool) -> bool {
        use crate::process::{Suspension, suspension};
        let want = if want {
            Suspension::All
        } else {
            Suspension::None
        };
        (0..40).any(|_| {
            let now = suspension(pid, created) == Ok(Some(want));
            if !now {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            now
        })
    }

    /// Ends the child however the test ends: `Child` doesn't on drop, and a
    /// suspended one would never finish on its own.
    struct KillOnDrop(std::process::Child);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn one_resume_clears_suspensions_stacked_on_a_live_process() {
        use std::os::windows::process::CommandExt;
        let child = KillOnDrop(
            std::process::Command::new("ping")
                .args(["-n", "30", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0)
                .spawn()
                .unwrap(),
        );
        let pid = child.0.id();
        let created = crate::process::list_lineage()
            .into_iter()
            .find(|&(p, ..)| p == pid)
            .unwrap()
            .2;
        for stale in [0, -1, created + 1] {
            assert!(matches!(
                super::set_suspended(pid, stale, true),
                Err(super::ActionError::NotFound)
            ));
        }
        super::set_suspended(pid, created, true).unwrap();
        assert!(settles(pid, created, true));
        // Suspending an already suspended process leaves its count alone…
        super::set_suspended(pid, created, true).unwrap();
        // …and counts stacked behind Argus's back, unequal across threads,
        // still clear in one resume.
        let handle = super::open_instance(pid, created, super::PROCESS_SUSPEND_RESUME).unwrap();
        assert!(unsafe { crate::nt::NtSuspendProcess(handle.0) }.is_ok());
        let mut buf = crate::util::AlignedBuf::with_capacity(512 * 1024);
        crate::nt::query_system_information(
            windows::Wdk::System::SystemInformation::SystemProcessInformation,
            &mut buf,
        )
        .unwrap();
        let thread = crate::nt::ProcessIter::new(&buf)
            .find(|e| e.info.unique_process_id.0 as usize as u32 == pid)
            .map(|e| e.threads[0].unique_thread.0 as usize as u32)
            .unwrap();
        use windows::Win32::System::Threading::{OpenThread, SuspendThread, THREAD_SUSPEND_RESUME};
        let thread = super::OwnedHandle(
            unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, thread) }.unwrap(),
        );
        for _ in 0..3 {
            assert_ne!(unsafe { SuspendThread(thread.0) }, u32::MAX);
        }
        super::set_suspended(pid, created, false).unwrap();
        assert!(settles(pid, created, false));
    }

    #[test]
    fn suspension_requires_a_successful_critical_process_query() {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Threading::GetCurrentProcess;
        assert!(super::require_suspendable(HANDLE::default()).is_err());
        assert!(super::require_suspendable(unsafe { GetCurrentProcess() }).is_ok());
    }

    #[test]
    fn unresolved_identity_never_uses_the_legacy_pid_only_action_path() {
        assert!(matches!(
            super::open_instance(
                std::process::id(),
                -1,
                super::PROCESS_QUERY_LIMITED_INFORMATION
            ),
            Err(super::ActionError::NotFound)
        ));
    }
    use super::*;

    #[test]
    fn criticality_unknown_never_authorizes_an_operation_even_when_forced() {
        for allow in [false, true] {
            let mut called = false;
            let result = permit_termination(Err(ActionError::CriticalityUnknown), allow)
                .map(|()| called = true);
            assert!(matches!(result, Err(ActionError::CriticalityUnknown)));
            assert!(!called);
            assert!(permit_termination(Ok(false), allow).is_ok());
        }
        assert!(matches!(
            permit_termination(Ok(true), false),
            Err(ActionError::Critical)
        ));
        assert!(permit_termination(Ok(true), true).is_ok());
        let mut report = TreeKillReport::default();
        report.note(Err(ActionError::CriticalityUnknown));
        assert_eq!((report.failed, report.criticality_unknown), (1, 1));
    }

    #[test]
    fn criticality_uses_limited_query_on_the_owned_dacl_fixture() {
        use std::ffi::c_void;
        use std::os::windows::{io::AsRawHandle, process::CommandExt};
        use std::process::{Command, Stdio};
        type Handle = *mut c_void;
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
                s: *const u16,
                revision: u32,
                sd: *mut Handle,
                len: *mut u32,
            ) -> i32;
            fn SetKernelObjectSecurity(h: Handle, info: u32, sd: Handle) -> i32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn LocalFree(h: Handle) -> Handle;
        }
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "actions::tests::criticality_child", "--ignored"])
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let sddl: Vec<u16> = "D:P(D;;0x0400;;;WD)(A;;0x00121001;;;WD)"
            .encode_utf16()
            .chain([0])
            .collect();
        let mut sd = std::ptr::null_mut();
        unsafe {
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut sd,
                    std::ptr::null_mut()
                ),
                0
            );
            let result = SetKernelObjectSecurity(child.as_raw_handle(), 4, sd);
            LocalFree(sd);
            assert_ne!(result, 0);
        }
        let full = unsafe {
            OpenProcess(
                windows::Win32::System::Threading::PROCESS_QUERY_INFORMATION,
                false,
                child.id(),
            )
        };
        if let Ok(handle) = full {
            drop(OwnedHandle(handle));
            panic!("fixture must deny full query");
        }
        let handle = open_instance(child.id(), 0, PROCESS_TERMINATE).unwrap();
        assert!(matches!(criticality(&handle), Ok(false)));
        assert!(permit_termination(criticality(&handle), false).is_ok());
        drop(handle);
        // No termination request is issued. EOF lets only this helper exit itself.
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());
    }

    #[test]
    #[ignore = "owned subprocess fixture, called by the parent regression"]
    fn criticality_child() {
        use std::io::Read;
        let mut byte = [0u8];
        let _ = std::io::stdin().read(&mut byte);
    }

    #[test]
    fn errors_keep_their_wire_form() {
        // The UI (`actions.error.<kind>`) and the service protocol read these tags.
        let json = |e: ActionError| serde_json::to_string(&e).unwrap();
        assert_eq!(
            json(ActionError::AccessDenied),
            r#"{"kind":"accessDenied"}"#
        );
        assert_eq!(json(ActionError::NotFound), r#"{"kind":"notFound"}"#);
        assert_eq!(json(ActionError::Critical), r#"{"kind":"critical"}"#);
        assert_eq!(
            json(ActionError::CriticalityUnknown),
            r#"{"kind":"criticalityUnknown"}"#
        );
        assert_eq!(json(ActionError::Refused), r#"{"kind":"refused"}"#);
        assert_eq!(
            json(ActionError::ServiceOutdated),
            r#"{"kind":"serviceOutdated"}"#
        );
        assert_eq!(
            json(ActionError::StillSuspended),
            r#"{"kind":"stillSuspended"}"#
        );
        assert_eq!(
            json(ActionError::Os("x".into())),
            r#"{"kind":"os","message":"x"}"#
        );
        assert!(matches!(
            serde_json::from_str(r#"{"kind":"serviceOutdated"}"#),
            Ok(ActionError::ServiceOutdated)
        ));
    }

    #[test]
    fn a_tree_report_keeps_whether_updating_the_service_would_help() {
        let mut report = TreeKillReport::default();
        report.note(Ok(()));
        report.note(Err(ActionError::NotFound));
        report.note(Err(ActionError::AccessDenied));
        // Gone or denied for good: an update would not help.
        assert!(!report.service_outdated);
        report.note(Err(ActionError::ServiceOutdated));
        report.note(Ok(()));
        assert_eq!((report.terminated, report.failed), (2, 3));
        assert!(report.service_outdated);
        // As the UI and the service protocol read it.
        assert_eq!(
            serde_json::to_string(&report).unwrap(),
            r#"{"terminated":2,"failed":3,"criticalityUnknown":0,"serviceOutdated":true}"#
        );
    }
}
