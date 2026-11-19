//! Host power state and background Efficiency Mode (EcoQoS) management.
//!
//! When Argus is not the foreground application and the machine is running on
//! battery or Windows Energy Saver, the host opts itself into EcoQoS
//! (`ProcessPowerThrottling`) without lowering its priority class. External
//! processes launched by Argus temporarily suspend self-throttling and clear
//! any inherited power throttling on the child handle so user-started tools do
//! not stay throttled.

use std::mem::size_of;
use std::sync::Mutex;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessInformation, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    ProcessPowerThrottling, SetProcessInformation,
};

/// Current system power supply and Windows Energy Saver state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerSource {
    pub on_battery: bool,
    pub energy_saver: bool,
}

impl PowerSource {
    /// Interprets raw `SYSTEM_POWER_STATUS` flags without WMI.
    pub const fn from_system_status(
        ac_line_status: u8,
        battery_flag: u8,
        system_status_flag: u8,
    ) -> Self {
        // ACLineStatus == 0 means offline (battery); BatteryFlag bit 128 means
        // no system battery (0xFF means unknown status, so do not mask 128 on 0xFF).
        let has_battery = battery_flag == 0xFF || (battery_flag & 128) == 0;
        Self {
            on_battery: ac_line_status == 0 && has_battery,
            energy_saver: system_status_flag == 1,
        }
    }

    /// Queries the live power state through `GetSystemPowerStatus`.
    pub fn current() -> Self {
        let mut status = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut status) }.is_ok() {
            Self::from_system_status(
                status.ACLineStatus,
                status.BatteryFlag,
                status.SystemStatusFlag,
            )
        } else {
            Self::default()
        }
    }

    /// Whether background work should conserve battery power.
    pub const fn conserving(self) -> bool {
        self.on_battery || self.energy_saver
    }
}

#[derive(Default)]
struct SelfThrottleState {
    desired: bool,
    applied: bool,
    launch_holds: usize,
}

static SELF_THROTTLE: Mutex<SelfThrottleState> = Mutex::new(SelfThrottleState {
    desired: false,
    applied: false,
    launch_holds: 0,
});

fn set_process_ecoqos(process: HANDLE, on: bool) -> windows::core::Result<()> {
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
            process,
            ProcessPowerThrottling,
            (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
}

/// Enables or disables EcoQoS on the current Argus process without altering its
/// priority class.
pub fn set_self_efficiency_mode(on: bool) {
    let mut state = SELF_THROTTLE.lock().unwrap();
    state.desired = on;
    let target = state.desired && state.launch_holds == 0;
    if target != state.applied && set_process_ecoqos(unsafe { GetCurrentProcess() }, target).is_ok()
    {
        state.applied = target;
    }
}

/// Whether `process` currently has execution-speed power throttling active.
pub fn is_process_throttled(process: HANDLE) -> bool {
    let mut state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ..Default::default()
    };
    unsafe {
        GetProcessInformation(
            process,
            ProcessPowerThrottling,
            (&mut state as *mut PROCESS_POWER_THROTTLING_STATE).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
    .is_ok()
        && (state.ControlMask & state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED) != 0
}

/// Whether the current Argus process currently has EcoQoS enabled.
pub fn is_self_efficiency_mode() -> bool {
    is_process_throttled(unsafe { GetCurrentProcess() })
}

/// Clears inherited `ProcessPowerThrottling` on a newly created external child
/// process handle.
pub fn clear_process_throttling(process: HANDLE) {
    if !process.is_invalid() && !process.0.is_null() {
        let _ = set_process_ecoqos(process, false);
    }
}

struct LaunchGuard;

impl Drop for LaunchGuard {
    fn drop(&mut self) {
        let mut state = SELF_THROTTLE.lock().unwrap();
        state.launch_holds = state.launch_holds.saturating_sub(1);
        let target = state.desired && state.launch_holds == 0;
        if target != state.applied
            && set_process_ecoqos(unsafe { GetCurrentProcess() }, target).is_ok()
        {
            state.applied = target;
        }
    }
}

/// Temporarily disables self EcoQoS across an external process launch so child
/// processes never inherit execution-speed throttling from Argus.
pub fn with_unthrottled_launch<T>(f: impl FnOnce() -> T) -> T {
    {
        let mut state = SELF_THROTTLE.lock().unwrap();
        state.launch_holds += 1;
        if state.applied && set_process_ecoqos(unsafe { GetCurrentProcess() }, false).is_ok() {
            state.applied = false;
        }
    }
    let _guard = LaunchGuard;
    f()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows::Win32::System::Threading::GetPriorityClass;

    #[test]
    fn power_source_distinguishes_ac_battery_desktop_and_energy_saver() {
        let ac = PowerSource::from_system_status(1, 0, 0);
        assert_eq!(
            ac,
            PowerSource {
                on_battery: false,
                energy_saver: false
            }
        );
        assert!(!ac.conserving());

        let ac_saver = PowerSource::from_system_status(1, 8, 1);
        assert!(!ac_saver.on_battery);
        assert!(ac_saver.energy_saver);
        assert!(ac_saver.conserving());

        let battery = PowerSource::from_system_status(0, 2, 0);
        assert!(battery.on_battery);
        assert!(!battery.energy_saver);
        assert!(battery.conserving());

        let no_battery = PowerSource::from_system_status(0, 128, 0);
        assert!(!no_battery.on_battery);
        assert!(!no_battery.conserving());

        let unknown_battery = PowerSource::from_system_status(0, 0xFF, 0);
        assert!(unknown_battery.on_battery);
        assert!(unknown_battery.conserving());
    }

    #[test]
    #[ignore = "owned subprocess fixture, called by self_efficiency_mode_preserves_priority_and_launches_unthrottled_children"]
    fn efficiency_mode_fixture() {
        let current = unsafe { GetCurrentProcess() };
        let initial_priority = unsafe { GetPriorityClass(current) };

        set_self_efficiency_mode(true);
        assert!(is_self_efficiency_mode());
        assert_eq!(unsafe { GetPriorityClass(current) }, initial_priority);

        let mut child = with_unthrottled_launch(|| {
            let child = Command::new("cmd.exe")
                .args(["/d", "/c", "pause"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x08000000)
                .spawn()
                .expect("spawn child");
            clear_process_throttling(HANDLE(child.as_raw_handle()));
            assert!(!is_self_efficiency_mode());
            child
        });

        let child_handle = HANDLE(child.as_raw_handle());
        assert!(!is_process_throttled(child_handle));
        assert!(is_self_efficiency_mode());
        assert_eq!(unsafe { GetPriorityClass(current) }, initial_priority);

        let _ = child.kill();
        let _ = child.wait();

        set_self_efficiency_mode(false);
        assert!(!is_self_efficiency_mode());
        assert_eq!(unsafe { GetPriorityClass(current) }, initial_priority);
    }

    #[test]
    fn self_efficiency_mode_preserves_priority_and_launches_unthrottled_children() {
        let exe = std::env::current_exe().expect("test binary path");
        let output = Command::new(exe)
            .args([
                "--exact",
                "power::tests::efficiency_mode_fixture",
                "--ignored",
                "--nocapture",
            ])
            .creation_flags(0x08000000)
            .output()
            .expect("run efficiency_mode_fixture");
        assert!(
            output.status.success(),
            "fixture failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
