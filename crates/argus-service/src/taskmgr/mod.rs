//! Task Manager replacement is opt-in. Status is read-only. All persistent
//! writes live behind explicit maintenance actions, with a durable journal.
mod launch;
mod probe;
pub mod ready;
mod registry;
mod state;

use crate::{
    security,
    util::{self, Handle, wide},
};
use argus_core::i18n::{Locale, text_in};
use argus_core::tr;
use serde::{Deserialize, Serialize};
use state::{Journal, Observation, Phase, Result, Store, Value};
use std::fs::{self, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Com::CoCreateGuid;
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW, SW_HIDE};
use windows::core::PCWSTR;

pub const LAUNCHER: &str = "argus-taskmgr-launcher.exe";
pub const RECOVER: &str = "argus-taskmgr-recover.exe";
const RECOVERY_TEXT: &str = "taskmgr-recovery.txt";
const MANAGE: &str = "--argus-taskmgr-manage";
pub type Assets = (&'static [u8], &'static [u8]);
/// The launcher and recovery program this build carries, kept by `run_mode`
/// for a service update to bring an enabled replacement along (`follow_update`).
/// None in a development build.
static ASSETS: OnceLock<Assets> = OnceLock::new();

pub fn id() -> Result<String> {
    Ok(format!(
        "{:?}",
        unsafe { CoCreateGuid() }.map_err(|e| e.to_string())?
    )
    .trim_matches(['{', '}'])
    .to_ascii_lowercase())
}
fn valid_image(name: &str) -> bool {
    name.strip_prefix("argus-taskmgr-ui-")
        .and_then(|s| s.strip_suffix(".exe"))
        .is_some_and(|s| s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
}
/// Whether `path` is one of the replacement's interface copies in this
/// build's protected folder. An Argus opened by the replacement runs one, and
/// the uninstaller's Restart Manager check knows only the installed argus.exe.
pub fn is_interface_copy(path: &Path) -> bool {
    let Ok(dir) = crate::install::directory() else {
        return false;
    };
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(valid_image)
        && path.parent().is_some_and(|parent| {
            parent
                .to_string_lossy()
                .eq_ignore_ascii_case(&dir.to_string_lossy())
        })
}
fn replacement(dir: &Path) -> Value {
    Value::string(&format!(
        "{} --task-manager",
        launch::quote(&dir.join(LAUNCHER).to_string_lossy())
    ))
}
fn machine() -> Result<registry::Machine> {
    Ok(registry::Machine::new(
        crate::install::directory().map_err(util::message)?,
    ))
}
fn parent(dir: &Path) -> Result<&Path> {
    dir.parent()
        .ok_or_else(|| tr!("taskManager.invalidDirectory"))
}
fn lock(dir: &Path) -> Result<fs::File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(dir.join("install.lock"))
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: &'static str,
    pub can_enable: bool,
    pub can_restore: bool,
    pub recovery_path: String,
    pub detail: String,
    /// What a conflict consists of, in fixed words for the log.
    #[serde(skip)]
    found: Option<String>,
    /// A self-test left its entry or files behind.
    #[serde(skip)]
    leftovers: bool,
    /// The version of the interface copy an enabled replacement starts, when
    /// it is older than this build.
    #[serde(skip)]
    older: Option<String>,
}

impl Status {
    /// For the diagnostic log: never the detail, which can hold paths.
    pub fn summary(&self) -> String {
        let mut text = self.state.to_string();
        if let Some(found) = &self.found {
            text += &format!(" ({found})");
        }
        if let Some(version) = &self.older {
            text += &format!("; the interface copy is version {version}");
        }
        if self.leftovers {
            text += "; self-test leftovers present";
        }
        text
    }

    /// The entry holds what Argus will not overwrite: say what it is.
    fn conflict(&mut self, detail: String, ours: &Value) {
        self.state = "conflict";
        self.detail = detail;
        let Ok(found) = registry::describe(registry::KEY) else {
            return;
        };
        if let Some(items) = found_items(&found, ours)
            .into_iter()
            .reduce(|first, second| tr!("taskManager.found.join", first = first, second = second))
        {
            self.detail = tr!(
                "taskManager.found.detail",
                status = self.detail,
                found = items
            );
        }
        self.found = Some(found_log(&found, ours)).filter(|log| !log.is_empty());
    }
}

/// The text of a string value, without its terminating NULs.
fn value_text(value: &Value) -> Option<String> {
    let (pairs, []) = value.bytes.as_chunks::<2>() else {
        return None;
    };
    if !matches!(value.kind, 1 | 2) {
        return None;
    }
    let units: Vec<u16> = pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
    Some(
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_owned(),
    )
}

/// One sentence for each thing a conflicting entry holds, for the window.
/// Argus's own value is no news there; what was added beside it is.
fn found_items(found: &registry::Found, ours: &Value) -> Vec<String> {
    let mut items = Vec::new();
    if let Some(value) = found.debugger.as_ref().filter(|value| *value != ours) {
        items.push(match value_text(value) {
            Some(text) if text.is_empty() => tr!("taskManager.found.emptyDebugger"),
            Some(text) => {
                let shown = if text.chars().count() > 200 {
                    text.chars().take(200).chain(Some('…')).collect()
                } else {
                    text
                };
                tr!("taskManager.found.debugger", value = shown)
            }
            None => tr!("taskManager.found.otherDebugger"),
        });
    }
    if found.use_filter {
        items.push(tr!("taskManager.found.useFilter"));
    }
    if found.subkey_count > 0 {
        let names = found.subkeys.join(&tr!("taskManager.found.separator"));
        items.push(if found.subkey_count as usize > found.subkeys.len() {
            tr!(
                "taskManager.found.moreSubkeys",
                count = found.subkey_count,
                names = names
            )
        } else {
            tr!("taskManager.found.subkeys", names = names)
        });
    }
    items
}

/// What a conflicting entry holds, for the log: fixed words, never a value.
fn found_log(found: &registry::Found, ours: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(value) = &found.debugger {
        let debugger = if value == ours {
            "Debugger names the Argus launcher"
        } else {
            match value_text(value) {
                Some(text) if text.is_empty() => "Debugger is empty",
                Some(_) => "Debugger names another program",
                None => "Debugger is not text",
            }
        };
        parts.push(debugger.to_string());
    }
    if found.use_filter {
        parts.push("UseFilter value".to_string());
    }
    if found.subkey_count > 0 {
        let perf = found
            .subkeys
            .iter()
            .any(|name| name.eq_ignore_ascii_case("PerfOptions"));
        parts.push(format!(
            "{} subkey{}{}",
            found.subkey_count,
            if found.subkey_count == 1 { "" } else { "s" },
            if perf { ", PerfOptions among them" } else { "" }
        ));
    }
    parts.join("; ")
}

