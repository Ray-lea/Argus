//! Windows services through the Service Control Manager.
//!
//! Status for every service is one `EnumServicesStatusExW` call. Static
//! configuration (start type, binary path, description) costs three RPCs per
//! service, so it is cached and refreshed only occasionally.
//!
//! Services installed by an MSIX package (`SERVICE_PKG_SERVICE`) get a
//! security descriptor that grants configuration changes only to
//! `NT SERVICE\AppXSvc`: neither administrators nor SYSTEM may change their
//! start type through the SCM. Their registry key keeps the usual ACL, so
//! [`ServiceCatalog::set_start_type_in_registry`] writes `Start` there instead.
//! In testing the SCM picked the new value up at once, but nothing documents
//! that; while the two differ, the registry value is shown as a pending start
//! type, which the SCM reads at the next boot at the latest.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_SAM_FLAGS,
    RegCloseKey, RegLoadMUIStringW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Services::{
    ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, ControlService,
    ENUM_SERVICE_STATUS_PROCESSW, ENUM_SERVICE_TYPE, EnumServicesStatusExW, OpenSCManagerW,
    OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfig2W, QueryServiceConfigW,
    QueryServiceStatusEx, SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_CONNECT,
    SC_MANAGER_ENUMERATE_SERVICE, SC_STATUS_PROCESS_INFO, SERVICE_CHANGE_CONFIG,
    SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_CONFIG_DESCRIPTION, SERVICE_CONTROL_STOP,
    SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DESCRIPTIONW, SERVICE_ERROR, SERVICE_NO_CHANGE,
    SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_START, SERVICE_START_TYPE,
    SERVICE_STATE_ALL, SERVICE_STATUS, SERVICE_STATUS_PROCESS, SERVICE_STOP, SERVICE_WIN32,
    StartServiceW,
};
use windows::core::{PCWSTR, PWSTR};

use crate::actions::ActionError;
use crate::tr;
use crate::util::{AlignedBuf, from_wide, wide};

const CONFIG_TTL: Duration = Duration::from_secs(60);
const SERVICE_ACCEPT_STOP: u32 = 0x1;
const SERVICE_WIN32_SHARE_PROCESS: u32 = 0x20;
const SERVICE_STOPPED: u32 = 1;
/// `SERVICE_PKG_SERVICE`: the service belongs to an MSIX package.
const SERVICE_PKG_SERVICE: u32 = 0x200;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    /// Key name, e.g. `Dnscache`.
    pub name: String,
    pub display_name: String,
    pub description: Option<Arc<str>>,
    /// `stopped` | `startPending` | `stopPending` | `running` | `continuePending` | `pausePending` | `paused`
    pub state: &'static str,
    /// `boot` | `system` | `auto` | `autoDelayed` | `manual` | `disabled`
    pub start_type: Option<&'static str>,
    /// Start type written to the registry that the SCM picks up at the next
    /// boot; None when it matches `start_type`.
    pub pending_start_type: Option<&'static str>,
    /// Installed by an MSIX package, so only its registry value can change
    /// the start type.
    pub packaged: bool,
    /// Hosting process; 0 when not running.
    pub pid: u32,
    /// Command line of the service binary.
    pub path: Option<Arc<str>>,
    pub account: Option<Arc<str>>,
    pub can_stop: bool,
    /// Shares its process with other services (svchost).
    pub shared: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartType {
    Auto,
    AutoDelayed,
    Manual,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Default)]
struct Config {
    /// Display name in the user's UI language (the SCM resolves it in the system one).
    display_name: Option<String>,
    start_type: Option<&'static str>,
    pending_start_type: Option<&'static str>,
    packaged: bool,
    path: Option<Arc<str>>,
    account: Option<Arc<str>>,
    description: Option<Arc<str>>,
}

#[derive(Default)]
struct ConfigCache {
    entries: HashMap<String, Arc<Config>>,
    loaded_at: Option<Instant>,
}

/// Service list with a cache for the per-service configuration.
#[derive(Default)]
pub struct ServiceCatalog {
    configs: Mutex<ConfigCache>,
}

