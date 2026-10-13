//! Task Manager replacement is opt-in. Status is read-only. All persistent
//! writes live behind explicit maintenance actions, with a durable journal.
mod launch;
pub mod ready;
mod registry;
mod state;

use crate::{
    security,
    util::{self, Handle, wide},
};
use argus_core::i18n::{Locale, text_in};
use argus_core::tr;
use serde::Serialize;
use state::{Phase, Result, Store, Value};
use std::fs::{self, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;
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
const MANAGE: &str = "--argus-taskmgr-manage";
pub type Assets = (&'static [u8], &'static [u8]);

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
fn replacement(dir: &Path) -> Value {
    Value::string(&format!(
        "{} --task-manager",
        launch::quote(&dir.join(LAUNCHER).to_string_lossy())
    ))
}
fn machine() -> Result<registry::Machine> {
    Ok(registry::Machine {
        dir: crate::install::directory().map_err(util::message)?,
    })
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
    };
    if !machine.dir.exists() {
        status.detail = tr!("taskManager.status.needsService");
        if live.debugger.is_some() || live.filtered {
            status.state = "conflict";
            status.detail = tr!("taskManager.status.conflict");
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
                tr!("taskManager.status.enabled")
            } else {
                tr!("taskManager.status.interrupted")
            };
            return Ok(status);
        }
        if live != journal.before {
            status.state = "conflict";
            status.detail = tr!("taskManager.status.modified");
            return Ok(status);
        }
    } else if live.debugger.as_ref() == Some(&replacement(&machine.dir)) {
        status.state = "recoveryNeeded";
        status.detail = tr!("taskManager.status.recordMissing");
        return Ok(status);
    }
    if live.debugger.is_some() || live.filtered {
        status.state = "conflict";
        status.detail = tr!("taskManager.status.conflict");
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

/// The service installer already holds install.lock when calling this.
pub(crate) fn restore_for_maintenance(dir: &Path) -> Result<()> {
    let mut machine = registry::Machine { dir: dir.into() };
    match machine.load()? {
        Some(mut journal) => state::restore(&mut machine, &mut journal),
        None if machine.observe()?.debugger.as_ref() == Some(&replacement(dir)) => {
            Err(tr!("taskManager.launcherKept"))
        }
        None => Ok(()),
    }
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
    cleanup_probe(&machine.dir)?;
    restore_for_maintenance(&machine.dir)
}

pub(crate) fn reconcile(dir: &Path) -> Result<()> {
    let machine = registry::Machine { dir: dir.into() };
    if dir.join("taskmgr-probe.txt").exists() {
        let _lock = lock(dir)?;
        cleanup_probe(dir)?;
    }
    if let Some(journal) = machine.load()?
        && matches!(journal.phase, Phase::Prepared | Phase::Restoring)
    {
        let _lock = lock(dir)?;
        restore_for_maintenance(dir)?;
    }
    Ok(())
}

fn enable(owner: &str, caller: u32, created: u64, assets: Assets) -> Result<()> {
    if cfg!(debug_assertions) {
        return Err(tr!("taskManager.devBuild"));
    }
    if !argus_core::privilege::is_elevated() {
        return Err(tr!("taskManager.enableNeedsAdmin"));
    }
    security::validate_sid(owner).map_err(util::message)?;
    let mut machine = machine()?;
    let _parent = security::protect_directory(parent(&machine.dir)?).map_err(util::message)?;
    let _dir = security::protect_directory(&machine.dir).map_err(util::message)?;
    if crate::install::owner(&machine.dir).map_err(util::message)? != owner {
        return Err(tr!("taskManager.ownerMismatch"));
    }
    let _lock = lock(&machine.dir)?;
    restore_for_maintenance(&machine.dir)?;
    let live = machine.observe()?;
    if live.debugger.is_some() || live.filtered {
        return Err(tr!("taskManager.conflict"));
    }
    // Errors name the step: the helper's message box is all the user sees.
    let image = format!("argus-taskmgr-ui-{}.exe", id()?);
    (|| {
        registry::atomic_file(&machine.dir, LAUNCHER, assets.0)?;
        registry::atomic_file(&machine.dir, RECOVER, assets.1)?;
        // Source code and embedded helper bytes are from the explicitly elevated
        // GUI executable, not adjacent user-writable sidecar files.
        let source = fs::read(std::env::current_exe().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        registry::atomic_file(&machine.dir, &image, &source)?;
        registry::atomic_file(
            &machine.dir,
            "taskmgr-recovery.txt",
            recovery_text().as_bytes(),
        )
    })()
    .map_err(|error| tr!("taskManager.prepareFailed", error = error))?;
    preflight(&machine.dir).map_err(|error| tr!("taskManager.selfTestFailed", error = error))?;
    let probe = launch::probe_as_caller(
        &machine.dir.join(LAUNCHER),
        &["--probe-ui".into(), image.clone(), owner.into()],
        caller,
        created,
        owner,
    )
    .map_err(|error| tr!("taskManager.uiStartFailed", error = error))?;
    if launch::exit_code(&probe, Duration::from_secs(15))? != 0 {
        return Err(tr!("taskManager.uiNotReady"));
    }
    let value = replacement(&machine.dir);
    state::enable(&mut machine, owner.into(), image, value)?;
    Ok(())
}

/// This runs only inside an explicitly elevated Enable action. The randomly
/// named probe has no GUI and never redirects taskmgr.exe. Cleanup is checked.
fn preflight(dir: &Path) -> Result<()> {
    cleanup_probe(dir)?;
    let name = format!("ArgusTaskmgrProbe-{}.exe", id()?);
    let path = dir.join(&name);
    let key = format!("{}\\{name}", registry::IFEO);
    let debugger = Value::string(&format!(
        "{} --probe-redirect",
        launch::quote(&dir.join(LAUNCHER).to_string_lossy())
    ));
    if registry::open(&key, false)?.is_some() {
        return Err(tr!("taskManager.probeNameTaken"));
    }
    registry::atomic_file(dir, "taskmgr-probe.txt", name.as_bytes())?;
    registry::atomic_file(
        dir,
        &name,
        &fs::read(dir.join(LAUNCHER)).map_err(|e| e.to_string())?,
    )?;
    let result = (|| {
        registry::write(&key, Some(&debugger))?;
        let normal = launch::spawn(&path, &["--probe-child".into()], false)?;
        if launch::exit_code(&normal, Duration::from_secs(5))? != 86 {
            return Err(tr!("taskManager.probeNotRedirected"));
        }
        let bypass = launch::spawn(&path, &["--probe-child".into()], true)?;
        if launch::exit_code(&bypass, Duration::from_secs(5))? != 0 {
            return Err(tr!("taskManager.probeFallbackFailed"));
        }
        Ok(())
    })();
    cleanup_probe(dir)?;
    result
}

fn cleanup_probe(dir: &Path) -> Result<()> {
    let record = dir.join("taskmgr-probe.txt");
    if !record.exists() {
        return Ok(());
    }
    let file = security::protect_directory(&record).map_err(util::message)?;
    if fs::metadata(&record).map_err(|e| e.to_string())?.len() > 128 {
        return Err(tr!("taskManager.probeRecordInvalid"));
    }
    let name = fs::read_to_string(&record).map_err(|e| e.to_string())?;
    if !name
        .strip_prefix("ArgusTaskmgrProbe-")
        .and_then(|s| s.strip_suffix(".exe"))
        .is_some_and(|s| s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
    {
        return Err(tr!("taskManager.probeRecordInvalid"));
    }
    let path = dir.join(&name);
    let key = format!("{}\\{name}", registry::IFEO);
    let debugger = Value::string(&format!(
        "{} --probe-redirect",
        launch::quote(&dir.join(LAUNCHER).to_string_lossy())
    ));
    let actual = registry::observe(&key)?;
    if actual.filtered || (actual.debugger.is_some() && actual.debugger.as_ref() != Some(&debugger))
    {
        return Err(tr!("taskManager.probeModified"));
    }
    registry::write(&key, None)?;
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RegDeleteKeyW};
    let code = unsafe { RegDeleteKeyW(HKEY_LOCAL_MACHINE, PCWSTR(wide(&key).as_ptr())) };
    if code.0 != 2 {
        code.ok().map_err(|e| e.to_string())?;
    }
    if path.exists() {
        drop(security::protect_directory(&path).map_err(util::message)?);
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    drop(file);
    fs::remove_file(record).map_err(|e| e.to_string())
}

pub fn manage(action: &str) -> Result<Status> {
    match action {
        "restore" => {
            if let Ok(Some(crate::Reply::Done)) =
                crate::request(crate::Operation::RestoreTaskManager)
            {
                return status();
            }
            elevate(
                &std::env::current_exe().map_err(|e| e.to_string())?,
                &[
                    MANAGE.into(),
                    "restore".into(),
                    security::current_sid().map_err(util::message)?,
                ],
            )?;
        }
        "enable" if status()?.can_enable => {
            elevate(
                &std::env::current_exe().map_err(|e| e.to_string())?,
                &[
                    MANAGE.into(),
                    "enable".into(),
                    security::current_sid().map_err(util::message)?,
                    std::process::id().to_string(),
                    launch::identity(unsafe {
                        windows::Win32::System::Threading::GetCurrentProcess()
                    })?
                    .to_string(),
                ],
            )?;
        }
        _ => return Err(tr!("taskManager.notAllowed")),
    }
    status()
}

fn elevate(exe: &Path, args: &[String]) -> Result<()> {
    let file = wide(&exe.to_string_lossy());
    let params = wide(
        &args
            .iter()
            .map(|s| launch::quote(s))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let verb = wide("runas");
    let directory = wide(&launch::system_directory()?.to_string_lossy());
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
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| e.to_string())?;
    if info.hProcess == HANDLE::default() {
        return Err(tr!("taskManager.helperNotStarted"));
    }
    let process = Handle(info.hProcess);
    if unsafe { WaitForSingleObject(process.0, 120_000) } != WAIT_OBJECT_0 {
        return Err(tr!("taskManager.helperBusy"));
    }
    let mut code = 0;
    unsafe { GetExitCodeProcess(process.0, &mut code) }.map_err(|e| e.to_string())?;
    if code == 0 {
        Ok(())
    } else {
        Err(tr!("taskManager.helperFailed"))
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

pub fn run_mode(assets: Option<Assets>) -> Option<i32> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, action, owner, pid, created] if mode == MANAGE && action == "enable" => {
            Some(code((|| {
                let pid = pid.parse().map_err(|_| tr!("taskManager.invalidCaller"))?;
                let created = created
                    .parse()
                    .map_err(|_| tr!("taskManager.invalidInstance"))?;
                assets
                    .ok_or_else(|| tr!("taskManager.noLauncher"))
                    .and_then(|a| enable(owner, pid, created, a))
            })()))
        }
        [mode, action, _owner] if mode == MANAGE && action == "restore" => Some(code(restore())),
        [mode] if mode == "--argus-taskmgr-uninstall" => Some(code(uninstall_guard())),
        _ if args.first().is_some_and(|s| s == MANAGE) => Some(87),
        _ => None,
    }
}

fn uninstall_guard() -> Result<()> {
    let machine = machine()?;
    if registry::observe(registry::KEY)?.debugger.as_ref() != Some(&replacement(&machine.dir)) {
        return Ok(());
    }
    manage("restore").map(|_| ())
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
            if result.is_ok() { 0 } else { 1 }
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
            if result.is_ok() {
                0
            } else {
                code(launch::native())
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
                .and_then(|exe| elevate(&exe, &["--restore".into()])),
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
}