/// Where an enable or restore stopped; fixed names for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    /// Rights, account, lock, an earlier record or another configuration.
    Check,
    /// The launcher, recovery program and interface copy.
    Prepare,
    /// Removing what an earlier self-test left.
    ProbeLeftover,
    /// The self-test's record and test program.
    ProbeFiles,
    /// Creating the self-test's IFEO entry.
    ProbeCreate,
    /// Writing that entry's Debugger value.
    ProbeWrite,
    /// The normal start of the test program, which IFEO must redirect.
    ProbeRedirect,
    /// The debug start that bypasses IFEO.
    ProbeBypass,
    /// Removing the self-test's entry and files afterwards.
    ProbeCleanup,
    /// The interface readiness check as the account that asked.
    Interface,
    /// The recovery record and taskmgr.exe's Debugger value.
    Commit,
    Restore,
    /// Starting the maintenance helper and waiting for it.
    Helper,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Prepare => "file preparation",
            Self::ProbeLeftover => "earlier self-test cleanup",
            Self::ProbeFiles => "self-test files",
            Self::ProbeCreate => "self-test entry creation",
            Self::ProbeWrite => "self-test entry write",
            Self::ProbeRedirect => "self-test redirect",
            Self::ProbeBypass => "self-test debug start",
            Self::ProbeCleanup => "self-test cleanup",
            Self::Interface => "interface check",
            Self::Commit => "taskmgr.exe entry",
            Self::Restore => "restore",
            Self::Helper => "maintenance helper",
        }
    }
}

/// Why an administrator could not change an entry under IFEO.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cause {
    /// Its permissions allow administrators to write: something outside them
    /// refused, usually security software that guards the registry or a policy.
    Blocked,
    /// Its permissions do not allow administrators to write.
    Permissions,
}

impl Cause {
    fn text(self) -> String {
        match self {
            Self::Blocked => tr!("taskManager.refused.blocked"),
            Self::Permissions => tr!("taskManager.refused.permissions"),
        }
    }

    fn log(self) -> &'static str {
        match self {
            Self::Blocked => "; the key's permissions allow administrators to write",
            Self::Permissions => "; the key's permissions do not allow administrators to write",
        }
    }
}

/// Adds why an administrator's IFEO change was refused to its error.
fn explain(error: String, cause: Option<Cause>) -> String {
    match cause {
        Some(cause) => tr!(
            "taskManager.refused.withCause",
            error = error,
            cause = cause.text()
        ),
        None => error,
    }
}

/// A failed enable or restore. The maintenance helper hands it to the window
/// in `ERROR_FILE`; the window shows `message`, and only the fixed step,
/// cause and classifications reach the log (`log`).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Failure {
    pub step: Step,
    pub message: String,
    /// `diagnostics::failure` of the step's own error.
    reason: String,
    cause: Option<Cause>,
    /// The self-test entry stayed: the classified error of its cleanup.
    kept: Option<String>,
    /// The replacement's state, when the action is not allowed in it.
    #[serde(skip)]
    state: Option<&'static str>,
}

impl Failure {
    fn new(step: Step, message: String) -> Self {
        Self {
            step,
            reason: argus_core::diagnostics::failure(&message).into_owned(),
            message,
            cause: None,
            kept: None,
            state: None,
        }
    }

    fn refused(step: Step, error: String, cause: Option<Cause>) -> Self {
        Self {
            cause,
            ..Self::new(step, explain(error, cause))
        }
    }

    /// The self-test failed, and so did its cleanup: the entry stays.
    fn kept(self, cleanup: Failure) -> Self {
        Self {
            message: tr!(
                "taskManager.probeKept",
                error = self.message,
                cleanup = cleanup.message
            ),
            cause: self.cause.or(cleanup.cause),
            kept: Some(cleanup.reason),
            ..self
        }
    }

    /// For the diagnostic log: fixed words and Windows error codes, never
    /// the text, which can hold paths.
    pub fn log(&self) -> String {
        use argus_core::diagnostics::failure;
        let mut line = match self.state {
            Some(state) => format!("at {}: not allowed while {state}", self.step.name()),
            None => format!("at {}: {}", self.step.name(), failure(&self.reason)),
        };
        if let Some(cause) = self.cause {
            line += cause.log();
        }
        if let Some(kept) = &self.kept {
            line += &format!("; self-test entry kept ({})", failure(kept));
        }
        line
    }
}

pub fn status() -> Result<Status> {
    let mut machine = machine()?;
    let live = machine.observe()?;
    let mut status = Status {
        state: "disabled",
        can_enable: false,
        can_restore: false,
        recovery_path: machine.dir.join(RECOVER).to_string_lossy().into_owned(),
        detail: tr!("taskManager.status.native"),
        found: None,
        leftovers: machine.dir.join(probe::RECORD).exists(),
        older: None,
    };
    let ours = replacement(&machine.dir);
    if !machine.dir.exists() {
        status.detail = tr!("taskManager.status.needsService");
        if live.debugger.is_some() || live.filtered {
            status.conflict(tr!("taskManager.status.conflict"), &ours);
        }
        return Ok(status);
    }
    let _parent = security::protect_directory(parent(&machine.dir)?).map_err(util::message)?;
    let _dir = security::protect_directory(&machine.dir).map_err(util::message)?;
    let installed = machine.dir.join("owner.sid").exists();
    let authorized = installed
        && crate::install::owner(&machine.dir).map_err(util::message)?
            == security::current_sid().map_err(util::message)?;
    if let Some(journal) = machine.load()? {
        if live.debugger.as_ref() == Some(&journal.replacement) && !live.filtered {
            status.state = if journal.phase == Phase::Active {
                "enabled"
            } else {
                "recoveryNeeded"
            };
            status.can_restore = authorized;
            status.detail = if status.state == "enabled" {
                status.older = older_interface(&machine.dir, &journal.image);
                match &status.older {
                    Some(version) => tr!("taskManager.status.enabledOlder", version = version),
                    None => tr!("taskManager.status.enabled"),
                }
            } else {
                tr!("taskManager.status.interrupted")
            };
            return Ok(status);
        }
        if live != journal.before {
            status.conflict(tr!("taskManager.status.modified"), &ours);
            return Ok(status);
        }
    } else if live.debugger.as_ref() == Some(&ours) {
        status.state = "recoveryNeeded";
        status.detail = tr!("taskManager.status.recordMissing");
        return Ok(status);
    }
    if live.debugger.is_some() || live.filtered {
        status.conflict(tr!("taskManager.status.conflict"), &ours);
    } else if !installed {
        status.detail = tr!("taskManager.status.serviceRemoved");
    } else if cfg!(debug_assertions) {
        status.detail = tr!("taskManager.status.devBuild");
    } else if !authorized {
        status.detail = tr!("taskManager.status.otherAccount");
    } else {
        status.can_enable = true;
    }
    Ok(status)
}

