//! Installer executable replacement. A file's lockers
//! are not necessarily Argus: only verified instances of the installed image
//! are registered for shutdown. Other processes are reported, never stopped.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::time::Duration;

use argus_core::{diagnostics, tr};
use windows::Win32::Foundation::{
    ERROR_MORE_DATA, ERROR_SUCCESS, FILETIME, GENERIC_READ, GENERIC_WRITE, HANDLE, HWND,
};
use windows::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, ReOpenFile,
};
use windows::Win32::System::Com::CoCreateGuid;
use windows::Win32::System::RestartManager::{
    CCH_RM_SESSION_KEY, RM_PROCESS_INFO, RM_UNIQUE_PROCESS, RmCritical, RmEndSession, RmExplorer,
    RmForceShutdown, RmGetList, RmRegisterResources, RmService, RmShutdown, RmStartSession,
};
use windows::Win32::System::SystemInformation::GetSystemTime;
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IDRETRY, MB_ICONWARNING, MB_RETRYCANCEL, MessageBoxW,
};
use windows::core::{PCWSTR, PWSTR};

use crate::tasks::{Handle, wide};

const ARG: &str = "--argus-installer-copy";
const MAX_LOCKERS: usize = 4096;

fn native_code(code: u32) -> String {
    match code {
        0 => "success; code=0 (0x00000000)".into(),
        234 => "buffer size required; code=234 (0x000000ea)".into(),
        _ => diagnostics::code(code),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Failure {
    stage: &'static str,
    code: u32,
}

impl Failure {
    fn win(stage: &'static str, error: windows::core::Error) -> Self {
        let code = error.code().0 as u32;
        Self {
            stage,
            code: if code & 0xffff_0000 == 0x8007_0000 {
                code & 0xffff
            } else {
                code
            },
        }
    }
}

struct Journal {
    file: Option<File>,
    path: Option<PathBuf>,
    error: Option<u32>,
}

impl Journal {
    fn new() -> Self {
        let result = (|| {
            let id = unsafe { CoCreateGuid() }
                .map_err(|error| Failure::win("CoCreateGuid", error).code)?;
            let path = std::env::temp_dir().join(format!("Argus-installer-{id:?}.log"));
            // CREATE_NEW never follows or overwrites an existing link/file,
            // including when the installer runs with a different user's token.
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
                .open(&path)
                .map_err(|error| error.raw_os_error().unwrap_or(1) as u32)?;
            let reader = unsafe {
                ReOpenFile(
                    HANDLE(file.as_raw_handle()),
                    GENERIC_READ.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    FILE_FLAGS_AND_ATTRIBUTES(0),
                )
            }
            .map_err(|error| Failure::win("ReOpenFile(log reader)", error).code)?;
            // Keep a read-only handle, without delete sharing, across the
            // dialog. Ordinary readers can open the log; its identity stays
            // pinned when an elevated retry reopens it for writing.
            let reader = unsafe { File::from_raw_handle(reader.0) };
            drop(file);
            Ok::<_, u32>((reader, path))
        })();
        match result {
            Ok((file, path)) => Self {
                file: Some(file),
                path: Some(path),
                error: None,
            },
            Err(code) => Self {
                file: None,
                path: None,
                error: Some(code),
            },
        }
    }

    fn event(&mut self, message: impl AsRef<str>) {
        let time = unsafe { GetSystemTime() };
        let timestamp = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            time.wYear,
            time.wMonth,
            time.wDay,
            time.wHour,
            time.wMinute,
            time.wSecond,
            time.wMilliseconds
        );
        if let Some(reader) = &self.file {
            let result = (|| {
                let handle = unsafe {
                    ReOpenFile(
                        HANDLE(reader.as_raw_handle()),
                        GENERIC_WRITE.0,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        FILE_FLAGS_AND_ATTRIBUTES(0),
                    )
                }
                .map_err(|error| Failure::win("ReOpenFile(log writer)", error).code)?;
                let mut writer = unsafe { File::from_raw_handle(handle.0) };
                writer
                    .seek(SeekFrom::End(0))
                    .and_then(|_| writeln!(writer, "{timestamp} {}", message.as_ref()))
                    .and_then(|()| writer.flush())
                    .map_err(|error| error.raw_os_error().unwrap_or(1) as u32)
            })();
            if let Err(code) = result {
                self.error = Some(code);
            }
        }
    }

    fn result(&mut self, stage: &'static str, code: u32) {
        self.event(format!("stage={stage}; {}", native_code(code)));
    }
}

struct Session(u32);

impl Session {
    fn new(log: &mut Journal) -> Result<Self, Failure> {
        let mut key = [0; CCH_RM_SESSION_KEY as usize + 1];
        let mut handle = 0;
        let result = unsafe { RmStartSession(&mut handle, None, PWSTR(key.as_mut_ptr())) };
        log.result("RmStartSession", result.0);
        if result == ERROR_SUCCESS {
            Ok(Self(handle))
        } else {
            Err(Failure {
                stage: "RmStartSession",
                code: result.0,
            })
        }
    }

    fn list(&self, log: &mut Journal) -> Result<Vec<RM_PROCESS_INFO>, Failure> {
        let mut infos: Vec<RM_PROCESS_INFO> = Vec::new();
        for _ in 0..5 {
            let (mut needed, mut count, mut reasons) = (0, infos.len() as u32, 0);
            let result = unsafe {
                RmGetList(
                    self.0,
                    &mut needed,
                    &mut count,
                    (!infos.is_empty()).then_some(infos.as_mut_ptr()),
                    &mut reasons,
                )
            };
            log.event(format!("stage=RmGetList; {}; needed={needed}; count={count}; reboot_reasons=0x{reasons:08x}", native_code(result.0)));
            if result == ERROR_SUCCESS {
                infos.truncate(count as usize);
                return Ok(infos);
            }
            if result != ERROR_MORE_DATA || needed as usize > MAX_LOCKERS {
                return Err(Failure {
                    stage: "RmGetList",
                    code: result.0,
                });
            }
            infos.resize_with((needed as usize + 4).min(MAX_LOCKERS), Default::default);
        }
        Err(Failure {
            stage: "RmGetList",
            code: ERROR_MORE_DATA.0,
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = unsafe { RmEndSession(self.0) };
    }
}

struct Locker {
    info: RM_PROCESS_INFO,
    image: Option<PathBuf>,
    ours: bool,
}

fn from_wide(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|&c| c == 0).unwrap_or(value.len())])
}

