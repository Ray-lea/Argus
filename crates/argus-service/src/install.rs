use std::fs::{self, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use argus_core::tr;
use serde::Serialize;
use windows::Win32::Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE,
    FILE_SHARE_READ,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Services::*;
use windows::Win32::System::Threading::{
    GetExitCodeProcess, INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW, SHGetKnownFolderPath, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{PCWSTR, PWSTR};

use crate::security;
use crate::util::{FOREIGN_SERVICE, Handle, OTHER_OWNER, Result, io, message, wide, win};

static MANAGING: AtomicBool = AtomicBool::new(false);
const OWNER_FILE: &str = "owner.sid";
const LOCK_FILE: &str = "install.lock";

struct Service(SC_HANDLE);
impl Drop for Service {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub state: &'static str,
    pub authorized: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

pub(crate) fn directory() -> Result<PathBuf> {
    let raw = unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, None) }
        .map_err(win)?;
    let base = unsafe { raw.to_string() }.map_err(|_| 87u32);
    unsafe {
        CoTaskMemFree(Some(raw.0.cast()));
    }
    Ok(PathBuf::from(base?).join(if cfg!(debug_assertions) {
        "Argus Privileged Dev"
    } else {
        "Argus Privileged"
    }))
}

pub(crate) fn owner(dir: &Path) -> Result<String> {
    let path = dir.join(OWNER_FILE);
    let _guard = security::protect_directory(&path)?;
    if fs::metadata(&path).map_err(io)?.len() > 184 {
        return Err(87);
    }
    let sid = fs::read_to_string(path).map_err(io)?;
    security::validate_sid(&sid)?;
    Ok(sid)
}

fn scm(access: u32) -> Result<Service> {
    unsafe { OpenSCManagerW(None, None, access) }
        .map(Service)
        .map_err(win)
}

fn open(manager: &Service, access: u32) -> Result<Option<Service>> {
    match unsafe {
        OpenServiceW(
            manager.0,
            PCWSTR(wide(crate::SERVICE_NAME).as_ptr()),
            access,
        )
    } {
        Ok(h) => Ok(Some(Service(h))),
        Err(e) if e.code() == ERROR_SERVICE_DOES_NOT_EXIST.to_hresult() => Ok(None),
        Err(e) => Err(win(e)),
    }
}

fn query(service: &Service) -> Result<SERVICE_STATUS_PROCESS> {
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0;
    unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            Some(std::slice::from_raw_parts_mut(
                (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
                std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
            )),
            &mut needed,
        )
    }
    .map_err(win)?;
    Ok(status)
}

/// Refuse an unrelated same-name service, even when an administrator invokes
/// the helper. Only versioned copies under our pinned protected folder qualify.
fn binary(service: &Service, dir: &Path) -> Result<PathBuf> {
    let mut needed = 0;
    unsafe {
        let _ = QueryServiceConfigW(service.0, None, 0, &mut needed);
    }
    if needed == 0 || needed > 64 * 1024 {
        return Err(FOREIGN_SERVICE);
    }
    let mut data = vec![0u64; (needed as usize).div_ceil(8)];
    let config = data.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    unsafe { QueryServiceConfigW(service.0, Some(config), needed, &mut needed) }.map_err(win)?;
    let config = unsafe { &*config };
    let command = unsafe { config.lpBinaryPathName.to_string() }.map_err(|_| FOREIGN_SERVICE)?;
    let account = unsafe { config.lpServiceStartName.to_string() }.map_err(|_| FOREIGN_SERVICE)?;
    if config.dwServiceType != SERVICE_WIN32_OWN_PROCESS
        || !account.eq_ignore_ascii_case("LocalSystem")
    {
        return Err(FOREIGN_SERVICE);
    }
    let path = command
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix(&format!("\" {}", crate::SERVICE_ARG)))
        .ok_or(FOREIGN_SERVICE)?;
    let path = PathBuf::from(path);
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or(FOREIGN_SERVICE)?;
    if path.parent() != Some(dir)
        || !filename.starts_with("argus-service-")
        || !filename.ends_with(".exe")
    {
        return Err(FOREIGN_SERVICE);
    }
    Ok(path)
}

pub fn status() -> std::result::Result<ServiceStatus, String> {
    inspect().map_err(message)
}

