//! A per-account grant to start only this protected Argus build. Task Scheduler
//! supplies an interactive administrator token; it never stores a password.
mod links;
mod task;
mod ticket;
pub use links::window_relaunch;

use crate::{
    security,
    util::{self, Handle, Result, io, wide, win},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0},
    Security::*,
    System::{Com::CoCreateGuid, Threading::*},
    UI::{Shell::*, WindowsAndMessaging::SW_HIDE},
};
use windows::core::{PCWSTR, PWSTR, w};

const MANAGE: &str = "--argus-admin-manage";
const RUN: &str = "--argus-admin-run";
pub const FALLBACK: &str = "--argus-admin-fallback";
pub const STANDARD_ONCE: &str = "--argus-standard-once";
pub const STARTED: &str = "--argus-admin-started";
pub const HANDOFF: &str = "--argus-admin-handoff";
pub use ticket::{Prepared, acknowledge, handoff_args, prepare, trusted_handoff};

static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
/// A persistent grant must never load a mutable development frontend.
pub fn configure_support(embedded: bool) {
    let _ = SUPPORTED.set(embedded);
}
pub fn supported() -> bool {
    SUPPORTED.get().copied().unwrap_or(false)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub supported: bool,
    pub enabled: bool,
    pub ready: bool,
    pub elevated: bool,
    pub eligible: bool,
    pub error: Option<String>,
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Grant {
    image: String,
    source: PathBuf,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.split('.').all(|part| !part.is_empty())
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
}
fn root(id: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err(87);
    }
    let service = crate::install::directory()?;
    Ok(service
        .parent()
        .ok_or(87u32)?
        .join("Argus Runtime")
        .join(id))
}
fn directory(id: &str, sid: &str) -> Result<PathBuf> {
    security::validate_sid(sid)?;
    Ok(root(id)?.join(sid))
}
fn grant(id: &str, sid: &str) -> Result<Option<(PathBuf, Grant)>> {
    let dir = directory(id, sid)?;
    if !dir.exists() {
        return Ok(None);
    }
    let _root = security::protect_directory(dir.parent().ok_or(87u32)?)?;
    let _dir = security::protect_directory(&dir)?;
    let path = dir.join("grant.json");
    let data = match fs::read(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        result => result.map_err(io)?,
    };
    if data.len() > 64 * 1024 {
        return Err(13);
    }
    let record: Grant = serde_json::from_slice(&data).map_err(|_| 13u32)?;
    if !image_name(&record.image) {
        return Err(13);
    }
    Ok(Some((dir, record)))
}
fn image_name(name: &str) -> bool {
    name.strip_prefix("argus-admin-")
        .and_then(|s| s.strip_suffix(".exe"))
        .is_some_and(ticket::valid_guid)
}
pub fn is_copy(path: &Path, id: &str) -> bool {
    let Ok(sid) = security::current_sid() else {
        return false;
    };
    let Ok(dir) = directory(id, &sid) else {
        return false;
    };
    path.parent().is_some_and(|p| {
        p.to_string_lossy()
            .eq_ignore_ascii_case(&dir.to_string_lossy())
    }) && path
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(image_name)
}
pub fn source(id: &str) -> Option<PathBuf> {
    let sid = security::current_sid().ok()?;
    let dir = directory(id, &sid).ok()?;
    if !dir.exists() {
        return None;
    }
    grant(id, &sid)
        .ok()?
        .map(|(_, r)| r)
        .or_else(|| read_launch(&dir).ok())
        .map(|r| r.source)
}
pub fn launcher(id: &str) -> Option<PathBuf> {
    let sid = security::current_sid().ok()?;
    let dir = directory(id, &sid).ok()?;
    if !dir.exists() {
        return None;
    }
    let record = grant(id, &sid)
        .ok()?
        .map(|(_, r)| r)
        .or_else(|| read_launch(&dir).ok())?;
    let launcher = dir.join("argus-launcher.exe");
    Some(if launcher.is_file() {
        launcher
    } else {
        dir.join(record.image)
    })
}
pub fn launcher_arguments(id: &str) -> String {
    if launcher(id).is_some_and(|p| p.file_name().is_some_and(|f| f == "argus-launcher.exe")) {
        format!("--argus-launch {id}")
    } else {
        String::new()
    }
}
pub fn enabled(id: &str) -> bool {
    security::current_sid()
        .ok()
        .is_some_and(|sid| grant(id, &sid).is_ok_and(|r| r.is_some()))
}