fn same_path(left: &Path, right: &Path) -> bool {
    let clean = |path: &Path| {
        let value: Vec<u16> = path.as_os_str().encode_wide().collect();
        if value.starts_with(&[92, 92, 63, 92, 85, 78, 67, 92]) {
            [vec![92, 92], value[8..].to_vec()].concat()
        } else if value.starts_with(&[92, 92, 63, 92]) {
            value[4..].to_vec()
        } else {
            value
        }
    };
    unsafe { CompareStringOrdinal(&clean(left), &clean(right), true) == CSTR_EQUAL }
}

fn identity(info: &RM_PROCESS_INFO) -> Result<PathBuf, Failure> {
    let handle = Handle(
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                info.Process.dwProcessId,
            )
        }
        .map_err(|error| Failure::win("OpenProcess", error))?,
    );
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }
        .map_err(|error| Failure::win("GetProcessTimes", error))?;
    if created != info.Process.ProcessStartTime {
        return Err(Failure {
            stage: "process identity changed",
            code: 1168,
        });
    }
    let mut buffer = vec![0u16; 32768];
    let mut size = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    }
    .map_err(|error| Failure::win("QueryFullProcessImageNameW", error))?;
    Ok(PathBuf::from(OsString::from_wide(&buffer[..size as usize])))
}

fn may_close(info: &RM_PROCESS_INFO, image: Option<&Path>, target: &Path) -> bool {
    info.Process.dwProcessId != std::process::id()
        && target
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("argus.exe"))
        && info.Process.ProcessStartTime != FILETIME::default()
        && ![RmService, RmCritical, RmExplorer].contains(&info.ApplicationType)
        && image.is_some_and(|path| same_path(path, target))
}