fn inspect() -> Result<ServiceStatus> {
    let manager = scm(SC_MANAGER_CONNECT)?;
    let Some(service) = open(&manager, SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)? else {
        return Ok(ServiceStatus {
            state: "notInstalled",
            authorized: false,
            version: None,
            error: None,
        });
    };
    let dir = directory()?;
    let _parent = security::protect_directory(dir.parent().ok_or(87u32)?)?;
    let _guard = security::protect_directory(&dir)?;
    binary(&service, &dir)?;
    let authorized = owner(&dir)? == security::current_sid()?;
    let s = query(&service)?;
    let mut status = ServiceStatus {
        state: match s.dwCurrentState {
            SERVICE_RUNNING => "running",
            SERVICE_STOPPED => "stopped",
            SERVICE_START_PENDING => "startPending",
            SERVICE_STOP_PENDING => "stopPending",
            _ => "unavailable",
        },
        authorized,
        version: None,
        error: None,
    };
    if s.dwCurrentState == SERVICE_RUNNING && authorized {
        match crate::pipe::request(crate::PIPE_NAME, s.dwProcessId, crate::Operation::Ping) {
            Ok(crate::Reply::Pong { version, .. }) => status.version = Some(version),
            Ok(_) => status.error = Some(message(crate::util::INCOMPATIBLE)),
            Err(e) => status.error = Some(message(e)),
        }
    } else if !authorized {
        status.error = Some(message(OTHER_OWNER));
    } else if s.dwCurrentState == SERVICE_STOPPED && s.dwWin32ExitCode != 0 {
        status.error = Some(message(s.dwWin32ExitCode));
    }
    Ok(status)
}

pub(crate) fn running_pid() -> Result<Option<u32>> {
    let manager = scm(SC_MANAGER_CONNECT)?;
    let Some(service) = open(&manager, SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)? else {
        return Ok(None);
    };
    let dir = directory()?;
    let _parent = security::protect_directory(dir.parent().ok_or(87u32)?)?;
    let _guard = security::protect_directory(&dir)?;
    binary(&service, &dir)?;
    if owner(&dir)? != security::current_sid()? {
        return Ok(None);
    }
    let s = query(&service)?;
    Ok((s.dwCurrentState == SERVICE_RUNNING).then_some(s.dwProcessId))
}

fn wait(service: &Service, wanted: SERVICE_STATUS_CURRENT_STATE) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let s = query(service)?;
        if s.dwCurrentState == wanted {
            return Ok(());
        }
        if wanted == SERVICE_RUNNING && s.dwCurrentState == SERVICE_STOPPED {
            return Err(if s.dwWin32ExitCode == 0 {
                1067
            } else {
                s.dwWin32ExitCode
            });
        }
        if Instant::now() >= deadline {
            return Err(1460);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn start(service: &Service) -> Result<()> {
    if query(service)?.dwCurrentState == SERVICE_RUNNING {
        return Ok(());
    }
    unsafe { StartServiceW(service.0, None) }.map_err(win)?;
    wait(service, SERVICE_RUNNING)
}

fn stop(service: &Service) -> Result<()> {
    let status = query(service)?;
    // SERVICE_STOPPED is reported before ServiceMain returns. Keep the process
    // handle and wait for exit as well before replacing/deleting its image.
    let process = if status.dwProcessId != 0 {
        match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, status.dwProcessId) } {
            Ok(h) => Some(Handle(h)),
            Err(e)
                if e.code() == windows::Win32::Foundation::ERROR_INVALID_PARAMETER.to_hresult() =>
            {
                None
            }
            Err(e) => return Err(win(e)),
        }
    } else {
        None
    };
    match query(service)?.dwCurrentState {
        SERVICE_STOPPED => {}
        SERVICE_STOP_PENDING => {}
        _ => {
            let mut s = SERVICE_STATUS::default();
            unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut s) }.map_err(win)?;
        }
    }
    wait(service, SERVICE_STOPPED)?;
    if let Some(process) = process
        && unsafe { WaitForSingleObject(process.0, 30_000) } != WAIT_OBJECT_0
    {
        return Err(1460);
    }
    Ok(())
}

fn command(path: &Path) -> Vec<u16> {
    wide(&format!("\"{}\" {}", path.display(), crate::SERVICE_ARG))
}