/// The version of the interface copy an enabled replacement starts, when it
/// is older than this build: an update did not reach it (a portable Argus
/// replaced by hand, or an installer that could not refresh the replacement).
fn older_interface(dir: &Path, image: &str) -> Option<String> {
    if !valid_image(image) {
        return None;
    }
    let copy = image_version(&dir.join(image))?;
    older(copy, crate::VERSION).then(|| format!("{}.{}.{}", copy.0, copy.1, copy.2))
}

/// Whether `copy` comes before `current` (a version string such as 0.2.0 or
/// 0.2.0-beta.1); the pre-release part does not count.
fn older(copy: (u32, u32, u32), current: &str) -> bool {
    let mut parts = current
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse::<u32>().ok());
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Some(major)), Some(Some(minor)), Some(Some(patch)), None) => {
            copy < (major, minor, patch)
        }
        _ => false,
    }
}

/// Major, minor and patch from an executable's version resource.
fn image_version(path: &Path) -> Option<(u32, u32, u32)> {
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };
    let path = wide(&path.to_string_lossy());
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    if !(52..=1024 * 1024).contains(&size) {
        return None;
    }
    let mut data = vec![0u64; (size as usize).div_ceil(8)];
    unsafe { GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()) }
        .ok()?;
    let mut pointer = std::ptr::null_mut();
    let mut len = 0;
    let start = data.as_ptr() as usize;
    if !unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            windows::core::w!(r"\"),
            &mut pointer,
            &mut len,
        )
    }
    .as_bool()
        || (len as usize) < size_of::<VS_FIXEDFILEINFO>()
        || (pointer as usize) < start
        || (pointer as usize)
            .checked_add(size_of::<VS_FIXEDFILEINFO>())
            .is_none_or(|end| end > start + size as usize)
    {
        return None;
    }
    let info = unsafe { std::ptr::read_unaligned(pointer.cast::<VS_FIXEDFILEINFO>()) };
    (info.dwSignature == 0xfeef04bd).then_some((
        info.dwFileVersionMS >> 16,
        info.dwFileVersionMS & 0xffff,
        info.dwFileVersionLS >> 16,
    ))
}

/// The service installer already holds install.lock when calling this.
pub(crate) fn restore_for_maintenance(dir: &Path) -> Result<()> {
    let mut machine = registry::Machine::new(dir.into());
    match machine.load()? {
        Some(mut journal) => state::settle(&mut machine, &mut journal),
        None if machine.observe()?.debugger.as_ref() == Some(&replacement(dir)) => {
            Err(tr!("taskManager.launcherKept"))
        }
        None => Ok(()),
    }
}

/// Why `restore_for_maintenance` held up the service: another program's
/// configuration in taskmgr.exe's entry, which Argus leaves to that program,
/// or a restore of Argus's own value that did not finish.
pub(crate) fn maintenance_error(dir: &Path) -> u32 {
    if registry::observe(registry::KEY).is_ok_and(|live| held_by_other(&live, &replacement(dir))) {
        util::TASK_MANAGER_CONFLICT
    } else {
        util::TASK_MANAGER_ACTIVE
    }
}

/// Whether the entry holds a configuration that is not Argus's.
fn held_by_other(live: &Observation, ours: &Value) -> bool {
    live.filtered || live.debugger.as_ref().is_some_and(|value| value != ours)
}

/// What bringing an enabled replacement to a new build touches in the
/// protected folder. `follow` keeps it apart from the order and rollback,
/// which the tests check without the folder.
trait Files {
    /// Whether `name` holds exactly `bytes`.
    fn holds(&mut self, name: &str, bytes: &[u8]) -> bool;
    /// Writes `name` in full beside the old one, then replaces it.
    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<()>;
    fn remove(&mut self, name: &str);
}

struct Folder<'a>(&'a Path);

impl Files for Folder<'_> {
    fn holds(&mut self, name: &str, bytes: &[u8]) -> bool {
        fs::read(self.0.join(name)).is_ok_and(|old| old == bytes)
    }
    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        registry::atomic_file(self.0, name, bytes)
    }
    fn remove(&mut self, name: &str) {
        let _ = fs::remove_file(self.0.join(name));
    }
}