fn lockers(target: &Path, log: &mut Journal) -> Result<Vec<Locker>, Failure> {
    let resolved = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    let session = Session::new(log)?;
    let path: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let result =
        unsafe { RmRegisterResources(session.0, Some(&[PCWSTR(path.as_ptr())]), None, None) };
    log.result("RmRegisterResources(file)", result.0);
    if result != ERROR_SUCCESS {
        return Err(Failure {
            stage: "RmRegisterResources(file)",
            code: result.0,
        });
    }
    Ok(session.list(log)?.into_iter().map(|info| {
        let image = match identity(&info) {
            Ok(path) => Some(path),
            Err(error) => {
                log.event(format!("pid={}; stage={}; {}", info.Process.dwProcessId, error.stage, diagnostics::code(error.code)));
                None
            }
        };
        let ours = may_close(&info, image.as_deref(), &resolved);
        log.event(format!("locker: pid={}; created={}:{}; type={}; status=0x{:08x}; session={}; close_allowed={ours}; image={}",
            info.Process.dwProcessId, info.Process.ProcessStartTime.dwHighDateTime, info.Process.ProcessStartTime.dwLowDateTime,
            info.ApplicationType.0, info.AppStatus, info.TSSessionId,
            image.as_deref().map(diagnostics::path).unwrap_or_else(|| "unavailable".into())));
        Locker { info, image, ours }
    }).collect())
}

fn close_ours(target: &Path, lockers: &[Locker], log: &mut Journal) -> Result<(), Failure> {
    let resolved = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    let targets: Vec<RM_UNIQUE_PROCESS> = lockers
        .iter()
        .filter(|locker| locker.ours)
        .filter(|locker| {
            identity(&locker.info)
                .is_ok_and(|image| may_close(&locker.info, Some(&image), &resolved))
        })
        .map(|locker| locker.info.Process)
        .collect();
    if targets.is_empty() {
        return Ok(());
    }
    let session = Session::new(log)?;
    // A separate process-only session cannot pick up a new third-party file
    // holder between enumeration and RmShutdown, unlike a file registration.
    let result = unsafe { RmRegisterResources(session.0, None, Some(&targets), None) };
    log.result("RmRegisterResources(Argus processes)", result.0);
    if result != ERROR_SUCCESS {
        return Err(Failure {
            stage: "RmRegisterResources(Argus processes)",
            code: result.0,
        });
    }
    let registered = session.list(log)?;
    if registered.iter().any(|item| {
        !targets.iter().any(|target| {
            item.Process.dwProcessId == target.dwProcessId
                && item.Process.ProcessStartTime == target.ProcessStartTime
        })
    }) {
        log.result("unexpected shutdown target", 13);
        return Err(Failure {
            stage: "unexpected shutdown target",
            code: 13,
        });
    }
    let result = unsafe { RmShutdown(session.0, RmForceShutdown.0 as u32, None) };
    log.result("RmShutdown(Argus processes only)", result.0);
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(Failure {
            stage: "RmShutdown",
            code: result.0,
        })
    }
}

/// Opens for writing without truncating or modifying the installed program.
/// This also detects locks Windows cannot attribute to a queryable process.
fn writable(target: &Path, log: &mut Journal) -> Result<(), Failure> {
    match OpenOptions::new()
        .write(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
        .open(target)
    {
        Ok(_) => {
            log.result("open installed file for writing", 0);
            Ok(())
        }
        Err(error) => {
            let code = error.raw_os_error().unwrap_or(1) as u32;
            log.result("open installed file for writing", code);
            if matches!(code, 2 | 3) {
                Ok(())
            } else {
                Err(Failure {
                    stage: "open installed file for writing",
                    code,
                })
            }
        }
    }
}

struct Blocked {
    // A read-only mapping can prevent replacement even when opening a write
    // handle succeeds. Keep that decision separate from native API failures.
    failure: Option<Failure>,
    lockers: Vec<Locker>,
}

impl Blocked {
    fn exit_code(&self) -> u32 {
        self.failure.map_or(32, |failure| failure.code.max(1))
    }
}

fn target_exists(target: &Path, log: &mut Journal) -> Result<bool, Failure> {
    match std::fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            log.event(format!(
                "target safety check: not a regular file; path={}",
                diagnostics::path(target)
            ));
            Err(Failure {
                stage: "target safety check",
                code: 0x2000_0001,
            })
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            let code = error.raw_os_error().unwrap_or(1) as u32;
            log.result("installed file metadata", code);
            Err(Failure {
                stage: "installed file metadata",
                code,
            })
        }
    }
}