fn own_token() -> Result<Handle> {
    let mut raw = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) }.map_err(win)?;
    Ok(Handle(raw))
}
pub fn eligible() -> bool {
    if argus_core::privilege::is_elevated() {
        return true;
    }
    elevation_type().is_ok_and(|kind| kind == TokenElevationTypeLimited)
}
fn elevation_type() -> Result<TOKEN_ELEVATION_TYPE> {
    let token = own_token()?;
    let mut kind = TOKEN_ELEVATION_TYPE::default();
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevationType,
            Some((&raw mut kind).cast()),
            size_of_val(&kind) as u32,
            &mut size,
        )
    }
    .map_err(win)?;
    Ok(kind)
}
/// Unsplit administrators (for example, with UAC off) already have their
/// account's default rights; revocation must not wait for an impossible
/// non-elevated bootstrap. Query failures still stop the switch before mutation.
pub fn can_drop_elevation() -> std::result::Result<bool, String> {
    elevation_type()
        .map(|kind| kind == TokenElevationTypeFull)
        .map_err(util::message)
}
pub fn status(id: &str) -> Status {
    let mut value = Status {
        supported: supported(),
        enabled: false,
        ready: false,
        elevated: argus_core::privilege::is_elevated(),
        eligible: eligible(),
        error: None,
    };
    let result = (|| {
        let sid = security::current_sid()?;
        let Some((dir, record)) = grant(id, &sid)? else {
            return Ok(());
        };
        value.enabled = true;
        if !value.supported {
            return Ok(());
        }
        let image = dir.join(&record.image);
        if outdated(&dir, &record) {
            return Err(util::ADMIN_OUTDATED);
        }
        let _pin = security::protect_directory(&image)?;
        let _ = task::checked(id, &sid, &image)?;
        value.ready = true;
        Ok(())
    })();
    if let Err(code) = result {
        value.error = Some(util::message(code));
    }
    value
}

fn create_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        let sd = security::directory_descriptor()?;
        unsafe {
            windows::Win32::Storage::FileSystem::CreateDirectoryW(
                PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
                Some(&sd.attributes()),
            )
        }
        .map_err(win)?;
    }
    security::protect_directory(path).map(|_| ())
}