/// Brings an enabled replacement to this build without turning it off, before
/// an update of the service (`install::run_helper`, which holds install.lock):
/// the interface as a new copy the record then names, since a running copy
/// cannot be replaced, and the launcher, recovery program and instructions in
/// place. taskmgr.exe's entry names the launcher and stays as it is, and the
/// launcher does not need the service, so the service can change under it.
/// False when the replacement is not whole (off, half way or overwritten by
/// someone else): nothing changed, and the caller restores as before.
pub(crate) fn follow_update(dir: &Path) -> Result<bool> {
    // A development build carries no launcher and cannot enable one.
    let Some(&assets) = ASSETS.get() else {
        return Ok(false);
    };
    let mut machine = registry::Machine::new(dir.into());
    let Some(mut journal) = machine.load()? else {
        return Ok(false);
    };
    // Source code and embedded helper bytes are from the installed, elevated
    // executable, as for an enable.
    let interface =
        fs::read(std::env::current_exe().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if !follow(
        &mut machine,
        &mut Folder(dir),
        &mut journal,
        &interface,
        assets,
    )? {
        return Ok(false);
    }
    // The copy the record named until now goes, unless it is running.
    sweep_images(&machine);
    Ok(true)
}

/// Whether the replacement is on and whole: a finished enable whose value
/// taskmgr.exe's entry still holds, unfiltered. `status` calls it enabled.
fn whole(journal: &Journal, live: &Observation) -> bool {
    journal.phase == Phase::Active
        && !live.filtered
        && live.debugger.as_ref() == Some(&journal.replacement)
}

/// The order of `follow_update`: the new interface copy first, under a name of
/// its own; then the launcher and recovery program; then the record. A failure
/// before the record names the new copy deletes it, so the record and the copy
/// it names stay as they were. False, touching nothing, when not `whole`.
fn follow(
    store: &mut impl Store,
    files: &mut impl Files,
    journal: &mut Journal,
    interface: &[u8],
    (launcher, recover): Assets,
) -> Result<bool> {
    if !whole(journal, &store.observe()?) {
        return Ok(false);
    }
    let image = if files.holds(&journal.image, interface) {
        None
    } else {
        let image = format!("argus-taskmgr-ui-{}.exe", id()?);
        files.put(&image, interface)?;
        Some(image)
    };
    let result: Result<()> = (|| {
        for (name, bytes) in [(LAUNCHER, launcher), (RECOVER, recover)] {
            if !files.holds(name, bytes) {
                files.put(name, bytes)?;
            }
        }
        if let Some(image) = &image {
            let next = Journal {
                image: image.clone(),
                ..journal.clone()
            };
            store.save(&next)?;
            *journal = next;
        }
        Ok(())
    })();
    if let (Err(_), Some(image)) = (&result, &image) {
        files.remove(image);
    }
    result?;
    let text = recovery_text();
    if !files.holds(RECOVERY_TEXT, text.as_bytes()) {
        // Instructions only; the old ones still lead to the same files.
        let _ = files.put(RECOVERY_TEXT, text.as_bytes());
    }
    Ok(true)
}

/// Deletes the interface copies no unfinished record names. For an update or
/// removal of the service, which restored the replacement first (or brought
/// it to this build) and holds install.lock.
pub(crate) fn sweep(dir: &Path) {
    sweep_images(&registry::Machine::new(dir.into()));
}

pub fn restore() -> Result<()> {
    if !argus_core::privilege::is_elevated() {
        return Err(tr!("taskManager.restoreNeedsAdmin"));
    }
    let machine = machine()?;
    if !machine.dir.exists() {
        if registry::observe(registry::KEY)?.debugger.as_ref() == Some(&replacement(&machine.dir)) {
            return Err(tr!("taskManager.everythingMissing"));
        }
        return Ok(());
    }
    let _parent = security::protect_directory(parent(&machine.dir)?).map_err(util::message)?;
    let _dir = security::protect_directory(&machine.dir).map_err(util::message)?;
    let _lock = lock(&machine.dir)?;
    probe::cleanup(&mut probe::System::new(&machine.dir), Step::ProbeLeftover)
        .map_err(|failure| failure.message)?;
    restore_for_maintenance(&machine.dir)?;
    sweep_images(&machine);
    Ok(())
}

/// Deletes the interface copies no unfinished recovery record names: from
/// attempts that failed, or replacements restored since. A copy that is
/// running cannot be deleted and waits for the next time. Callers hold
/// install.lock.
fn sweep_images(machine: &registry::Machine) {
    let Ok(journal) = machine.load() else { return };
    let keep = journal
        .filter(|j| j.phase != Phase::Restored)
        .map(|j| j.image);
    remove_images(&machine.dir, keep.as_deref());
}

fn remove_images(dir: &Path, keep: Option<&str>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if let Some(name) = name.to_str()
            && valid_image(name)
            && keep != Some(name)
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub(crate) fn reconcile(dir: &Path) -> Result<()> {
    let machine = registry::Machine::new(dir.into());
    if dir.join(probe::RECORD).exists() {
        let _lock = lock(dir)?;
        probe::cleanup(&mut probe::System::new(dir), Step::ProbeLeftover)
            .map_err(|failure| failure.message)?;
    }
    if let Some(journal) = machine.load()?
        && matches!(journal.phase, Phase::Prepared | Phase::Restoring)
    {
        let _lock = lock(dir)?;
        restore_for_maintenance(dir)?;
    }
    Ok(())
}

fn enable(
    owner: &str,
    caller: u32,
    created: u64,
    assets: Assets,
) -> std::result::Result<(), Failure> {
    let check = |message| Failure::new(Step::Check, message);
    if cfg!(debug_assertions) {
        return Err(check(tr!("taskManager.devBuild")));
    }
    if !argus_core::privilege::is_elevated() {
        return Err(check(tr!("taskManager.enableNeedsAdmin")));
    }
    security::validate_sid(owner)
        .map_err(util::message)
        .map_err(check)?;
    let mut machine = machine().map_err(check)?;
    let _parent = security::protect_directory(parent(&machine.dir).map_err(check)?)
        .map_err(util::message)
        .map_err(check)?;
    let _dir = security::protect_directory(&machine.dir)
        .map_err(util::message)
        .map_err(check)?;
    if crate::install::owner(&machine.dir)
        .map_err(util::message)
        .map_err(check)?
        != owner
    {
        return Err(check(tr!("taskManager.ownerMismatch")));
    }
    let _lock = lock(&machine.dir).map_err(check)?;
    restore_for_maintenance(&machine.dir).map_err(check)?;
    let live = machine.observe().map_err(check)?;
    if live.debugger.is_some() || live.filtered {
        return Err(check(tr!("taskManager.conflict")));
    }
    sweep_images(&machine);
    // Errors name the step: the window shows them, and the log the step. The
    // self-test needs only the launcher; the interface copy waits for it.
    (|| {
        registry::atomic_file(&machine.dir, LAUNCHER, assets.0)?;
        registry::atomic_file(&machine.dir, RECOVER, assets.1)
    })()
    .map_err(prepare)?;
    probe::run(&mut probe::System::new(&machine.dir))?;
    let image = format!("argus-taskmgr-ui-{}.exe", id().map_err(check)?);
    let result = finish_enable(&mut machine, &image, owner, caller, created);
    if result.is_err() {
        // No active record names it. If its readiness check still runs it,
        // the next sweep deletes it.
        let _ = fs::remove_file(machine.dir.join(&image));
    }
    result
}

fn prepare(error: String) -> Failure {
    Failure::new(
        Step::Prepare,
        tr!("taskManager.prepareFailed", error = error),
    )
}

/// The rest of an enable once the self-test passed: the interface copy, its
/// readiness check as the account that asked, then the record and taskmgr.exe.
fn finish_enable(
    machine: &mut registry::Machine,
    image: &str,
    owner: &str,
    caller: u32,
    created: u64,
) -> std::result::Result<(), Failure> {
    (|| {
        // Source code and embedded helper bytes are from the explicitly elevated
        // GUI executable, not adjacent user-writable sidecar files.
        let source = fs::read(std::env::current_exe().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        registry::atomic_file(&machine.dir, image, &source)?;
        registry::atomic_file(&machine.dir, RECOVERY_TEXT, recovery_text().as_bytes())
    })()
    .map_err(prepare)?;
    let interface = |message| Failure::new(Step::Interface, message);
    let probe = launch::probe_as_caller(
        &machine.dir.join(LAUNCHER),
        &["--probe-ui".into(), image.into(), owner.into()],
        caller,
        created,
        owner,
    )
    .map_err(|error| interface(tr!("taskManager.uiStartFailed", error = error)))?;
    if launch::exit_code(&probe, Duration::from_secs(15)).map_err(interface)? != 0 {
        return Err(interface(tr!("taskManager.uiNotReady")));
    }
    let value = replacement(&machine.dir);
    state::enable(machine, owner.into(), image.into(), value).map_err(|message| Failure {
        cause: machine.refused,
        ..Failure::new(Step::Commit, message)
    })?;
    Ok(())
}

pub fn manage(action: &str) -> std::result::Result<Status, Failure> {
    let check = |message| Failure::new(Step::Check, message);
    match action {
        "restore" => {
            if let Ok(Some(crate::Reply::Done)) =
                crate::request(crate::Operation::RestoreTaskManager)
            {
                return status().map_err(check);
            }
            elevate(
                &std::env::current_exe().map_err(|e| check(e.to_string()))?,
                &[
                    MANAGE.into(),
                    "restore".into(),
                    security::current_sid()
                        .map_err(util::message)
                        .map_err(check)?,
                ],
            )?;
        }
        "enable" => {
            let current = status().map_err(check)?;
            if !current.can_enable {
                return Err(Failure {
                    state: Some(current.state),
                    ..check(tr!("taskManager.notAllowed"))
                });
            }
            elevate(
                &std::env::current_exe().map_err(|e| check(e.to_string()))?,
                &[
                    MANAGE.into(),
                    "enable".into(),
                    security::current_sid()
                        .map_err(util::message)
                        .map_err(check)?,
                    std::process::id().to_string(),
                    launch::identity(unsafe {
                        windows::Win32::System::Threading::GetCurrentProcess()
                    })
                    .map_err(check)?
                    .to_string(),
                ],
            )?;
        }
        _ => return Err(check(tr!("taskManager.notAllowed"))),
    }
    status().map_err(check)
}

/// A failed maintenance helper leaves its `Failure` here, in the protected
/// folder beside the launcher, for the window that started it to show and log
/// (`elevate` reads it back). Only administrators can write the folder.
const ERROR_FILE: &str = "taskmgr-error.json";
/// Where Argus 0.1.0 left the error, as plain text.
const OLD_ERROR_FILE: &str = "taskmgr-error.txt";

/// The helper's error from this run, if it left one.
fn helper_error(since: SystemTime) -> Option<Failure> {
    read_failure(&machine().ok()?.dir.join(ERROR_FILE), since)
}

fn read_failure(path: &Path, since: SystemTime) -> Option<Failure> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > 8 * 1024 || meta.modified().ok()? + Duration::from_secs(2) < since {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn elevate(exe: &Path, args: &[String]) -> std::result::Result<(), Failure> {
    let helper = |message| Failure::new(Step::Helper, message);
    let started = SystemTime::now();
    let file = wide(&exe.to_string_lossy());
    let params = wide(
        &args
            .iter()
            .map(|s| launch::quote(s))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let verb = wide("runas");
    let directory = wide(
        &launch::system_directory()
            .map_err(helper)?
            .to_string_lossy(),
    );
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        lpDirectory: PCWSTR(directory.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| helper(e.to_string()))?;
    if info.hProcess == HANDLE::default() {
        return Err(helper(tr!("taskManager.helperNotStarted")));
    }
    let process = Handle(info.hProcess);
    if unsafe { WaitForSingleObject(process.0, 120_000) } != WAIT_OBJECT_0 {
        return Err(helper(tr!("taskManager.helperBusy")));
    }
    let mut code = 0;
    unsafe { GetExitCodeProcess(process.0, &mut code) }.map_err(|e| helper(e.to_string()))?;
    if code == 0 {
        Ok(())
    } else {
        Err(helper_error(started).unwrap_or_else(|| helper(tr!("taskManager.helperFailed"))))
    }
}

fn alert(error: &str) {
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(wide(error).as_ptr()),
            PCWSTR(wide(&tr!("taskManager.alertTitle")).as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}
fn code(result: Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            alert(&error);
            1
        }
    }
}

/// Exit code of a maintenance helper: its error goes back to the window that
/// started it; a message box only when it cannot be left there.
fn report(result: std::result::Result<(), Failure>) -> i32 {
    let Err(failure) = result else { return 0 };
    let written = serde_json::to_vec(&failure)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            machine().and_then(|m| registry::atomic_file(&m.dir, ERROR_FILE, &bytes))
        });
    if written.is_err() {
        alert(&failure.message);
    }
    1
}

/// Appends a line to `taskmgr.log` beside Argus's own log. The launcher and
/// its probe have no window to report to, and a fallback to the system Task
/// Manager would otherwise leave no trace. Bounded; no paths or names.
fn log_line(event: &'static str, error: &str) {
    use std::io::Write;
    let text = argus_core::diagnostics::failure(error);
    let Some(dir) = log_dir() else { return };
    let path = dir.join("taskmgr.log");
    if fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = fs::remove_file(&path);
    }
    let _ = fs::create_dir_all(&dir);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        let _ = writeln!(
            file,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {event}: {text}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        );
    }
}

