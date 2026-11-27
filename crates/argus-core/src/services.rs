//! Windows services through the Service Control Manager.
//!
//! Status for every service is one `EnumServicesStatusExW` call. Static
//! configuration (start type, binary path, description) costs three RPCs per
//! service, so it is cached and refreshed only occasionally.
//!
//! Some MSIX services deny startup changes through the SCM. For an
//! unprotected packaged service, a confirmed registry change can take effect
//! immediately or after a restart. The list shows any pending startup type.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_SERVICE_DOES_NOT_EXIST, ERROR_SERVICE_MARKED_FOR_DELETE,
    ERROR_SUCCESS,
};
use windows::Win32::Storage::FileSystem::{CommitTransaction, CreateTransaction};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_LINK,
    REG_OPTION_OPEN_LINK, REG_SAM_FLAGS, RegCloseKey, RegLoadMUIStringW, RegOpenKeyExW,
    RegOpenKeyTransactedW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Services::{
    ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, ControlService, DeleteService,
    ENUM_SERVICE_STATUS_PROCESSW, ENUM_SERVICE_TYPE, EnumServicesStatusExW, OpenSCManagerW,
    OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfig2W, QueryServiceConfigW,
    QueryServiceStatusEx, SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_CONNECT,
    SC_MANAGER_ENUMERATE_SERVICE, SC_STATUS_PROCESS_INFO, SERVICE_CHANGE_CONFIG,
    SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_CONFIG_DESCRIPTION,
    SERVICE_CONFIG_LAUNCH_PROTECTED, SERVICE_CONTROL_STOP, SERVICE_DELAYED_AUTO_START_INFO,
    SERVICE_DESCRIPTIONW, SERVICE_ERROR, SERVICE_LAUNCH_PROTECTED_INFO,
    SERVICE_LAUNCH_PROTECTED_NONE, SERVICE_NO_CHANGE, SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS,
    SERVICE_START, SERVICE_START_TYPE, SERVICE_STATE_ALL, SERVICE_STATUS, SERVICE_STATUS_PROCESS,
    SERVICE_STOP, SERVICE_WIN32, StartServiceW,
};
use windows::Win32::System::SystemServices::SERVICE_PKG_SERVICE;
use windows::core::{PCWSTR, PWSTR};

use crate::actions::ActionError;
use crate::tr;
use crate::util::{AlignedBuf, OwnedHandle, from_wide, wide};

const CONFIG_TTL: Duration = Duration::from_secs(60);
const SERVICE_ACCEPT_STOP: u32 = 0x1;
const SERVICE_WIN32_SHARE_PROCESS: u32 = 0x20;
const SERVICE_STOPPED: u32 = 1;

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
    /// An MSIX service with no Windows launch protection.
    pub registry_start_type_allowed: bool,
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
    registry_start_type_allowed: bool,
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
    unsafe { OpenServiceW(scm.0, PCWSTR(w.as_ptr()), access) }
        .map(Handle)
        .map_err(|e| {
            if e.code() == ERROR_SERVICE_DOES_NOT_EXIST.to_hresult() {
                ActionError::NotFound
            } else {
                e.into()
            }
        })
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
            registry_start_type_allowed: config.registry_start_type_allowed,
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

    /// Removes a service from the Service Control Manager. Windows takes it
    /// away once it has stopped and every handle to it is closed: a running
    /// one stays until then (`remains`). One marked for deletion already
    /// counts as deleted.
    pub fn delete(&self, name: &str) -> Result<(), ActionError> {
        let scm = open_scm().inspect_err(|error| {
            crate::diagnostics::warn("service delete OpenSCManagerW", error.diagnostic())
        })?;
        let w = wide(name);
        let svc = unsafe { OpenServiceW(scm.0, PCWSTR(w.as_ptr()), DELETE_ACCESS) }
            .map(Handle)
            .map_err(|error| {
                if error.code() == ERROR_SERVICE_DOES_NOT_EXIST.to_hresult() {
                    ActionError::NotFound
                } else {
                    crate::diagnostics::windows_failure("service delete OpenServiceW", &error);
                    error.into()
                }
            })?;
        match unsafe { DeleteService(svc.0) } {
            Err(error) if error.code() != ERROR_SERVICE_MARKED_FOR_DELETE.to_hresult() => {
                crate::diagnostics::windows_failure("service DeleteService", &error);
                return Err(error.into());
            }
            _ => {}
        }
        self.invalidate(name);
        Ok(())
    }

    /// Rechecks eligibility after confirmation. Keeping the service handle open
    /// prevents the SCM from deleting and recreating that name during the write.
    pub fn set_start_type_in_registry(
        &self,
        name: &str,
        start: StartType,
    ) -> Result<(), ActionError> {
        let scm = open_scm()?;
        let svc = open_service(&scm, name, SERVICE_QUERY_CONFIG)?;
        let buf = basic_config(&svc)?;
        let config = unsafe { &*buf.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
        if !registry_start_type_allowed(&svc, config.dwServiceType) {
            return Err(ActionError::Refused);
        }
        write_registry_start_type(name, start)?;
        self.invalidate(name);
        Ok(())
    }
}