fn prepare(target: &Path, log: &mut Journal) -> Result<(), Blocked> {
    match target_exists(target, log) {
        Ok(false) => {
            log.event("installed file is absent");
            return Ok(());
        }
        Ok(true) => {}
        Err(failure) => {
            return Err(Blocked {
                failure: Some(failure),
                lockers: Vec::new(),
            });
        }
    }
    let initial = writable(target, log).err();
    let found = match lockers(target, log) {
        Ok(found) => found,
        Err(failure) => {
            log.result(failure.stage, failure.code);
            if let Some(failure) = initial {
                return Err(Blocked {
                    failure: Some(failure),
                    lockers: Vec::new(),
                });
            }
            return Ok(()); // The copy still checks actual write access.
        }
    };
    if found.is_empty() && initial.is_none() {
        return Ok(());
    }
    let shutdown = close_ours(target, &found, log).err();
    if let Some(failure) = shutdown {
        // A process may finish exiting despite a failed shutdown call. Keep
        // the native failure, but decide from the current file state below.
        log.result(failure.stage, failure.code);
    }
    for _ in 0..10 {
        if writable(target, log).is_ok() {
            break;
        }
        if !found.iter().any(|locker| locker.ours) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let current = match lockers(target, log) {
        Ok(current) => current,
        Err(error) => {
            log.result(error.stage, error.code);
            found
        }
    };
    let mut failure = writable(target, log).err();
    if !current.iter().any(|locker| locker.ours) && failure.is_none() {
        // Permissive readers need not exit. The copy below detects mapping-only
        // locks as well as locks acquired after this snapshot.
        return Ok(());
    }
    if current.iter().any(|locker| locker.ours) {
        failure = shutdown.or(failure);
    }
    Err(Blocked {
        failure,
        lockers: current,
    })
}

fn copy_main(source: &Path, target: &Path, log: &mut Journal) -> Result<(), Blocked> {
    target_exists(target, log).map_err(|failure| Blocked {
        failure: Some(failure),
        lockers: Vec::new(),
    })?;
    // Mappings with a closed file handle can be absent from Restart Manager,
    // and a process can acquire the file after preflight. Capture the actual
    // replacement result too, inside the same diagnostic/retry operation.
    let io = |stage, error: std::io::Error| Failure {
        stage,
        code: error.raw_os_error().unwrap_or(1) as u32,
    };
    let result = (|| {
        let mut input = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(source)
            .map_err(|error| io("open update source", error))?;
        let modified = input
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        // Preserve NSIS's destination inheritance instead of copying %TEMP%'s
        // security resource properties or encryption to a per-machine install.
        let mut output = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(target)
            .map_err(|error| io("open installed file for replacement", error))?;
        let count = std::io::copy(&mut input, &mut output)
            .map_err(|error| io("copy program bytes", error))?;
        output
            .sync_all()
            .map_err(|error| io("FlushFileBuffers", error))?;
        if let Some(modified) = modified
            && let Err(error) = output.set_times(std::fs::FileTimes::new().set_modified(modified))
        {
            log.event(format!(
                "stage=restore file timestamp; {}",
                diagnostics::io(&error)
            ));
        }
        Ok::<_, Failure>(count)
    })();
    match result {
        Ok(count) => {
            log.event(format!("program file copied; bytes={count}"));
            Ok(())
        }
        Err(failure) => {
            log.event(format!(
                "stage={}; {}; source={}; target={}",
                failure.stage,
                diagnostics::code(failure.code),
                diagnostics::path(source),
                diagnostics::path(target)
            ));
            let lockers = lockers(target, log).unwrap_or_default();
            Err(Blocked {
                failure: Some(failure),
                lockers,
            })
        }
    }
}

fn label(locker: &Locker) -> String {
    let name = locker
        .image
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| from_wide(&locker.info.strAppName));
    let name: String = name.chars().filter(|c| !c.is_control()).take(72).collect();
    format!(
        "{} · PID {}",
        if name.is_empty() {
            tr!("installer.unknownProcess")
        } else {
            name
        },
        locker.info.Process.dwProcessId
    )
}

