//! One explicit settings action prepares the official PawnIO driver. No
//! installer URL, executable path or service name is accepted from IPC/CLI.
mod legacy;
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
    ERROR_SERVICE_DOES_NOT_EXIST, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, NTSTATUS,
    RtlNtStatusToDosError, SetLastError, WAIT_OBJECT_0, WIN32_ERROR,
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
const LEGACY_LAYOUT: u32 = 0x2000_0109;
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
    MigrateLegacy,
}

#[derive(Default)]
struct Inspection {
    stage: &'static str,
    service_type: Option<u32>,
    service_state: Option<u32>,
    start_type: Option<u32>,
    image_location: &'static str,
    path_component: &'static str,
    version: Option<(u32, u32, u32, u32)>,
    translations: Vec<u32>,
    original_name_matches: Option<bool>,
    version_info_bytes: Option<u32>,
    native_error: Option<u32>,
    legacy_driver: bool,
}

impl Inspection {
    fn summary(&self, result: &Result<Plan>) -> String {
        let outcome = match result {
            Ok(Plan::Ready) => "ready".into(),
            Ok(Plan::Start) => "start required".into(),
            Ok(Plan::Install) => "install/update required".into(),
            Ok(Plan::MigrateLegacy) => "legacy migration required".into(),
            Err(code) => format!("error {code:#010x}"),
        };
        let version = self
            .version
            .map(|(a, b, c, d)| format!("{a}.{b}.{c}.{d}"))
            .unwrap_or_else(|| "unavailable".into());
        let translations = self
            .translations
            .iter()
            .map(|pair| format!("{pair:08x}"))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "stage={}; result={outcome}; native_error={:?}; service_type={:?}; service_state={:?}; start_type={:?}; image_location={}; path_component={}; file_version={version}; version_info_bytes={:?}; version_translations=[{translations}]; original_name_matches={:?}; official_legacy_driver={}",
            self.stage,
            self.native_error,
            self.service_type,
            self.service_state,
            self.start_type,
            self.image_location,
            self.path_component,
            self.version_info_bytes,
            self.original_name_matches,
            self.legacy_driver
        )
    }
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
    if matches!(plan, Plan::Install | Plan::MigrateLegacy) {
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
        LEGACY_LAYOUT => tr!("sensorSetup.legacyLayout"),
        1061 | 1062 => format!("{} {}", tr!("sensorSetup.driverInUse"), util::message(code)),
        1072 => format!("{} {}", tr!("sensorSetup.reboot"), util::message(code)),
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

fn image_path(
    raw: &str,
    windows: &Path,
    programs: &Path,
    trace: &mut Inspection,
) -> Result<PathBuf> {
    trace.stage = "driver image path syntax";
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
    if !path.is_absolute() {
        trace.image_location = "relative or unsupported prefix";
        return Err(FOREIGN_DRIVER);
    }
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        trace.image_location = "path traversal";
        return Err(FOREIGN_DRIVER);
    }
    if path
        .file_name()
        .is_none_or(|n| !n.eq_ignore_ascii_case("PawnIO.sys"))
    {
        trace.image_location = "unexpected driver filename";
        return Err(FOREIGN_DRIVER);
    }
    let path_text = path.to_string_lossy().to_ascii_lowercase();
    let allowed = [
        windows.join(r"System32\drivers"),
        windows.join(r"System32\DriverStore\FileRepository"),
        programs.join("PawnIO"),
    ];
    let location = allowed.iter().position(|dir| {
        path_text.starts_with(&format!("{}\\", dir.to_string_lossy().to_ascii_lowercase()))
    });
    trace.stage = "driver image location policy";
    trace.image_location = match location {
        Some(0) => "System32 drivers",
        Some(1) => "DriverStore FileRepository",
        Some(2) => "Program Files PawnIO",
        _ if path_text.starts_with(&format!(
            "{}\\",
            programs.to_string_lossy().to_ascii_lowercase()
        )) =>
        {
            "other Program Files location"
        }
        _ => "outside supported roots",
    };
    if location.is_none() {
        return Err(FOREIGN_DRIVER);
    }
    Ok(path)
}