fn install(id: &str, sid: &str, enable: bool) -> Result<()> {
    install_inner(id, sid, enable, false)
}
fn install_inner(id: &str, sid: &str, enable: bool, maintenance: bool) -> Result<()> {
    if enable && !supported() {
        return Err(50);
    }
    let same_account = security::current_sid()? == sid;
    if !argus_core::privilege::is_elevated() || (!same_account && !maintenance) {
        return Err(5);
    }
    security::validate_sid(sid)?;
    let root = root(id)?;
    let base = root.parent().ok_or(87u32)?;
    let _parent = security::protect_directory(base.parent().ok_or(87u32)?)?;
    if !enable && !root.exists() {
        return Ok(());
    }
    create_directory(base)?;
    create_directory(&root)?;
    let dir = directory(id, sid)?;
    create_directory(&dir)?;
    let _base = security::protect_directory(base)?;
    let _root = security::protect_directory(&root)?;
    let _dir = security::protect_directory(&dir)?;
    use std::os::windows::fs::OpenOptionsExt;
    let _lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(dir.join("install.lock"))
        .map_err(io)?;
    let previous = grant(id, sid)?;
    let scheduler = task::Scheduler::new()?;
    if !enable && !maintenance {
        scheduler.remove(id, sid)?;
        match fs::remove_file(dir.join("grant.json")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(e)),
            _ => {}
        }
        // Running images are swept on a later management operation.
        let keep = read_launch(&dir).ok().map(|r| r.image);
        sweep(&dir, keep.as_deref());
        return Ok(());
    }
    let current = std::env::current_exe().map_err(io)?;
    let source = if is_copy(&current, id) {
        previous
            .as_ref()
            .map(|(_, r)| r.clone())
            .or_else(|| read_launch(&dir).ok())
            .ok_or(13u32)?
            .source
    } else {
        links::original(id, &current)
    };
    let image = format!("argus-admin-{}.exe", guid()?);
    let path = dir.join(&image);
    let mut input = fs::File::open(&current).map_err(io)?;
    let mut output = crate::install::new_protected_file(&path)?;
    std::io::copy(&mut input, &mut output).map_err(io)?;
    output.sync_all().map_err(io)?;
    drop(output);
    let record = Grant { image, source };
    if let Some(bytes) = crate::taskmgr::launcher_asset() {
        let pending = dir.join(format!("launcher-{}.exe", guid()?));
        let mut file = crate::install::new_protected_file(&pending)?;
        file.write_all(bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        replace(&pending, &dir.join("argus-launcher.exe"))?;
        let pending = dir.join(format!("icon-{}.ico", guid()?));
        let mut file = crate::install::new_protected_file(&pending)?;
        file.write_all(include_bytes!("../../../../src-tauri/icons/icon.ico"))
            .map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        replace(&pending, &dir.join("argus.ico"))?;
    }
    // Prepare the complete replacement before changing the authoritative task.
    let pending = dir.join(format!("grant-{}.json", guid()?));
    let mut output = crate::install::new_protected_file(&pending)?;
    output
        .write_all(&serde_json::to_vec(&record).map_err(|_| 13u32)?)
        .map_err(io)?;
    output.sync_all().map_err(io)?;
    drop(output);
    let result = (|| {
        // An older replacement copy must also learn the new startup policy.
        if id == "dev.argus.taskmanager" {
            let service = crate::install::directory()?;
            if service.exists() {
                let _guard = security::protect_directory(&service)?;
                crate::taskmgr::follow_update(&service).map_err(|error| {
                    launch_failure("administrator task manager copy refresh", error)
                })?;
            }
        }
        if enable {
            scheduler.register(id, sid, &path)?;
        }
        write_launch(&dir, &record)?;
        if enable {
            replace(&pending, &dir.join("grant.json"))?;
        } else {
            fs::remove_file(&pending).map_err(io)?;
        }
        Ok(())
    })();
    if result.is_err() {
        if let Some((old_dir, old)) = &previous {
            let _ = scheduler.register(id, sid, &old_dir.join(&old.image));
        } else {
            let _ = scheduler.remove(id, sid);
        }
        let _ = fs::remove_file(&pending);
        // Preserve a record that may already have committed; status exposes
        // any inconsistency and repair can complete it with explicit consent.
        return result;
    }
    sweep(&dir, Some(&record.image));
    if same_account {
        links::retarget(id, &record.source, &dir.join("argus-launcher.exe"))?;
    }
    Ok(())
}
fn write_launch(dir: &Path, record: &Grant) -> Result<()> {
    let path = dir.join(format!("launch-{}.json", guid()?));
    let mut file = crate::install::new_protected_file(&path)?;
    file.write_all(&serde_json::to_vec(record).map_err(|_| 13u32)?)
        .map_err(io)?;
    file.sync_all().map_err(io)?;
    drop(file);
    replace(&path, &dir.join("launch.json"))
}
fn read_launch(dir: &Path) -> Result<Grant> {
    let _pin = security::protect_directory(dir)?;
    let data = fs::read(dir.join("launch.json")).map_err(io)?;
    if data.len() > 64 * 1024 {
        return Err(13);
    }
    let value: Grant = serde_json::from_slice(&data).map_err(|_| 13u32)?;
    if !image_name(&value.image) {
        return Err(13);
    }
    Ok(value)
}
fn outdated(dir: &Path, record: &Grant) -> bool {
    crate::taskmgr::image_version(&dir.join(&record.image)).is_some_and(|version| {
        crate::taskmgr::older(version, crate::VERSION)
            || crate::taskmgr::image_version(&record.source).is_some_and(|source| source > version)
    })
}

