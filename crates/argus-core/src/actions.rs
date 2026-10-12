//! Operations that change process state.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME};
use windows::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcessId,
    GetPriorityClass, GetProcessTimes, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_CREATION_FLAGS,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SET_INFORMATION, PROCESS_TERMINATE, ProcessPowerThrottling, REALTIME_PRIORITY_CLASS,
    SetPriorityClass, SetProcessInformation, TerminateProcess,
};

use crate::process::list_lineage;
use crate::util::{OwnedHandle, filetime_ticks, ticks_to_unix_ms};

const PROCESS_BREAK_ON_TERMINATION: PROCESSINFOCLASS = PROCESSINFOCLASS(29);

#[derive(Debug, Clone, Serialize)]
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
    Os(String),
}

impl From<windows::core::Error> for ActionError {
    fn from(e: windows::core::Error) -> Self {
        if e.code() == ERROR_ACCESS_DENIED.to_hresult() {
            ActionError::AccessDenied
        } else if e.code() == ERROR_INVALID_PARAMETER.to_hresult() {
            ActionError::NotFound
        } else {
            ActionError::Os(e.message())
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
    let handle = unsafe { OpenProcess(access | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }?;
    let handle = OwnedHandle(handle);
    if create_time != 0 {
        let (mut created, mut exited, mut kernel, mut user) = Default::default();
        unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }?;
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

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeKillReport {
    pub terminated: u32,
    pub failed: u32,
}

/// Terminates a process and all of its descendants, deepest first.
pub fn terminate_tree(pid: u32, create_time: i64) -> Result<TreeKillReport, ActionError> {
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
        match terminate(p, c, false) {
            Ok(()) => report.terminated += 1,
            Err(_) => report.failed += 1,
        }
    }
    terminate(root.0, root.1, false)?;
    report.terminated += 1;
    Ok(report)
}
