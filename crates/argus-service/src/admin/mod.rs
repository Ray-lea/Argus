//! A per-account grant to start only this protected Argus build. Task Scheduler
//! supplies an interactive administrator token; it never stores a password.
mod commit;
mod image;
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
/// The administrator copy of Argus the task starts. Argus 0.5 and earlier gave
/// each copy a name of its own, `argus-admin-<GUID>.exe`, which a record may
/// still name.
const IMAGE: &str = "argus-admin.exe";
const STEM: &str = "argus-admin";
const LAUNCHER: &str = "argus-launcher.exe";
const ICON: &str = "argus.ico";
/// A key of the registry hive that holds the Task Scheduler's registrations
/// (`Schedule\TaskCache`), which reach the disk lazily, as the SCM's do.
/// Flushing any key writes its whole hive; this one every account may open.
const TASK_HIVE: &str = "SOFTWARE";

static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
/// A persistent grant is offered by release builds only, never by a mutable
/// development build.
pub fn configure_support(release: bool) {
    let _ = SUPPORTED.set(release);
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
/// The identifier's folders, one for each account: `runtime\<id>` in the
/// protected folder.
fn root(id: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err(87);
    }
    Ok(crate::layout::runtime()?.join(id))
}
/// Where Argus 0.5 and earlier kept them: `Program Files\Argus Runtime\<id>`.
fn legacy_root(id: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err(87);
    }
    Ok(crate::layout::legacy_runtime()?.join(id))
}
fn directory(id: &str, sid: &str) -> Result<PathBuf> {
    security::validate_sid(sid)?;
    Ok(root(id)?.join(sid))
}
fn legacy_directory(id: &str, sid: &str) -> Result<PathBuf> {
    security::validate_sid(sid)?;
    Ok(legacy_root(id)?.join(sid))
}
/// Whether `dir` holds an account's records.
fn records(dir: &Path) -> bool {
    dir.join("launch.json").exists() || dir.join("grant.json").exists()
}
/// The account's folder that holds its records: the protected folder's, or
/// one Argus 0.5 or earlier made, until a grant or maintenance moves it
/// (`install_inner`). Records are only ever written to the protected folder.
fn located(id: &str, sid: &str) -> Result<PathBuf> {
    Ok(located_in(directory(id, sid)?, legacy_directory(id, sid)?))
}
fn located_in(dir: PathBuf, legacy: PathBuf) -> PathBuf {
    let granted = |dir: &Path| dir.join("grant.json").exists();
    // A grant the move has not committed yet (cut short after the launch
    // record) is still the earlier folder's: revocation deletes both.
    let moving = granted(&legacy) && !granted(&dir);
    if (records(&dir) && !moving) || !records(&legacy) {
        dir
    } else {
        legacy
    }
}
fn grant(id: &str, sid: &str) -> Result<Option<(PathBuf, Grant)>> {
    let dir = located(id, sid)?;
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
/// What a record may name: `IMAGE`, or a copy of Argus 0.5 or earlier.
fn image_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(IMAGE) || legacy_image_name(name)
}
fn legacy_image_name(name: &str) -> bool {
    name.strip_prefix("argus-admin-")
        .and_then(|s| s.strip_suffix(".exe"))
        .is_some_and(util::valid_guid)
}
/// An administrator copy's file: one a record may name, or the name a
/// replaced copy keeps while it still runs.
fn copy_name(name: &str) -> bool {
    image_name(name) || crate::layout::is_aside(STEM, name)
}
/// Whether `path` is an administrator copy of this account's, in its folder
/// or the one Argus 0.5 or earlier kept.
pub fn is_copy(path: &Path, id: &str) -> bool {
    let Ok(sid) = security::current_sid() else {
        return false;
    };
    let within = |dir: Result<PathBuf>| {
        dir.is_ok_and(|dir| {
            path.parent().is_some_and(|p| {
                p.to_string_lossy()
                    .eq_ignore_ascii_case(&dir.to_string_lossy())
            })
        })
    };
    (within(directory(id, &sid)) || within(legacy_directory(id, &sid)))
        && path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(copy_name)
}
pub fn source(id: &str) -> Option<PathBuf> {
    let sid = security::current_sid().ok()?;
    let dir = located(id, &sid).ok()?;
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
    let dir = located(id, &sid).ok()?;
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
        let dir = located(id, &sid)?;
        // Every grant creates this directory before it registers the task.
        // Without it there is nothing to show or revoke, and no reason to
        // start Task Scheduler on every launch.
        if !dir.exists() {
            return Ok(());
        }
        // Task Scheduler is authoritative even when runtime records are no
        // longer trusted. Keep revocation available for a damaged grant.
        value.enabled = match task::Scheduler::new().and_then(|s| s.exists(id, &sid)) {
            Ok(enabled) => enabled,
            // Without a grant record the account has revoked administrator
            // startup, or never completed it: a failure has nothing to report.
            Err(_) if !dir.join("grant.json").exists() => return Ok(()),
            Err(code) => return Err(code),
        };
        if !value.enabled {
            return Ok(());
        }
        let Some((dir, record)) = grant(id, &sid)? else {
            return Err(util::ADMIN_OUTDATED);
        };
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
    if !enable && !maintenance {
        // Establish Task Scheduler's COM security before shell folder resolution.
        let scheduler = task::Scheduler::new()?;
        let current = located(id, sid)?;
        revoke(&current, || scheduler.remove(id, sid))?;
        let (dir, legacy) = (directory(id, sid)?, legacy_directory(id, sid)?);
        if current != dir {
            return Ok(());
        }
        // A grant of Argus 0.5 or earlier the move did not finish goes too,
        // or it would count again.
        revoke(&legacy, || Ok(()))?;
        // The standard launcher stays; this account's shortcuts and sign-in
        // entry follow it from where Argus 0.5 or earlier kept it, if one was
        // left there to forward. That goes once the task's deletion is on
        // disk, which could otherwise bring back one that names its copy.
        let moved = legacy.join(LAUNCHER);
        let followed = !moved.is_file() || links::follow(id, &moved, &dir.join(LAUNCHER));
        if flush_tasks() {
            retire_legacy(id, sid, !followed);
        }
        return Ok(());
    }
    // Establish Task Scheduler's COM security before shell folder resolution.
    let early_scheduler = if enable {
        Some(task::Scheduler::new()?)
    } else {
        None
    };
    let root = root(id)?;
    if !enable && !root.exists() && !legacy_root(id)?.exists() {
        return Ok(());
    }
    // Program Files and the protected folder, which administrator startup
    // may make before the service does or keep after it is gone.
    let _pins = crate::layout::ensure_root()?;
    let _runtime = crate::layout::create_protected(root.parent().ok_or(87u32)?)?;
    let _root = crate::layout::create_protected(&root)?;
    let dir = directory(id, sid)?;
    let _dir = crate::layout::create_protected(&dir)?;
    use std::os::windows::fs::OpenOptionsExt;
    let _lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(dir.join("install.lock"))
        .map_err(io)?;
    // Where the records are until this moves them: Argus 0.5 and earlier
    // kept them in a folder of their own.
    let current_dir = located(id, sid)?;
    let moving = current_dir != dir;
    let previous = grant(id, sid)?;
    let scheduler = match early_scheduler {
        Some(scheduler) => scheduler,
        None => task::Scheduler::new()?,
    };
    let current = std::env::current_exe().map_err(io)?;
    let source = if is_copy(&current, id) {
        previous
            .as_ref()
            .map(|(_, r)| r.clone())
            .or_else(|| read_launch(&current_dir).ok())
            .ok_or(13u32)?
            .source
    } else {
        links::original(id, &current)
    };
    let pending = dir.join(format!("grant-{}.json", guid()?));
    commit::commit(
        &mut commit::Runtime {
            id,
            sid,
            dir,
            current,
            record: Grant {
                image: IMAGE.into(),
                source,
            },
            previous,
            scheduler: &scheduler,
            enable,
            pending,
            swapped: None,
        },
        enable,
        moving,
        same_account,
    )
}

