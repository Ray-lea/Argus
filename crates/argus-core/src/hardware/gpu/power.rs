//! Keeps telemetry from waking an adapter that powers down when idle.
//!
//! The discrete GPU of a hybrid laptop turns itself off (D3) while nothing
//! uses it. A query that its driver answers from the hardware turns it on
//! again: the NVML, ADL and IGCL calls, and the D3DKMT adapter and node perf
//! data. Sent with every sample, they wake the adapter each time, and sent to
//! an adapter that is on but idle, they never let it turn off. The scheduler
//! and memory statistics come from the graphics kernel alone and are read
//! regardless.

use std::time::{Duration, Instant};

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_PropertyW, CM_Get_Device_IDW, CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW,
    CR_SUCCESS, MAX_DEVICE_ID_LEN,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_PowerData, DEVPROP_TYPE_BINARY, DEVPROPTYPE,
};
use windows::Win32::System::Power::{
    CM_POWER_DATA, PowerDeviceD0, PowerDeviceD1, PowerDeviceD2, PowerDeviceD3,
};
use windows::core::PCWSTR;

/// `D3DKMT_ADAPTERTYPE` bits of an adapter Windows powers down when idle: the
/// discrete GPU of a hybrid system, and any adapter under runtime power
/// management (WDDM 2.9).
const HYBRID_DISCRETE: u32 = 0x10;
const RUNTIME_POWER_MANAGEMENT: u32 = 0x2000;

/// How often an adapter that is on but idle is still asked. It has to stay on
/// this long with nothing asked of it first, which shows that something else
/// keeps it on; an adapter left alone turns off within seconds. Vendor energy
/// counters accept intervals up to 30 s, so the readings stay continuous.
const IDLE_READ_EVERY: Duration = Duration::from_secs(20);

/// How often the power state of an adapter that is not known to power down is
/// looked at, to notice one that does without its type saying so. Reading the
/// state takes about 0.2 ms.
const STATE_EVERY: Duration = Duration::from_secs(5);

pub(super) fn powers_down(adapter_type: u32) -> bool {
    adapter_type & (HYBRID_DISCRETE | RUNTIME_POWER_MANAGEMENT) != 0
}

/// An adapter's device in the PnP tree, kept by instance ID: a device node
/// handle belongs to the enumeration that returned it.
pub(super) struct Device(Vec<u16>);

impl Device {
    pub fn of(node: u32) -> Option<Self> {
        let mut id = vec![0u16; MAX_DEVICE_ID_LEN as usize + 1];
        (unsafe { CM_Get_Device_IDW(node, &mut id, 0) } == CR_SUCCESS).then_some(Self(id))
    }

    /// Whether the device is in D0, from the state PnP keeps for its node;
    /// nothing is sent to the device. None when the node is gone or names no
    /// state.
    pub fn working(&self) -> Option<bool> {
        Self::state(self.locate()?)
    }

    fn locate(&self) -> Option<u32> {
        let mut node = 0;
        (unsafe {
            CM_Locate_DevNodeW(&mut node, PCWSTR(self.0.as_ptr()), CM_LOCATE_DEVNODE_NORMAL)
        } == CR_SUCCESS)
            .then_some(node)
    }