/// Closes a service or SCM handle on drop.
struct Handle(SC_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn open_scm() -> Result<Handle, ActionError> {
    let h = unsafe {
        OpenSCManagerW(
            PCWSTR::null(),
            PCWSTR::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
        )
    }?;
    Ok(Handle(h))
}

fn open_service(scm: &Handle, name: &str, access: u32) -> Result<Handle, ActionError> {
    let w = wide(name);
    Ok(Handle(unsafe {
        OpenServiceW(scm.0, PCWSTR(w.as_ptr()), access)
    }?))
}

unsafe fn pwstr(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = unsafe { p.to_string() }.ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

impl ServiceCatalog {
    pub fn list(&self) -> Result<Vec<ServiceInfo>, ActionError> {
        let scm = open_scm()?;
        let mut buf = AlignedBuf::with_capacity(256 * 1024);
        let mut out = Vec::new();
        let mut resume = 0u32;
        loop {
            let (mut needed, mut count) = (0u32, 0u32);
            let bytes =
                unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), buf.len()) };
            let r = unsafe {
                EnumServicesStatusExW(
                    scm.0,
                    SC_ENUM_PROCESS_INFO,
                    SERVICE_WIN32,
                    SERVICE_STATE_ALL,
                    Some(bytes),
                    &mut needed,
                    &mut count,
                    Some(&mut resume),
                    PCWSTR::null(),
                )
            };
            let entries = unsafe {
                std::slice::from_raw_parts(
                    buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW,
                    count as usize,
                )
            };
            for e in entries {
                out.push(self.describe(&scm, e));
            }
            match r {
                Ok(()) => break,
                // ERROR_MORE_DATA: the next call continues from `resume`.
                Err(err)
                    if err.code() == windows::Win32::Foundation::ERROR_MORE_DATA.to_hresult() =>
                {
                    buf.grow_to(needed as usize + 4096);
                }
                Err(err) => return Err(err.into()),
            }
        }
        out.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
        });
        Ok(out)
    }

    fn describe(&self, scm: &Handle, e: &ENUM_SERVICE_STATUS_PROCESSW) -> ServiceInfo {
        let name = unsafe { pwstr(e.lpServiceName) }.unwrap_or_default();
        let st = &e.ServiceStatusProcess;
        let config = self.config(scm, &name);
        ServiceInfo {
            display_name: config
                .display_name
                .clone()
                .or_else(|| unsafe { pwstr(e.lpDisplayName) })
                .unwrap_or_else(|| name.clone()),
            description: config.description.clone(),
            state: state_name(st.dwCurrentState.0),
            start_type: config.start_type,
            pending_start_type: config.pending_start_type,
            packaged: config.packaged,
            pid: st.dwProcessId,
            path: config.path.clone(),
            account: config.account.clone(),
            can_stop: st.dwControlsAccepted & SERVICE_ACCEPT_STOP != 0,
            shared: st.dwServiceType.0 & SERVICE_WIN32_SHARE_PROCESS != 0,
            name,
        }
    }

    fn config(&self, scm: &Handle, name: &str) -> Arc<Config> {
        {
            let mut guard = self.configs.lock().unwrap();
            if guard.loaded_at.is_none_or(|t| t.elapsed() > CONFIG_TTL) {
                guard.entries.clear();
                guard.loaded_at = Some(Instant::now());
            }
            if let Some(c) = guard.entries.get(name) {
                return c.clone();
            }
        }
        let c = Arc::new(query_config(scm, name).unwrap_or_default());
        self.configs
            .lock()
            .unwrap()
            .entries
            .insert(name.to_owned(), c.clone());
        c
    }

    /// Forgets cached configuration, e.g. after changing a start type.
    pub fn invalidate(&self, name: &str) {
        self.configs.lock().unwrap().entries.remove(name);
    }

    pub fn control(&self, name: &str, action: ServiceAction) -> Result<(), ActionError> {
        let scm = open_scm()?;
        match action {
            ServiceAction::Start => start(&scm, name),
            ServiceAction::Stop => stop(&scm, name, false),
            ServiceAction::Restart => {
                stop(&scm, name, true)?;
                start(&scm, name)
            }
        }
    }

    pub fn set_start_type(&self, name: &str, start: StartType) -> Result<(), ActionError> {
        let scm = open_scm()?;
        let svc = open_service(&scm, name, SERVICE_CHANGE_CONFIG)?;
        let code = match start {
            StartType::Auto | StartType::AutoDelayed => 2,
            StartType::Manual => 3,
            StartType::Disabled => 4,
        };
        unsafe {
            ChangeServiceConfigW(
                svc.0,
                ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
                SERVICE_START_TYPE(code),
                SERVICE_ERROR(SERVICE_NO_CHANGE),
                PCWSTR::null(),
                PCWSTR::null(),
                None,
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
            )
        }?;
        if matches!(start, StartType::Auto | StartType::AutoDelayed) {
            let info = SERVICE_DELAYED_AUTO_START_INFO {
                fDelayedAutostart: (start == StartType::AutoDelayed).into(),
            };
            unsafe {
                ChangeServiceConfig2W(
                    svc.0,
                    SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                    Some((&info as *const SERVICE_DELAYED_AUTO_START_INFO).cast()),
                )
            }?;
        }
        self.invalidate(name);
        Ok(())
    }

    /// Writes the start type of a packaged service straight to its registry
    /// key, for when the SCM refuses the change (see the module docs). The SCM
    /// usually applies it at once, at the next boot at the latest; an update of
    /// the package may restore its own value. Refuses services that are not packaged: those go through the SCM.
    pub fn set_start_type_in_registry(
        &self,
        name: &str,
        start: StartType,
    ) -> Result<(), ActionError> {
        let scm = open_scm()?;
        let packaged = query_config(&scm, name)
            .ok_or(ActionError::NotFound)?
            .packaged;
        if !packaged {
            return Err(ActionError::Refused);
        }
        let key =
            open_registry_key(name, KEY_QUERY_VALUE | KEY_SET_VALUE).map_err(ActionError::from)?;
        let (code, delayed) = start_values(start);
        set_dword(&key, "Start", code)?;
        if let Some(delayed) = delayed {
            set_dword(&key, "DelayedAutostart", delayed.into())?;
        }
        self.invalidate(name);
        Ok(())
    }
}

