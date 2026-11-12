//! Operations that change process state.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Wdk::System::Threading::{
    NtQueryInformationProcess, PROCESSINFOCLASS, ProcessIoPriority,
};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, STATUS_ACCESS_DENIED,
};
use windows::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcessId,
    GetPriorityClass, GetProcessInformation, GetProcessTimes, HIGH_PRIORITY_CLASS,
    IDLE_PRIORITY_CLASS, MEMORY_PRIORITY, MEMORY_PRIORITY_INFORMATION, NORMAL_PRIORITY_CLASS,
    OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_CREATION_FLAGS,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SET_INFORMATION, PROCESS_SUSPEND_RESUME, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    ProcessMemoryPriority, ProcessPowerThrottling, REALTIME_PRIORITY_CLASS, SetPriorityClass,
    SetProcessInformation, TerminateProcess, WaitForSingleObject,
};

use crate::process::list_lineage;
use crate::util::{OwnedHandle, filetime_ticks, ticks_to_unix_ms};

const PROCESS_BREAK_ON_TERMINATION: PROCESSINFOCLASS = PROCESSINFOCLASS(29);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum ActionError {
    /// Needs administrator rights (or the target is protected).
    AccessDenied,
    /// The process has already exited (or its PID was reused).
    NotFound,
    /// Terminating it would crash the system; requires explicit confirmation.
    Critical,
    /// Refused: the System process, Idle, or Argus itself.
    Refused,
    /// Denied to Argus's own rights, which acted because the privileged
    /// service speaks another protocol and refused it: updating the service
    /// lets it act again. Only the host reports it; the service never sends it.
    ServiceOutdated,
    /// Resuming could not be confirmed: every thread is still suspended
    /// (another tool suspended the process more than once, so it needs
    /// another resume), or the thread states could not be read.
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
    if !allow_critical && is_critical(pid) {
        return Err(ActionError::Critical);
    }
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
            if is_critical(pid) {
                return None;
            }
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

/// Whether the process is marked critical (terminating it bugchecks the
/// system). Needs full query access, so this only detects it when elevated —
/// which is also the only case where termination could succeed.
fn is_critical(pid: u32) -> bool {
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) }) else {
        return false;
    };
    let handle = OwnedHandle(handle);
    let mut flag = 0u32;
    let status = unsafe {
        NtQueryInformationProcess(
            handle.0,
            PROCESS_BREAK_ON_TERMINATION,
            (&mut flag as *mut u32).cast(),
            4,
            std::ptr::null_mut(),
        )
    };
    status.0 >= 0 && flag != 0
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

/// A process's I/O priority hint, which the I/O manager and storage stack use
/// to order its requests. Windows reserves `High` for processes with the
/// privilege to raise priority and `Critical` for the memory manager, so they
/// can be read but not set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IoPriority {
    VeryLow,
    Low,
    Normal,
    High,
    Critical,
}

impl IoPriority {
    const ALL: [Self; 5] = [
        Self::VeryLow,
        Self::Low,
        Self::Normal,
        Self::High,
        Self::Critical,
    ];

    /// The `IO_PRIORITY_HINT` value.
    fn hint(self) -> u32 {
        Self::ALL.iter().position(|p| *p == self).unwrap() as u32
    }

    fn from_hint(hint: u32) -> Option<Self> {
        Self::ALL.get(hint as usize).copied()
    }

    /// Whether a user may set it.
    pub fn settable(self) -> bool {
        matches!(self, Self::VeryLow | Self::Low | Self::Normal)
    }
}

/// A process's memory priority: the lower it is, the sooner its pages leave
/// the working set and standby list under memory pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryPriority {
    VeryLow,
    Low,
    Medium,
    BelowNormal,
    Normal,
}

impl MemoryPriority {
    const ALL: [Self; 5] = [
        Self::VeryLow,
        Self::Low,
        Self::Medium,
        Self::BelowNormal,
        Self::Normal,
    ];

    /// The `MEMORY_PRIORITY` value, 1 to 5.
    fn level(self) -> u32 {
        Self::ALL.iter().position(|p| *p == self).unwrap() as u32 + 1
    }

    fn from_level(level: u32) -> Option<Self> {
        Self::ALL.get((level as usize).checked_sub(1)?).copied()
    }
}

