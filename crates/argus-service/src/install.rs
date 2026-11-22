use std::fs::{self, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use argus_core::tr;
use serde::Serialize;
use windows::Win32::Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE,
    FILE_SHARE_READ,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, REG_DWORD, REG_VALUE_TYPE, RegCloseKey, RegFlushKey,
    RegOpenKeyExW, RegQueryValueExW,
};
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
use crate::util::{
    DAMAGED, FOREIGN_SERVICE, Handle, OTHER_OWNER, Result, TASK_MANAGER_FOLLOW, UNSAFE_PATH, io,
    message, wide, win,
};

static MANAGING: AtomicBool = AtomicBool::new(false);
const OWNER_FILE: &str = "owner.sid";
const LOCK_FILE: &str = "install.lock";
/// The service's executable, which the registration names. Argus 0.5 and
/// earlier gave every copy a name of its own (`legacy_image_name`).
pub(crate) const IMAGE: &str = "argus-service.exe";
const STEM: &str = "argus-service";
/// A new executable, written in full before it takes `IMAGE`'s place.
const STAGED: &str = "argus-service.new.exe";
/// `IMAGE` as an update found it, until the new one has started: what the
/// update goes back to when it does not. `layout::aside` names it so.
const BACKUP: &str = "argus-service.old.exe";
/// Where the SCM keeps its database.
const SERVICES_KEY: &str = r"SYSTEM\CurrentControlSet\Services";
/// What `manage` and the elevated helper accept.
const ACTIONS: [&str; 5] = ["install", "update", "start", "uninstall", "repair"];

#[derive(Default)]
struct StoppedFailure(Option<(u32, u32)>);

impl StoppedFailure {
    fn observe(&mut self, status: Option<&SERVICE_STATUS_PROCESS>) -> Option<(u32, u32)> {
        // 1077 is the normal initial status of a service that has never run.
        let failure = status
            .filter(|s| {
                s.dwCurrentState == SERVICE_STOPPED && !matches!(s.dwWin32ExitCode, 0 | 1077)
            })
            .map(|s| (s.dwWin32ExitCode, s.dwServiceSpecificExitCode));
        let changed = std::mem::replace(&mut self.0, failure) != failure;
        failure.filter(|_| changed)
    }
}

fn note_state(status: Option<&SERVICE_STATUS_PROCESS>) {
    static STOPPED: Mutex<StoppedFailure> = Mutex::new(StoppedFailure(None));
    // SCM retains the last exit code until the next start. Polling that same
    // stopped state is not another failure; recovery re-arms the observation.
    let failure = STOPPED.lock().unwrap().observe(status);
    if let Some((code, specific)) = failure {
        argus_core::diagnostics::warn(
            "privileged service stopped",
            format!(
                "{}; service_specific_exit={specific}",
                argus_core::diagnostics::code(code),
            ),
        );
    }
}

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
    /// `notInstalled`, `running`, `stopped`, `startPending`, `stopPending`;
    /// `broken`: registered, but part of the installation is gone (`damage`);
    /// `deletePending`: deleted, the registration going with its last handle;
    /// `unavailable`: nothing can be said or trusted (`error`).
    pub state: &'static str,
    pub authorized: bool,
    pub version: Option<String>,
    pub error: Option<String>,
    /// The running service speaks another protocol (`error` says so too):
    /// updating it is what reconnects, unlike a failure that may pass.
    pub incompatible: bool,
    /// What a `broken` installation lacks.
    pub damage: Option<Damage>,
    /// The action that brings a `broken` installation back, where this
    /// account may run one: `update` when the authorization record still
    /// names it, `repair` when the record has to be written anew, which is
    /// confirmed as an installation is. None when the record names another
    /// account: only removing is left.
    pub repair: Option<&'static str>,
    /// The authorization record was read and names another account.
    #[serde(skip)]
    pub(crate) other_owner: bool,
    #[serde(skip)]
    pub(crate) error_code: Option<u32>,
}

impl ServiceStatus {
    pub(crate) fn new(state: &'static str) -> Self {
        Self {
            state,
            authorized: false,
            version: None,
            error: None,
            incompatible: false,
            damage: None,
            repair: None,
            other_owner: false,
            error_code: None,
        }
    }

    fn failed(state: &'static str, code: u32) -> Self {
        Self {
            error: Some(message(code)),
            error_code: Some(code),
            ..Self::new(state)
        }
    }

    /// Whether the service is another account's. False where nothing says
    /// whose it is (a missing folder or record), which `damage` reports.
    pub fn other_owner(&self) -> bool {
        self.other_owner
    }
}

/// What a registered service lacks of its files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Damage {
    /// The service folder.
    Directory,
    /// The authorization record: absent, or not an account's SID.
    Owner,
    /// The executable the registration names.
    Image,
    OwnerAndImage,
}

impl Damage {
    /// For the diagnostic log, in fixed words.
    pub fn log(self) -> &'static str {
        match self {
            Self::Directory => "service folder missing",
            Self::Owner => "authorization record missing or unreadable",
            Self::Image => "registered image missing",
            Self::OwnerAndImage => "registered image and authorization record missing",
        }
    }

    fn text(self) -> String {
        match self {
            Self::Directory => tr!("service.damaged.directory"),
            Self::Owner => tr!("service.damaged.owner"),
            Self::Image => tr!("service.damaged.image"),
            Self::OwnerAndImage => tr!("service.damaged.ownerAndImage"),
        }
    }
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
    read_owner(&path)
}

fn read_owner(path: &Path) -> Result<String> {
    if fs::metadata(path).map_err(io)?.len() > 184 {
        return Err(87);
    }
    let sid = fs::read_to_string(path).map_err(io)?;
    security::validate_sid(&sid)?;
    Ok(sid)
}

/// Opens a path of the installation for a check, as
/// `security::protect_present` does. The tests examine folders of their own,
/// which that refuses.
type Pin<'a> = &'a dyn Fn(&Path) -> Result<Handle>;

/// The authorization record, `owner.sid`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Record {
    /// The account it authorizes.
    Account(String),
    /// Absent, or not an account's SID; the code says which. A crash or a
    /// removal that was cut short leaves this behind.
    Unusable(u32),
    /// Its own owner, permissions or link are not safe: it is not read, and
    /// nothing here replaces it.
    Unsafe(u32),
}

fn record(dir: &Path, pin: Pin) -> Record {
    let path = dir.join(OWNER_FILE);
    let _guard = match pin(&path) {
        Ok(guard) => guard,
        Err(code @ (2 | 3)) => return Record::Unusable(code),
        Err(code) => return Record::Unsafe(code),
    };
    match read_owner(&path) {
        Ok(sid) => Record::Account(sid),
        Err(code) => Record::Unusable(code),
    }
}

/// The code with which the registered image is not there, if it is not.
fn image_missing(image: &Path) -> Option<u32> {
    match fs::symlink_metadata(image).map_err(io) {
        Err(code @ (2 | 3)) => Some(code),
        _ => None,
    }
}

/// What a registered service has of its files.
#[derive(Debug, PartialEq, Eq)]
enum Health {
    /// All of them; the account the record authorizes.
    Whole(String),
    /// The folder or the record cannot be trusted (owner, permissions, link)
    /// or examined: nothing in it is read. Removing stays possible.
    Unsafe(u32),
    Damaged(Finding),
}

#[derive(Debug, PartialEq, Eq)]
struct Finding {
    damage: Damage,
    /// The code of the check that found it.
    code: u32,
    /// The account the record authorizes, where the record is intact.
    owner: Option<String>,
}

/// Examines the folder, the record and the registered image. The handles pin
/// the folder for as long as the caller keeps them.
fn examine(dir: &Path, image: &Path, pin: Pin) -> (Option<(Handle, Handle)>, Health) {
    let Some(parent) = dir.parent() else {
        return (None, Health::Unsafe(87));
    };
    let parent = match pin(parent) {
        Ok(parent) => parent,
        Err(code) => return (None, Health::Unsafe(code)),
    };
    let folder = match pin(dir) {
        Ok(folder) => folder,
        Err(code @ (2 | 3)) => {
            let finding = Finding {
                damage: Damage::Directory,
                code,
                owner: None,
            };
            return (None, Health::Damaged(finding));
        }
        Err(code) => return (None, Health::Unsafe(code)),
    };
    let health = match (record(dir, pin), image_missing(image)) {
        (Record::Unsafe(code), _) => Health::Unsafe(code),
        (Record::Account(owner), None) => Health::Whole(owner),
        (Record::Account(owner), Some(code)) => Health::Damaged(Finding {
            damage: Damage::Image,
            code,
            owner: Some(owner),
        }),
        (Record::Unusable(code), image) => Health::Damaged(Finding {
            damage: if image.is_some() {
                Damage::OwnerAndImage
            } else {
                Damage::Owner
            },
            code,
            owner: None,
        }),
    };
    (Some((parent, folder)), health)
}

struct RegKey(HKEY);
impl Drop for RegKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn registry_key(root: HKEY, path: &str) -> Result<RegKey> {
    let mut key = HKEY::default();
    let code = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(wide(path).as_ptr()),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if code.0 != 0 {
        return Err(code.0);
    }
    Ok(RegKey(key))
}

/// Writes the registry hive that holds `path` to disk.
pub(crate) fn flush_key(root: HKEY, path: &str) -> Result<()> {
    let key = registry_key(root, path)?;
    match unsafe { RegFlushKey(key.0) }.0 {
        0 => Ok(()),
        code => Err(code),
    }
}

/// Whether the SCM has marked the service registered at `path` for deletion
/// (`DeleteFlag`): another program's handle keeps the registration until it
/// closes, and the SCM drops it when Windows starts at the latest.
fn marked_for_deletion(root: HKEY, path: &str) -> bool {
    let Ok(key) = registry_key(root, path) else {
        return false;
    };
    let mut kind = REG_VALUE_TYPE::default();
    let mut flag = 0u32;
    let mut size = size_of::<u32>() as u32;
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            windows::core::w!("DeleteFlag"),
            None,
            Some(&mut kind),
            Some((&raw mut flag).cast()),
            Some(&mut size),
        )
    };
    code.0 == 0 && kind == REG_DWORD && size == size_of::<u32>() as u32 && flag != 0
}

