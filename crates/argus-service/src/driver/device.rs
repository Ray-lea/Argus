//! Read-only PnP state: a loaded driver service can have no usable device.
use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::core::{PCWSTR, w};

use super::{DEVICE_DISABLED, NOT_READY, REBOOT_REQUIRED};
use crate::util::Result;

#[derive(Debug, Default)]
pub(super) struct State {
    nodes: u32,
    started: u32,
    problem: Option<u32>,
    needs_restart: bool,
}

impl State {
    fn note(&mut self, status: CM_DEVNODE_STATUS_FLAGS, problem: CM_PROB) {
        self.nodes += 1;
        self.needs_restart |= status.contains(DN_NEED_RESTART);
        if status.contains(DN_HAS_PROBLEM) {
            // Keep a disabled-device diagnosis when several instances exist.
            if self.problem.is_none() || problem == CM_PROB_DISABLED {
                self.problem = Some(problem.0);
            }
        } else if status.contains(DN_STARTED) {
            self.started += 1;
        }
    }

    pub(super) fn check(&self, service_running: bool) -> Result<()> {
        if self.needs_restart {
            return Err(REBOOT_REQUIRED);
        }
        if self.started != 0 {
            return Ok(());
        }
        if self.problem == Some(CM_PROB_DISABLED.0)
            || self.problem == Some(CM_PROB_HARDWARE_DISABLED.0)
        {
            return Err(DEVICE_DISABLED);
        }
        if self.nodes == 0 || self.problem.is_some() || service_running {
            return Err(NOT_READY);
        }
        Ok(())
    }
}

fn checked(code: CONFIGRET) -> Result<()> {
    if code == CR_SUCCESS {
        Ok(())
    } else {
        Err(unsafe { CM_MapCrToWin32Err(code, 31) })
    }
}

pub(super) fn inspect() -> Result<State> {
    // SERVICE alone may create a legacy devnode. Diagnostics must not do so.
    const FLAGS: u32 =
        CM_GETIDLIST_FILTER_SERVICE | CM_GETIDLIST_FILTER_PRESENT | CM_GETIDLIST_DONOTGENERATE;
    for _ in 0..3 {
        let mut size = 0;
        checked(unsafe { CM_Get_Device_ID_List_SizeW(&mut size, w!("PawnIO"), FLAGS) })?;
        if size == 0 || size > 32 * 1024 {
            return Err(NOT_READY);
        }
        let mut ids = vec![0u16; size as usize];
        let result = unsafe { CM_Get_Device_ID_ListW(w!("PawnIO"), &mut ids, FLAGS) };
        if result == CR_BUFFER_SMALL {
            continue;
        }
        checked(result)?;
        let mut state = State::default();
        let mut offset = 0;
        while offset < ids.len() && ids[offset] != 0 {
            let length = ids[offset..]
                .iter()
                .position(|c| *c == 0)
                .ok_or(NOT_READY)?;
            if state.nodes == 64 {
                return Err(NOT_READY);
            }
            let mut node = 0;
            checked(unsafe {
                CM_Locate_DevNodeW(
                    &mut node,
                    PCWSTR(ids[offset..].as_ptr()),
                    CM_LOCATE_DEVNODE_NORMAL,
                )
            })?;
            let mut status = CM_DEVNODE_STATUS_FLAGS::default();
            let mut problem = CM_PROB::default();
            checked(unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, node, 0) })?;
            state.note(status, problem);
            offset += length + 1;
        }
        return Ok(state);
    }
    Err(NOT_READY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loaded_service_does_not_make_a_disabled_device_ready() {
        let mut state = State::default();
        state.note(DN_HAS_PROBLEM, CM_PROB_DISABLED);
        assert_eq!(state.check(false), Err(DEVICE_DISABLED));
        assert_eq!(state.check(true), Err(DEVICE_DISABLED));
    }

    #[test]
    fn missing_or_failed_devices_never_report_ready() {
        assert_eq!(State::default().check(true), Err(NOT_READY));
        let mut state = State::default();
        state.note(DN_HAS_PROBLEM, CM_PROB_FAILED_START);
        assert_eq!(state.check(true), Err(NOT_READY));
        assert_eq!(state.check(false), Err(NOT_READY));
    }

    #[test]
    fn a_pending_device_change_requires_reboot_even_while_it_still_runs() {
        let mut state = State::default();
        state.note(DN_STARTED | DN_NEED_RESTART, CM_PROB::default());
        assert_eq!(state.check(true), Err(REBOOT_REQUIRED));
        assert_eq!(state.check(false), Err(REBOOT_REQUIRED));
    }

    #[test]
    fn a_healthy_stopped_device_can_be_started_but_is_not_ready() {
        let mut state = State::default();
        state.note(CM_DEVNODE_STATUS_FLAGS::default(), CM_PROB::default());
        assert_eq!(state.check(false), Ok(()));
        assert_eq!(state.check(true), Err(NOT_READY));
        state.note(DN_STARTED, CM_PROB::default());
        assert_eq!(state.check(true), Ok(()));
    }
}