/// Argus's log folder for the account this process runs as.
fn log_dir() -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    let raw =
        unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }.ok()?;
    let base = unsafe { raw.to_string() };
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    let identifier = if cfg!(debug_assertions) {
        "dev.argus.taskmanager.devbuild"
    } else {
        "dev.argus.taskmanager"
    };
    Some(
        std::path::PathBuf::from(base.ok()?)
            .join(identifier)
            .join("logs")
            .join("diagnostics"),
    )
}

pub fn run_mode(assets: Option<Assets>) -> Option<i32> {
    if let Some(assets) = assets {
        let _ = ASSETS.set(assets);
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, action, owner, pid, created] if mode == MANAGE && action == "enable" => {
            clear_error();
            Some(report((|| {
                let check = |message| Failure::new(Step::Check, message);
                let pid = pid
                    .parse()
                    .map_err(|_| check(tr!("taskManager.invalidCaller")))?;
                let created = created
                    .parse()
                    .map_err(|_| check(tr!("taskManager.invalidInstance")))?;
                let assets = assets.ok_or_else(|| check(tr!("taskManager.noLauncher")))?;
                enable(owner, pid, created, assets)
            })()))
        }
        [mode, action, _owner] if mode == MANAGE && action == "restore" => {
            clear_error();
            Some(report(
                restore().map_err(|message| Failure::new(Step::Restore, message)),
            ))
        }
        [mode] if mode == "--argus-taskmgr-uninstall" => Some(code(uninstall_guard())),
        _ if args.first().is_some_and(|s| s == MANAGE) => Some(87),
        _ => None,
    }
}