/// The I/O priority of a process, through a handle with limited query
/// rights; None when it can't be read.
pub(crate) fn io_priority(process: windows::Win32::Foundation::HANDLE) -> Option<IoPriority> {
    let mut hint = 0u32;
    let status = unsafe {
        NtQueryInformationProcess(
            process,
            ProcessIoPriority,
            (&mut hint as *mut u32).cast(),
            size_of::<u32>() as u32,
            std::ptr::null_mut(),
        )
    };
    status
        .is_ok()
        .then(|| IoPriority::from_hint(hint))
        .flatten()
}

/// The memory priority of a process, like `io_priority`.
pub(crate) fn memory_priority(
    process: windows::Win32::Foundation::HANDLE,
) -> Option<MemoryPriority> {
    let mut info = MEMORY_PRIORITY_INFORMATION::default();
    unsafe {
        GetProcessInformation(
            process,
            ProcessMemoryPriority,
            (&mut info as *mut MEMORY_PRIORITY_INFORMATION).cast(),
            size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    }
    .ok()?;
    MemoryPriority::from_level(info.MemoryPriority.0)
}

/// Sets the I/O priority of a process instance and returns the one it ended
/// up with.
pub fn set_io_priority(
    pid: u32,
    create_time: i64,
    priority: IoPriority,
) -> Result<IoPriority, ActionError> {
    if !priority.settable() {
        return Err(ActionError::Refused);
    }
    let handle = open_instance(pid, create_time, PROCESS_SET_INFORMATION)?;
    let hint = priority.hint();
    let status = unsafe {
        crate::nt::NtSetInformationProcess(
            handle.0,
            ProcessIoPriority,
            (&hint as *const u32).cast(),
            size_of::<u32>() as u32,
        )
    };
    if status == STATUS_ACCESS_DENIED {
        return Err(ActionError::AccessDenied);
    }
    status.ok()?;
    Ok(io_priority(handle.0).unwrap_or(priority))
}

/// Sets the memory priority of a process instance and returns the one it
/// ended up with.
pub fn set_memory_priority(
    pid: u32,
    create_time: i64,
    priority: MemoryPriority,
) -> Result<MemoryPriority, ActionError> {
    let handle = open_instance(pid, create_time, PROCESS_SET_INFORMATION)?;
    let info = MEMORY_PRIORITY_INFORMATION {
        MemoryPriority: MEMORY_PRIORITY(priority.level()),
    };
    unsafe {
        SetProcessInformation(
            handle.0,
            ProcessMemoryPriority,
            (&info as *const MEMORY_PRIORITY_INFORMATION).cast(),
            size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    }?;
    Ok(memory_priority(handle.0).unwrap_or(priority))
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

/// How long a suspend or resume waits for the live thread states to show it:
/// the native calls return before every thread has stopped or started again,
/// and the refresh that follows an action would read the old state.
const SETTLE: Duration = Duration::from_millis(250);

/// Image names whose stall cannot be undone from Argus, though Windows does
/// not mark them critical: the Desktop Window Manager (the whole screen,
/// Argus's window included, stops updating) and Argus's own privileged
/// service (`argus-service-<pid>-<stamp>.exe`, which every Argus waits on).
fn essential_image(name: &str) -> bool {
    let name = name.to_lowercase();
    name == "dwm.exe" || (name.starts_with("argus-service-") && name.ends_with(".exe"))
}

/// The file name of the process's image, from a handle with limited query rights.
fn image_name(handle: windows::Win32::Foundation::HANDLE) -> Result<String, ActionError> {
    use windows::Win32::System::Threading::{PROCESS_NAME_WIN32, QueryFullProcessImageNameW};
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }?;
    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    Ok(path.rsplit('\\').next().unwrap_or_default().to_owned())
}

fn require_suspendable(handle: windows::Win32::Foundation::HANDLE) -> Result<(), ActionError> {
    // Query the already verified instance with limited query rights. A failed
    // query must stop the operation before any suspension.
    let mut critical = windows::core::BOOL::default();
    unsafe { windows::Win32::System::Threading::IsProcessCritical(handle, &mut critical) }?;
    if critical.as_bool() {
        return Err(ActionError::Critical);
    }
    if essential_image(&image_name(handle)?) {
        return Err(ActionError::Refused);
    }
    Ok(())
}

/// Waits up to `SETTLE` for the instance's suspension to satisfy `done`;
/// `None` is an instance that left the process list.
fn settles(
    pid: u32,
    create_time: i64,
    done: impl Fn(Option<crate::process::Suspension>) -> bool,
) -> bool {
    let start = Instant::now();
    loop {
        if crate::process::suspension(pid, create_time).is_ok_and(&done) {
            return true;
        }
        if start.elapsed() >= SETTLE {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Suspends or resumes every thread of a process instance, the way Resource
/// Monitor's "Suspend process" does. Suspending is refused for the System
/// process, Argus itself, critical processes, whose stall hangs Windows, and
/// the images `essential_image` names. Processes Windows froze are left to
/// Windows in both directions: resuming does not thaw them.
///
/// Each native call moves every thread's suspend count by one, so a resume
/// undoes exactly one suspend: threads the program or a debugger suspended
/// on purpose stay suspended. A fully suspended process is not suspended
/// again, so suspending twice never needs two resumes. When another tool
/// suspended the process more than once, it stays suspended after a resume
/// (`StillSuspended`) and each further resume takes one more off.
pub fn set_suspended(pid: u32, create_time: i64, suspend: bool) -> Result<(), ActionError> {
    use crate::process::{Suspension, suspension};
    if pid == 0 || pid == 4 || pid == unsafe { GetCurrentProcessId() } {
        return Err(ActionError::Refused);
    }
    if create_time <= 0 {
        return Err(ActionError::NotFound);
    }
    let handle = open_instance(pid, create_time, PROCESS_SUSPEND_RESUME)?;
    if crate::process::frozen(handle.0) {
        return Err(ActionError::Refused);
    }
    let check = |status: windows::Win32::Foundation::NTSTATUS| {
        if status == STATUS_ACCESS_DENIED {
            return Err(ActionError::AccessDenied);
        }
        Ok(status.ok()?)
    };
    if suspend {
        require_suspendable(handle.0)?;
        // An unreadable state suspends anyway: skipping would leave the
        // process running unasked.
        if suspension(pid, create_time) == Ok(Some(Suspension::All)) {
            return Ok(());
        }
        check(unsafe { crate::nt::NtSuspendProcess(handle.0) })?;
        // Best effort: the suspension holds either way, only the next
        // refresh might still list the process as running.
        settles(pid, create_time, |state| {
            matches!(state, None | Some(Suspension::All))
        });
        return Ok(());
    }
    check(unsafe { crate::nt::NtResumeProcess(handle.0) })?;
    // Running, partly suspended by its own choice, or gone: resumed. Still
    // fully suspended, or unreadable throughout: not confirmed.
    if settles(pid, create_time, |state| state != Some(Suspension::All)) {
        Ok(())
    } else {
        Err(ActionError::StillSuspended)
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeKillReport {
    pub terminated: u32,
    pub failed: u32,
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
    fn io_and_memory_priorities_are_set_on_the_instance_and_read_back() {
        use super::{IoPriority, MemoryPriority};
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
        let handle =
            super::open_instance(pid, created, super::PROCESS_QUERY_LIMITED_INFORMATION).unwrap();
        // The child inherits its levels from whatever runs the tests (a CI
        // runner starts it at low I/O priority), so only readability is given.
        assert!(super::io_priority(handle.0).is_some());
        assert!(super::memory_priority(handle.0).is_some());
        for priority in [IoPriority::VeryLow, IoPriority::Low, IoPriority::Normal] {
            assert_eq!(
                super::set_io_priority(pid, created, priority).unwrap(),
                priority
            );
            assert_eq!(super::io_priority(handle.0), Some(priority));
        }
        for priority in [IoPriority::High, IoPriority::Critical] {
            assert!(matches!(
                super::set_io_priority(pid, created, priority),
                Err(super::ActionError::Refused)
            ));
        }
        for priority in [
            MemoryPriority::VeryLow,
            MemoryPriority::Low,
            MemoryPriority::Medium,
            MemoryPriority::BelowNormal,
            MemoryPriority::Normal,
        ] {
            assert_eq!(
                super::set_memory_priority(pid, created, priority).unwrap(),
                priority
            );
            assert_eq!(super::memory_priority(handle.0), Some(priority));
        }
        // Another instance under the same PID is never touched.
        assert!(matches!(
            super::set_io_priority(pid, created + 1, IoPriority::Low),
            Err(super::ActionError::NotFound)
        ));
        assert!(matches!(
            super::set_memory_priority(pid, created + 1, MemoryPriority::Low),
            Err(super::ActionError::NotFound)
        ));
    }

    #[test]
    fn a_resume_undoes_one_suspend_and_keeps_the_programs_own() {
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
        // A running process is not frozen, and a suspended one only suspended.
        let handle = super::open_instance(pid, created, super::PROCESS_SUSPEND_RESUME).unwrap();
        assert!(!crate::process::frozen(handle.0));
        super::set_suspended(pid, created, true).unwrap();
        // The call returns once the threads show it.
        assert_eq!(
            crate::process::suspension(pid, created).unwrap(),
            Some(crate::process::Suspension::All)
        );
        assert!(!crate::process::frozen(handle.0));
        // Suspending an already suspended process leaves its count alone, so
        // one resume undoes both calls.
        super::set_suspended(pid, created, true).unwrap();
        super::set_suspended(pid, created, false).unwrap();
        assert!(settles(pid, created, false));

        // Another tool's extra suspend survives one resume, and the next one clears it.
        super::set_suspended(pid, created, true).unwrap();
        assert!(unsafe { crate::nt::NtSuspendProcess(handle.0) }.is_ok());
        assert!(matches!(
            super::set_suspended(pid, created, false),
            Err(super::ActionError::StillSuspended)
        ));
        super::set_suspended(pid, created, false).unwrap();
        assert!(settles(pid, created, false));

        // A thread the program suspended itself stays suspended after a
        // suspend and resume, while the rest run again.
        let mut buf = crate::util::AlignedBuf::with_capacity(512 * 1024);
        crate::nt::query_system_information(
            windows::Wdk::System::SystemInformation::SystemProcessInformation,
            &mut buf,
        )
        .unwrap();
        let threads: Vec<u32> = crate::nt::ProcessIter::new(&buf)
            .find(|e| e.info.unique_process_id.0 as usize as u32 == pid)
            .map(|e| {
                e.threads
                    .iter()
                    .map(|t| t.unique_thread.0 as usize as u32)
                    .collect()
            })
            .unwrap();
        if threads.len() < 2 {
            return; // Nothing would be left running to tell the cases apart.
        }
        use windows::Win32::System::Threading::{OpenThread, SuspendThread, THREAD_SUSPEND_RESUME};
        let own = super::OwnedHandle(
            unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, threads[0]) }.unwrap(),
        );
        assert_ne!(unsafe { SuspendThread(own.0) }, u32::MAX);
        super::set_suspended(pid, created, true).unwrap();
        super::set_suspended(pid, created, false).unwrap();
        assert_eq!(
            crate::process::suspension(pid, created).unwrap(),
            Some(crate::process::Suspension::Partial)
        );
    }

    #[test]
    fn the_desktop_window_manager_and_argus_service_are_essential() {
        for name in ["dwm.exe", "DWM.EXE", "argus-service-123-456.exe"] {
            assert!(super::essential_image(name), "{name}");
        }
        for name in [
            "explorer.exe",
            "argus.exe",
            "argus-service.txt",
            "notdwm.exe",
        ] {
            assert!(!super::essential_image(name), "{name}");
        }
        use windows::Win32::System::Threading::GetCurrentProcess;
        let own = super::image_name(unsafe { GetCurrentProcess() }).unwrap();
        assert!(own.to_lowercase().ends_with(".exe") && !own.contains('\\'));
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
    fn errors_keep_their_wire_form() {
        // The UI (`actions.error.<kind>`) and the service protocol read these tags.
        let json = |e: ActionError| serde_json::to_string(&e).unwrap();
        assert_eq!(
            json(ActionError::AccessDenied),
            r#"{"kind":"accessDenied"}"#
        );
        assert_eq!(json(ActionError::NotFound), r#"{"kind":"notFound"}"#);
        assert_eq!(json(ActionError::Critical), r#"{"kind":"critical"}"#);
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
            r#"{"terminated":2,"failed":3,"serviceOutdated":true}"#
        );
    }
}