fn scm(access: u32) -> Result<Service> {
    crate::util::stage(
        "service OpenSCManagerW",
        unsafe { OpenSCManagerW(None, None, access) }
            .map(Service)
            .map_err(win),
    )
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
        Err(e) if e.code() == ERROR_SERVICE_DOES_NOT_EXIST.to_hresult() => {
            note_state(None);
            Ok(None)
        }
        Err(e) => crate::util::stage("service OpenServiceW", Err(win(e))),
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
    .map_err(win)
    .inspect_err(|code| {
        argus_core::diagnostics::warn(
            "service QueryServiceStatusEx",
            argus_core::diagnostics::code(*code),
        )
    })?;
    note_state(Some(&status));
    Ok(status)
}

/// Refuse an unrelated same-name service, even when an administrator invokes
/// the helper. Only the service's executable in our pinned protected folder
/// qualifies, under its name or one an earlier version gave it.
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
    let named = filename.eq_ignore_ascii_case(IMAGE)
        || (filename.starts_with("argus-service-") && filename.ends_with(".exe"));
    if path.parent() != Some(dir) || !named {
        return Err(FOREIGN_SERVICE);
    }
    Ok(path)
}

pub fn status() -> std::result::Result<ServiceStatus, String> {
    crate::util::stage("privileged service status", inspect()).map_err(message)
}

/// Whether a service of this name is registered and stays so, whatever it
/// has left of its files: one marked for deletion is on its way out.
pub(crate) fn is_registered() -> bool {
    scm(SC_MANAGER_CONNECT)
        .and_then(|manager| open(&manager, SERVICE_QUERY_STATUS))
        .is_ok_and(|service| service.is_some())
        && !marked_for_deletion(HKEY_LOCAL_MACHINE, &registration())
}

/// The registry key of the service's registration.
fn registration() -> String {
    format!(r"{SERVICES_KEY}\{}", crate::SERVICE_NAME)
}

fn inspect() -> Result<ServiceStatus> {
    let manager = scm(SC_MANAGER_CONNECT)?;
    let Some(service) = open(&manager, SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)? else {
        return Ok(ServiceStatus::new("notInstalled"));
    };
    let dir = directory()?;
    let image = match binary(&service, &dir) {
        Ok(image) => image,
        // Another program's service of this name, or a registration that no
        // longer reads as this one: nothing of it is Argus's to manage.
        Err(code) => return Ok(ServiceStatus::failed("unavailable", code)),
    };
    // A removal deletes the files once the deletion is on disk, while another
    // program's handle may still keep the registration: that is no damage.
    if marked_for_deletion(HKEY_LOCAL_MACHINE, &registration()) {
        return Ok(ServiceStatus::new("deletePending"));
    }
    let (_pins, health) = examine(&dir, &image, &security::protect_present);
    if let Health::Damaged(finding) = &health {
        let lacking = match finding.damage {
            Damage::Directory => dir.clone(),
            Damage::Image => image.clone(),
            Damage::Owner | Damage::OwnerAndImage => dir.join(OWNER_FILE),
        };
        argus_core::diagnostics::warn(
            "privileged service installation damaged",
            format!(
                "{}; {}; path={}",
                finding.damage.log(),
                argus_core::diagnostics::code(finding.code),
                crate::util::log_path(&lacking)
            ),
        );
    }
    let s = query(&service)?;
    Ok(describe(health, &s, &security::current_sid()?, |pid| {
        crate::pipe::request(crate::PIPE_NAME, pid, crate::Operation::Ping)
    }))
}

/// The status of a registered service of ours for `account`, from what
/// `examine` found and the SCM reports. `ping` asks the running service of
/// this account for its version.
fn describe(
    health: Health,
    s: &SERVICE_STATUS_PROCESS,
    account: &str,
    ping: impl FnOnce(u32) -> Result<crate::Reply>,
) -> ServiceStatus {
    let owner = match health {
        Health::Whole(owner) => owner,
        Health::Unsafe(code) => return ServiceStatus::failed("unavailable", code),
        Health::Damaged(finding) => return broken(finding, account),
    };
    let authorized = owner == account;
    let mut status = ServiceStatus {
        authorized,
        other_owner: !authorized,
        ..ServiceStatus::new(match s.dwCurrentState {
            SERVICE_RUNNING => "running",
            SERVICE_STOPPED => "stopped",
            SERVICE_START_PENDING => "startPending",
            SERVICE_STOP_PENDING => "stopPending",
            _ => "unavailable",
        })
    };
    if s.dwCurrentState == SERVICE_RUNNING && authorized {
        note_ping(&mut status, ping(s.dwProcessId));
    } else if !authorized {
        status.error_code = Some(OTHER_OWNER);
        status.error = Some(message(OTHER_OWNER));
    } else if s.dwCurrentState == SERVICE_STOPPED && !matches!(s.dwWin32ExitCode, 0 | 1077) {
        status.error_code = Some(s.dwWin32ExitCode);
        status.error = Some(message(s.dwWin32ExitCode));
    }
    status
}

/// A registered service that lacks part of its files. It cannot start, or
/// cannot be shown to be this account's, whatever the SCM says of it.
fn broken(finding: Finding, account: &str) -> ServiceStatus {
    let authorized = finding.owner.as_deref() == Some(account);
    let other_owner = finding.owner.is_some() && !authorized;
    let text = finding.damage.text();
    ServiceStatus {
        authorized,
        other_owner,
        damage: Some(finding.damage),
        repair: match finding.owner {
            Some(_) if authorized => Some("update"),
            Some(_) => None,
            None => Some("repair"),
        },
        error: Some(if other_owner {
            tr!("app.join", first = text, second = message(OTHER_OWNER))
        } else {
            text
        }),
        error_code: Some(finding.code),
        ..ServiceStatus::new("broken")
    }
}

/// Records what a Ping of the running service of this account answered.
fn note_ping(status: &mut ServiceStatus, ping: Result<crate::Reply>) {
    let code = match ping {
        Ok(crate::Reply::Pong { version, .. }) => {
            status.version = Some(version);
            return;
        }
        // Only a service of another protocol answers a Ping otherwise.
        Ok(_) => crate::util::INCOMPATIBLE,
        Err(e) => e,
    };
    status.incompatible = code == crate::util::INCOMPATIBLE;
    status.error_code = Some(code);
    status.error = Some(message(code));
}

pub(crate) fn running_pid() -> Result<Option<u32>> {
    let manager = scm(SC_MANAGER_CONNECT)?;
    let Some(service) = open(&manager, SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)? else {
        return Ok(None);
    };
    let s = query(&service)?;
    if s.dwCurrentState != SERVICE_RUNNING {
        return Ok(None);
    }
    let dir = directory()?;
    let _parent = security::protect_directory(dir.parent().ok_or(87u32)?)?;
    let _guard = security::protect_directory(&dir)?;
    binary(&service, &dir)?;
    match record(&dir, &security::protect_present) {
        Record::Account(owner) => Ok((owner == security::current_sid()?).then_some(s.dwProcessId)),
        // A service whose record is gone cannot be shown to be this
        // account's: as absent as another account's. `status` reports it.
        Record::Unusable(_) => Ok(None),
        Record::Unsafe(code) => Err(code),
    }
}

fn wait(service: &Service, wanted: SERVICE_STATUS_CURRENT_STATE) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let s = query(service)?;
        if s.dwCurrentState == wanted {
            return Ok(());
        }
        if wanted == SERVICE_RUNNING && s.dwCurrentState == SERVICE_STOPPED {
            argus_core::diagnostics::warn(
                "service stopped while starting",
                format!(
                    "state={}; win32_exit={}; service_specific_exit={}; checkpoint={}; wait_hint={}",
                    s.dwCurrentState.0,
                    s.dwWin32ExitCode,
                    s.dwServiceSpecificExitCode,
                    s.dwCheckPoint,
                    s.dwWaitHint
                ),
            );
            return Err(if s.dwWin32ExitCode == 0 {
                1067
            } else {
                s.dwWin32ExitCode
            });
        }
        if Instant::now() >= deadline {
            argus_core::diagnostics::warn(
                "service state transition timed out",
                format!(
                    "wanted={}; actual={}; win32_exit={}; service_specific_exit={}; checkpoint={}; wait_hint={}",
                    wanted.0,
                    s.dwCurrentState.0,
                    s.dwWin32ExitCode,
                    s.dwServiceSpecificExitCode,
                    s.dwCheckPoint,
                    s.dwWaitHint
                ),
            );
            return Err(1460);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn start(service: &Service) -> Result<()> {
    if query(service)?.dwCurrentState == SERVICE_RUNNING {
        return Ok(());
    }
    crate::util::stage(
        "service StartServiceW",
        unsafe { StartServiceW(service.0, None) }.map_err(win),
    )?;
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
            crate::util::stage(
                "service ControlService STOP",
                unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut s) }.map_err(win),
            )?;
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
    crate::util::file_stage(
        "service ChangeServiceConfigW",
        path,
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
        .map_err(win),
    )
}

/// Writes this executable in full as `path`, in place of a file left there.
fn write_binary(path: &Path) -> Result<()> {
    crate::util::file_stage("service executable copy", path, write_binary_inner(path))
}

fn write_binary_inner(path: &Path) -> Result<()> {
    remove_file(path)?;
    let source_path = std::env::current_exe().map_err(io)?;
    let mut source = crate::util::file_stage(
        "service source executable open",
        &source_path,
        fs::File::open(&source_path).map_err(io),
    )?;
    let mut dest = new_protected_file(path)?;
    let result = std::io::copy(&mut source, &mut dest).and_then(|_| dest.sync_all());
    drop(dest);
    if let Err(e) = result {
        argus_core::diagnostics::file_failure("service executable copy and flush", path, &e);
        let _ = fs::remove_file(path);
        return Err(io(e));
    }
    Ok(())
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
    .map_err(win)
    .inspect_err(|code| {
        argus_core::diagnostics::warn(
            "protected file CreateFileW",
            format!(
                "{}; path={}",
                argus_core::diagnostics::code(*code),
                argus_core::diagnostics::path(path)
            ),
        )
    })?;
    Ok(unsafe { fs::File::from_raw_handle(h.0) })
}