/// A helper starts without an earlier run's error.
fn clear_error() {
    if let Ok(machine) = machine() {
        let _ = fs::remove_file(machine.dir.join(ERROR_FILE));
        let _ = fs::remove_file(machine.dir.join(OLD_ERROR_FILE));
    }
}

fn uninstall_guard() -> Result<()> {
    let machine = machine()?;
    if registry::observe(registry::KEY)?.debugger.as_ref() != Some(&replacement(&machine.dir)) {
        return Ok(());
    }
    manage("restore")
        .map(|_| ())
        .map_err(|failure| failure.message)
}

pub fn launcher_main() -> i32 {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--probe-child") if args.len() == 1 => 0,
        Some("--probe-redirect") => 86,
        Some("--probe-ui") if args.len() == 3 && valid_image(&args[1]) => {
            let result = (|| {
                let machine = machine()?;
                let _dir = security::protect_directory(&machine.dir).map_err(util::message)?;
                let image = machine.dir.join(&args[1]);
                let _image = security::protect_directory(&image).map_err(util::message)?;
                ready::activate(&image, &args[2], true)
            })();
            match result {
                Ok(()) => 0,
                Err(error) => {
                    log_line("enable check failed", &error);
                    1
                }
            }
        }
        Some("--native") if args.len() == 1 => code(launch::native()),
        Some("--task-manager") => {
            let result = (|| {
                let machine = machine()?;
                let _dir = security::protect_directory(&machine.dir).map_err(util::message)?;
                let journal = machine
                    .load()?
                    .ok_or_else(|| tr!("taskManager.recordMissing"))?;
                if journal.phase != Phase::Active {
                    return Err(tr!("taskManager.notActive"));
                }
                let image = machine.dir.join(journal.image);
                let _image = security::protect_directory(&image).map_err(util::message)?;
                ready::activate(&image, &journal.owner, false)
            })();
            match result {
                Ok(()) => 0,
                Err(error) => {
                    log_line("Argus launch failed; opening Windows Task Manager", &error);
                    let native = launch::native();
                    if let Err(error) = &native {
                        log_line("Windows Task Manager launch failed", error);
                    }
                    code(native)
                }
            }
        }
        _ => 87,
    }
}

pub fn recover_main() -> i32 {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--uninstall-check"] {
        let result = (|| {
            let machine = machine()?;
            if registry::observe(registry::KEY)?.debugger.as_ref()
                != Some(&replacement(&machine.dir))
            {
                return Ok(());
            }
            if argus_core::privilege::is_elevated() {
                restore()
            } else {
                elevate(
                    &std::env::current_exe().map_err(|e| e.to_string())?,
                    &["--restore".into()],
                )
                .map_err(|failure| failure.message)
            }
        })();
        return code(result);
    }
    if args == ["--restore"] && argus_core::privilege::is_elevated() {
        return code(restore());
    }
    if args.is_empty() || args == ["--restore"] {
        return code(
            std::env::current_exe()
                .map_err(|e| e.to_string())
                .and_then(|exe| {
                    elevate(&exe, &["--restore".into()]).map_err(|failure| failure.message)
                }),
        );
    }
    87
}