/// Keep every directory below the known protected root pinned while reading
/// the version and, for a stopped driver, until StartService finishes.
fn pin_image(
    path: &Path,
    windows: &Path,
    programs: &Path,
    trace: &mut Inspection,
) -> Result<Vec<Handle>> {
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
            trace.stage = "driver path owner/ACL/reparse validation";
            trace.path_component = if part == path {
                "driver file"
            } else if part == root {
                "protected root"
            } else if part.as_os_str().eq_ignore_ascii_case(
                windows
                    .join(r"System32\DriverStore\FileRepository")
                    .as_os_str(),
            ) {
                "FileRepository directory"
            } else {
                "ancestor directory"
            };
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

fn image_version(path: &Path, trace: &mut Inspection) -> Result<Option<(u32, u32, u32, u32)>> {
    match file_version(path, trace) {
        Ok(version) => Ok(Some(version)),
        Err(FOREIGN_DRIVER) if trace.version_info_bytes == Some(0) => {
            trace.stage = "legacy driver SHA-256 identity check";
            legacy::verify_image(path)?;
            trace.legacy_driver = true;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn file_version(path: &Path, trace: &mut Inspection) -> Result<(u32, u32, u32, u32)> {
    trace.stage = "GetFileVersionInfoSizeW";
    let path = wide(&path.to_string_lossy());
    let size = unsafe {
        SetLastError(WIN32_ERROR(0));
        GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None)
    };
    trace.version_info_bytes = Some(size);
    if size == 0 {
        trace.native_error = Some(unsafe { GetLastError() }.0);
    }
    if !(52..=1024 * 1024).contains(&size) {
        return Err(FOREIGN_DRIVER);
    }
    let mut data = vec![0u64; (size as usize).div_ceil(8)];
    trace.stage = "GetFileVersionInfoW";
    unsafe { GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()) }
        .map_err(win)?;
    let mut pointer = std::ptr::null_mut();
    let mut len = 0;
    trace.stage = "VerQueryValueW fixed version block";
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
    trace.version = Some((
        info.dwFileVersionMS >> 16,
        info.dwFileVersionMS & 0xffff,
        info.dwFileVersionLS >> 16,
        info.dwFileVersionLS & 0xffff,
    ));
    let mut translation = std::ptr::null_mut();
    let mut translation_bytes = 0;
    if unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            w!(r"\VarFileInfo\Translation"),
            &mut translation,
            &mut translation_bytes,
        )
    }
    .as_bool()
        && !translation.is_null()
        && translation_bytes <= 64
        && translation_bytes % 4 == 0
        && (translation as usize) >= start
        && (translation as usize)
            .checked_add(translation_bytes as usize)
            .is_some_and(|end| end <= start + size as usize)
    {
        for index in 0..translation_bytes as usize / 4 {
            let pair = unsafe { std::ptr::read_unaligned(translation.cast::<u32>().add(index)) };
            trace.translations.push(pair.rotate_left(16));
        }
    }
    let mut original = std::ptr::null_mut();
    let mut characters = 0;
    trace.stage = "VerQueryValueW OriginalFilename (040904b0)";
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
    trace.original_name_matches = Some(name.eq_ignore_ascii_case("PawnIO.sys"));
    trace.stage = "OriginalFilename identity check";
    if trace.original_name_matches == Some(false) {
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
    inspect_with_trace(service, &mut Inspection::default())
}

fn inspect_with_trace(service: &Service, trace: &mut Inspection) -> Result<(Plan, Vec<Handle>)> {
    trace.service_state = state(service).ok().map(|status| status.dwCurrentState.0);
    trace.stage = "QueryServiceConfigW buffer size";
    let mut size = 0;
    unsafe {
        let _ = QueryServiceConfigW(service.0, None, 0, &mut size);
    }
    if size == 0 || size > 64 * 1024 {
        return Err(FOREIGN_DRIVER);
    }
    let mut data = vec![0u64; (size as usize).div_ceil(8)];
    let config = data.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    trace.stage = "QueryServiceConfigW";
    unsafe { QueryServiceConfigW(service.0, Some(config), size, &mut size) }.map_err(win)?;
    let config = unsafe { &*config };
    trace.service_type = Some(config.dwServiceType.0);
    trace.start_type = Some(config.dwStartType.0);
    trace.stage = "service type validation";
    if config.dwServiceType != SERVICE_KERNEL_DRIVER {
        return Err(FOREIGN_DRIVER);
    }
    trace.stage = "service image path decoding";
    let raw = unsafe { config.lpBinaryPathName.to_string() }.map_err(|_| FOREIGN_DRIVER)?;
    let windows = windows_dir()?;
    let programs = program_files()?;
    let path = image_path(&raw, &windows, &programs, trace)?;
    let pins = pin_image(&path, &windows, &programs, trace)?;
    let Some(version) = image_version(&path, trace)? else {
        trace.stage = "legacy installation layout validation";
        if !path
            .as_os_str()
            .eq_ignore_ascii_case(programs.join(r"PawnIO\PawnIO.sys").as_os_str())
        {
            return Err(LEGACY_LAYOUT);
        }
        legacy::validate_installation(&programs)?;
        trace.stage = "inspection complete";
        return Ok((Plan::MigrateLegacy, pins));
    };
    let compatible = version >= MIN_VERSION;
    trace.stage = "QueryServiceStatusEx";
    let running = state(service)?.dwCurrentState == SERVICE_RUNNING;
    trace.stage = "inspection complete";
    Ok((plan(true, compatible, running), pins))
}

/// A fresh, read-only failure snapshot. Every string field is a fixed category;
/// paths, account names and version-resource strings never enter public logs.
pub fn diagnostic_status() -> String {
    let mut trace = Inspection {
        stage: "OpenSCManagerW/OpenServiceW",
        ..Default::default()
    };
    let result =
        open(SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS).and_then(|service| match service {
            Some(service) => inspect_with_trace(&service, &mut trace).map(|(plan, _)| plan),
            None => {
                trace.stage = "service not installed";
                Ok(Plan::Install)
            }
        });
    let service = match crate::status() {
        Ok(status) => format!(
            "privileged_service={}; authorized={}; service_issue={}",
            status.state,
            status.authorized,
            crate::status_issue(&status).unwrap_or_else(|| "none".into())
        ),
        Err(error) => format!(
            "privileged_service_query={}",
            argus_core::diagnostics::failure(&error)
        ),
    };
    format!(
        "elevated={}; {service}; {}",
        argus_core::privilege::is_elevated(),
        trace.summary(&result)
    )
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
    if matches!(plan, Plan::Install | Plan::MigrateLegacy) {
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

fn stage_package(path: &Path, bytes: &[u8], manifest: &package::Manifest) -> Result<std::fs::File> {
    let mut file = crate::install::new_protected_file(path)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(io)?;
    drop(file);
    package::verify(path, manifest)
}

fn run_installer(path: &Path, args: &[&str], legacy: bool) -> Result<()> {
    // Never kill an installer mid-operation or delete its executable while running.
    let directory = path.parent().ok_or(87u32)?;
    let code = Command::new(path)
        .args(args)
        .current_dir(directory)
        // 2.0.1 renames removed files into TEMP, which must be on the same
        // volume as Program Files. Only this child gets the protected TEMP.
        .env("TEMP", directory)
        .env("TMP", directory)
        .creation_flags(0x08000000)
        .status()
        .map_err(io)?
        .code();
    installer_exit(normalize_installer_exit(code, legacy))
}

fn normalize_installer_exit(code: Option<i32>, legacy: bool) -> Option<i32> {
    // 2.0.1 returns NTSTATUS, whereas 2.2.0 returns DOS errors.
    code.map(|value| {
        if legacy && value < 0 {
            unsafe { RtlNtStatusToDosError(NTSTATUS(value)) as i32 }
        } else {
            value
        }
    })
}

fn migrate_then_install(
    uninstall: impl FnOnce() -> Result<()>,
    removed: impl FnOnce() -> Result<bool>,
    install: impl FnOnce() -> Result<()>,
) -> Result<()> {
    uninstall()?;
    if !removed()? {
        return Err(REBOOT_REQUIRED);
    }
    install()
}

fn install_package(root: &Path, migrate: bool) -> Result<()> {
    // Acquire both payloads before touching a working legacy installation.
    let bytes = package::download(package::manifest())?;
    let legacy_bytes = migrate
        .then(|| package::download(package::legacy_manifest()))
        .transpose()?;
    let id = unsafe { CoCreateGuid() }.map_err(win)?;
    let directory = root.join(format!("{id:?}"));
    let guard = protected_dir(&directory)?;
    let installer = directory.join("PawnIO_setup.exe");
    let legacy_installer = directory.join("PawnIO_legacy_setup.exe");
    let result = (|| {
        let _verified = stage_package(&installer, &bytes, package::manifest())?;
        let install = || run_installer(&installer, &INSTALL_ARGS, false);
        if let Some(bytes) = legacy_bytes {
            let _legacy_verified =
                stage_package(&legacy_installer, &bytes, package::legacy_manifest())?;
            // Other monitoring software can change the shared service while downloading.
            // Recheck its type, full driver digest, protected path and uninstall metadata.
            // Release driver path pins before the official uninstaller removes those files.
            if current_plan()? != Plan::MigrateLegacy {
                return Err(LEGACY_LAYOUT);
            }
            migrate_then_install(
                || run_installer(&legacy_installer, &["-uninstall", "-silent"], true),
                || match open(SERVICE_QUERY_STATUS) {
                    Ok(service) => Ok(service.is_none()),
                    Err(1072) => Ok(false),
                    Err(error) => Err(error),
                },
                install,
            )
        } else {
            match current_plan()? {
                Plan::Install => install(),
                Plan::Ready | Plan::Start => Ok(()),
                Plan::MigrateLegacy => Err(LEGACY_LAYOUT),
            }
        }
    })();
    let _ = fs::remove_file(&legacy_installer);
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
    let plan = current_plan()?;
    let migrate = plan == Plan::MigrateLegacy;
    finish_preparation(plan, || install_package(&root, migrate), start)
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
    package::verify(path, package::manifest()).map_err(message)?;
    Ok(package::version())
}

/// Exercises the real HTTPS download and digest check without writing or running it.
pub fn verify_installer_download() -> std::result::Result<&'static str, String> {
    package::download(package::manifest()).map_err(message)?;
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
        println!("{}", diagnostic_status());
    }

    #[test]
    #[ignore = "Read-only regression using locally extracted official installer samples"]
    fn official_driver_samples() {
        let root = std::env::var_os("ARGUS_PAWNIO_SAMPLES").expect("official samples directory");
        for version in ["2.0.1", "2.1.0", "2.2.0"] {
            let path = Path::new(&root).join(version).join("files/9/PawnIO.sys");
            let mut trace = Inspection::default();
            let result = image_version(&path, &mut trace);
            println!(
                "sample={version}; result={result:?}; {}",
                trace.summary(&result.map(|version| if version.is_none() {
                    Plan::MigrateLegacy
                } else {
                    Plan::Install
                }))
            );
            assert!(
                result.is_ok(),
                "official {version} must be recognized for reuse or upgrade"
            );
            assert_eq!(result.unwrap().is_none(), version == "2.0.1");
        }
        let arm = Path::new(&root).join("2.0.1/files/3/PawnIO.sys");
        assert_eq!(image_version(&arm, &mut Inspection::default()), Ok(None));
        let unsigned = Path::new(&root).join("2.0.1/files/11/PawnIO.sys");
        assert_eq!(
            image_version(&unsigned, &mut Inspection::default()),
            Err(FOREIGN_DRIVER)
        );
        package::verify(
            &Path::new(&root).join("2.0.1/PawnIO_setup.exe"),
            package::legacy_manifest(),
        )
        .unwrap();
        package::verify(
            &Path::new(&root).join("2.2.0/PawnIO_setup.exe"),
            package::manifest(),
        )
        .unwrap();
    }

    #[test]
    fn legacy_migration_stops_on_failure_or_pending_removal() {
        for code in [1223, 3010, 1641, 5, 1061, 1072] {
            assert_eq!(
                migrate_then_install(
                    || Err(code),
                    || panic!("uninstall failed"),
                    || panic!("uninstall failed"),
                ),
                Err(code)
            );
        }
        assert_eq!(
            migrate_then_install(
                || Ok(()),
                || Ok(false),
                || panic!("old service still registered"),
            ),
            Err(REBOOT_REQUIRED)
        );
        let order = std::cell::RefCell::new(Vec::new());
        migrate_then_install(
            || {
                order.borrow_mut().push("uninstall");
                Ok(())
            },
            || {
                order.borrow_mut().push("removed");
                Ok(true)
            },
            || {
                order.borrow_mut().push("install");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*order.borrow(), ["uninstall", "removed", "install"]);
        assert_eq!(
            migrate_then_install(|| Ok(()), || Ok(true), || Err(5)),
            Err(5)
        );
    }

    #[test]
    fn legacy_ntstatus_is_preserved_as_a_dos_error() {
        assert_eq!(normalize_installer_exit(Some(0), true), Some(0));
        assert_eq!(
            normalize_installer_exit(Some(0xc0000022u32 as i32), true),
            Some(5)
        );
        assert_eq!(
            normalize_installer_exit(Some(0xc0000022u32 as i32), false),
            Some(0xc0000022u32 as i32)
        );
        for code in [3010, 1072, 1223] {
            assert_eq!(normalize_installer_exit(Some(code), true), Some(code));
        }
    }

    #[test]
    fn driver_diagnostics_name_the_failed_check_without_disclosing_paths() {
        let mut trace = Inspection::default();
        let result = image_path(
            r"C:\Users\Private Person\Secret Folder\PawnIO.sys",
            Path::new(r"C:\Windows"),
            Path::new(r"C:\Program Files"),
            &mut trace,
        );
        assert!(result.is_err());
        let line = trace.summary(&Err(FOREIGN_DRIVER));
        assert!(line.contains("image location policy") && line.contains("outside supported roots"));
        for private in ["Private Person", "Secret Folder", "C:", "\\"] {
            assert!(!line.contains(private));
        }
        let mut trace = Inspection::default();
        assert!(
            image_path(
                r"C:\Program Files\Other Monitor\PawnIO.sys",
                Path::new(r"C:\Windows"),
                Path::new(r"C:\Program Files"),
                &mut trace
            )
            .is_err()
        );
        let line = trace.summary(&Err(FOREIGN_DRIVER));
        assert!(line.contains("other Program Files location"));
        assert!(!line.contains("Other Monitor"));
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
            assert!(image_path(path, windows, programs, &mut Inspection::default()).is_ok());
        }
        for path in [
            r"C:\Users\Public\PawnIO.sys",
            r"C:\Windows\System32\drivers\Other.sys",
            r"C:\Windows\System32\drivers\..\PawnIO.sys",
            r"\\server\share\PawnIO.sys",
            r#""C:\Windows\System32\drivers\PawnIO.sys" --argument"#,
            r"C:\Program Files\PawnIO-other\PawnIO.sys",
        ] {
            assert!(image_path(path, windows, programs, &mut Inspection::default()).is_err());
        }
    }
}