fn set_binary(service: &Service, path: &Path) -> Result<()> {
    unsafe {
        ChangeServiceConfigW(
            service.0,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_START_TYPE(SERVICE_NO_CHANGE),
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR(command(path).as_ptr()),
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }
    .map_err(win)
}

fn copy_binary(dir: &Path) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| 87u32)?
        .as_nanos();
    let path = dir.join(format!("argus-service-{}-{stamp}.exe", std::process::id()));
    let mut source = fs::File::open(std::env::current_exe().map_err(io)?).map_err(io)?;
    let mut dest = new_protected_file(&path)?;
    let result = std::io::copy(&mut source, &mut dest).and_then(|_| dest.sync_all());
    drop(dest);
    if let Err(e) = result {
        let _ = fs::remove_file(&path);
        return Err(io(e));
    }
    Ok(path)
}

pub(crate) fn new_protected_file(path: &Path) -> Result<fs::File> {
    let sd = security::directory_descriptor()?;
    let h = unsafe {
        CreateFileW(
            PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
            FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ,
            Some(&sd.attributes()),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(win)?;
    Ok(unsafe { fs::File::from_raw_handle(h.0) })
}

fn install_or_update(
    manager: &Service,
    existing: Option<&Service>,
    dir: &Path,
    sid: &str,
) -> Result<()> {
    let old = existing.map(|s| binary(s, dir)).transpose()?;
    let was_running = existing
        .map(|s| query(s).map(|q| q.dwCurrentState == SERVICE_RUNNING))
        .transpose()?
        .unwrap_or(false);
    if dir.join(OWNER_FILE).exists() && owner(dir)? != sid {
        return Err(OTHER_OWNER);
    }
    let new = copy_binary(dir)?;
    let result = (|| {
        if let Some(service) = existing {
            stop(service)?;
            if let Err(e) = set_binary(service, &new).and_then(|_| start(service)) {
                // Do not delete the new binary unless rollback actually succeeds.
                stop(service)?;
                set_binary(service, old.as_ref().ok_or(FOREIGN_SERVICE)?)?;
                if was_running {
                    start(service)?;
                }
                return Err(e);
            }
        } else {
            if !dir.join(OWNER_FILE).exists() {
                use std::io::Write;
                let mut file = new_protected_file(&dir.join(OWNER_FILE))?;
                file.write_all(sid.as_bytes())
                    .and_then(|_| file.sync_all())
                    .map_err(io)?;
            }
            let service = Service(
                unsafe {
                    CreateServiceW(
                        manager.0,
                        PCWSTR(wide(crate::SERVICE_NAME).as_ptr()),
                        PCWSTR(
                            wide(&if cfg!(debug_assertions) {
                                tr!("service.displayNameDev")
                            } else {
                                tr!("service.displayName")
                            })
                            .as_ptr(),
                        ),
                        SERVICE_ALL_ACCESS,
                        SERVICE_WIN32_OWN_PROCESS,
                        SERVICE_AUTO_START,
                        SERVICE_ERROR_NORMAL,
                        PCWSTR(command(&new).as_ptr()),
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                }
                .map_err(win)?,
            );
            let mut desc_text = wide(&tr!("service.description"));
            let desc = SERVICE_DESCRIPTIONW {
                lpDescription: PWSTR(desc_text.as_mut_ptr()),
            };
            unsafe {
                let _ = ChangeServiceConfig2W(
                    service.0,
                    SERVICE_CONFIG_DESCRIPTION,
                    Some((&desc as *const SERVICE_DESCRIPTIONW).cast()),
                );
            }
            if let Err(e) = start(&service) {
                stop(&service)?;
                unsafe { DeleteService(service.0) }.map_err(win)?;
                return Err(e);
            }
        }
        Ok(())
    })();
    if result.is_ok() {
        if let Some(old) = old {
            let _ = fs::remove_file(old);
        }
    } else {
        // Preserve whichever binary the SCM still references after a failed
        // rollback. A failed repair must not make the service unbootable.
        let referenced = open(manager, SERVICE_QUERY_CONFIG)?.and_then(|s| binary(&s, dir).ok());
        if referenced.as_ref() != Some(&new) {
            let _ = fs::remove_file(new);
        }
        if referenced.is_none() {
            let _ = fs::remove_file(dir.join(OWNER_FILE));
        }
    }
    result
}

pub(crate) fn run_helper(action: &str, sid: &str) -> Result<()> {
    if !matches!(action, "install" | "update" | "start" | "uninstall") {
        return Err(87);
    }
    // Uninstalling authorizes nobody, and an uninstaller may run as SYSTEM
    // (deployment tools do), whose SID is not a user account's.
    if action != "uninstall" {
        security::validate_sid(sid)?;
    }
    if !argus_core::privilege::is_elevated() {
        return Err(5);
    }
    let manager = scm(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)?;
    let existing = open(&manager, SERVICE_ALL_ACCESS)?;
    let dir = directory()?;
    let _parent = security::protect_directory(dir.parent().ok_or(87u32)?)?;
    if action == "install" {
        if existing.is_some() {
            return Err(1073);
        }
        let sd = security::directory_descriptor()?;
        if let Err(e) = unsafe {
            CreateDirectoryW(
                PCWSTR(wide(&dir.to_string_lossy()).as_ptr()),
                Some(&sd.attributes()),
            )
        } {
            let code = win(e);
            if code != 183 {
                return Err(code);
            }
        }
    }
    let guard = security::protect_directory(&dir)?;
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(dir.join(LOCK_FILE))
        .map_err(io)?;
    let result = match action {
        "install" | "update" => {
            if action == "update" && existing.is_none() {
                return Err(1060);
            }
            // Never retire service/recovery files while IFEO still owns them.
            crate::taskmgr::restore_for_maintenance(&dir).map_err(|_| 0x2000_0005u32)?;
            install_or_update(&manager, existing.as_ref(), &dir, sid)
        }
        "start" => {
            let s = existing.as_ref().ok_or(1060u32)?;
            binary(s, &dir)?;
            if owner(&dir)? != sid {
                return Err(OTHER_OWNER);
            }
            start(s)
        }
        "uninstall" => {
            crate::taskmgr::restore_for_maintenance(&dir).map_err(|_| 0x2000_0005u32)?;
            let s = existing.as_ref().ok_or(1060u32)?;
            let exe = binary(s, &dir)?;
            stop(s)?;
            unsafe { DeleteService(s.0) }.map_err(win)?;
            drop(existing);
            fs::remove_file(exe).map_err(io)?;
            fs::remove_file(dir.join(OWNER_FILE)).map_err(io)
        }
        _ => unreachable!(),
    };
    drop(lock);
    drop(guard);
    if action == "uninstall" && result.is_ok() {
        fs::remove_file(dir.join(LOCK_FILE)).map_err(io)?;
        // Never recursively delete: preserve any unrelated or leftover files.
        match fs::remove_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(145) => {}
            Err(e) => return Err(io(e)),
        }
    }
    result
}

/// UAC belongs to this explicit settings action, never a background retry.
pub fn manage(action: &str) -> std::result::Result<ServiceStatus, String> {
    if !matches!(action, "install" | "update" | "start" | "uninstall") {
        return Err(message(87));
    }
    if MANAGING.swap(true, Ordering::AcqRel) {
        return Err(tr!("service.busy"));
    }
    struct Busy;
    impl Drop for Busy {
        fn drop(&mut self) {
            MANAGING.store(false, Ordering::Release);
        }
    }
    let _busy = Busy;
    let sid = security::current_sid().map_err(message)?;
    if argus_core::privilege::is_elevated() {
        run_helper(action, &sid).map_err(message)?;
    } else {
        let exe = wide(
            &std::env::current_exe()
                .map_err(|e| e.to_string())?
                .to_string_lossy(),
        );
        let parameters = wide(&format!("{} {action} {sid}", crate::INSTALL_ARG));
        let verb = wide("runas");
        let working = directory()
            .map_err(message)?
            .parent()
            .ok_or_else(|| message(87))?
            .to_path_buf();
        let working = wide(&working.to_string_lossy());
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(exe.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            lpDirectory: PCWSTR(working.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.map_err(|e| message(win(e)))?;
        if info.hProcess == HANDLE::default() {
            return Err(message(6));
        }
        let process = Handle(info.hProcess);
        if unsafe { WaitForSingleObject(process.0, INFINITE) } != WAIT_OBJECT_0 {
            return Err(message(6));
        }
        let mut exit = 0;
        unsafe { GetExitCodeProcess(process.0, &mut exit) }.map_err(|e| message(win(e)))?;
        if exit != 0 {
            return Err(message(exit));
        }
    }
    status()
}