/// Writes the Task Scheduler's registrations to disk; false when that failed.
fn flush_tasks() -> bool {
    util::stage(
        "administrator task registration flush",
        crate::install::flush_key(
            windows::Win32::System::Registry::HKEY_LOCAL_MACHINE,
            TASK_HIVE,
        ),
    )
    .is_ok()
}

/// Writes this build's launcher into `dir` in place of the one there.
fn put_launcher(dir: &Path, bytes: &[u8]) -> Result<()> {
    let pending = dir.join(format!("launcher-{}.exe", guid()?));
    let mut file = crate::install::new_protected_file(&pending)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = written
        .map_err(io)
        .and_then(|()| replace(&pending, &dir.join(LAUNCHER)));
    if result.is_err() {
        let _ = fs::remove_file(&pending);
    }
    result
}

/// Once the account's records are in the protected folder, deletes what
/// Argus 0.5 or earlier left where it kept them. With `forward` the old
/// launcher stays, brought to this build, and starts what the protected
/// folder's records name (`launcher_mode`): another account's shortcuts, or
/// those of this account that could not be changed (`links::retarget`), may
/// still start it. The account's next grant, revocation or the uninstaller
/// deletes it then. Failures leave files for a later maintenance to delete.
fn retire_legacy(id: &str, sid: &str, forward: bool) {
    let (Ok(dir), Ok(legacy)) = (directory(id, sid), legacy_directory(id, sid)) else {
        return;
    };
    if !records(&dir) || located_in(dir, legacy.clone()) == legacy || !legacy.exists() {
        return;
    }
    let result = retire(&legacy, forward);
    if let Err(code) = &result {
        argus_core::diagnostics::warn(
            "administrator earlier folder cleanup",
            format!(
                "{}; path={}",
                argus_core::diagnostics::code(*code),
                util::log_path(&legacy)
            ),
        );
    }
    if let Ok(root) = legacy_root(id) {
        remove_empty_runtime(&root);
    }
}