/// The standard right to delete an object, which `DeleteService` needs.
const DELETE_ACCESS: u32 = 0x0001_0000;

/// Whether the Service Control Manager still has `name`: after deleting it,
/// until it has stopped and every handle to it is closed. Unsure counts as
/// still there.
pub fn remains(name: &str) -> bool {
    let Ok(scm) = open_scm() else {
        return true;
    };
    let w = wide(name);
    match unsafe { OpenServiceW(scm.0, PCWSTR(w.as_ptr()), SERVICE_QUERY_STATUS) } {
        Ok(svc) => {
            drop(Handle(svc));
            true
        }
        Err(error) => error.code() != ERROR_SERVICE_DOES_NOT_EXIST.to_hresult(),
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

/// Updates the two parts of an automatic startup type in a single transaction.
fn write_registry_start_type(name: &str, start: StartType) -> Result<(), ActionError> {
    let key = open_registry_key(
        name,
        KEY_QUERY_VALUE | KEY_SET_VALUE,
        Some(REG_OPTION_OPEN_LINK.0),
    )?;
    let mut kind = REG_DWORD;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            windows::core::w!("SymbolicLinkValue"),
            None,
            Some(&mut kind),
            None,
            None,
        )
    };
    if status != ERROR_FILE_NOT_FOUND {
        status.ok()?;
        if kind == REG_LINK {
            return Err(ActionError::Refused);
        }
    }

    let txn = OwnedHandle(unsafe {
        CreateTransaction(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            PCWSTR::null(),
        )
    }?);
    let mut transacted = HKEY::default();
    // Reopen the checked key itself, without resolving its service name again.
    unsafe {
        RegOpenKeyTransactedW(
            key.0,
            PCWSTR::null(),
            None,
            KEY_SET_VALUE,
            &mut transacted,
            txn.0,
            None,
        )
    }
    .ok()?;
    let transacted = RegKey(transacted);
    let (code, delayed) = start_values(start);
    set_dword(&transacted, "Start", code)?;
    if let Some(delayed) = delayed {
        set_dword(&transacted, "DelayedAutostart", delayed.into())?;
    }
    unsafe { CommitTransaction(txn.0) }?;
    Ok(())
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

fn basic_config(svc: &Handle) -> windows::core::Result<AlignedBuf> {
    // QueryServiceConfigW documents an 8 KiB maximum including its strings.
    let mut buf = AlignedBuf::with_capacity(8192);
    let mut needed = 0;
    unsafe {
        QueryServiceConfigW(
            svc.0,
            Some(buf.as_mut_ptr().cast()),
            buf.len() as u32,
            &mut needed,
        )
    }?;
    Ok(buf)
}

fn registry_start_type_allowed(svc: &Handle, service_type: ENUM_SERVICE_TYPE) -> bool {
    service_type.0 & SERVICE_PKG_SERVICE != 0
        && query_config2(svc, SERVICE_CONFIG_LAUNCH_PROTECTED).is_some_and(|buf| {
            let info = unsafe { &*buf.as_ptr().cast::<SERVICE_LAUNCH_PROTECTED_INFO>() };
            info.dwLaunchProtected == SERVICE_LAUNCH_PROTECTED_NONE
        })
}

fn query_config(scm: &Handle, name: &str) -> Option<Config> {
    let svc = open_service(scm, name, SERVICE_QUERY_CONFIG).ok()?;
    let mut config = Config::default();

    if let Ok(buf) = basic_config(&svc) {
        let c = unsafe { &*buf.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
        config.start_type = registry_start_name(c.dwStartType.0, false);
        config.registry_start_type_allowed = registry_start_type_allowed(&svc, c.dwServiceType);
        config.path = unsafe { pwstr(c.lpBinaryPathName) }.map(Into::into);
        config.account = unsafe { pwstr(c.lpServiceStartName) }.map(Into::into);
    }

    if let Some(buf) = query_config2(&svc, SERVICE_CONFIG_DESCRIPTION) {
        let d = unsafe { &*(buf.as_ptr() as *const SERVICE_DESCRIPTIONW) };
        config.description = unsafe { pwstr(d.lpDescription) }.map(Into::into);
    }
    let key = open_registry_key(name, KEY_READ, None).ok();
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

fn open_registry_key(
    name: &str,
    access: REG_SAM_FLAGS,
    options: Option<u32>,
) -> windows::core::Result<RegKey> {
    let path = wide(&format!(r"SYSTEM\CurrentControlSet\Services\{name}"));
    let mut key = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(path.as_ptr()),
            options,
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
    use super::{Handle, ServiceCatalog, StartType, registry_start_name, remains, start_values};
    use crate::actions::ActionError;
    use crate::util::wide;
    use windows::Win32::System::Services::{
        CreateServiceW, OpenSCManagerW, SC_MANAGER_CONNECT, SC_MANAGER_CREATE_SERVICE,
        SERVICE_DEMAND_START, SERVICE_ERROR_IGNORE, SERVICE_QUERY_STATUS,
        SERVICE_WIN32_OWN_PROCESS,
    };
    use windows::core::PCWSTR;

    /// A name no service has, and none of another test run.
    fn unique(prefix: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.subsec_nanos());
        format!("{prefix}{}x{nanos}", std::process::id())
    }

    #[test]
    fn deleting_a_service_that_is_not_there_says_so() {
        let name = unique("ArgusMissing");
        assert!(matches!(
            ServiceCatalog::default().delete(&name),
            Err(ActionError::NotFound)
        ));
        assert!(!remains(&name));
        // Every Windows has the RPC service.
        assert!(remains("RpcSs"));
    }

    /// With administrator rights (as on CI), a service of the test's own
    /// goes: held open, it stays marked until the last handle closes, and
    /// deleting it again counts as done.
    #[test]
    fn a_service_of_its_own_is_deleted() {
        if !crate::privilege::is_elevated() {
            return;
        }
        let name = unique("ArgusDeleteTest");
        let scm = Handle(
            unsafe {
                OpenSCManagerW(
                    PCWSTR::null(),
                    PCWSTR::null(),
                    SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE,
                )
            }
            .expect("service manager"),
        );
        let (service, binary) = (
            wide(&name),
            wide(r"C:\Windows\System32\svchost.exe -k ArgusNone"),
        );
        // Never started; deleted below, or by the catalog when a check fails.
        let held = Handle(
            unsafe {
                CreateServiceW(
                    scm.0,
                    PCWSTR(service.as_ptr()),
                    PCWSTR(service.as_ptr()),
                    SERVICE_QUERY_STATUS,
                    SERVICE_WIN32_OWN_PROCESS,
                    SERVICE_DEMAND_START,
                    SERVICE_ERROR_IGNORE,
                    PCWSTR(binary.as_ptr()),
                    PCWSTR::null(),
                    None,
                    PCWSTR::null(),
                    PCWSTR::null(),
                    PCWSTR::null(),
                )
            }
            .expect("test service"),
        );
        struct Gone<'a>(&'a str);
        impl Drop for Gone<'_> {
            fn drop(&mut self) {
                let _ = ServiceCatalog::default().delete(self.0);
            }
        }
        let _gone = Gone(&name);
        let catalog = ServiceCatalog::default();
        assert!(matches!(
            catalog.set_start_type_in_registry(&name, StartType::Disabled),
            Err(ActionError::Refused)
        ));
        // Exercise the underlying transaction on this disposable service only.
        super::write_registry_start_type(&name, StartType::AutoDelayed).unwrap();
        let key = super::open_registry_key(&name, super::KEY_READ, None).unwrap();
        assert_eq!(super::read_dword(&key, "Start"), Some(2));
        assert_eq!(super::read_dword(&key, "DelayedAutostart"), Some(1));
        drop(key);
        assert!(catalog.delete(&name).is_ok());
        assert!(remains(&name), "held open, it stays until closed");
        assert!(catalog.delete(&name).is_ok(), "marked already counts");
        drop(held);
        // The manager removes it once every handle is closed, and another
        // process (a scanner on a CI runner) may hold one a moment longer.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while remains(&name) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!remains(&name));
        assert!(matches!(catalog.delete(&name), Err(ActionError::NotFound)));
    }

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