/// The small asInvoker executable is stable across GUI upgrades and may be
/// pinned or used by Startup Apps. Its action is always a fixed Argus image.
pub fn launcher_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_none_or(|a| a != "--argus-launch") {
        return None;
    }
    Some(
        util::stage(
            "administrator unified launcher",
            (|| {
                let id = args.get(1).filter(|id| valid_id(id)).ok_or(87u32)?;
                if args[2..]
                    .iter()
                    .any(|a| !matches!(a.as_str(), "--hidden" | "--updated"))
                {
                    return Err(87);
                }
                let sid = security::current_sid()?;
                let dir = directory(id, &sid)?;
                let record = read_launch(&dir)?;
                let image = dir.join(&record.image);
                let _image = security::protect_directory(&image)?;
                let mut launch_args = args[2..].to_vec();
                if enabled(id) {
                    match prepare(id, &launch_args, true).and_then(Prepared::commit) {
                        Ok(()) => return Ok(()),
                        Err(_) => launch_args.insert(0, FALLBACK.into()),
                    }
                } else {
                    launch_args.insert(0, STANDARD_ONCE.into());
                }
                // A portable update can be user-writable. Only start its new GUI at
                // standard integrity; copying it into the grant still needs UAC.
                let fallback = if outdated(&dir, &record) && record.source.is_file() {
                    &record.source
                } else {
                    &image
                };
                crate::taskmgr::launch::standard_spawn(fallback, &launch_args)
                    .map_err(|error| launch_failure("administrator launcher fallback", error))?;
                Ok(())
            })(),
        )
        .err()
        .unwrap_or(0) as i32,
    )
}
fn sweep(dir: &Path, keep: Option<&str>) {
    if let Ok(files) = fs::read_dir(dir) {
        for file in files.flatten() {
            if let Some(name) = file.file_name().to_str()
                && image_name(name)
                && Some(name) != keep
            {
                let _ = fs::remove_file(file.path());
            }
        }
    }
}
fn replace(from: &Path, to: &Path) -> Result<()> {
    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    unsafe {
        MoveFileExW(
            PCWSTR(wide(&from.to_string_lossy()).as_ptr()),
            PCWSTR(wide(&to.to_string_lossy()).as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(win)
}
fn guid() -> Result<String> {
    Ok(format!("{:?}", unsafe { CoCreateGuid() }.map_err(win)?)
        .trim_matches(['{', '}'])
        .to_ascii_lowercase())
}
fn launch_failure(stage: &'static str, error: String) -> u32 {
    argus_core::diagnostics::warn(stage, argus_core::diagnostics::failure(&error));
    31
}
fn session(pid: u32) -> Result<u32> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
    }
    let mut value = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut value) } == 0 {
        return Err(win(windows::core::Error::from_thread()));
    }
    if value == 0 {
        return Err(5);
    }
    Ok(value)
}
fn identity(handle: HANDLE) -> Result<u64> {
    let (mut c, mut e, mut k, mut u) = Default::default();
    unsafe { GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u) }.map_err(win)?;
    Ok((u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
}

/// Explicit UI action only. Startup never invokes UAC, even when repair is needed.
pub fn manage(id: &str, enable: bool) -> std::result::Result<Status, String> {
    if enable && !supported() {
        return Err(argus_core::tr!("admin.development"));
    }
    use std::sync::atomic::{AtomicBool, Ordering};
    static MANAGING: AtomicBool = AtomicBool::new(false);
    if MANAGING.swap(true, Ordering::AcqRel) {
        return Err(argus_core::tr!("service.busy"));
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            MANAGING.store(false, Ordering::Release);
        }
    }
    let _guard = Guard;
    if enable
        && let Ok(current) = std::env::current_exe()
        && is_copy(&current, id)
        && let Ok(sid) = security::current_sid()
        && let Ok(Some((dir, record))) = grant(id, &sid)
        && outdated(&dir, &record)
    {
        // Never silently elevate a newer portable executable from its
        // user-writable source. Its own UI must request the new authorization.
        return Err(argus_core::tr!("admin.outdated"));
    }
    if !eligible() {
        return Err(argus_core::tr!("admin.standardAccount"));
    }
    let result = (|| {
        let sid = security::current_sid()?;
        if argus_core::privilege::is_elevated() {
            return install(id, &sid, enable);
        }
        let exe = wide(&std::env::current_exe().map_err(io)?.to_string_lossy());
        let args = wide(&format!(
            "{MANAGE} {} {sid}",
            if enable { "enable" } else { "disable" }
        ));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(exe.as_ptr()),
            lpParameters: PCWSTR(args.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.map_err(win)?;
        let child = Handle(info.hProcess);
        if unsafe { WaitForSingleObject(child.0, INFINITE) } != WAIT_OBJECT_0 {
            return Err(6);
        }
        let mut exit = 0;
        unsafe { GetExitCodeProcess(child.0, &mut exit) }.map_err(win)?;
        crate::diagnostics::collect("administrator authorization helper", child.0, exit);
        if exit != 0 {
            return Err(exit);
        }
        Ok(())
    })();
    util::stage("administrator startup management", result).map_err(util::message)?;
    Ok(status(id))
}

/// The identifier is compiled into the host, never supplied by task arguments.
pub fn run_mode(id: &str) -> Option<i32> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = args.first()?.as_str();
    if !matches!(
        mode,
        MANAGE | RUN | ticket::STANDARD | "--argus-admin-refresh" | "--argus-admin-remove"
    ) {
        return None;
    }
    Some(crate::diagnostics::helper(
        "administrator startup helper",
        || match mode {
            MANAGE => match args.as_slice() {
                [_, action, sid] if matches!(action.as_str(), "enable" | "disable") => {
                    install(id, sid, action == "enable")
                }
                _ => Err(87),
            },
            RUN => match args.as_slice() {
                [_, token] => ticket::broker(id, token),
                _ => Err(87),
            },
            "--argus-admin-refresh" if args.len() == 1 => refresh(id, false),
            "--argus-admin-remove" if args.len() == 1 => refresh(id, true),
            _ => ticket::standard_mode(id, &args).unwrap_or(Err(87)),
        },
    ))
}
fn refresh(id: &str, remove: bool) -> Result<()> {
    if !argus_core::privilege::is_elevated() {
        return Err(5);
    }
    let root = root(id)?;
    if !root.exists() {
        return Ok(());
    }
    let guard = security::protect_directory(&root)?;
    for entry in fs::read_dir(&root).map_err(io)? {
        let entry = entry.map_err(io)?;
        let sid = entry.file_name().to_string_lossy().into_owned();
        security::validate_sid(&sid)?;
        let dir = directory(id, &sid)?;
        let pin = security::protect_directory(&dir)?;
        if remove {
            // Revocation must also work after a failed first installation or a
            // partially completed uninstall. Neither record is needed to
            // identify this account's task and our fixed runtime filenames.
            task::Scheduler::new()?.remove(id, &sid)?;
            remove_runtime_files(&dir)?;
            drop(pin);
            fs::remove_dir(&dir).map_err(io)?;
        } else {
            let authorized = grant(id, &sid)?.is_some();
            // Existing protected records are the only authority for maintenance;
            // an installer cannot create a new grant for a different account.
            if !authorized && !dir.join("launch.json").exists() {
                continue;
            }
            read_launch(&dir)?;
            install_inner(id, &sid, authorized, true)?;
        }
    }
    drop(guard);
    if remove {
        fs::remove_dir(&root).map_err(io)?;
    }
    Ok(())
}