/// Deletes the known files of a folder Argus 0.5 or earlier kept, by name,
/// trusting none of it; with `forward`, all but the launcher, brought to this
/// build first, and its icon, which shortcuts show.
fn retire(legacy: &Path, forward: bool) -> Result<()> {
    if forward {
        let Some(bytes) = crate::taskmgr::launcher_asset() else {
            // Nothing to forward with: the folder stays as it is.
            return Ok(());
        };
        // A program is only written to a folder that is still protected.
        let _pin = security::protect_directory(legacy)?;
        put_launcher(legacy, bytes)?;
    }
    let Some(directory) = crate::cleanup::Directory::open(legacy)? else {
        return Ok(());
    };
    let lock = match directory.lock("install.lock", false) {
        Ok(lock) => lock,
        Err(32) => return Err(32),
        Err(_) => None,
    };
    let mut result = Ok(());
    for name in directory.names(runtime_file)? {
        let kept = name.eq_ignore_ascii_case("install.lock")
            || (forward
                && (name.eq_ignore_ascii_case(LAUNCHER) || name.eq_ignore_ascii_case(ICON)));
        if !kept {
            // A copy still running stays for a later maintenance.
            result = result.and(directory.remove_file(&name));
        }
    }
    drop(lock);
    if !forward && result.is_ok() {
        directory.remove_file("install.lock")?;
        directory.remove_empty()?;
    }
    result
}

/// Deletes `root` (`<runtime>\<id>`) and the folder above it when empty.
fn remove_empty_runtime(root: &Path) {
    if let Ok(Some(folder)) = crate::cleanup::Directory::open(root) {
        let _ = folder.remove_empty();
    }
    if let Some(base) = root.parent()
        && let Ok(Some(base)) = crate::cleanup::Directory::open(base)
    {
        let _ = base.remove_empty();
    }
}