    fn state(node: u32) -> Option<bool> {
        let mut data = CM_POWER_DATA::default();
        let mut size = size_of::<CM_POWER_DATA>() as u32;
        let mut kind = DEVPROPTYPE::default();
        let status = unsafe {
            CM_Get_DevNode_PropertyW(
                node,
                &DEVPKEY_Device_PowerData,
                &mut kind,
                Some((&raw mut data).cast()),
                &mut size,
                0,
            )
        };
        if status != CR_SUCCESS
            || kind != DEVPROP_TYPE_BINARY
            || (size as usize) < std::mem::offset_of!(CM_POWER_DATA, PD_Capabilities)
        {
            return None;
        }
        let state = data.PD_MostRecentPowerState;
        if state == PowerDeviceD0 {
            Some(true)
        } else if [PowerDeviceD1, PowerDeviceD2, PowerDeviceD3].contains(&state) {
            Some(false)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Access {
    /// Ask the driver.
    Read,
    /// On but idle: asking would keep it on, so the last readings stand.
    Keep,
    /// Powered down: asking would wake it, and there is nothing to read.
    Asleep,
}

/// Decides, sample by sample, whether an adapter's driver is asked.
pub(super) struct Gate {
    /// Windows powers the adapter down when idle: its type says so, or it
    /// has been seen powered down.
    powers_down: bool,
    read_at: Option<Instant>,
    /// The power state as last looked at, and when.
    looked: Option<(Instant, Option<bool>)>,
    #[cfg(test)]
    pub last: Option<Access>,
}

impl Gate {
    pub fn new(powers_down: bool) -> Self {
        Self {
            powers_down,
            read_at: None,
            looked: None,
            #[cfg(test)]
            last: None,
        }
    }

    #[cfg(test)]
    pub fn powers_down(&self) -> bool {
        self.powers_down
    }

    /// Decides for this sample. `worked`: the engine counters show work in
    /// this interval, so the adapter is on. `busy`: its driver reported work
    /// when last asked. The power state is read with every sample in which an
    /// adapter that powers down shows no work.
    pub fn sample(
        &mut self,
        device: Option<&Device>,
        worked: bool,
        busy: bool,
        now: Instant,
    ) -> Access {
        let working = match self.looked {
            _ if worked => Some(true),
            Some((at, state))
                if !self.powers_down && now.saturating_duration_since(at) < STATE_EVERY =>
            {
                state
            }
            _ => {
                let state = device.and_then(Device::working);
                self.looked = Some((now, state));
                state
            }
        };
        let access = self.decide(working, worked || busy, now);
        #[cfg(test)]
        {
            self.last = Some(access);
        }
        access
    }

    /// `working` is the device's power state where known, `active` whether
    /// the adapter did work in this interval that no query of ours caused.
    fn decide(&mut self, working: Option<bool>, active: bool, now: Instant) -> Access {
        if working == Some(false) {
            self.powers_down = true;
            return Access::Asleep;
        }
        let due = self
            .read_at
            .is_none_or(|at| now.saturating_duration_since(at) >= IDLE_READ_EVERY);
        if !self.powers_down || active || due {
            self.read_at = Some(now);
            Access::Read
        } else {
            Access::Keep
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    #[test]
    fn an_adapter_that_never_powers_down_is_asked_with_every_sample() {
        let start = Instant::now();
        let mut gate = Gate::new(powers_down(0x0b));
        for second in 0..60 {
            for working in [Some(true), None] {
                assert_eq!(gate.decide(working, false, at(start, second)), Access::Read);
            }
        }
    }

    #[test]
    fn a_powered_down_adapter_is_never_asked() {
        let start = Instant::now();
        // Not marked by its type: the observed state alone decides.
        let mut gate = Gate::new(false);
        for second in 0..120 {
            assert_eq!(
                gate.decide(Some(false), second % 7 == 0, at(start, second)),
                Access::Asleep
            );
        }
    }

    #[test]
    fn an_idle_adapter_is_left_alone_long_enough_to_power_down() {
        let start = Instant::now();
        for adapter_type in [HYBRID_DISCRETE | 1, RUNTIME_POWER_MANAGEMENT | 1] {
            let mut gate = Gate::new(powers_down(adapter_type));
            // The first sample reads; an idle adapter is then asked once per
            // period only, and only while it stays on unasked.
            let reads: Vec<u64> = (0..=45)
                .filter(|&second| gate.decide(Some(true), false, at(start, second)) == Access::Read)
                .collect();
            assert_eq!(reads, [0, 20, 40]);
        }
    }

    #[test]
    fn work_on_the_adapter_brings_every_sample_back_and_idling_ends_it() {
        let start = Instant::now();
        let mut gate = Gate::new(true);
        assert_eq!(gate.decide(Some(true), false, start), Access::Read);
        assert_eq!(gate.decide(Some(true), false, at(start, 1)), Access::Keep);
        for second in 2..10 {
            assert_eq!(
                gate.decide(Some(true), true, at(start, second)),
                Access::Read
            );
        }
        // Idle again: nothing is asked while it powers down.
        for second in 10..15 {
            assert_eq!(
                gate.decide(Some(true), false, at(start, second)),
                Access::Keep
            );
        }
        assert_eq!(
            gate.decide(Some(false), false, at(start, 15)),
            Access::Asleep
        );
        // Awake without work: asked no sooner than any idle adapter.
        for second in 16..29 {
            assert_eq!(
                gate.decide(Some(true), false, at(start, second)),
                Access::Keep
            );
        }
        assert_eq!(gate.decide(Some(true), false, at(start, 29)), Access::Read);
    }

    #[test]
    fn an_adapter_seen_powered_down_is_treated_as_one_that_powers_down() {
        let start = Instant::now();
        let mut gate = Gate::new(false);
        assert_eq!(gate.decide(Some(true), false, start), Access::Read);
        assert_eq!(gate.decide(Some(true), false, at(start, 1)), Access::Read);
        assert_eq!(
            gate.decide(Some(false), false, at(start, 2)),
            Access::Asleep
        );
        assert_eq!(gate.decide(Some(true), false, at(start, 3)), Access::Keep);
        // An unknown state does not wake it either.
        assert_eq!(gate.decide(None, false, at(start, 4)), Access::Keep);
        assert_eq!(gate.decide(None, true, at(start, 5)), Access::Read);
    }

    #[test]
    #[ignore = "Requires live Windows graphics adapters"]
    fn live_adapter_power_state() {
        use windows::Win32::Devices::DeviceAndDriverInstallation::{
            DIGCF_PRESENT, GUID_DEVCLASS_DISPLAY, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList,
            SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        };
        let set = unsafe {
            SetupDiGetClassDevsW(Some(&GUID_DEVCLASS_DISPLAY), None, None, DIGCF_PRESENT)
        }
        .unwrap();
        let mut devices = Vec::new();
        for index in 0..256 {
            let mut device = SP_DEVINFO_DATA {
                cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if unsafe { SetupDiEnumDeviceInfo(set, index, &mut device) }.is_err() {
                break;
            }
            devices.extend(Device::of(device.DevInst));
        }
        // The IDs outlive the enumeration they came from.
        unsafe { SetupDiDestroyDeviceInfoList(set) }.unwrap();
        assert!(!devices.is_empty(), "no display device");
        for device in devices {
            let rounds = 1000;
            let started = Instant::now();
            let mut node = None;
            for _ in 0..rounds {
                node = device.locate();
            }
            let locate = started.elapsed().as_secs_f64() * 1e6 / f64::from(rounds);
            let started = Instant::now();
            let mut working = None;
            for _ in 0..rounds {
                working = node.and_then(Device::state);
            }
            println!(
                "{}",
                serde_json::json!({"working": working, "locateMicros": locate,
                    "stateMicros": started.elapsed().as_secs_f64() * 1e6 / f64::from(rounds)})
            );
        }
    }
}