/// `Start` and, for automatic starts, `DelayedAutostart` registry values.
fn start_values(start: StartType) -> (u32, Option<bool>) {
    match start {
        StartType::Auto => (2, Some(false)),
        StartType::AutoDelayed => (2, Some(true)),
        StartType::Manual => (3, None),
        StartType::Disabled => (4, None),
    }
}

/// The start type the registry values describe, named like `start_type`.
fn registry_start_name(start: u32, delayed: bool) -> Option<&'static str> {
    match (start, delayed) {
        (0, _) => Some("boot"),
        (1, _) => Some("system"),
        (2, true) => Some("autoDelayed"),
        (2, false) => Some("auto"),
        (3, _) => Some("manual"),
        (4, _) => Some("disabled"),
        _ => None,
    }
}

fn set_dword(key: &RegKey, value: &str, data: u32) -> Result<(), ActionError> {
    let v = wide(value);
    unsafe {
        RegSetValueExW(
            key.0,
            PCWSTR(v.as_ptr()),
            None,
            REG_DWORD,
            Some(&data.to_le_bytes()),
        )
    }
    .ok()?;
    Ok(())
}

fn read_dword(key: &RegKey, value: &str) -> Option<u32> {
    let v = wide(value);
    let mut data = [0u8; 4];
    let mut len = data.len() as u32;
    let mut kind = REG_DWORD;
    let r = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(v.as_ptr()),
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut len),
        )
    };
    (r == ERROR_SUCCESS && kind == REG_DWORD && len == 4).then(|| u32::from_le_bytes(data))
}

fn start(scm: &Handle, name: &str) -> Result<(), ActionError> {
    let svc = open_service(scm, name, SERVICE_START)?;
    unsafe { StartServiceW(svc.0, None) }?;
    Ok(())
}

/// Sends a stop request; with `wait`, blocks until the service has stopped
/// (needed before starting it again on restart).
fn stop(scm: &Handle, name: &str, wait: bool) -> Result<(), ActionError> {
    let svc = open_service(scm, name, SERVICE_STOP | SERVICE_QUERY_STATUS)?;
    let mut status = SERVICE_STATUS::default();
    unsafe { ControlService(svc.0, SERVICE_CONTROL_STOP, &mut status) }?;
    if !wait {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let mut buf = vec![0u8; std::mem::size_of::<SERVICE_STATUS_PROCESS>()];
        let mut needed = 0u32;
        unsafe {
            QueryServiceStatusEx(svc.0, SC_STATUS_PROCESS_INFO, Some(&mut buf), &mut needed)
        }?;
        let st = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const SERVICE_STATUS_PROCESS) };
        if st.dwCurrentState.0 == SERVICE_STOPPED {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(ActionError::Os(tr!("core.services.stopTimeout")))
}