fn message(blocked: &Blocked, log: &Journal) -> String {
    let reason = if !blocked.lockers.is_empty() {
        tr!("installer.occupied")
    } else {
        match blocked.failure {
            Some(Failure { code: 32 | 33, .. }) => tr!("installer.unknownLocker"),
            Some(Failure { code: 1224, .. }) => tr!("installer.mappedFile"),
            Some(Failure {
                stage: "target safety check",
                ..
            }) => tr!("installer.unsafeTarget"),
            Some(Failure {
                stage:
                    "open installed file for writing"
                    | "open installed file for replacement"
                    | "open update source",
                code: 5,
            }) => tr!("installer.accessDenied"),
            Some(Failure { code: 39 | 112, .. }) => tr!("installer.diskFull"),
            Some(Failure {
                code: 350 | 121, ..
            }) => tr!("installer.restartRequired"),
            Some(Failure {
                stage:
                    "copy program bytes"
                    | "FlushFileBuffers"
                    | "open update source"
                    | "open installed file for replacement",
                ..
            }) => tr!("installer.copyFailed"),
            _ => tr!("installer.checkFailed"),
        }
    };
    let mut text = reason;
    for locker in blocked.lockers.iter().take(8) {
        text.push_str(&format!("\n{}", label(locker)));
    }
    if blocked.lockers.len() > 8 {
        text.push_str(&format!(
            "\n{}",
            tr!("installer.moreProcesses", count = blocked.lockers.len() - 8)
        ));
    }
    text.push_str(&format!("\n\n{}", tr!("installer.retryHint")));
    if let Some(failure) = blocked.failure {
        text.push_str(&format!(
            "\n{}",
            tr!(
                "installer.errorCode",
                code = format!("{} (0x{:08X})", failure.code, failure.code)
            )
        ));
    }
    if let Some(path) = &log.path {
        text.push_str(&format!(
            "\n\n{}",
            tr!("installer.logPath", path = path.display())
        ));
    }
    if let Some(code) = log.error {
        text.push_str(&format!("\n{}", tr!("installer.logFailed", code = code)));
    }
    text
}