/// Emergency instructions left beside the launcher for whoever administers the
/// machine, in every language: the one who enabled the replacement may not be
/// the one who reads them.
fn recovery_text() -> String {
    Locale::ALL
        .iter()
        .map(|&locale| text_in(locale, "taskManager.recoveryFile"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_text_has_every_locale_and_the_exact_names() {
        let text = recovery_text();
        let key = format!(r"HKLM\{}", registry::KEY);
        for locale in Locale::ALL {
            let part = text_in(locale, "taskManager.recoveryFile");
            assert!(text.contains(&part), "{}", locale.tag());
            for name in [key.as_str(), RECOVER, LAUNCHER, registry::JOURNAL] {
                assert!(part.contains(name), "{}: {name}", locale.tag());
            }
        }
        assert!(!text.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn a_conflict_is_described_for_the_window_and_classified_for_the_log() {
        let ours = replacement(Path::new(r"C:\Program Files\Argus Privileged"));
        let other = r"C:\Users\Someone Private\tools\procexp64.exe";
        let found = registry::Found {
            debugger: Some(Value::string(other)),
            use_filter: true,
            subkeys: vec!["PerfOptions".into()],
            subkey_count: 1,
        };
        let items = found_items(&found, &ours);
        assert_eq!(items.len(), 3);
        assert!(items[0].contains(other));
        assert_eq!(
            found_log(&found, &ours),
            "Debugger names another program; UseFilter value; 1 subkey, PerfOptions among them"
        );
        for (debugger, class, items) in [
            (ours.clone(), "Debugger names the Argus launcher", 0),
            (Value::string(""), "Debugger is empty", 1),
            (
                Value {
                    kind: 4,
                    bytes: vec![1, 0, 0, 0],
                },
                "Debugger is not text",
                1,
            ),
        ] {
            let found = registry::Found {
                debugger: Some(debugger),
                ..Default::default()
            };
            assert_eq!(found_log(&found, &ours), class);
            assert_eq!(found_items(&found, &ours).len(), items);
        }
        // Only the first names are listed; the count says how many there are.
        let many = registry::Found {
            subkeys: ["a", "b", "c", "d"].map(String::from).into(),
            subkey_count: 6,
            ..Default::default()
        };
        assert_eq!(found_log(&many, &ours), "6 subkeys");
        assert!(found_items(&many, &ours)[0].contains('6'));
    }

    #[test]
    fn the_status_summary_never_holds_the_detail() {
        let status = Status {
            state: "conflict",
            can_enable: false,
            can_restore: false,
            recovery_path: r"C:\Program Files\Argus Privileged".into(),
            detail: r"C:\Users\Someone Private".into(),
            found: Some("UseFilter value".into()),
            leftovers: true,
            older: None,
        };
        assert_eq!(
            status.summary(),
            "conflict (UseFilter value); self-test leftovers present"
        );
        let status = Status {
            state: "enabled",
            found: None,
            leftovers: false,
            older: Some("0.1.0".into()),
            ..status
        };
        assert_eq!(
            status.summary(),
            "enabled; the interface copy is version 0.1.0"
        );
    }

    #[test]
    fn an_older_interface_copy_is_told_by_its_version() {
        assert!(older((0, 1, 0), "0.2.0"));
        assert!(older((0, 2, 0), "0.2.1-beta.1"));
        assert!(older((0, 9, 9), "1.0.0"));
        assert!(!older((0, 2, 0), "0.2.0"));
        assert!(!older((0, 2, 0), "0.2.0-beta.1"));
        assert!(!older((0, 3, 0), "0.2.0"));
        assert!(!older((0, 1, 0), "0.2"));
        assert!(!older((0, 1, 0), "0.2.0.1"));
        assert!(!older((0, 1, 0), "next"));
        // A real version resource; a missing file has none.
        let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("kernel32.dll");
        assert!(image_version(&system).is_some_and(|(major, ..)| major >= 6));
        let missing = std::env::temp_dir().join(format!("argus-taskmgr-ui-{}.exe", id().unwrap()));
        assert_eq!(image_version(&missing), None);
        // Only a file named as a copy counts.
        assert_eq!(
            older_interface(system.parent().unwrap(), "kernel32.dll"),
            None
        );
        assert_eq!(
            older_interface(
                missing.parent().unwrap(),
                &missing.file_name().unwrap().to_string_lossy()
            ),
            None
        );
    }

    #[test]
    fn only_interface_copies_no_record_names_are_removed() {
        let dir = std::env::temp_dir().join(format!("argus-taskmgr-{}", id().unwrap()));
        fs::create_dir_all(&dir).unwrap();
        let copy = || format!("argus-taskmgr-ui-{}.exe", id().unwrap());
        let (old, kept, running) = (copy(), copy(), copy());
        let others = [
            LAUNCHER,
            RECOVER,
            registry::JOURNAL,
            "argus-taskmgr-ui-x.exe",
        ];
        for name in [&old, &kept, &running]
            .into_iter()
            .map(String::as_str)
            .chain(others)
        {
            fs::write(dir.join(name), b"x").unwrap();
        }
        // Open without FILE_SHARE_DELETE, as a running copy is.
        let busy = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(dir.join(&running))
            .unwrap();
        remove_images(&dir, Some(&kept));
        drop(busy);
        assert!(!dir.join(&old).exists());
        assert!(dir.join(&kept).exists() && dir.join(&running).exists());
        assert!(others.iter().all(|name| dir.join(name).exists()));
        fs::remove_dir_all(dir).unwrap();
    }

    fn journal(phase: Phase) -> Journal {
        Journal {
            version: 1,
            owner: "S-1-5-21-1-2-3-1001".into(),
            image: format!("argus-taskmgr-ui-{}.exe", id().unwrap()),
            before: Observation::default(),
            replacement: Value::string("launcher.exe --task-manager"),
            phase,
        }
    }

    #[test]
    fn interface_copies_are_named_ones_in_the_protected_folder() {
        let dir = crate::install::directory().unwrap();
        let copy = format!("argus-taskmgr-ui-{}.exe", id().unwrap());
        assert!(is_interface_copy(&dir.join(&copy)));
        // Paths read back from Windows may differ in case.
        let upper = dir.to_string_lossy().to_uppercase();
        assert!(is_interface_copy(&Path::new(&upper).join(&copy)));
        assert!(!is_interface_copy(&dir.join(LAUNCHER)));
        assert!(!is_interface_copy(
            &dir.join("argus-taskmgr-ui-not-a-guid.exe")
        ));
        assert!(!is_interface_copy(&dir.join("argus.exe")));
        assert!(!is_interface_copy(
            &Path::new(r"C:\Program Files\Argus").join(&copy)
        ));
        assert!(!is_interface_copy(&dir.join("sub").join(&copy)));
    }

    #[test]
    fn only_a_finished_enable_argus_still_holds_is_whole() {
        let ours = Value::string("launcher.exe --task-manager");
        let foreign = Observation {
            debugger: Some(Value::string("procexp64.exe")),
            filtered: false,
        };
        let filtered = Observation {
            debugger: Some(ours.clone()),
            filtered: true,
        };
        let argus = Observation {
            debugger: Some(ours.clone()),
            filtered: false,
        };
        let clean = Observation::default();
        // A service update keeps it on only then; anything else is restored first.
        assert!(whole(&journal(Phase::Active), &argus));
        for live in [&clean, &foreign, &filtered] {
            assert!(!whole(&journal(Phase::Active), live));
        }
        for phase in [Phase::Prepared, Phase::Restoring, Phase::Restored] {
            for live in [&clean, &foreign, &filtered, &argus] {
                assert!(!whole(&journal(phase), live));
            }
        }
        // What a held up service update or removal blames.
        let filtered = Observation {
            debugger: None,
            filtered: true,
        };
        assert!(held_by_other(&foreign, &ours) && held_by_other(&filtered, &ours));
        assert!(!held_by_other(&argus, &ours) && !held_by_other(&clean, &ours));
    }

    /// The record and taskmgr.exe's entry in memory.
    #[derive(Default)]
    struct Record {
        live: Observation,
        saved: Option<Journal>,
        fail_save: bool,
    }

    impl Store for Record {
        fn observe(&mut self) -> Result<Observation> {
            Ok(self.live.clone())
        }
        fn write_debugger(&mut self, _: Option<&Value>) -> Result<()> {
            panic!("following an update never writes the entry")
        }
        fn save(&mut self, journal: &Journal) -> Result<()> {
            if self.fail_save {
                return Err("disk failure".into());
            }
            self.saved = Some(journal.clone());
            Ok(())
        }
    }

    /// The protected folder in memory. A busy file cannot be replaced, as a
    /// running program cannot.
    #[derive(Clone, Default, PartialEq, Debug)]
    struct Disk {
        files: std::collections::BTreeMap<String, Vec<u8>>,
        busy: Option<&'static str>,
    }

    impl Files for Disk {
        fn holds(&mut self, name: &str, bytes: &[u8]) -> bool {
            self.files.get(name).is_some_and(|old| old == bytes)
        }
        fn put(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
            if self.busy == Some(name) {
                return Err("in use".into());
            }
            self.files.insert(name.into(), bytes.into());
            Ok(())
        }
        fn remove(&mut self, name: &str) {
            self.files.remove(name);
        }
    }

    const NEW: Assets = (b"launcher 2", b"recover 2");

    /// A replacement enabled by the last build.
    fn enabled() -> (Record, Disk, Journal) {
        let j = journal(Phase::Active);
        let record = Record {
            live: Observation {
                debugger: Some(j.replacement.clone()),
                filtered: false,
            },
            saved: Some(j.clone()),
            fail_save: false,
        };
        let mut disk = Disk::default();
        for (name, bytes) in [
            (j.image.as_str(), b"ui 1".as_slice()),
            (LAUNCHER, b"launcher 1"),
            (RECOVER, b"recover 1"),
            (RECOVERY_TEXT, b"text 1"),
        ] {
            disk.files.insert(name.into(), bytes.into());
        }
        (record, disk, j)
    }

    #[test]
    fn an_enabled_replacement_follows_a_new_build() {
        let (mut record, mut disk, mut j) = enabled();
        let (old, entry) = (j.clone(), record.live.clone());
        assert!(follow(&mut record, &mut disk, &mut j, b"ui 2", NEW).unwrap());
        // A new copy the record names; the old one waits for the sweep.
        assert_ne!(j.image, old.image);
        assert!(valid_image(&j.image));
        assert_eq!(record.saved.as_ref().map(|s| &s.image), Some(&j.image));
        // Nothing else in the record changes.
        let json = |j: &Journal| serde_json::to_value(j).unwrap();
        let renamed = Journal {
            image: old.image.clone(),
            ..j.clone()
        };
        assert_eq!(json(&renamed), json(&old));
        assert_eq!(disk.files[&j.image], b"ui 2");
        assert_eq!(disk.files[&old.image], b"ui 1");
        assert_eq!(disk.files[LAUNCHER], NEW.0);
        assert_eq!(disk.files[RECOVER], NEW.1);
        assert_eq!(disk.files[RECOVERY_TEXT], recovery_text().as_bytes());
        assert_eq!(record.live, entry);
        // Already this build: nothing is written again.
        let before = (disk.clone(), j.clone());
        assert!(follow(&mut record, &mut disk, &mut j, b"ui 2", NEW).unwrap());
        assert_eq!((&disk, &j.image), (&before.0, &before.1.image));
    }

    #[test]
    fn a_replacement_that_is_not_on_is_left_alone() {
        let clean = Observation::default();
        let foreign = Observation {
            debugger: Some(Value::string("procexp64.exe")),
            filtered: false,
        };
        for (phase, live) in [
            (Phase::Restored, Some(clean.clone())),
            (Phase::Restored, None),
            (Phase::Prepared, None),
            (Phase::Restoring, None),
            (Phase::Active, Some(foreign)),
            (Phase::Active, Some(clean)),
            (Phase::Active, None),
        ] {
            let (mut record, mut disk, mut j) = enabled();
            j.phase = phase;
            match live {
                Some(live) => record.live = live,
                // Argus's value; filtered as well when the record is on.
                None => record.live.filtered = phase == Phase::Active,
            }
            let before = (disk.clone(), j.image.clone(), record.saved.clone());
            assert!(
                !follow(&mut record, &mut disk, &mut j, b"ui 2", NEW).unwrap(),
                "{phase:?}"
            );
            assert_eq!((&disk, &j.image), (&before.0, &before.1), "{phase:?}");
            assert_eq!(
                record.saved.map(|s| s.image),
                before.2.map(|s| s.image),
                "{phase:?}"
            );
        }
    }

    #[test]
    fn a_failed_follow_keeps_the_record_and_the_copy_it_names() {
        // The launcher is running: the new copy goes, the record stays.
        let (mut record, mut disk, mut j) = enabled();
        disk.busy = Some(LAUNCHER);
        let before = (disk.files.clone(), j.clone(), record.saved.clone());
        assert!(follow(&mut record, &mut disk, &mut j, b"ui 2", NEW).is_err());
        assert_eq!(disk.files, before.0);
        assert_eq!(j.image, before.1.image);
        assert_eq!(record.saved.map(|s| s.image), before.2.map(|s| s.image));
        // The record cannot be saved: the new copy goes too.
        let (mut record, mut disk, mut j) = enabled();
        record.fail_save = true;
        let old = j.image.clone();
        assert!(follow(&mut record, &mut disk, &mut j, b"ui 2", NEW).is_err());
        assert_eq!(j.image, old);
        assert!(
            disk.files
                .keys()
                .filter(|name| valid_image(name))
                .eq([&old])
        );
    }

    #[test]
    fn helper_failures_reach_the_window_but_stale_or_foreign_files_do_not() {
        let dir = std::env::temp_dir().join(format!("argus-taskmgr-{}", id().unwrap()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(ERROR_FILE);
        let failure = Failure::refused(Step::ProbeCreate, util::message(5), Some(Cause::Blocked));
        fs::write(&path, serde_json::to_vec(&failure).unwrap()).unwrap();
        let read = read_failure(&path, SystemTime::now()).unwrap();
        assert_eq!(
            (read.step, read.cause),
            (Step::ProbeCreate, Some(Cause::Blocked))
        );
        assert_eq!(read.message, failure.message);
        // From an earlier run, or not written by this version's helper.
        let later = SystemTime::now() + Duration::from_secs(60);
        assert!(read_failure(&path, later).is_none());
        fs::write(&path, "plain text").unwrap();
        assert!(read_failure(&path, SystemTime::now()).is_none());
        fs::write(&path, vec![b' '; 9 * 1024]).unwrap();
        assert!(read_failure(&path, SystemTime::now()).is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_log_line_names_the_step_cause_and_code_but_never_the_text() {
        let private = format!(r"C:\Users\Someone Private\x: {}", util::message(5));
        let failure = Failure::refused(Step::ProbeCreate, private.clone(), Some(Cause::Blocked))
            .kept(Failure::new(Step::ProbeCleanup, private));
        let line = failure.log();
        assert!(line.starts_with(&format!("at {}:", Step::ProbeCreate.name())));
        assert!(line.contains("(os error 5)"), "{line}");
        assert!(line.contains(Cause::Blocked.log()), "{line}");
        assert!(line.contains("self-test entry kept"), "{line}");
        assert!(!line.contains("Someone") && !line.contains('\\'), "{line}");
        let refused = Failure {
            state: Some("conflict"),
            ..Failure::new(Step::Check, tr!("taskManager.notAllowed"))
        };
        assert!(refused.log().contains("not allowed while conflict"));
    }
}