fn query_config(scm: &Handle, name: &str) -> Option<Config> {
    let svc = open_service(scm, name, SERVICE_QUERY_CONFIG).ok()?;
    let mut config = Config::default();

    let mut needed = 0u32;
    let _ = unsafe { QueryServiceConfigW(svc.0, None, 0, &mut needed) };
    if needed > 0 {
        let mut buf = AlignedBuf::with_capacity(needed as usize);
        if unsafe { QueryServiceConfigW(svc.0, Some(buf.as_mut_ptr().cast()), needed, &mut needed) }
            .is_ok()
        {
            let c = unsafe { &*(buf.as_ptr() as *const QUERY_SERVICE_CONFIGW) };
            config.start_type = match c.dwStartType.0 {
                0 => Some("boot"),
                1 => Some("system"),
                2 => Some("auto"),
                3 => Some("manual"),
                4 => Some("disabled"),
                _ => None,
            };
            config.packaged = c.dwServiceType.0 & SERVICE_PKG_SERVICE != 0;
            config.path = unsafe { pwstr(c.lpBinaryPathName) }.map(Into::into);
            config.account = unsafe { pwstr(c.lpServiceStartName) }.map(Into::into);
        }
    }

    if let Some(buf) = query_config2(&svc, SERVICE_CONFIG_DESCRIPTION) {
        let d = unsafe { &*(buf.as_ptr() as *const SERVICE_DESCRIPTIONW) };
        config.description = unsafe { pwstr(d.lpDescription) }.map(Into::into);
    }
    let key = open_registry_key(name, KEY_READ).ok();
    if let Some(key) = &key {
        config.display_name = mui_string(key, "DisplayName");
        if let Some(d) = mui_string(key, "Description") {
            config.description = Some(d.into());
        }
    }
    if config.start_type == Some("auto")
        && let Some(buf) = query_config2(&svc, SERVICE_CONFIG_DELAYED_AUTO_START_INFO)
    {
        let info = unsafe { &*(buf.as_ptr() as *const SERVICE_DELAYED_AUTO_START_INFO) };
        if info.fDelayedAutostart.as_bool() {
            config.start_type = Some("autoDelayed");
        }
    }
    // A registry value the SCM has not taken up (written by Argus or by hand)
    // shows as pending; the SCM reads it at the next boot at the latest.
    if let Some(key) = &key
        && let Some(start) = read_dword(key, "Start")
    {
        let delayed = read_dword(key, "DelayedAutostart").is_some_and(|d| d != 0);
        config.pending_start_type = registry_start_name(start, delayed)
            .filter(|registry| config.start_type.is_some_and(|scm| scm != *registry));
    }
    Some(config)
}

/// Closes a registry key on drop.
struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open_registry_key(name: &str, access: REG_SAM_FLAGS) -> windows::core::Result<RegKey> {
    let path = wide(&format!(r"SYSTEM\CurrentControlSet\Services\{name}"));
    let mut key = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(path.as_ptr()),
            None,
            access,
            &mut key,
        )
    }
    .ok()?;
    Ok(RegKey(key))
}

/// Reads a registry string, resolving `@file.dll,-id` references in the
/// caller's UI language.
fn mui_string(key: &RegKey, value: &str) -> Option<String> {
    let v = wide(value);
    let mut buf = vec![0u16; 2048];
    let mut len = 0u32;
    let r = unsafe {
        RegLoadMUIStringW(
            key.0,
            PCWSTR(v.as_ptr()),
            Some(PWSTR(buf.as_mut_ptr())),
            (buf.len() * 2) as u32,
            Some(&mut len),
            0,
            PCWSTR::null(),
        )
    };
    let s = from_wide(&buf);
    (r == ERROR_SUCCESS && !s.trim().is_empty()).then(|| s.trim().to_owned())
}

fn query_config2(
    svc: &Handle,
    level: windows::Win32::System::Services::SERVICE_CONFIG,
) -> Option<AlignedBuf> {
    let mut needed = 0u32;
    let _ = unsafe { QueryServiceConfig2W(svc.0, level, None, &mut needed) };
    if needed == 0 {
        return None;
    }
    let mut buf = AlignedBuf::with_capacity(needed as usize);
    let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), buf.len()) };
    unsafe { QueryServiceConfig2W(svc.0, level, Some(bytes), &mut needed) }.ok()?;
    Some(buf)
}

fn state_name(state: u32) -> &'static str {
    match state {
        1 => "stopped",
        2 => "startPending",
        3 => "stopPending",
        4 => "running",
        5 => "continuePending",
        6 => "pausePending",
        7 => "paused",
        _ => "stopped",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_values_read_back_as_the_chosen_start_type() {
        for (start, name) in [
            (StartType::Auto, "auto"),
            (StartType::AutoDelayed, "autoDelayed"),
            (StartType::Manual, "manual"),
            (StartType::Disabled, "disabled"),
        ] {
            let (code, delayed) = start_values(start);
            assert_eq!(
                registry_start_name(code, delayed.unwrap_or(false)),
                Some(name)
            );
        }
    }

    #[test]
    fn delayed_flag_only_matters_for_automatic_starts() {
        assert_eq!(registry_start_name(3, true), Some("manual"));
        assert_eq!(registry_start_name(4, true), Some("disabled"));
        assert_eq!(registry_start_name(7, false), None);
    }
}