/// For an account whose folder an installer running as another account
/// moved: points this account's shortcuts and sign-in entry at the protected
/// launcher, from the one Argus 0.5 or earlier kept, which still forwards to
/// it. Needs no elevation; the old launcher goes with the account's next
/// grant or revocation, or the uninstaller.
pub fn follow_layout(id: &str) {
    let Ok(sid) = security::current_sid() else {
        return;
    };
    let (Ok(dir), Ok(legacy)) = (directory(id, &sid), legacy_directory(id, &sid)) else {
        return;
    };
    let moved = legacy.join(LAUNCHER);
    if !records(&dir) || located_in(dir.clone(), legacy) != dir || !moved.is_file() {
        return;
    }
    // What fails is logged; the next start tries again.
    links::follow(id, &moved, &dir.join(LAUNCHER));
}
/// Revocation trusts neither the owner nor the ACL of the runtime directory
/// (#46). It still takes the lock that enable and maintenance hold while they
/// change the task and the grant record, so the two always end up together,
/// and deletes only by name inside pinned directories.
fn revoke(dir: &Path, mut remove_task: impl FnMut() -> Result<()>) -> Result<()> {
    let directory = match crate::cleanup::Directory::open(dir) {
        Ok(Some(directory)) => directory,
        Ok(None) => {
            remove_task()?;
            // A first grant creates the directory before it takes the lock and
            // registers its task. If one did so meanwhile, finish under its lock.
            match crate::cleanup::Directory::open(dir)? {
                Some(directory) => directory,
                None => return Ok(()),
            }
        }
        // Nothing here can be locked or cleaned; the task still goes.
        Err(code) => {
            remove_task()?;
            return Err(code);
        }
    };
    let _lock = match directory.lock("install.lock", true) {
        Ok(lock) => lock,
        // Enable or maintenance is changing the grant; retry afterwards.
        Err(32) => return Err(32),
        // They hold it only in a directory whose ACL they verified. A lock
        // that administrators cannot open, or a link in its place, means the
        // directory was changed outside Argus (#46): revocation goes on
        // without the lock rather than fail, and never opens the link.
        Err(_) => None,
    };
    remove_task()?;
    directory.remove_file("grant.json")?;
    // The standard launcher still starts the image its record names. Without
    // a trusted record, leave every copy to the next grant or the uninstaller;
    // a running copy stays until a later management operation.
    if let Ok(launch) = read_launch(dir) {
        sweep(dir, Some(&launch.image));
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
    let image = dir.join(&record.image);
    crate::taskmgr::image_version(&image).is_some_and(|version| {
        crate::taskmgr::older(version, crate::VERSION)
            || crate::taskmgr::image_version(&record.source).is_some_and(|source| {
                source > version || (source == version && image::changed(&image, &record.source))
            })
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
                let dir = located(id, &sid)?;
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
/// Deletes the administrator copies in `dir` but `keep`, unless they still run.
fn sweep(dir: &Path, keep: Option<&str>) {
    if let Ok(files) = fs::read_dir(dir) {
        for file in files.flatten() {
            if let Some(name) = file.file_name().to_str()
                && copy_name(name)
                && !keep.is_some_and(|keep| keep.eq_ignore_ascii_case(name))
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
        util::stage(
            "administrator ShellExecuteExW (UAC)",
            unsafe { ShellExecuteExW(&mut info) }.map_err(win),
        )?;
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
            "--argus-admin-refresh" if args.len() == 1 => {
                // Every installation runs this. What the sensor driver's setups
                // of Argus 0.5 and earlier left goes now: this version keeps
                // it in the protected folder.
                let _ = crate::driver::retire_legacy_setup();
                refresh(id, false)
            }
            "--argus-admin-remove" if args.len() == 1 => refresh(id, true),
            _ => ticket::standard_mode(id, &args).unwrap_or(Err(87)),
        },
    ))
}
fn refresh(id: &str, remove: bool) -> Result<()> {
    if !argus_core::privilege::is_elevated() {
        return Err(5);
    }
    if remove {
        task::Scheduler::new()?.remove_all(id)?;
        return remove_runtime(id);
    }
    // The accounts with a folder here or where Argus 0.5 and earlier kept
    // them: maintenance moves the latter.
    let mut accounts = std::collections::BTreeSet::new();
    let mut guards = Vec::new();
    let legacy = legacy_root(id)?;
    let root = root(id)?;
    if root.exists() {
        guards.push(security::protect_directory(&root)?);
        for entry in fs::read_dir(&root).map_err(io)? {
            let entry = entry.map_err(io)?;
            let sid = entry.file_name().to_string_lossy().into_owned();
            security::validate_sid(&sid)?;
            accounts.insert(sid);
        }
    }
    // The earlier folder is not held, so that maintenance can delete it once
    // empty, and what is wrong with it holds up no account's maintenance.
    if legacy.exists() {
        let listed = security::protect_directory(&legacy).and_then(|_guard| {
            Ok(fs::read_dir(&legacy)
                .map_err(io)?
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|sid| security::validate_sid(sid).is_ok())
                .collect::<Vec<_>>())
        });
        match listed {
            Ok(sids) => accounts.extend(sids),
            Err(code) => argus_core::diagnostics::warn(
                "administrator earlier folder listing",
                argus_core::diagnostics::code(code),
            ),
        }
    }
    for sid in accounts {
        let dir = located(id, &sid)?;
        if !dir.exists() {
            continue;
        }
        let pin = security::protect_directory(&dir)?;
        let authorized = grant(id, &sid)?.is_some();
        // Existing protected records are the only authority for maintenance;
        // an installer cannot create a new grant for a different account.
        if !authorized && !dir.join("launch.json").exists() {
            continue;
        }
        read_launch(&dir)?;
        // Not held either: the folder may be the earlier one, which goes.
        drop(pin);
        install_inner(id, &sid, authorized, true)?;
    }
    drop(guards);
    remove_empty_runtime(&legacy);
    Ok(())
}

/// For the uninstaller: every account's files of this identifier, in the
/// protected folder and where Argus 0.5 and earlier kept them.
fn remove_runtime(id: &str) -> Result<()> {
    let mut result = Ok(());
    for root in [root(id)?, legacy_root(id)?] {
        result = result.and(remove_runtime_in(&root));
        // The shared `runtime` (`Argus Runtime`) goes with its last
        // identifier, including one an earlier version left empty. An empty
        // folder never fails the uninstall.
        remove_empty_runtime(&root);
    }
    // So does the protected folder, when nothing else is left in it.
    if let Ok(base) = crate::layout::root()
        && let Ok(Some(base)) = crate::cleanup::Directory::open(&base)
    {
        let _ = base.remove_empty();
    }
    result
}

fn remove_runtime_in(root: &Path) -> Result<()> {
    let Some(guard) = crate::cleanup::Directory::open(root)? else {
        return Ok(());
    };
    for entry in fs::read_dir(root).map_err(io)? {
        let entry = entry.map_err(io)?;
        let sid = entry.file_name().to_string_lossy().into_owned();
        if security::validate_sid(&sid).is_err() {
            continue;
        }
        let dir = root.join(&sid);
        if let Some(pin) = crate::cleanup::Directory::open(&dir)? {
            remove_runtime_files(&dir)?;
            pin.remove_empty()?;
        }
    }
    guard.remove_empty()
}

fn runtime_file(name: &str) -> bool {
    copy_name(name)
        || matches!(
            name,
            "grant.json" | "launch.json" | "argus-launcher.exe" | "argus.ico" | "install.lock"
        )
        || [
            ("launcher-", ".exe"),
            ("image-", ".exe"),
            ("icon-", ".ico"),
            ("grant-", ".json"),
            ("launch-", ".json"),
        ]
        .iter()
        .any(|(prefix, suffix)| {
            name.strip_prefix(prefix)
                .and_then(|s| s.strip_suffix(suffix))
                .is_some_and(util::valid_guid)
        })
}
fn remove_runtime_files(dir: &Path) -> Result<()> {
    let Some(directory) = crate::cleanup::Directory::open(dir)? else {
        return Ok(());
    };
    for file in fs::read_dir(dir).map_err(io)? {
        let file = file.map_err(io)?;
        if file.file_name().to_str().is_some_and(runtime_file) {
            // Never recurse into an unexpected directory or follow a link.
            directory.remove_file(&file.file_name().to_string_lossy())?;
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
            "argus-admin.exe".into(),
            "argus-admin.old.exe".into(),
            "argus-admin.old-2.exe".into(),
            format!("image-{nonce}.exe"),
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
    fn revocation_takes_the_management_lock_without_trusting_the_directory() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("argus-revoke-{}", guid().unwrap()));
        let removed = std::cell::Cell::new(0);
        let remove = || -> Result<()> {
            removed.set(removed.get() + 1);
            Ok(())
        };
        revoke(&dir, remove).unwrap();
        assert_eq!(removed.get(), 1);
        assert!(!dir.exists());

        fs::create_dir(&dir).unwrap();
        let image = format!("argus-admin-{}.exe", guid().unwrap());
        let stale = format!("argus-admin-{}.exe", guid().unwrap());
        let record = Grant {
            image: image.clone(),
            source: dir.join("argus.exe"),
        };
        fs::write(dir.join("grant.json"), serde_json::to_vec(&record).unwrap()).unwrap();
        fs::write(
            dir.join("launch.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        fs::write(dir.join(&image), b"image").unwrap();
        fs::write(dir.join(&stale), b"image").unwrap();
        // The open that enable and maintenance hold while changing both.
        let held = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(dir.join("install.lock"))
            .unwrap();
        assert_eq!(revoke(&dir, remove), Err(32));
        assert_eq!(removed.get(), 1);
        assert!(dir.join("grant.json").exists());
        drop(held);
        revoke(&dir, remove).unwrap();
        assert_eq!(removed.get(), 2);
        assert!(!dir.join("grant.json").exists());
        // A user-owned directory is not trusted to name the launcher's image.
        assert!(security::protect_directory(&dir).is_err());
        assert!(dir.join(&image).exists() && dir.join(&stale).exists());

        // A link in place of the lock is never opened, and does not keep the
        // account's privileges in place.
        let launch = fs::read(dir.join("launch.json")).unwrap();
        fs::remove_file(dir.join("install.lock")).unwrap();
        fs::hard_link(dir.join("launch.json"), dir.join("install.lock")).unwrap();
        fs::write(dir.join("grant.json"), &launch).unwrap();
        revoke(&dir, remove).unwrap();
        assert_eq!(removed.get(), 3);
        assert!(!dir.join("grant.json").exists());
        assert_eq!(fs::read(dir.join("install.lock")).unwrap(), launch);
        let target = dir.join("target");
        fs::remove_file(dir.join("install.lock")).unwrap();
        match crate::cleanup::tests::reparse_file(&dir.join("install.lock"), &target) {
            Ok(()) => {
                revoke(&dir, remove).unwrap();
                assert_eq!(removed.get(), 4);
                assert!(!target.exists());
            }
            Err(error) => eprintln!("no file reparse point for this account: {error}"),
        }

        remove_runtime_files(&dir).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn records_are_read_where_they_are_until_they_move() {
        let base = std::env::temp_dir().join(format!("argus-located-{}", guid().unwrap()));
        let (dir, legacy) = (base.join("new"), base.join("legacy"));
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&legacy).unwrap();
        let at = || located_in(dir.clone(), legacy.clone());
        // Neither: new records go to the protected folder.
        assert_eq!(at(), dir);
        // Argus 0.5 or earlier kept them, and nothing moved them yet.
        fs::write(legacy.join("launch.json"), b"{}").unwrap();
        assert_eq!(at(), legacy);
        fs::write(legacy.join("grant.json"), b"{}").unwrap();
        fs::remove_file(legacy.join("launch.json")).unwrap();
        assert_eq!(at(), legacy);
        // A copy or launcher alone written here moves nothing.
        fs::write(dir.join(IMAGE), b"image").unwrap();
        fs::write(dir.join(LAUNCHER), b"launcher").unwrap();
        assert_eq!(at(), legacy);
        // A grant writes the launch record once the task names the copy
        // here, then commits the grant: until then the earlier folder's
        // grant is the one read, as when a crash cut the move short.
        fs::write(dir.join("launch.json"), b"{}").unwrap();
        assert_eq!(at(), legacy);
        fs::write(dir.join("grant.json"), b"{}").unwrap();
        assert_eq!(at(), dir);
        // A revoked grant's move: only launch records on either side.
        fs::remove_file(dir.join("grant.json")).unwrap();
        fs::remove_file(legacy.join("grant.json")).unwrap();
        fs::write(legacy.join("launch.json"), b"{}").unwrap();
        assert_eq!(at(), dir);
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn the_earlier_folder_goes_by_name_and_a_running_copy_waits() {
        use std::os::windows::fs::OpenOptionsExt;
        let base = std::env::temp_dir().join(format!("argus-retire-{}", guid().unwrap()));
        let legacy = base.join("S-1-5-21-1-2-3-1001");
        fs::create_dir_all(&legacy).unwrap();
        let copy = format!("argus-admin-{}.exe", guid().unwrap());
        for name in [
            copy.as_str(),
            "grant.json",
            "launch.json",
            LAUNCHER,
            ICON,
            "install.lock",
            "notes.txt",
        ] {
            fs::write(legacy.join(name), b"x").unwrap();
        }
        // Still running: it stays, and so does the folder.
        let running = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(legacy.join(&copy))
            .unwrap();
        assert!(retire(&legacy, false).is_err());
        let mut left: Vec<_> = fs::read_dir(&legacy)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, [copy.as_str(), "install.lock", "notes.txt"]);
        drop(running);
        // Only known files go: the folder stays for what Argus did not write.
        assert_eq!(retire(&legacy, false), Ok(()));
        assert!(!legacy.exists() || fs::read_dir(&legacy).unwrap().count() == 1);
        fs::remove_file(legacy.join("notes.txt")).unwrap();
        assert!(retire(&legacy, false).is_ok());
        assert!(!legacy.exists());
        // A development build has no launcher to forward with: nothing goes.
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("launch.json"), b"{}").unwrap();
        assert_eq!(retire(&legacy, true), Ok(()));
        assert!(legacy.join("launch.json").exists());
        fs::remove_dir_all(base).unwrap();
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