fn runtime_file(name: &str) -> bool {
    image_name(name)
        || matches!(
            name,
            "grant.json" | "launch.json" | "argus-launcher.exe" | "argus.ico" | "install.lock"
        )
        || [
            ("launcher-", ".exe"),
            ("icon-", ".ico"),
            ("grant-", ".json"),
            ("launch-", ".json"),
        ]
        .iter()
        .any(|(prefix, suffix)| {
            name.strip_prefix(prefix)
                .and_then(|s| s.strip_suffix(suffix))
                .is_some_and(ticket::valid_guid)
        })
}
fn remove_runtime_files(dir: &Path) -> Result<()> {
    for file in fs::read_dir(dir).map_err(io)? {
        let file = file.map_err(io)?;
        if file.file_name().to_str().is_some_and(runtime_file) {
            // Never recurse into an unexpected directory or follow a link.
            fs::remove_file(file.path()).map_err(io)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_unconfigured_host_cannot_create_an_administrator_grant() {
        assert_eq!(
            install_inner("dev.argus.unsupported-test", "not-a-sid", true, false),
            Err(50)
        );
    }
    #[test]
    fn runtime_names_cannot_escape_the_protected_directory() {
        for id in [
            "",
            ".",
            "..",
            "app.",
            ".app",
            "..\\other",
            "test/other",
            "test value",
            "x\0y",
        ] {
            assert!(!valid_id(id));
        }
        assert!(valid_id("dev.argus.taskmanager.admin-test"));
        assert!(!image_name("argus-admin-../../other.exe"));
        assert!(image_name(
            "argus-admin-12345678-1234-1234-1234-123456789abc.exe"
        ));
    }
    #[test]
    fn incomplete_runtime_cleanup_keeps_unknown_files_and_needs_no_records() {
        let dir = std::env::temp_dir().join(format!("argus-cleanup-{}", guid().unwrap()));
        fs::create_dir(&dir).unwrap();
        let nonce = guid().unwrap();
        for name in [
            "install.lock".into(),
            "grant.json".into(),
            format!("argus-admin-{nonce}.exe"),
            format!("launcher-{nonce}.exe"),
            format!("icon-{nonce}.ico"),
            format!("grant-{nonce}.json"),
            format!("launch-{nonce}.json"),
            "launcher-unknown.exe".into(),
        ] {
            fs::write(dir.join(name), b"incomplete").unwrap();
        }
        remove_runtime_files(&dir).unwrap();
        let remaining: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(remaining, ["launcher-unknown.exe"]);
        fs::remove_file(dir.join("launcher-unknown.exe")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn interrupted_runtime_cleanup_can_be_retried_after_a_file_is_released() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("argus-cleanup-{}", guid().unwrap()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("argus-launcher.exe");
        fs::write(&path, b"held image").unwrap();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert_eq!(remove_runtime_files(&dir), Err(32));
        assert!(path.exists());
        drop(held);
        remove_runtime_files(&dir).unwrap();
        remove_runtime_files(&dir).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