/// The service's executables in its folder: `IMAGE`, a new one on its way
/// there (`STAGED`), the one it replaced (`BACKUP`), and copies of Argus 0.5
/// and earlier.
fn image_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(IMAGE)
        || name.eq_ignore_ascii_case(STAGED)
        || crate::layout::is_aside(STEM, name)
        || legacy_image_name(name)
}

/// The names Argus 0.5 and earlier gave the service's copies:
/// argus-service-<process>-<time>.exe.
fn legacy_image_name(name: &str) -> bool {
    name.strip_prefix("argus-service-")
        .and_then(|rest| rest.strip_suffix(".exe"))
        .and_then(|stamp| stamp.split_once('-'))
        .is_some_and(|(process, time)| {
            [process, time]
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// What installing, updating and removing the service do to the machine.
/// `Machine` does it; the tests keep the registration and the files in
/// memory, refuse steps and look at what a crash after each one would leave.
trait Steps {
    /// The image the registration names; None without a registration.
    fn registered(&mut self) -> Result<Option<PathBuf>>;
    fn running(&mut self) -> Result<bool>;
    fn stop(&mut self) -> Result<()>;
    fn start(&mut self) -> Result<()>;
    /// Registers the service, naming `image`.
    fn create(&mut self, image: &Path) -> Result<()>;
    /// Makes the registration name `image`.
    fn point(&mut self, image: &Path) -> Result<()>;
    /// Deletes the registration. It may outlast the call, marked for
    /// deletion, while another program holds a handle to it.
    fn delete(&mut self) -> Result<()>;
    /// Writes the registration, or its deletion, to disk.
    fn flush(&mut self) -> Result<()>;
    /// The path of `name` in the service folder.
    fn path(&self, name: &str) -> PathBuf;
    /// Writes this executable in full as `path`, in place of a file there.
    fn write(&mut self, path: &Path) -> Result<()>;
    /// Gives the file `from` names the name `to` as well, in place of a file
    /// there.
    fn link(&mut self, from: &Path, to: &Path) -> Result<()>;
    /// Renames `from` to `to`, in place of a file there, in one step: `to`
    /// names a whole file throughout, the one before or `from`'s.
    fn rename(&mut self, from: &Path, to: &Path) -> Result<()>;
    fn present(&mut self, image: &Path) -> bool;
    /// The service executables in the folder.
    fn images(&mut self) -> Vec<PathBuf>;
    fn remove(&mut self, image: &Path) -> Result<()>;
    fn record(&mut self) -> Record;
    /// Writes the authorization record for `sid`, in place of any other.
    fn authorize(&mut self, sid: &str) -> Result<()>;
    fn forget(&mut self) -> Result<()>;
}

/// The service's registration and its trusted, pinned folder. A removal
/// uses the registration only: it deletes files through `cleanup`, which
/// needs no trust in the folder.
struct Machine<'a> {
    manager: &'a Service,
    /// The registration, opened with every access; None before the service
    /// is created and once it is deleted.
    service: Option<Service>,
    dir: &'a Path,
}

impl Machine<'_> {
    fn service(&self) -> Result<&Service> {
        self.service.as_ref().ok_or(1060)
    }
}

impl Steps for Machine<'_> {
    fn registered(&mut self) -> Result<Option<PathBuf>> {
        // Asked of the SCM by name: a deleted registration that another
        // program's handle keeps is still one.
        open(self.manager, SERVICE_QUERY_CONFIG)?
            .map(|service| binary(&service, self.dir))
            .transpose()
    }

    fn running(&mut self) -> Result<bool> {
        Ok(query(self.service()?)?.dwCurrentState == SERVICE_RUNNING)
    }

    fn stop(&mut self) -> Result<()> {
        stop(self.service()?)
    }

    fn start(&mut self) -> Result<()> {
        start(self.service()?)
    }

    fn create(&mut self, image: &Path) -> Result<()> {
        let service = Service(
            unsafe {
                CreateServiceW(
                    self.manager.0,
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
                    PCWSTR(command(image).as_ptr()),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .map_err(win)
            .inspect_err(|code| {
                argus_core::diagnostics::warn(
                    "service CreateServiceW",
                    argus_core::diagnostics::code(*code),
                )
            })?,
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
        self.service = Some(service);
        Ok(())
    }

    fn point(&mut self, image: &Path) -> Result<()> {
        set_binary(self.service()?, image)
    }

    fn delete(&mut self) -> Result<()> {
        let service = self.service()?;
        match unsafe { DeleteService(service.0) }.map_err(win) {
            // 1072: an earlier removal marked it, and a handle still keeps it.
            Ok(()) | Err(1072) => {}
            Err(code) => return crate::util::stage("service DeleteService", Err(code)),
        }
        // The SCM drops the registration when its last handle closes.
        self.service = None;
        let deadline = Instant::now() + Duration::from_secs(3);
        while !matches!(open(self.manager, SERVICE_QUERY_STATUS), Ok(None)) {
            if Instant::now() >= deadline {
                argus_core::diagnostics::warn(
                    "service registration removal pending",
                    "marked for deletion; another handle keeps it",
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        crate::util::stage(
            "service registration RegFlushKey",
            flush_key(HKEY_LOCAL_MACHINE, SERVICES_KEY),
        )
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn write(&mut self, path: &Path) -> Result<()> {
        write_binary(path)
    }

    fn link(&mut self, from: &Path, to: &Path) -> Result<()> {
        remove_file(to)?;
        crate::util::file_stage(
            "service executable backup link",
            to,
            fs::hard_link(from, to).map_err(io),
        )
    }

    fn rename(&mut self, from: &Path, to: &Path) -> Result<()> {
        crate::util::file_stage(
            "service executable rename",
            to,
            crate::layout::rename(from, to),
        )
    }

    fn present(&mut self, image: &Path) -> bool {
        image_missing(image).is_none()
    }

    fn images(&mut self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| entry.file_name().to_str().is_some_and(image_name))
            .map(|entry| entry.path())
            .collect()
    }

    fn remove(&mut self, image: &Path) -> Result<()> {
        remove_file(image)
    }

    fn record(&mut self) -> Record {
        record(self.dir, &security::protect_present)
    }

    fn authorize(&mut self, sid: &str) -> Result<()> {
        use std::io::Write;
        let path = self.dir.join(OWNER_FILE);
        remove_file(&path)?;
        let mut file = new_protected_file(&path)?;
        file.write_all(sid.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(io)
    }

    fn forget(&mut self) -> Result<()> {
        remove_file(&self.dir.join(OWNER_FILE))
    }
}

/// Deletes a file of the pinned service folder; one already gone is fine.
fn remove_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            argus_core::diagnostics::file_failure("service file removal", path, &error);
            Err(io(error))
        }
        _ => Ok(()),
    }
}

/// Installs the service for `sid`, or brings a registered one to this
/// executable.
///
/// The registration names `IMAGE`, which an update replaces in place
/// (`swap`). One of Argus 0.5 or earlier names a copy of its own: the update
/// writes `IMAGE` beside it and changes the registration once (`change`).
///
/// The SCM changes its database in the registry at once, but the registry
/// reaches the disk lazily: a crash can bring back a registration from before
/// the last change. So an image is deleted only once a registration that does
/// not name it has been written to disk (`settle`); until then every image a
/// registration named stays, and the service can start whatever comes back.
/// `IMAGE` itself is only ever replaced in one step, never deleted.
fn install_or_update(system: &mut impl Steps, sid: &str) -> Result<()> {
    let old = system.registered()?;
    let was_running = old.is_some() && system.running()?;
    let authorize = match (system.record(), &old) {
        (Record::Unsafe(code), _) => return Err(code),
        (Record::Account(owner), _) if owner == sid => false,
        // A registered service stays with the account its record names.
        // Only a repair or a removal changes that, and neither comes here.
        (Record::Account(_), Some(_)) => return Err(OTHER_OWNER),
        (Record::Unusable(_), Some(_)) => return Err(DAMAGED),
        // Without a service a record authorizes nobody: a removal or an
        // installation that was cut short left it behind.
        (_, None) => true,
    };
    let (image, staged) = (system.path(IMAGE), system.path(STAGED));
    system.write(&staged)?;
    let result = if old.as_deref().is_some_and(|old| same_path(old, &image)) {
        swap(system, &image, &staged, was_running)
    } else {
        system.rename(&staged, &image).and_then(|()| {
            change(
                system,
                old.as_deref(),
                &image,
                was_running,
                authorize.then_some(sid),
            )
        })
    };
    settle(system, old.is_none() && result.is_err());
    result
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

/// Replaces the registered `image` with `staged`, the registration naming it
/// all along. The image it replaces keeps the name `BACKUP` until the new one
/// has started, and comes back when it does not. An image that is gone is
/// nothing to go back to: the new one stays, and may start the next time.
fn swap(system: &mut impl Steps, image: &Path, staged: &Path, was_running: bool) -> Result<()> {
    let backup = system.path(BACKUP);
    system.stop()?;
    let backed_up = system.present(image);
    let swapped = if backed_up {
        system.link(image, &backup)
    } else {
        Ok(())
    }
    .and_then(|()| system.rename(staged, image));
    if let Err(error) = swapped {
        // `image` is as it was.
        if was_running {
            system.start()?;
        }
        return Err(error);
    }
    if let Err(error) = system.start() {
        if backed_up {
            system.stop()?;
            system.rename(&backup, image)?;
            if was_running {
                system.start()?;
            }
        }
        return Err(error);
    }
    Ok(())
}

/// Registers `new`, in place of `old` if there is one, and starts it. When
/// that fails, the registration goes back to an `old` that is still there.
fn change(
    system: &mut impl Steps,
    old: Option<&Path>,
    new: &Path,
    was_running: bool,
    authorize: Option<&str>,
) -> Result<()> {
    let Some(old) = old else {
        if let Some(sid) = authorize {
            system.authorize(sid)?;
        }
        system.create(new)?;
        if let Err(error) = system.start() {
            system.stop()?;
            system.delete()?;
            return Err(error);
        }
        return Ok(());
    };
    system.stop()?;
    if let Err(error) = system.point(new).and_then(|_| system.start()) {
        // An image that is gone is nothing to go back to: a new one that
        // got registered stays, and may start the next time.
        if system.present(old) {
            system.stop()?;
            system.point(old)?;
            if was_running {
                system.start()?;
            }
        }
        return Err(error);
    }
    Ok(())
}

/// Deletes the images no registration names any more: the one an update
/// replaced, the new one of an attempt that failed, and those an earlier
/// attempt or crash left behind. After a failed installation the record
/// written for it goes too. Nothing is deleted unless the registration read
/// here has reached the disk.
fn settle(system: &mut impl Steps, installation_failed: bool) {
    // Read anew: a rollback may have stopped half way.
    let Ok(registered) = system.registered() else {
        return;
    };
    if system.flush().is_err() {
        return;
    }
    let registered = registered.as_deref().and_then(Path::file_name);
    for image in system.images() {
        let named = image
            .file_name()
            .zip(registered)
            .is_some_and(|(image, registered)| image.eq_ignore_ascii_case(registered));
        if !named {
            // One still running, or held by a scanner, waits for the next time.
            let _ = system.remove(&image);
        }
    }
    if registered.is_none() && installation_failed {
        let _ = system.forget();
    }
}

/// Stops and deletes a registered service, then writes that to disk. False
/// when it could not be written: the files then stay, since a crash could
/// bring the registration back.
fn unregister(system: &mut impl Steps) -> Result<bool> {
    if system.registered()?.is_some() {
        system.stop()?;
        system.delete()?;
    }
    Ok(system.flush().is_ok())
}

pub(crate) fn run_helper(action: &str, sid: &str) -> Result<()> {
    if !ACTIONS.contains(&action) {
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
    if action == "uninstall" {
        return uninstall();
    }
    let manager = scm(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)?;
    let existing = open(&manager, SERVICE_ALL_ACCESS)?;
    let dir = directory()?;
    let _parent = security::protect_directory(dir.parent().ok_or(87u32)?)?;
    match (action, &existing) {
        ("install", Some(_)) => return Err(1073),
        ("install", None) => {}
        (_, None) => return Err(1060),
        // Not another program's service of this name, whatever is asked.
        (_, Some(service)) => {
            binary(service, &dir)?;
        }
    }
    // A repair brings back a folder that is gone, as an installation makes it.
    if matches!(action, "install" | "repair") {
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
    let lock = management_lock(&dir)?;
    let mut machine = Machine {
        manager: &manager,
        service: existing,
        dir: &dir,
    };
    // Whose the registered service is. Only a repair goes on without a
    // record: its elevation was confirmed as an installation's is, and it
    // authorizes `sid` as one does. Never a record that names another account.
    let reauthorize = action != "install"
        && match machine.record() {
            Record::Account(owner) if owner == sid => false,
            Record::Account(_) => return Err(OTHER_OWNER),
            Record::Unsafe(code) => return Err(code),
            Record::Unusable(code) => {
                argus_core::diagnostics::warn(
                    "service authorization record unusable",
                    format!("{}; action={action}", argus_core::diagnostics::code(code)),
                );
                if action != "repair" {
                    return Err(DAMAGED);
                }
                true
            }
        };
    let result = if action == "start" {
        machine.start()
    } else {
        if reauthorize {
            // As before a removal: the replacement was enabled for the
            // account the lost record named.
            restore_for_removal(&dir)?;
            machine.authorize(sid)?;
        } else {
            // An enabled replacement comes along and stays on: its files are
            // apart from the service's and its launcher does not need it.
            // Anything else is restored first: never retire service/recovery
            // files while IFEO still owns them.
            let followed = action != "install"
                && crate::taskmgr::follow_update(&dir).map_err(|_| TASK_MANAGER_FOLLOW)?;
            if !followed {
                restore_task_manager(&dir)?;
            }
        }
        install_or_update(&mut machine, sid)
    };
    drop(lock);
    drop(guard);
    result
}

fn management_lock(dir: &Path) -> Result<fs::File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(dir.join(LOCK_FILE))
        .map_err(io)
        .inspect_err(|code| {
            argus_core::diagnostics::warn(
                "service management lock",
                format!(
                    "{}; path={}",
                    argus_core::diagnostics::code(*code),
                    argus_core::diagnostics::path(&dir.join(LOCK_FILE))
                ),
            )
        })
}

fn uninstall() -> Result<()> {
    let manager = scm(SC_MANAGER_CONNECT)?;
    let existing = open(&manager, SERVICE_ALL_ACCESS)?;
    let dir = directory()?;
    let protected = (|| {
        let parent = security::protect_directory(dir.parent().ok_or(87u32)?)?;
        let directory = security::protect_directory(&dir)?;
        Ok((parent, directory))
    })();
    let pins = match protected {
        Ok(pins) => Some(pins),
        Err(UNSAFE_PATH | 2 | 3 | 5) => None,
        Err(code) => return Err(code),
    };
    // Healthy installations retain the same lock as install/update. A
    // damaged path cannot authorize those operations, but can still revoke.
    let lock = pins.as_ref().map(|_| management_lock(&dir)).transpose()?;
    // SCM owns this configuration. Do not read authorization records from a
    // damaged directory or execute any program stored there while removing it.
    if let Some(service) = &existing {
        binary(service, &dir)?;
    }
    if lock.is_some() {
        restore_for_removal(&dir)?;
    } else {
        crate::taskmgr::restore().map_err(|_| crate::taskmgr::maintenance_error(&dir))?;
    }
    let mut machine = Machine {
        manager: &manager,
        service: existing,
        dir: &dir,
    };
    // The files go only once the deletion is on disk: a crash could
    // otherwise bring back a registration that names a deleted image and
    // starts with Windows. A registration that another program's handle
    // keeps for now is marked for deletion there, and never starts again.
    if !unregister(&mut machine)? {
        return Ok(());
    }
    let Some(directory) = crate::cleanup::Directory::open(&dir)? else {
        return Ok(());
    };
    let removed = clean(&directory);
    drop(lock);
    drop(pins);
    directory.remove_file(LOCK_FILE)?;
    directory.remove_empty()?;
    removed
}

/// Deletes what a removed service leaves in its folder: the authorization
/// record first, which without a service authorizes nobody and must not meet
/// a later installation for another account, then every service executable,
/// including those an update could not delete. Goes on past a file that
/// cannot be deleted and reports the first such failure.
fn clean(directory: &crate::cleanup::Directory) -> Result<()> {
    let mut result = directory.remove_file(OWNER_FILE);
    for image in directory.names(image_name)? {
        result = result.and(directory.remove_file(&image));
    }
    result
}

/// Restores the system task manager before the service changes, then deletes
/// the interface copies nothing names any more (older versions left them).
/// The caller holds install.lock.
fn restore_task_manager(dir: &Path) -> Result<()> {
    crate::taskmgr::restore_for_maintenance(dir)
        .map_err(|_| crate::taskmgr::maintenance_error(dir))?;
    crate::taskmgr::sweep(dir);
    Ok(())
}

/// `restore_task_manager` for a removal, or a repair that starts over as one
/// does: when the recovery record cannot do it, deleting Argus's own value
/// from taskmgr.exe's entry is the whole restore. The caller holds install.lock.
fn restore_for_removal(dir: &Path) -> Result<()> {
    if restore_task_manager(dir).is_err() {
        crate::taskmgr::restore_registration(dir)
            .map_err(|_| crate::taskmgr::maintenance_error(dir))?;
    }
    Ok(())
}

/// Brings an installed service to this executable, for the installer: it
/// already runs elevated after replacing argus.exe, so an update asks for
/// administrator rights once. The service stays with the account that
/// installed it, and an enabled task manager replacement follows along and
/// stays on. When this fails, Argus offers the service update when it starts.
///
/// A registered service whose image is gone comes back with the update: its
/// record still says whose it is. One whose record is gone stays as it is,
/// since nothing here says whom to authorize and the installer may run as
/// another account: Argus reports the damage and offers the repair when it
/// starts.
pub(crate) fn refresh() -> Result<()> {
    if !argus_core::privilege::is_elevated() {
        return Err(5);
    }
    let owner = {
        let manager = scm(SC_MANAGER_CONNECT)?;
        let Some(service) = open(&manager, SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)? else {
            return Ok(());
        };
        let dir = directory()?;
        let image = binary(&service, &dir)?;
        match examine(&dir, &image, &security::protect_present).1 {
            Health::Whole(owner) => owner,
            Health::Unsafe(code) => return Err(code),
            Health::Damaged(finding) => {
                argus_core::diagnostics::warn(
                    "privileged service installation damaged",
                    format!(
                        "{}; {}",
                        finding.damage.log(),
                        argus_core::diagnostics::code(finding.code)
                    ),
                );
                finding.owner.ok_or(DAMAGED)?
            }
        }
    };
    run_helper("update", &owner)
}

/// UAC belongs to this explicit settings action, never a background retry.
pub fn manage(action: &str) -> std::result::Result<ServiceStatus, String> {
    if !ACTIONS.contains(&action) {
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
    let sid = crate::util::stage(
        "service management current account",
        security::current_sid(),
    )
    .map_err(message)?;
    if argus_core::privilege::is_elevated() {
        crate::util::stage("service management helper", run_helper(action, &sid))
            .map_err(message)?;
    } else {
        let executable = std::env::current_exe().map_err(|error| {
            argus_core::diagnostics::warn(
                "service helper executable",
                argus_core::diagnostics::io(&error),
            );
            error.to_string()
        })?;
        let exe = wide(&executable.to_string_lossy());
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
        crate::util::file_stage(
            "service ShellExecuteExW (UAC)",
            &executable,
            unsafe { ShellExecuteExW(&mut info) }.map_err(win),
        )
        .map_err(message)?;
        if info.hProcess == HANDLE::default() {
            return Err(message(6));
        }
        let process = Handle(info.hProcess);
        if unsafe { WaitForSingleObject(process.0, INFINITE) } != WAIT_OBJECT_0 {
            return Err(message(6));
        }
        let mut exit = 0;
        crate::util::stage(
            "service GetExitCodeProcess",
            unsafe { GetExitCodeProcess(process.0, &mut exit) }.map_err(win),
        )
        .map_err(message)?;
        crate::diagnostics::collect("privileged service elevated operation", process.0, exit);
        if exit != 0 {
            return Err(message(exit));
        }
    }
    status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::INCOMPATIBLE;

    #[test]
    fn stopped_state_logs_changes_and_rearms_after_recovery_or_removal() {
        let mut observed = StoppedFailure::default();
        let newly_registered = SERVICE_STATUS_PROCESS {
            dwCurrentState: SERVICE_STOPPED,
            dwWin32ExitCode: 1077,
            ..Default::default()
        };
        assert_eq!(observed.observe(Some(&newly_registered)), None);
        let mut failed = SERVICE_STATUS_PROCESS {
            dwCurrentState: SERVICE_STOPPED,
            dwWin32ExitCode: 232,
            ..Default::default()
        };
        assert_eq!(observed.observe(Some(&failed)), Some((232, 0)));
        for _ in 0..5000 {
            assert_eq!(observed.observe(Some(&failed)), None);
        }
        failed.dwWin32ExitCode = 1066;
        failed.dwServiceSpecificExitCode = 1;
        assert_eq!(observed.observe(Some(&failed)), Some((1066, 1)));
        failed.dwServiceSpecificExitCode = 2;
        assert_eq!(observed.observe(Some(&failed)), Some((1066, 2)));
        for state in [SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STOPPED] {
            let healthy = SERVICE_STATUS_PROCESS {
                dwCurrentState: state,
                ..Default::default()
            };
            assert_eq!(observed.observe(Some(&healthy)), None);
            assert_eq!(observed.observe(Some(&failed)), Some((1066, 2)));
            assert_eq!(observed.observe(Some(&failed)), None);
        }
        assert_eq!(observed.observe(None), None);
        assert_eq!(observed.observe(Some(&failed)), Some((1066, 2)));
    }

    fn pinged(ping: Result<crate::Reply>) -> ServiceStatus {
        let mut status = ServiceStatus {
            authorized: true,
            ..ServiceStatus::new("running")
        };
        note_ping(&mut status, ping);
        status
    }

    #[test]
    fn a_ping_tells_an_incompatible_service_from_a_failed_one() {
        let connected = pinged(Ok(crate::Reply::Pong {
            version: "0.2.0".into(),
            protocol: crate::PROTOCOL,
            diagnostics: None,
            sensor_driver_version: None,
        }));
        assert_eq!(connected.version.as_deref(), Some("0.2.0"));
        assert!(connected.error.is_none() && !connected.incompatible);

        // Another protocol, refused by the pipe or answered with something else.
        for ping in [Err(INCOMPATIBLE), Ok(crate::Reply::Done)] {
            let status = pinged(ping);
            assert!(status.incompatible);
            assert_eq!(status.error, Some(message(INCOMPATIBLE)));
            assert!(status.version.is_none());
        }

        // A broken pipe or a timeout may pass: still an error, not incompatible.
        for code in [109, 1460] {
            let status = pinged(Err(code));
            assert!(!status.incompatible);
            assert_eq!(status.error, Some(message(code)));
        }
    }

    const ACCOUNT: &str = "S-1-5-21-1004336348-1177238915-682003330-1001";
    const OTHER: &str = "S-1-5-21-1004336348-1177238915-682003330-1002";

    /// A folder of the test's own, with the service folder (not made yet)
    /// inside it, as Program Files holds the real one.
    struct Folder(PathBuf);
    impl Folder {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("argus-install-{}", crate::taskmgr::id().unwrap()));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn dir(&self) -> PathBuf {
            self.0.join("Argus Privileged")
        }
        fn image(&self) -> PathBuf {
            self.dir().join(IMAGE)
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The path checks without the owner and permission rules, which a
    /// test's own folder cannot meet: what is absent is absent all the same.
    fn pinned(path: &Path) -> Result<Handle> {
        use std::os::windows::io::IntoRawHandle;
        OpenOptions::new()
            .access_mode(0x80)
            .share_mode(3)
            .custom_flags(0x0220_0000)
            .open(path)
            .map(|file| Handle(HANDLE(file.into_raw_handle())))
            .map_err(io)
    }

    fn damaged(damage: Damage, code: u32, owner: Option<&str>) -> Health {
        Health::Damaged(Finding {
            damage,
            code,
            owner: owner.map(str::to_owned),
        })
    }

    #[test]
    fn a_registered_service_is_examined_for_its_folder_record_and_image() {
        let folder = Folder::new();
        let (dir, image) = (folder.dir(), folder.image());
        let health = || examine(&dir, &image, &pinned).1;

        // The whole folder is gone.
        assert_eq!(health(), damaged(Damage::Directory, 2, None));
        assert!(examine(&dir, &image, &pinned).0.is_none());

        fs::create_dir(&dir).unwrap();
        assert_eq!(health(), damaged(Damage::OwnerAndImage, 2, None));
        assert!(examine(&dir, &image, &pinned).0.is_some());

        // The image without the record, whose loss is what is reported.
        fs::write(&image, b"image").unwrap();
        assert_eq!(health(), damaged(Damage::Owner, 2, None));

        // A record that is there but names no account is as good as none.
        let record = dir.join(OWNER_FILE);
        for unusable in [
            String::new(),
            "S-1-1-0".into(),
            "not a sid".into(),
            format!("{ACCOUNT}\n"),
            "S".repeat(185),
        ] {
            fs::write(&record, unusable).unwrap();
            let Health::Damaged(finding) = health() else {
                panic!("an unusable record passed");
            };
            assert_eq!((finding.damage, finding.owner), (Damage::Owner, None));
            assert!(!matches!(finding.code, 0 | 2 | 3));
        }

        fs::write(&record, ACCOUNT).unwrap();
        assert_eq!(health(), Health::Whole(ACCOUNT.into()));

        // The record without the image still says whose the service is.
        fs::remove_file(&image).unwrap();
        assert_eq!(health(), damaged(Damage::Image, 2, Some(ACCOUNT)));
    }

    #[test]
    fn what_cannot_be_trusted_is_not_reported_as_damage() {
        let folder = Folder::new();
        let (dir, image) = (folder.dir(), folder.image());
        fs::create_dir(&dir).unwrap();
        fs::write(&image, b"image").unwrap();
        fs::write(dir.join(OWNER_FILE), ACCOUNT).unwrap();

        // The service's own check refuses a folder an account can write to.
        assert_eq!(
            examine(&dir, &image, &security::protect_present).1,
            Health::Unsafe(UNSAFE_PATH)
        );
        assert_eq!(
            record(&dir, &security::protect_present),
            Record::Unsafe(UNSAFE_PATH)
        );
        // So does one that refuses only the folder, or only the record.
        for refused in [dir.clone(), dir.join(OWNER_FILE)] {
            let pin = |path: &Path| {
                if path == refused {
                    Err(UNSAFE_PATH)
                } else {
                    pinned(path)
                }
            };
            assert_eq!(examine(&dir, &image, &pin).1, Health::Unsafe(UNSAFE_PATH));
        }
        // Any other failure to open the record is not its absence either.
        let pin = |path: &Path| {
            if path.ends_with(OWNER_FILE) {
                Err(5)
            } else {
                pinned(path)
            }
        };
        assert_eq!(examine(&dir, &image, &pin).1, Health::Unsafe(5));
        assert_eq!(record(&dir, &pin), Record::Unsafe(5));
        assert_eq!(record(&dir, &pinned), Record::Account(ACCOUNT.into()));
    }

    #[test]
    #[ignore = "read-only: needs this build's service installed for this account and running"]
    fn the_installed_service_is_examined_in_its_protected_folder() {
        let manager = scm(SC_MANAGER_CONNECT).unwrap();
        let service = open(&manager, SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)
            .unwrap()
            .expect("the service is installed");
        let dir = directory().unwrap();
        let image = binary(&service, &dir).unwrap();
        let account = security::current_sid().unwrap();
        let check = &security::protect_present;

        assert!(is_registered());
        let (pins, health) = examine(&dir, &image, check);
        assert!(pins.is_some());
        assert_eq!(health, Health::Whole(account.clone()));
        let status = inspect().unwrap();
        assert_eq!((status.state, status.authorized), ("running", true));
        assert!(status.damage.is_none() && status.repair.is_none() && !status.other_owner());
        assert!(running_pid().unwrap().is_some());

        // What damage reads as there, with nothing touched: an image and a
        // folder that do not exist beside the real ones.
        let absent = dir.join("argus-service-0-0.exe");
        assert_eq!(
            examine(&dir, &absent, check).1,
            damaged(Damage::Image, 2, Some(&account))
        );
        let absent = dir.with_file_name(format!("Argus {}", crate::taskmgr::id().unwrap()));
        assert_eq!(
            examine(&absent, &absent.join("argus-service-0-0.exe"), check).1,
            damaged(Damage::Directory, 2, None)
        );
    }

    fn stopped(exit: u32) -> SERVICE_STATUS_PROCESS {
        SERVICE_STATUS_PROCESS {
            dwCurrentState: SERVICE_STOPPED,
            dwWin32ExitCode: exit,
            ..Default::default()
        }
    }

    fn unreached(_: u32) -> Result<crate::Reply> {
        panic!("only a whole installation of this account is asked")
    }

    #[test]
    fn a_damaged_installation_is_a_status_with_its_code_and_what_can_be_done() {
        // The record is gone: nothing says whose the service is, so bringing
        // it back authorizes anew, which the interface confirms as it does
        // an installation.
        for damage in [Damage::Owner, Damage::OwnerAndImage, Damage::Directory] {
            let status = describe(damaged(damage, 2, None), &stopped(2), ACCOUNT, unreached);
            assert_eq!(status.state, "broken");
            assert_eq!(status.damage, Some(damage));
            assert_eq!(status.repair, Some("repair"));
            assert_eq!(status.error_code, Some(2));
            assert_eq!(status.error, Some(damage.text()));
            assert!(!status.authorized && !status.other_owner());
        }

        // The image is gone, the record names this account: an update is the repair.
        let status = describe(
            damaged(Damage::Image, 2, Some(ACCOUNT)),
            &stopped(2),
            ACCOUNT,
            unreached,
        );
        assert_eq!((status.state, status.repair), ("broken", Some("update")));
        assert!(status.authorized && !status.other_owner());
        assert_eq!(status.error, Some(Damage::Image.text()));

        // Another account's: only removing is left, and the status says whose it is.
        let status = describe(
            damaged(Damage::Image, 3, Some(OTHER)),
            &stopped(2),
            ACCOUNT,
            unreached,
        );
        assert_eq!((status.state, status.repair), ("broken", None));
        assert_eq!(status.error_code, Some(3));
        assert!(!status.authorized && status.other_owner());
        assert_ne!(status.error, Some(Damage::Image.text()));

        // Whatever the SCM reports of it, a damaged service is not connected to.
        let running = SERVICE_STATUS_PROCESS {
            dwCurrentState: SERVICE_RUNNING,
            dwProcessId: 4242,
            ..Default::default()
        };
        let status = describe(
            damaged(Damage::Owner, 2, None),
            &running,
            ACCOUNT,
            unreached,
        );
        assert_eq!(status.state, "broken");

        let value = serde_json::to_value(describe(
            damaged(Damage::OwnerAndImage, 2, None),
            &stopped(2),
            ACCOUNT,
            unreached,
        ))
        .unwrap();
        assert_eq!(value["state"], "broken");
        assert_eq!(value["damage"], "ownerAndImage");
        assert_eq!(value["repair"], "repair");
        assert!(value.get("otherOwner").is_none() && value.get("errorCode").is_none());
    }

    #[test]
    fn a_whole_installation_keeps_its_states() {
        let status = describe(
            Health::Whole(ACCOUNT.into()),
            &stopped(1077),
            ACCOUNT,
            unreached,
        );
        assert_eq!((status.state, status.authorized), ("stopped", true));
        assert!(status.error.is_none() && status.damage.is_none() && status.repair.is_none());

        let status = describe(
            Health::Whole(ACCOUNT.into()),
            &stopped(2),
            ACCOUNT,
            unreached,
        );
        assert_eq!(status.error_code, Some(2));
        assert_eq!(status.error, Some(message(2)));

        let status = describe(Health::Whole(OTHER.into()), &stopped(0), ACCOUNT, unreached);
        assert_eq!(status.error_code, Some(OTHER_OWNER));
        assert!(!status.authorized && status.other_owner());

        let running = SERVICE_STATUS_PROCESS {
            dwCurrentState: SERVICE_RUNNING,
            dwProcessId: 4242,
            ..Default::default()
        };
        let status = describe(Health::Whole(ACCOUNT.into()), &running, ACCOUNT, |pid| {
            assert_eq!(pid, 4242);
            Ok(crate::Reply::Pong {
                version: "0.5.0".into(),
                protocol: crate::PROTOCOL,
                diagnostics: None,
                sensor_driver_version: None,
            })
        });
        assert_eq!(status.state, "running");
        assert_eq!(status.version.as_deref(), Some("0.5.0"));

        // An unsafe folder stays what it was: unavailable, with the cause.
        let status = describe(Health::Unsafe(UNSAFE_PATH), &running, ACCOUNT, unreached);
        assert_eq!(status.state, "unavailable");
        assert_eq!(status.error_code, Some(UNSAFE_PATH));
        assert!(status.damage.is_none() && status.repair.is_none());
    }

    #[test]
    fn only_copies_of_the_service_executable_are_swept() {
        for name in [
            "argus-service.exe",
            "Argus-Service.EXE",
            "argus-service.new.exe",
            "argus-service.old.exe",
            "argus-service.old-2.exe",
            "argus-service-4242-1.exe",
            "argus-service-1-18446744073709551615.exe",
        ] {
            assert!(image_name(name), "{name}");
        }
        for name in [
            "argus-service.exe.bak",
            "argus-service.new",
            "argus-service.old-0.exe",
            "argus-service-.exe",
            "argus-service--1.exe",
            "argus-service-1-.exe",
            "argus-service-1.exe",
            "argus-service-1-2-3.exe",
            "argus-service-a-1.exe",
            "argus-service-1-2.exe.bak",
            "argus-service-1-2.dll",
            "argus-taskmgr-launcher.exe",
            "argus-taskmgr-ui-01234567-89ab-cdef-0123-456789abcdef.exe",
            "owner.sid",
            "install.lock",
            "",
        ] {
            assert!(!image_name(name), "{name}");
        }
    }

    /// A scratch key under HKCU, removed with its values.
    struct ScratchKey(String);
    impl ScratchKey {
        fn new() -> Self {
            use windows::Win32::System::Registry::{
                HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, RegCreateKeyExW,
            };
            let path = format!(r"Software\ArgusTest-{}", crate::taskmgr::id().unwrap());
            let mut key = HKEY::default();
            let code = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(wide(&path).as_ptr()),
                    None,
                    None,
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    None,
                    &mut key,
                    None,
                )
            };
            assert_eq!(code.0, 0);
            drop(RegKey(key));
            Self(path)
        }
        fn set(&self, kind: REG_VALUE_TYPE, bytes: &[u8]) {
            use windows::Win32::System::Registry::{
                HKEY_CURRENT_USER, KEY_SET_VALUE, RegSetValueExW,
            };
            let mut key = HKEY::default();
            let code = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(wide(&self.0).as_ptr()),
                    None,
                    KEY_SET_VALUE,
                    &mut key,
                )
            };
            assert_eq!(code.0, 0);
            let key = RegKey(key);
            let code = unsafe {
                RegSetValueExW(
                    key.0,
                    windows::core::w!("DeleteFlag"),
                    None,
                    kind,
                    Some(bytes),
                )
            };
            assert_eq!(code.0, 0);
        }
    }
    impl Drop for ScratchKey {
        fn drop(&mut self) {
            use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteKeyW};
            unsafe {
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(wide(&self.0).as_ptr()));
            }
        }
    }

    #[test]
    fn a_registration_marked_for_deletion_is_told_by_its_flag() {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ};
        let key = ScratchKey::new();
        let marked = || marked_for_deletion(HKEY_CURRENT_USER, &key.0);
        assert!(!marked());
        key.set(REG_DWORD, &1u32.to_le_bytes());
        assert!(marked());
        key.set(REG_DWORD, &0u32.to_le_bytes());
        assert!(!marked());
        // Only the SCM's own value counts, not something else of that name.
        key.set(REG_SZ, &[b'1', 0, 0, 0]);
        assert!(!marked());
        key.set(REG_DWORD, &[1]);
        assert!(!marked());
        assert!(!marked_for_deletion(
            HKEY_CURRENT_USER,
            &format!(r"{}\missing", key.0)
        ));
    }

    #[test]
    fn a_registry_key_is_flushed_through_a_query_handle() {
        use windows::Win32::System::Registry::HKEY_CURRENT_USER;
        let key = ScratchKey::new();
        key.set(REG_DWORD, &1u32.to_le_bytes());
        assert_eq!(flush_key(HKEY_CURRENT_USER, &key.0), Ok(()));
        assert_eq!(
            flush_key(HKEY_CURRENT_USER, &format!(r"{}\missing", key.0)),
            Err(2)
        );
    }

    /// The registration and the folder in memory. Every change of a file or
    /// of the registration is checked against what a crash at that moment
    /// would leave.
    struct World {
        /// The image the registration names, as the SCM holds it.
        scm: Option<PathBuf>,
        /// Every registration the disk may hold: the SCM's since the last
        /// flush, any of which a crash can bring back.
        disk: Vec<Option<PathBuf>>,
        /// The folder's files, and which build each holds: "new" is this one.
        files: std::collections::BTreeMap<PathBuf, &'static str>,
        /// Registered images that were gone before anything here ran: damage
        /// an update repairs, not one it caused.
        lost: Vec<PathBuf>,
        record: Record,
        running: bool,
        /// A handle of another program keeps a deleted registration.
        held: bool,
        /// The calls that fail: the nth of a step, counted from one.
        refused: Vec<(&'static str, usize)>,
        calls: std::collections::HashMap<&'static str, usize>,
        steps: Vec<String>,
    }

    const REFUSED: u32 = 0x00c0_ffee;

    impl World {
        /// A service registered with `image`, for `ACCOUNT`.
        fn installed(image: &str, present: bool) -> Self {
            let image = PathBuf::from(image);
            Self {
                files: present
                    .then(|| (image.clone(), "old"))
                    .into_iter()
                    .collect(),
                lost: (!present).then(|| image.clone()).into_iter().collect(),
                record: Record::Account(ACCOUNT.into()),
                ..Self::new(Some(image))
            }
        }

        fn new(scm: Option<PathBuf>) -> Self {
            Self {
                disk: vec![scm.clone()],
                scm,
                files: Default::default(),
                lost: Vec::new(),
                record: Record::Unusable(2),
                running: false,
                held: false,
                refused: Vec::new(),
                calls: Default::default(),
                steps: Vec::new(),
            }
        }

        fn refusing(mut self, step: &'static str, nth: usize) -> Self {
            self.refused.push((step, nth));
            self
        }

        fn step(&mut self, name: &'static str, on: &[&Path]) -> Result<()> {
            self.steps.push(
                std::iter::once(name.to_owned())
                    .chain(on.iter().map(|path| path.display().to_string()))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            let nth = self.calls.entry(name).or_default();
            *nth += 1;
            if self.refused.contains(&(name, *nth)) {
                return Err(REFUSED);
            }
            Ok(())
        }

        fn register(&mut self, image: Option<PathBuf>) {
            self.scm = image.clone();
            self.disk.push(image);
            self.survives_a_crash();
        }

        /// After a crash now, Windows starts with one of `disk`: each of them
        /// must name a file that is there.
        fn survives_a_crash(&self) {
            for image in self.disk.iter().flatten() {
                assert!(
                    self.files.contains_key(image) || self.lost.contains(image),
                    "a crash after {:?} can bring back a registration of the missing {}",
                    self.steps.last(),
                    image.display()
                );
            }
        }

        fn put(&mut self, path: &Path, build: &'static str) {
            self.files.insert(path.into(), build);
            self.lost.retain(|lost| lost != path);
        }

        fn files(&self) -> Vec<&str> {
            self.files.keys().filter_map(|file| file.to_str()).collect()
        }

        /// Which build the registered image holds.
        fn build(&self) -> Option<&'static str> {
            self.files.get(self.scm.as_ref()?).copied()
        }

        fn steps(&self) -> Vec<&str> {
            self.steps.iter().map(String::as_str).collect()
        }
    }

    impl Steps for World {
        fn registered(&mut self) -> Result<Option<PathBuf>> {
            self.step("registered", &[])?;
            Ok(self.scm.clone())
        }
        fn running(&mut self) -> Result<bool> {
            Ok(self.running)
        }
        fn stop(&mut self) -> Result<()> {
            self.step("stop", &[])?;
            self.running = false;
            Ok(())
        }
        fn start(&mut self) -> Result<()> {
            self.step("start", &[])?;
            // An image that is not there cannot start.
            match &self.scm {
                Some(image) if self.files.contains_key(image) => {
                    self.running = true;
                    Ok(())
                }
                Some(_) => Err(2),
                None => Err(1060),
            }
        }
        fn create(&mut self, image: &Path) -> Result<()> {
            self.step("create", &[image])?;
            self.register(Some(image.into()));
            Ok(())
        }
        fn point(&mut self, image: &Path) -> Result<()> {
            self.step("point", &[image])?;
            self.register(Some(image.into()));
            Ok(())
        }
        fn delete(&mut self) -> Result<()> {
            self.step("delete", &[])?;
            // On disk a deletion, or the mark that keeps the service from
            // ever starting again: no registration either way.
            self.disk.push(None);
            if !self.held {
                self.scm = None;
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<()> {
            self.step("flush", &[])?;
            self.disk = vec![if self.held { None } else { self.scm.clone() }];
            Ok(())
        }
        fn path(&self, name: &str) -> PathBuf {
            PathBuf::from(name)
        }
        fn write(&mut self, path: &Path) -> Result<()> {
            self.step("write", &[path])?;
            self.put(path, "new");
            self.survives_a_crash();
            Ok(())
        }
        fn link(&mut self, from: &Path, to: &Path) -> Result<()> {
            self.step("link", &[from, to])?;
            let build = *self.files.get(from).ok_or(2u32)?;
            self.put(to, build);
            Ok(())
        }
        fn rename(&mut self, from: &Path, to: &Path) -> Result<()> {
            self.step("rename", &[from, to])?;
            let build = self.files.remove(from).ok_or(2u32)?;
            self.put(to, build);
            self.survives_a_crash();
            Ok(())
        }
        fn present(&mut self, image: &Path) -> bool {
            self.files.contains_key(image)
        }
        fn images(&mut self) -> Vec<PathBuf> {
            self.files.keys().cloned().collect()
        }
        fn remove(&mut self, image: &Path) -> Result<()> {
            self.step("remove", &[image])?;
            self.files.remove(image);
            self.survives_a_crash();
            Ok(())
        }
        fn record(&mut self) -> Record {
            self.record.clone()
        }
        fn authorize(&mut self, sid: &str) -> Result<()> {
            self.step("authorize", &[])?;
            self.record = Record::Account(sid.into());
            Ok(())
        }
        fn forget(&mut self) -> Result<()> {
            self.step("forget", &[])?;
            self.record = Record::Unusable(2);
            Ok(())
        }
    }

    #[test]
    #[should_panic(expected = "can bring back a registration of the missing old.exe")]
    fn deleting_an_image_before_the_flush_is_what_the_model_catches() {
        // What an update did before: the new path set, the old image deleted
        // at once, the registry still to reach the disk.
        let mut world = World::installed("old.exe", true);
        world.files.insert("new.exe".into(), "new");
        world.point(Path::new("new.exe")).unwrap();
        world.remove(Path::new("old.exe")).unwrap();
    }

    #[test]
    fn the_names_of_the_service_files_hold_together() {
        assert_eq!(BACKUP, crate::layout::aside(STEM, 1));
        assert_eq!(IMAGE, format!("{STEM}.exe"));
    }

    #[test]
    fn an_update_of_an_earlier_version_moves_its_service_to_the_fixed_name() {
        let mut world = World::installed("old.exe", true);
        world.running = true;
        // An earlier update could not delete this one.
        world.files.insert("left.exe".into(), "old");
        assert_eq!(install_or_update(&mut world, ACCOUNT), Ok(()));
        assert_eq!(
            world.steps(),
            [
                "registered",
                "write argus-service.new.exe",
                "rename argus-service.new.exe argus-service.exe",
                "stop",
                "point argus-service.exe",
                "start",
                "registered",
                "flush",
                "remove left.exe",
                "remove old.exe",
            ]
        );
        assert_eq!(world.files(), [IMAGE]);
        assert_eq!(world.scm.as_deref(), Some(Path::new(IMAGE)));
        assert_eq!(world.build(), Some("new"));
        assert!(world.running);
        assert_eq!(world.record, Record::Account(ACCOUNT.into()));
    }

    #[test]
    fn an_update_replaces_the_fixed_image_where_the_registration_names_it() {
        let mut world = World::installed(IMAGE, true);
        world.running = true;
        assert_eq!(install_or_update(&mut world, ACCOUNT), Ok(()));
        assert_eq!(
            world.steps(),
            [
                "registered",
                "write argus-service.new.exe",
                "stop",
                "link argus-service.exe argus-service.old.exe",
                "rename argus-service.new.exe argus-service.exe",
                "start",
                "registered",
                "flush",
                "remove argus-service.old.exe",
            ]
        );
        assert_eq!(world.files(), [IMAGE]);
        assert_eq!(world.build(), Some("new"));
        assert!(world.running);
        // The registration never changed.
        assert_eq!(world.disk, vec![Some(PathBuf::from(IMAGE))]);

        // A service that was stopped is started by its update, as before.
        let mut world = World::installed(IMAGE, true);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Ok(()));
        assert!(world.running);
    }

    #[test]
    fn a_fixed_image_that_does_not_start_gives_way_to_the_one_before() {
        let mut world = World::installed(IMAGE, true).refusing("start", 1);
        world.running = true;
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!(
            world.steps(),
            [
                "registered",
                "write argus-service.new.exe",
                "stop",
                "link argus-service.exe argus-service.old.exe",
                "rename argus-service.new.exe argus-service.exe",
                "start",
                "stop",
                "rename argus-service.old.exe argus-service.exe",
                "start",
                "registered",
                "flush",
            ]
        );
        assert_eq!(world.files(), [IMAGE]);
        assert_eq!(world.build(), Some("old"));
        assert!(world.running);

        // Stopped before: stopped after.
        let mut world = World::installed(IMAGE, true).refusing("start", 1);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!((world.build(), world.running), (Some("old"), false));

        // The image could not be kept aside, or not be replaced: it is as
        // it was, and runs again.
        for (step, nth) in [("link", 1), ("rename", 1)] {
            let mut world = World::installed(IMAGE, true).refusing(step, nth);
            world.running = true;
            assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
            assert_eq!(world.files(), [IMAGE], "{step}");
            assert_eq!(world.build(), Some("old"), "{step}");
            assert!(world.running, "{step}");
        }

        // Going back failed too: the new image stays registered and there.
        let mut world = World::installed(IMAGE, true)
            .refusing("start", 1)
            .refusing("rename", 2);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!(world.files(), [IMAGE]);
        assert_eq!(world.build(), Some("new"));

        // The service does not stop: nothing changed but the copy, which goes.
        let mut world = World::installed(IMAGE, true).refusing("stop", 1);
        world.running = true;
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!(world.files(), [IMAGE]);
        assert_eq!(world.build(), Some("old"));
        assert!(world.running);
    }

    #[test]
    fn whatever_step_fails_the_registration_names_an_image_that_is_there() {
        let steps = [
            "registered",
            "write",
            "rename",
            "link",
            "stop",
            "start",
            "point",
            "create",
            "delete",
            "flush",
            "remove",
            "authorize",
            "forget",
        ];
        let worlds: [fn() -> World; 4] = [
            || World::installed("old.exe", true),
            || World::installed(IMAGE, true),
            // Registered, its image gone.
            || World::installed("old.exe", false),
            || World::new(None),
        ];
        for (index, make) in worlds.iter().enumerate() {
            for step in steps {
                for nth in 1..=3 {
                    let initial = make();
                    let mut world = make().refusing(step, nth);
                    world.running = world.build().is_some();
                    let result = install_or_update(&mut world, ACCOUNT);
                    let case = format!("world {index}, {step} #{nth}: {:?}", world.steps());
                    // `survives_a_crash` checked each step on the way. The
                    // registered image is there, unless it was gone before.
                    if let Some(image) = &world.scm {
                        assert!(
                            world.files.contains_key(image) || !initial.files.contains_key(image),
                            "{case}"
                        );
                    }
                    // Nothing but what the registration names stays behind,
                    // unless what allows deleting (the registration read
                    // anew and written to disk) or the deletion failed.
                    let kept =
                        matches!(step, "flush" | "remove") || (step, nth) == ("registered", 2);
                    if !kept {
                        assert!(
                            world
                                .files
                                .keys()
                                .all(|file| Some(file) == world.scm.as_ref()),
                            "{case}"
                        );
                    }
                    if result.is_ok() {
                        assert_eq!(world.scm.as_deref(), Some(Path::new(IMAGE)), "{case}");
                        assert_eq!(world.build(), Some("new"), "{case}");
                        assert!(world.running, "{case}");
                    }
                }
            }
        }
    }

    #[test]
    fn without_the_flush_an_update_deletes_nothing() {
        let mut world = World::installed("old.exe", true).refusing("flush", 1);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Ok(()));
        assert_eq!(world.files(), [IMAGE, "old.exe"]);
        assert_eq!(world.scm.as_deref(), Some(Path::new(IMAGE)));

        let mut world = World::installed(IMAGE, true).refusing("flush", 1);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Ok(()));
        assert_eq!(world.files(), [IMAGE, BACKUP]);
    }

    #[test]
    fn a_failed_update_goes_back_and_deletes_the_new_image_after_the_flush() {
        let mut world = World::installed("old.exe", true).refusing("start", 1);
        world.running = true;
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!(
            world.steps(),
            [
                "registered",
                "write argus-service.new.exe",
                "rename argus-service.new.exe argus-service.exe",
                "stop",
                "point argus-service.exe",
                "start",
                "stop",
                "point old.exe",
                "start",
                "registered",
                "flush",
                "remove argus-service.exe",
            ]
        );
        assert_eq!(world.files(), ["old.exe"]);
        assert_eq!(world.scm.as_deref(), Some(Path::new("old.exe")));
        assert!(world.running);

        // The path could not even be set: the old registration stands.
        let mut world = World::installed("old.exe", true).refusing("point", 1);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!(world.files(), ["old.exe"]);
        assert_eq!(world.scm.as_deref(), Some(Path::new("old.exe")));

        // Going back failed too: the image the registration names stays.
        let mut world = World::installed("old.exe", true)
            .refusing("start", 1)
            .refusing("point", 2);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!(world.scm.as_deref(), Some(Path::new(IMAGE)));
        assert!(world.files.contains_key(Path::new(IMAGE)));

        // Nothing could be copied: nothing changed.
        let mut world = World::installed("old.exe", true).refusing("write", 1);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!(world.steps(), ["registered", "write argus-service.new.exe"]);
    }

    #[test]
    fn an_update_brings_back_a_missing_image() {
        for old in ["old.exe", IMAGE] {
            let mut world = World::installed(old, false);
            assert_eq!(install_or_update(&mut world, ACCOUNT), Ok(()), "{old}");
            assert_eq!(world.files(), [IMAGE], "{old}");
            assert_eq!(world.build(), Some("new"), "{old}");
            assert!(world.running, "{old}");
        }

        // The new image does not start: there is no old one to go back to,
        // so the registration keeps naming a file that exists.
        let mut world = World::installed("old.exe", false).refusing("start", 1);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!(world.files(), [IMAGE]);
        assert_eq!(world.scm.as_deref(), Some(Path::new(IMAGE)));

        // The new path could not be set: as damaged as before, without the copy.
        let mut world = World::installed("old.exe", false).refusing("point", 1);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert!(world.files.is_empty());
        assert_eq!(world.scm.as_deref(), Some(Path::new("old.exe")));
    }

    #[test]
    fn a_registered_service_is_never_given_to_another_account_or_to_none() {
        for (record, error) in [
            (Record::Account(OTHER.into()), OTHER_OWNER),
            (Record::Unusable(2), DAMAGED),
            (Record::Unusable(87), DAMAGED),
            (Record::Unsafe(UNSAFE_PATH), UNSAFE_PATH),
        ] {
            let mut world = World::installed("old.exe", true);
            world.record = record.clone();
            assert_eq!(install_or_update(&mut world, ACCOUNT), Err(error));
            assert_eq!(world.steps(), ["registered"]);
            assert_eq!(world.record, record);
        }
        // Nor does an installation read a record it cannot trust.
        let mut world = World::new(None);
        world.record = Record::Unsafe(UNSAFE_PATH);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(UNSAFE_PATH));
        assert_eq!(world.steps(), ["registered"]);
    }

    #[test]
    fn an_installation_writes_the_record_and_clears_what_a_cut_short_one_left() {
        let mut world = World::new(None);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Ok(()));
        assert_eq!(
            world.steps(),
            [
                "registered",
                "write argus-service.new.exe",
                "rename argus-service.new.exe argus-service.exe",
                "authorize",
                "create argus-service.exe",
                "start",
                "registered",
                "flush",
            ]
        );
        assert_eq!(world.record, Record::Account(ACCOUNT.into()));
        assert_eq!(world.files(), [IMAGE]);

        // A removal cut short left another account's record and images:
        // without a service they authorize and start nothing.
        let mut world = World::new(None);
        world.record = Record::Account(OTHER.into());
        world.files.insert("left.exe".into(), "old");
        world.files.insert(IMAGE.into(), "old");
        assert_eq!(install_or_update(&mut world, ACCOUNT), Ok(()));
        assert_eq!(world.record, Record::Account(ACCOUNT.into()));
        assert_eq!(world.files(), [IMAGE]);
        assert_eq!(world.build(), Some("new"));

        // This account's own record is kept as it is.
        let mut world = World::new(None);
        world.record = Record::Account(ACCOUNT.into());
        assert_eq!(install_or_update(&mut world, ACCOUNT), Ok(()));
        assert!(!world.steps().contains(&"authorize"));
    }

    #[test]
    fn a_failed_installation_leaves_neither_registration_nor_files() {
        let mut world = World::new(None).refusing("start", 1);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!(
            world.steps(),
            [
                "registered",
                "write argus-service.new.exe",
                "rename argus-service.new.exe argus-service.exe",
                "authorize",
                "create argus-service.exe",
                "start",
                "stop",
                "delete",
                "registered",
                "flush",
                "remove argus-service.exe",
                "forget",
            ]
        );
        assert!(world.scm.is_none() && world.files.is_empty());
        assert_eq!(world.record, Record::Unusable(2));

        // The service could not be registered at all.
        let mut world = World::new(None).refusing("create", 1);
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert!(world.files.is_empty());
        assert_eq!(world.record, Record::Unusable(2));

        // Another program's handle keeps the deleted registration for now:
        // what it names stays until it is gone.
        let mut world = World::new(None).refusing("start", 1);
        world.held = true;
        assert_eq!(install_or_update(&mut world, ACCOUNT), Err(REFUSED));
        assert_eq!(world.files(), [IMAGE]);
        assert_eq!(world.record, Record::Account(ACCOUNT.into()));
    }

    #[test]
    fn a_removal_puts_the_deletion_on_disk_before_any_file_goes() {
        let mut world = World::installed(IMAGE, true);
        world.running = true;
        assert_eq!(unregister(&mut world), Ok(true));
        assert_eq!(world.steps(), ["registered", "stop", "delete", "flush"]);
        assert_eq!(world.disk, vec![None::<PathBuf>]);
        // Only now may the files go, which the model checks.
        world.remove(Path::new(IMAGE)).unwrap();

        // Not written out: the caller keeps the files.
        let mut world = World::installed(IMAGE, true).refusing("flush", 1);
        assert_eq!(unregister(&mut world), Ok(false));
        assert_eq!(world.files(), [IMAGE]);

        // A service that does not stop keeps its registration and files.
        let mut world = World::installed(IMAGE, true).refusing("stop", 1);
        assert_eq!(unregister(&mut world), Err(REFUSED));
        assert_eq!(world.steps(), ["registered", "stop"]);
        assert!(world.scm.is_some());

        // Nothing registered: what an earlier removal left may still go.
        let mut world = World::new(None);
        assert_eq!(unregister(&mut world), Ok(true));
        assert_eq!(world.steps(), ["registered", "flush"]);

        // Kept by another program's handle: marked on disk, never to start.
        let mut world = World::installed("old.exe", true);
        world.held = true;
        assert_eq!(unregister(&mut world), Ok(true));
        world.remove(Path::new("old.exe")).unwrap();
    }

    #[test]
    fn a_removal_deletes_the_record_and_every_service_executable() {
        let folder = Folder::new();
        let dir = folder.dir();
        fs::create_dir(&dir).unwrap();
        let kept = [
            "argus-taskmgr-launcher.exe",
            "taskmgr-recovery.json",
            LOCK_FILE,
        ];
        let removed = [
            OWNER_FILE,
            IMAGE,
            STAGED,
            BACKUP,
            "argus-service-4242-1.exe",
            "argus-service-4242-2.exe",
        ];
        for name in kept.iter().chain(&removed) {
            fs::write(dir.join(name), b"file").unwrap();
        }
        let names = |dir: &Path| {
            let mut names: Vec<String> = fs::read_dir(dir)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        };
        let directory = crate::cleanup::Directory::open(&dir).unwrap().unwrap();
        assert_eq!(clean(&directory), Ok(()));
        let mut expected = kept.map(str::to_owned).to_vec();
        expected.sort();
        assert_eq!(names(&dir), expected);
        // Again, with nothing left of the service: fine.
        assert_eq!(clean(&directory), Ok(()));

        // What cannot be deleted is reported, after everything else went.
        fs::write(dir.join(OWNER_FILE), ACCOUNT).unwrap();
        fs::create_dir(dir.join("argus-service-1-1.exe")).unwrap();
        fs::write(dir.join("argus-service-2-2.exe"), b"file").unwrap();
        assert!(clean(&directory).is_err());
        let mut expected = kept.map(str::to_owned).to_vec();
        expected.push("argus-service-1-1.exe".into());
        expected.sort();
        assert_eq!(names(&dir), expected);
        drop(directory);
    }
}