/// Called by the new executable extracted to NSIS's private plugin directory,
/// before single-instance handling, WebView2 or privileged-service setup.
pub fn run_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_none_or(|arg| arg != ARG) {
        return None;
    }
    if args.len() != 5 {
        return Some(87);
    }
    let target = PathBuf::from(&args[1]);
    let Some(parent) = args[2].to_str().and_then(|value| {
        value.parse::<usize>().ok().or_else(|| {
            value
                .parse::<i32>()
                .ok()
                .map(|value| value as isize as usize)
        })
    }) else {
        return Some(87);
    };
    let language = match args[3].to_str() {
        Some("2052") => argus_core::i18n::Locale::ZhCn,
        Some("1033") => argus_core::i18n::Locale::EnUs,
        _ => return Some(87),
    };
    let silent = match args[4].to_str() {
        Some("silent") => true,
        Some("interactive") => false,
        _ => return Some(87),
    };
    if !target.is_absolute()
        || !target
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("argus.exe"))
    {
        return Some(87);
    }
    argus_core::i18n::set_locale(language);
    let mut log = Journal::new();
    let source = match std::env::current_exe() {
        Ok(source) => source,
        Err(error) => {
            let code = error.raw_os_error().unwrap_or(1) as u32;
            log.result("current executable path", code);
            return Some(code.max(1) as i32);
        }
    };
    log.event(format!(
        "installer preflight: version={}; silent={silent}; elevated={}; target={}",
        env!("CARGO_PKG_VERSION"),
        argus_core::privilege::is_elevated(),
        diagnostics::path(&target)
    ));
    loop {
        match prepare(&target, &mut log).and_then(|()| copy_main(&source, &target, &mut log)) {
            Ok(()) => {
                log.event("result=copied");
                return Some(0);
            }
            Err(blocked) => {
                if let Some(failure) = blocked.failure {
                    log.event(format!(
                        "result=blocked; stage={}; {}",
                        failure.stage,
                        diagnostics::code(failure.code)
                    ));
                } else {
                    log.event("result=blocked; reason=file holders remain; native_error=none; helper_exit=32");
                }
                if silent {
                    return Some(blocked.exit_code() as i32);
                }
                let text = wide(&message(&blocked, &log));
                let title = wide(&tr!("installer.title"));
                let answer = unsafe {
                    MessageBoxW(
                        Some(HWND(parent as *mut _)),
                        PCWSTR(text.as_ptr()),
                        PCWSTR(title.as_ptr()),
                        MB_RETRYCANCEL | MB_ICONWARNING,
                    )
                };
                if answer != IDRETRY {
                    log.event("result=cancelled");
                    return Some(1223);
                }
                log.event("action=retry");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::time::Instant;
    use windows::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_READ, MapViewOfFile, PAGE_READONLY, UnmapViewOfFile,
    };

    struct Fixture(Child);

    #[test]
    fn diagnostic_log_is_readable_while_a_retry_waits_and_keeps_its_identity() {
        use std::io::Read;
        let mut log = Journal::new();
        log.result("first check", 32);
        let path = log.path.clone().unwrap();
        let mut reader = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(&path)
            .unwrap();
        let mut text = String::new();
        reader.read_to_string(&mut text).unwrap();
        assert!(text.contains("code=32 (0x00000020)"));
        drop(reader);
        assert!(std::fs::rename(&path, path.with_extension("moved")).is_err());
        log.result("retry", 0);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("stage=retry; success; code=0"));
        assert_eq!(log.error, None);
        drop(log);
        std::fs::remove_file(path).unwrap();
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    #[ignore = "subprocess fixture, run by a parent test"]
    fn held_file_fixture() {
        let path = PathBuf::from(std::env::var_os("ARGUS_INSTALLER_TEST_FILE").unwrap());
        let mapped =
            std::env::var_os("ARGUS_INSTALLER_TEST_MAPPING").is_some_and(|value| value == "1");
        let held = OpenOptions::new()
            .read(true)
            .share_mode(if mapped {
                FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0
            } else {
                FILE_SHARE_READ.0
            })
            .open(&path)
            .unwrap();
        let mapping = mapped.then(|| {
            let handle = Handle(
                unsafe {
                    CreateFileMappingW(
                        HANDLE(held.as_raw_handle()),
                        None,
                        PAGE_READONLY,
                        0,
                        0,
                        PCWSTR::null(),
                    )
                }
                .unwrap(),
            );
            let view = unsafe { MapViewOfFile(handle.0, FILE_MAP_READ, 0, 0, 0) };
            assert!(!view.Value.is_null());
            unsafe {
                std::ptr::read_volatile(view.Value.cast::<u8>());
            }
            (handle, view)
        });
        // A mapping must remain detectable after the original file handle closes.
        let held = if mapped {
            drop(held);
            None
        } else {
            Some(held)
        };
        std::fs::write(path.with_extension("ready"), b"").unwrap();
        let until = Instant::now() + Duration::from_secs(60);
        while !path.with_extension("release").exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(25));
        }
        if let Some((_handle, view)) = mapping {
            unsafe { UnmapViewOfFile(view) }.unwrap();
        }
        drop(held);
    }

    #[test]
    fn external_locker_is_reported_and_retry_succeeds_without_ending_it() {
        check_external_locker(false);
    }

    #[test]
    fn read_only_mapping_is_reported_even_when_a_write_handle_can_open() {
        check_external_locker(true);
    }

    fn check_external_locker(mapped: bool) {
        let directory = std::env::temp_dir().join(format!(
            "argus-locker-test-{:?}",
            unsafe { CoCreateGuid() }.unwrap()
        ));
        std::fs::create_dir(&directory).unwrap();
        let target = directory.join("argus.exe");
        let source = directory.join("replacement.exe");
        std::fs::write(&source, b"replacement program").unwrap();
        std::fs::write(&target, b"unchanged program").unwrap();
        let mut child = Fixture(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "installer::tests::held_file_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("ARGUS_INSTALLER_TEST_FILE", &target)
                .env(
                    "ARGUS_INSTALLER_TEST_MAPPING",
                    if mapped { "1" } else { "0" },
                )
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let until = Instant::now() + Duration::from_secs(15);
        while !target.with_extension("ready").exists() {
            assert!(Instant::now() < until, "file holder did not start");
            assert!(child.0.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(25));
        }
        let mut log = Journal::new();
        if mapped {
            assert!(writable(&target, &mut log).is_ok());
        }
        let attempt = if mapped {
            assert!(prepare(&target, &mut log).is_ok());
            copy_main(&source, &target, &mut log)
        } else {
            prepare(&target, &mut log)
        };
        let blocked = match attempt {
            Ok(()) => panic!("file lock was ignored"),
            Err(blocked) => blocked,
        };
        assert_eq!(
            blocked.failure.unwrap().code,
            if mapped { 1224 } else { 32 }
        );
        assert_eq!(blocked.exit_code(), if mapped { 1224 } else { 32 });
        if !mapped {
            assert!(
                blocked
                    .lockers
                    .iter()
                    .any(|locker| locker.info.Process.dwProcessId == child.0.id() && !locker.ours)
            );
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "external process was ended"
        );
        std::fs::write(target.with_extension("release"), b"").unwrap();
        assert!(child.0.wait().unwrap().success());
        assert!(prepare(&target, &mut log).is_ok());
        assert_eq!(std::fs::read(&target).unwrap(), b"unchanged program");
        if mapped {
            assert!(copy_main(&source, &target, &mut log).is_ok());
            assert_eq!(std::fs::read(&target).unwrap(), b"replacement program");
        }
        let path = log.path.clone().unwrap();
        drop(log);
        let recorded = std::fs::read_to_string(&path).unwrap();
        assert_eq!(recorded.contains("code=32 (0x00000020)"), !mapped);
        if mapped {
            assert!(recorded.contains("code=1224 (0x000004c8)"));
        } else {
            assert!(recorded.contains(&format!("locker: pid={}", child.0.id())));
        }
        assert!(!recorded.contains("RmShutdown"));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(source).unwrap();
        for file in [
            &target,
            &target.with_extension("ready"),
            &target.with_extension("release"),
        ] {
            std::fs::remove_file(file).unwrap();
        }
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn shutdown_requires_the_exact_image_and_a_non_service_process() {
        let target = Path::new(r"C:\Program Files\Argus\argus.exe");
        let mut info = RM_PROCESS_INFO::default();
        info.Process.dwProcessId = 10;
        info.Process.ProcessStartTime.dwLowDateTime = 42;
        assert!(may_close(
            &info,
            Some(Path::new(r"\\?\C:\PROGRAM FILES\Argus\argus.exe")),
            target
        ));
        for image in [
            None,
            Some(Path::new(r"C:\Other\argus.exe")),
            Some(Path::new(r"C:\Guard\SGuard64.exe")),
        ] {
            assert!(!may_close(&info, image, target));
        }
        for kind in [RmService, RmCritical, RmExplorer] {
            info.ApplicationType = kind;
            assert!(!may_close(&info, Some(target), target));
        }
    }

    #[test]
    fn path_identity_preserves_windows_unicode_and_unc_names() {
        assert!(same_path(
            Path::new(r"\\?\UNC\server\share\argus.exe"),
            Path::new(r"\\SERVER\share\ARGUS.EXE")
        ));
        let mut left: Vec<u16> = r"C:\".encode_utf16().collect();
        left.push(0xd800);
        left.extend(r"\argus.exe".encode_utf16());
        let mut right = left.clone();
        right[3] = 0xd801;
        assert!(!same_path(
            &PathBuf::from(OsString::from_wide(&left)),
            &PathBuf::from(OsString::from_wide(&right))
        ));
    }

    #[test]
    fn write_probe_preserves_bytes_and_reports_sharing_violations() {
        let directory = std::env::temp_dir().join(format!(
            "argus-installer-test-{:?}",
            unsafe { CoCreateGuid() }.unwrap()
        ));
        std::fs::create_dir(&directory).unwrap();
        let target = directory.join("argus.exe");
        let original = b"installer preflight must not change this file";
        std::fs::write(&target, original).unwrap();
        let mut log = Journal::new();
        assert!(writable(&target, &mut log).is_ok());
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(&target)
            .unwrap();
        assert_eq!(writable(&target, &mut log).unwrap_err().code, 32);
        drop(held);
        assert!(writable(&target, &mut log).is_ok());
        assert_eq!(std::fs::read(&target).unwrap(), original);
        std::fs::remove_file(&target).unwrap();
        assert!(writable(&target, &mut log).is_ok());
        std::fs::remove_dir(directory).unwrap();
        let path = log.path.clone();
        drop(log);
        if let Some(path) = path {
            std::fs::remove_file(path).unwrap()
        }
    }
}
