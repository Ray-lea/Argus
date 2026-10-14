//! One explicit settings action prepares the official PawnIO driver. No
//! installer URL, executable path or service name is accepted from IPC/CLI.
mod package;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use argus_core::tr;
use windows::Win32::Foundation::{
    ERROR_SERVICE_DOES_NOT_EXIST, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetFileVersionInfoSizeW, GetFileVersionInfoW, OPEN_EXISTING,
    VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows::Win32::System::Com::{CoCreateGuid, CoTaskMemFree};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Services::*;
use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    FOLDERID_Windows, KF_FLAG_DEFAULT, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW, SHGetKnownFolderPath, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{PCWSTR, w};

use crate::util::{Handle, Result, io, wide, win};
use crate::{security, util};

pub(crate) const MANAGE_ARG: &str = "--argus-sensor-driver-manage";
const DOWNLOAD_FAILED: u32 = 0x2000_0101;
const PACKAGE_INVALID: u32 = 0x2000_0102;
const SIGNATURE_INVALID: u32 = 0x2000_0103;
const FOREIGN_DRIVER: u32 = 0x2000_0104;
const NOT_READY: u32 = 0x2000_0105;
const UNSAFE_DRIVER_PATH: u32 = 0x2000_0106;
const WRITABLE_DRIVER_STORE: u32 = 0x2000_0107;
const UNSAFE_SETUP_PATH: u32 = 0x2000_0108;
const REBOOT_REQUIRED: u32 = 3010;
const MIN_VERSION: (u32, u32, u32, u32) = (2, 2, 0, 0);
const INSTALL_ARGS: [&str; 2] = ["-install", "-silent"];

struct Service(SC_HANDLE);
impl Drop for Service {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Plan {
    Ready,
    Start,
    Install,
}

fn plan(installed: bool, compatible: bool, running: bool) -> Plan {
    if !installed || !compatible {
        Plan::Install
    } else if running {
        Plan::Ready
    } else {
        Plan::Start
    }
}

fn finish_preparation(
    plan: Plan,
    install: impl FnOnce() -> Result<()>,
    start: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if plan == Plan::Install {
        install()?;
    }
    start()
}

fn installer_exit(code: Option<i32>) -> Result<()> {
    match code.map(|code| code as u32) {
        Some(0) => Ok(()),
        Some(code) => Err(code),
        None => Err(31),
    }
}

fn message(code: u32) -> String {
    match code {
        DOWNLOAD_FAILED => tr!("sensorSetup.download"),
        PACKAGE_INVALID => tr!("sensorSetup.packageInvalid"),
        SIGNATURE_INVALID => tr!("sensorSetup.signatureInvalid"),
        FOREIGN_DRIVER => tr!("sensorSetup.foreignDriver"),
        NOT_READY => tr!("sensorSetup.notReady"),
        UNSAFE_DRIVER_PATH => tr!("sensorSetup.unsafeDriverPath"),
        WRITABLE_DRIVER_STORE => tr!("sensorSetup.writableDriverStore"),
        UNSAFE_SETUP_PATH => tr!("sensorSetup.unsafeSetupPath"),
        REBOOT_REQUIRED | 1641 => tr!("sensorSetup.reboot"),
        _ => util::message(code),
    }
}

fn program_files() -> Result<PathBuf> {
    Ok(crate::install::directory()?
        .parent()
        .ok_or(87u32)?
        .to_owned())
}
fn windows_dir() -> Result<PathBuf> {
    let raw =
        unsafe { SHGetKnownFolderPath(&FOLDERID_Windows, KF_FLAG_DEFAULT, None) }.map_err(win)?;
    let value = unsafe { raw.to_string() }.map_err(|_| 87u32);
    unsafe {
        CoTaskMemFree(Some(raw.0.cast()));
    }
    value.map(PathBuf::from)
}

fn image_path(raw: &str, windows: &Path, programs: &Path) -> Result<PathBuf> {
    let raw = raw.trim();
    let raw = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(raw);
    if raw.contains(['"', '\0', '\n', '\r']) {
        return Err(FOREIGN_DRIVER);
    }
    let lower = raw.to_ascii_lowercase();
    let path = if lower.starts_with(r"\systemroot\") {
        windows.join(&raw[12..])
    } else if lower.starts_with(r"%systemroot%\") {
        windows.join(&raw[13..])
    } else if lower.starts_with(r"system32\") {
        windows.join(raw)
    } else {
        PathBuf::from(
            raw.strip_prefix(r"\??\")
                .or_else(|| raw.strip_prefix(r"\\?\"))
                .unwrap_or(raw),
        )
    };
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        || path
            .file_name()
            .is_none_or(|n| !n.eq_ignore_ascii_case("PawnIO.sys"))
    {
        return Err(FOREIGN_DRIVER);
    }
    let path_text = path.to_string_lossy().to_ascii_lowercase();
    let allowed = [
        windows.join(r"System32\drivers"),
        windows.join(r"System32\DriverStore\FileRepository"),
        programs.join("PawnIO"),
    ];
    if !allowed.iter().any(|dir| {
        path_text.starts_with(&format!("{}\\", dir.to_string_lossy().to_ascii_lowercase()))
    }) {
        return Err(FOREIGN_DRIVER);
    }
    Ok(path)
}

/// Keep every directory below the known protected root pinned while reading
/// the version and, for a stopped driver, until StartService finishes.
fn pin_image(path: &Path, windows: &Path, programs: &Path) -> Result<Vec<Handle>> {
    let root = if path
        .to_string_lossy()
        .to_ascii_lowercase()
        .starts_with(&windows.to_string_lossy().to_ascii_lowercase())
    {
        windows
    } else {
        programs
    };
    let root_depth = root.components().count();
    let mut chain: Vec<_> = path
        .ancestors()
        .filter(|p| p.components().count() >= root_depth)
        .collect();
    chain.reverse();
    chain
        .into_iter()
        .map(|part| {
            security::inspect_path(part).map_err(|error| driver_path_error(part, windows, error))
        })
        .collect()
}

fn driver_path_error(path: &Path, windows: &Path, error: security::PathError) -> u32 {
    if let security::PathError::Os(code) = error {
        return code;
    }
    if error == security::PathError::Writable
        && path.as_os_str().eq_ignore_ascii_case(
            windows
                .join(r"System32\DriverStore\FileRepository")
                .as_os_str(),
        )
    {
        WRITABLE_DRIVER_STORE
    } else {
        UNSAFE_DRIVER_PATH
    }
}

fn protect_setup_path(path: &Path) -> Result<Handle> {
    security::protect_directory(path).map_err(|code| {
        if code == util::UNSAFE_PATH {
            UNSAFE_SETUP_PATH
        } else {
            code
        }
    })
}

fn file_version(path: &Path) -> Result<(u32, u32, u32, u32)> {
    let path = wide(&path.to_string_lossy());
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    if !(52..=1024 * 1024).contains(&size) {
        return Err(FOREIGN_DRIVER);
    }
    let mut data = vec![0u64; (size as usize).div_ceil(8)];
    unsafe { GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()) }
        .map_err(win)?;
    let mut pointer = std::ptr::null_mut();
    let mut len = 0;
    if !unsafe { VerQueryValueW(data.as_ptr().cast(), w!(r"\"), &mut pointer, &mut len) }.as_bool()
        || len < size_of::<VS_FIXEDFILEINFO>() as u32
        || pointer.is_null()
    {
        return Err(FOREIGN_DRIVER);
    }
    let start = data.as_ptr() as usize;
    if (pointer as usize) < start
        || (pointer as usize)
            .checked_add(size_of::<VS_FIXEDFILEINFO>())
            .is_none_or(|end| end > start + size as usize)
    {
        return Err(FOREIGN_DRIVER);
    }
    let info = unsafe { std::ptr::read_unaligned(pointer.cast::<VS_FIXEDFILEINFO>()) };
    if info.dwSignature != 0xfeef04bd {
        return Err(FOREIGN_DRIVER);
    }
    let mut original = std::ptr::null_mut();
    let mut characters = 0;
    if !unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            w!(r"\StringFileInfo\040904b0\OriginalFilename"),
            &mut original,
            &mut characters,
        )
    }
    .as_bool()
        || !(1..=256).contains(&characters)
        || original.is_null()
        || (original as usize) < start
        || (original as usize)
            .checked_add(characters as usize * 2)
            .is_none_or(|end| end > start + size as usize)
    {
        return Err(FOREIGN_DRIVER);
    }
    let original =
        unsafe { std::slice::from_raw_parts(original.cast::<u16>(), characters as usize) };
    let name = String::from_utf16_lossy(original)
        .trim_end_matches('\0')
        .to_owned();
    if !name.eq_ignore_ascii_case("PawnIO.sys") {
        return Err(FOREIGN_DRIVER);
    }
    Ok((
        info.dwFileVersionMS >> 16,
        info.dwFileVersionMS & 0xffff,
        info.dwFileVersionLS >> 16,
        info.dwFileVersionLS & 0xffff,
    ))
}

fn open(access: u32) -> Result<Option<Service>> {
    let manager = Service(unsafe { OpenSCManagerW(None, None, SC_MANAGER_CONNECT) }.map_err(win)?);
    match unsafe { OpenServiceW(manager.0, w!("PawnIO"), access) } {
        Ok(value) => Ok(Some(Service(value))),
        Err(error) if error.code() == ERROR_SERVICE_DOES_NOT_EXIST.to_hresult() => Ok(None),
        Err(error) => Err(win(error)),
    }
}

fn state(service: &Service) -> Result<SERVICE_STATUS_PROCESS> {
    let mut value = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0;
    unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            Some(std::slice::from_raw_parts_mut(
                (&mut value as *mut SERVICE_STATUS_PROCESS).cast(),
                size_of::<SERVICE_STATUS_PROCESS>(),
            )),
            &mut needed,
        )
    }
    .map_err(win)?;
    if value.dwServiceType != SERVICE_KERNEL_DRIVER {
        return Err(FOREIGN_DRIVER);
    }
    Ok(value)
}

fn inspect(service: &Service) -> Result<(Plan, Vec<Handle>)> {
    let mut size = 0;
    unsafe {
        let _ = QueryServiceConfigW(service.0, None, 0, &mut size);
    }
    if size == 0 || size > 64 * 1024 {
        return Err(FOREIGN_DRIVER);
    }
    let mut data = vec![0u64; (size as usize).div_ceil(8)];
    let config = data.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    unsafe { QueryServiceConfigW(service.0, Some(config), size, &mut size) }.map_err(win)?;
    let config = unsafe { &*config };
    if config.dwServiceType != SERVICE_KERNEL_DRIVER {
        return Err(FOREIGN_DRIVER);
    }
    let raw = unsafe { config.lpBinaryPathName.to_string() }.map_err(|_| FOREIGN_DRIVER)?;
    let windows = windows_dir()?;
    let programs = program_files()?;
    let path = image_path(&raw, &windows, &programs)?;
    let pins = pin_image(&path, &windows, &programs)?;
    let compatible = file_version(&path)? >= MIN_VERSION;
    Ok((
        plan(
            true,
            compatible,
            state(service)?.dwCurrentState == SERVICE_RUNNING,
        ),
        pins,
    ))
}

fn current_plan() -> Result<Plan> {
    let Some(service) = open(SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS)? else {
        return Ok(Plan::Install);
    };
    inspect(&service).map(|(plan, _)| plan)
}

/// A fixed version IOCTL only: this neither loads a module nor starts a driver.
pub(crate) fn device_version() -> Option<u32> {
    let handle = Handle(
        unsafe {
            CreateFileW(
                w!(r"\\?\GLOBALROOT\Device\PawnIO"),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .ok()?,
    );
    let mut version = 0u32;
    let mut returned = 0;
    unsafe {
        DeviceIoControl(
            handle.0,
            (41394 << 16) | (0x861 << 2),
            None,
            0,
            Some((&mut version as *mut u32).cast()),
            4,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    (returned == 4).then_some(version)
}

fn start() -> Result<()> {
    let service =
        open(SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | SERVICE_START)?.ok_or(NOT_READY)?;
    let (plan, _pins) = inspect(&service)?;
    if plan == Plan::Install {
        return Err(NOT_READY);
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut attempted = false;
    loop {
        let status = state(&service)?;
        if status.dwCurrentState == SERVICE_RUNNING
            && device_version().is_some_and(|v| v >= 0x0002_0200)
        {
            return Ok(());
        }
        if status.dwCurrentState == SERVICE_STOPPED {
            if !attempted {
                if let Err(error) = unsafe { StartServiceW(service.0, None) } {
                    let code = win(error);
                    if code != 1056 {
                        return Err(code);
                    }
                }
                attempted = true;
                continue;
            }
            return Err(if status.dwWin32ExitCode == 0 {
                NOT_READY
            } else {
                status.dwWin32ExitCode
            });
        }
        if Instant::now() >= deadline {
            return Err(1460);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn protected_dir(path: &Path) -> Result<Handle> {
    let descriptor = security::directory_descriptor()?;
    if let Err(error) = unsafe {
        CreateDirectoryW(
            PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
            Some(&descriptor.attributes()),
        )
    } {
        let code = win(error);
        if code != 183 {
            return Err(code);
        }
    }
    protect_setup_path(path)
}

fn install_package(root: &Path) -> Result<()> {
    let bytes = package::download()?;
    let id = unsafe { CoCreateGuid() }.map_err(win)?;
    let directory = root.join(format!("{id:?}"));
    let guard = protected_dir(&directory)?;
    let installer = directory.join("PawnIO_setup.exe");
    let result = (|| {
        let mut file = crate::install::new_protected_file(&installer)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(io)?;
        drop(file);
        let _verified = package::verify(&installer)?;
        // Wait for the real installer exit; never kill a driver installation
        // mid-operation or delete its executable while it is still running.
        let status = Command::new(&installer)
            .args(INSTALL_ARGS)
            .current_dir(&directory)
            .creation_flags(0x08000000)
            .status()
            .map_err(io)?;
        installer_exit(status.code())
    })();
    let _ = fs::remove_file(&installer);
    drop(guard);
    let _ = fs::remove_dir(&directory);
    result
}

pub(crate) fn run_helper() -> Result<()> {
    if !argus_core::privilege::is_elevated() {
        return Err(5);
    }
    let programs = program_files()?;
    let _parent = protect_setup_path(&programs)?;
    let root = programs.join("Argus Sensor Setup");
    let _root = protected_dir(&root)?;
    let _lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(root.join("install.lock"))
        .map_err(io)?;
    finish_preparation(current_plan()?, || install_package(&root), start)
}

/// Called only when the settings switch is explicitly turned on. A shared
/// driver is never uninstalled/stopped, and saved preferences never trigger UAC.
pub fn ensure_ready() -> std::result::Result<(), String> {
    require_service()?;
    if current_plan().map_err(message)? == Plan::Ready {
        return Ok(());
    }
    elevate_and_prepare()?;
    if current_plan().map_err(message)? != Plan::Ready {
        return Err(message(NOT_READY));
    }
    require_service()
}

fn require_service() -> std::result::Result<(), String> {
    let service = crate::status()?;
    if service.state != "running" || !service.authorized || service.error.is_some() {
        return Err(tr!("sensorSetup.serviceRequired"));
    }
    Ok(())
}

fn elevate_and_prepare() -> std::result::Result<(), String> {
    if argus_core::privilege::is_elevated() {
        run_helper().map_err(message)?;
    } else {
        let executable = wide(
            &std::env::current_exe()
                .map_err(|e| e.to_string())?
                .to_string_lossy(),
        );
        let directory = wide(&program_files().map_err(message)?.to_string_lossy());
        let parameters = wide(MANAGE_ARG);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(executable.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            lpDirectory: PCWSTR(directory.as_ptr()),
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
        let mut code = 0;
        unsafe { GetExitCodeProcess(process.0, &mut code) }.map_err(|e| message(win(e)))?;
        if code != 0 {
            return Err(message(code));
        }
    }
    Ok(())
}

/// Read-only verification for the developer probe; never launches the file.
pub fn verify_installer(path: &Path) -> std::result::Result<&'static str, String> {
    package::verify(path).map_err(message)?;
    Ok(package::version())
}

/// Exercises the real HTTPS download and digest check without writing or running it.
pub fn verify_installer_download() -> std::result::Result<&'static str, String> {
    package::download().map_err(message)?;
    Ok(package::version())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Read-only check of the locally installed PawnIO driver"]
    fn installed_driver_preflight() {
        let plan = current_plan().unwrap();
        assert!(matches!(plan, Plan::Ready | Plan::Start));
    }

    #[test]
    fn unsafe_driver_paths_report_the_driver_instead_of_the_argus_service() {
        let windows = Path::new(r"C:\Windows");
        let repository = Path::new(r"C:\WINDOWS\System32\DriverStore\FileRepository");
        assert_eq!(
            driver_path_error(repository, windows, security::PathError::Writable),
            WRITABLE_DRIVER_STORE
        );
        for error in [
            security::PathError::Owner,
            security::PathError::Permissions,
            security::PathError::ReparsePoint,
        ] {
            assert_eq!(
                driver_path_error(repository, windows, error),
                UNSAFE_DRIVER_PATH
            );
        }
        assert_eq!(
            driver_path_error(
                &repository.join("PawnIO.sys"),
                windows,
                security::PathError::Writable
            ),
            UNSAFE_DRIVER_PATH
        );
        assert_eq!(
            driver_path_error(repository, windows, security::PathError::Os(5)),
            5
        );
        assert_eq!(
            protect_setup_path(&std::env::temp_dir()).err(),
            Some(UNSAFE_SETUP_PATH)
        );
    }

    #[test]
    fn cancellation_failure_and_reboot_never_start_collection() {
        for code in [1223, 3010, 1641, 5] {
            assert_eq!(
                finish_preparation(
                    Plan::Install,
                    || installer_exit(Some(code)),
                    || panic!("installation did not finish")
                ),
                Err(code as u32)
            );
        }
        assert!(installer_exit(None).is_err());
        assert!(
            finish_preparation(
                Plan::Start,
                || panic!("driver is already installed"),
                || Ok(())
            )
            .is_ok()
        );
        let order = std::cell::RefCell::new(Vec::new());
        finish_preparation(
            Plan::Install,
            || {
                order.borrow_mut().push("install");
                Ok(())
            },
            || {
                order.borrow_mut().push("start");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*order.borrow(), ["install", "start"]);
    }
    #[test]
    fn reuses_running_driver_and_never_downgrades_newer_versions() {
        assert_eq!(plan(false, false, false), Plan::Install);
        assert_eq!(plan(true, false, true), Plan::Install);
        assert_eq!(plan(true, true, false), Plan::Start);
        assert_eq!(plan(true, true, true), Plan::Ready);
        assert!((3, 0, 0, 0) > MIN_VERSION);
        assert_eq!(INSTALL_ARGS, ["-install", "-silent"]);
    }
    #[test]
    fn only_fixed_driver_locations_are_eligible_for_start() {
        let windows = Path::new(r"C:\Windows");
        let programs = Path::new(r"C:\Program Files");
        for path in [
            r"\SystemRoot\System32\drivers\PawnIO.sys",
            r"%SystemRoot%\System32\DriverStore\FileRepository\pawnio.inf_amd64_a\PawnIO.sys",
            r#""C:\Program Files\PawnIO\PawnIO.sys""#,
        ] {
            assert!(image_path(path, windows, programs).is_ok());
        }
        for path in [
            r"C:\Users\Public\PawnIO.sys",
            r"C:\Windows\System32\drivers\Other.sys",
            r"C:\Windows\System32\drivers\..\PawnIO.sys",
            r"\\server\share\PawnIO.sys",
            r#""C:\Windows\System32\drivers\PawnIO.sys" --argument"#,
            r"C:\Program Files\PawnIO-other\PawnIO.sys",
        ] {
            assert!(image_path(path, windows, programs).is_err());
        }
    }
}
