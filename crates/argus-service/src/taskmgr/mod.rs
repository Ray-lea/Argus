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
        detail: "当前使用 Windows 任务管理器。".into(),
    };
    if !machine.dir.exists() {
        status.detail = "先安装特权服务，再启用任务管理器替换。".into();
        if live.debugger.is_some() || live.filtered {
            status.state = "conflict";
            status.detail = "已有其他替换或路径过滤配置，Argus 不会覆盖。".into();
        }
        return Ok(status);
    }
    let _parent = security::protect_directory(machine.dir.parent().ok_or("无效目录")?)
        .map_err(util::message)?;
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
                "任务管理器入口已交给 Argus；启动失败时尝试打开原生任务管理器。"
            } else {
                "上次操作未完成，请先恢复 Windows 任务管理器。"
            }
            .into();
            return Ok(status);
        }
        if live != journal.before {
            status.state = "conflict";
            status.detail = "配置已被其他程序修改，Argus 不会覆盖；恢复记录仍保留。".into();
            return Ok(status);
        }
    } else if live.debugger.as_ref() == Some(&replacement(&machine.dir)) {
        status.state = "recoveryNeeded";
        status.detail = "发现 Argus 替换但恢复记录缺失，请按应急恢复说明处理。".into();
        return Ok(status);
    }
    if live.debugger.is_some() || live.filtered {
        status.state = "conflict";
        status.detail = "已有其他替换或路径过滤配置，Argus 不会覆盖。".into();
    } else if !installed {
        status.detail = "特权服务已卸载，恢复程序和备份仍保留。".into();
    } else if cfg!(debug_assertions) {
        status.detail = "开发版不提供系统任务管理器替换，请使用发布版。".into();
    } else if !authorized {
        status.detail = "特权服务授权给另一个 Windows 账户。".into();
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
            Err("Argus 替换的恢复记录缺失，已阻止删除启动器".into())
        }
        None => Ok(()),
    }
}

pub fn restore() -> Result<()> {
    if !argus_core::privilege::is_elevated() {
        return Err("恢复需要管理员权限".into());
    }
    let machine = machine()?;
    if !machine.dir.exists() {
        if registry::observe(registry::KEY)?.debugger.as_ref() == Some(&replacement(&machine.dir)) {
            return Err("恢复记录和启动器目录缺失，请使用应急恢复说明".into());
        }
        return Ok(());
    }
    let _parent = security::protect_directory(machine.dir.parent().ok_or("无效目录")?)
        .map_err(util::message)?;
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
        return Err("开发版不允许启用替换".into());
    }
    if !argus_core::privilege::is_elevated() {
        return Err("启用需要管理员权限".into());
    }
    security::validate_sid(owner).map_err(util::message)?;
    let mut machine = machine()?;
    let _parent = security::protect_directory(machine.dir.parent().ok_or("无效目录")?)
        .map_err(util::message)?;
    let _dir = security::protect_directory(&machine.dir).map_err(util::message)?;
    if crate::install::owner(&machine.dir).map_err(util::message)? != owner {
        return Err("服务授权账户不匹配".into());
    }
    let _lock = lock(&machine.dir)?;
    restore_for_maintenance(&machine.dir)?;
    let live = machine.observe()?;
    if live.debugger.is_some() || live.filtered {
        return Err("已有其他替换或路径过滤配置，已停止启用".into());
    }
    registry::atomic_file(&machine.dir, LAUNCHER, assets.0)?;
    registry::atomic_file(&machine.dir, RECOVER, assets.1)?;
    let image = format!("argus-taskmgr-ui-{}.exe", id()?);
    // Source code and embedded helper bytes are from the explicitly elevated
    // GUI executable, not adjacent user-writable sidecar files.
    let source =
        fs::read(std::env::current_exe().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    registry::atomic_file(&machine.dir, &image, &source)?;
    registry::atomic_file(&machine.dir, "taskmgr-recovery.txt", RECOVERY.as_bytes())?;
    preflight(&machine.dir)?;
    let probe = launch::probe_as_caller(
        &machine.dir.join(LAUNCHER),
        &["--probe-ui".into(), image.clone(), owner.into()],
        caller,
        created,
        owner,
    )?;
    if launch::exit_code(&probe, Duration::from_secs(15))? != 0 {
        return Err("标准权限界面的就绪检查失败，未启用替换".into());
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
        return Err("启动自检名称冲突".into());
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
            return Err("IFEO 自检未发生预期重定向".into());
        }
        let bypass = launch::spawn(&path, &["--probe-child".into()], true)?;
        if launch::exit_code(&bypass, Duration::from_secs(5))? != 0 {
            return Err("原生回退自检失败".into());
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
        return Err("启动自检恢复记录异常".into());
    }
    let name = fs::read_to_string(&record).map_err(|e| e.to_string())?;
    if !name
        .strip_prefix("ArgusTaskmgrProbe-")
        .and_then(|s| s.strip_suffix(".exe"))
        .is_some_and(|s| s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
    {
        return Err("启动自检恢复记录异常".into());
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
        return Err("自检配置被外部修改，已保留条目供检查".into());
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
        _ => return Err("当前状态不允许此操作，请刷新后重试".into()),
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
        return Err("未能启动管理员维护助手".into());
    }
    let process = Handle(info.hProcess);
    if unsafe { WaitForSingleObject(process.0, 120_000) } != WAIT_OBJECT_0 {
        return Err("维护助手仍在处理，请刷新状态；不要重复操作".into());
    }
    let mut code = 0;
    unsafe { GetExitCodeProcess(process.0, &mut code) }.map_err(|e| e.to_string())?;
    if code == 0 {
        Ok(())
    } else {
        Err("任务管理器设置未完成，请查看维护助手提示并刷新状态".into())
    }
}

fn alert(error: &str) {
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(wide(error).as_ptr()),
            PCWSTR(wide("Argus 任务管理器恢复").as_ptr()),
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
                let pid = pid.parse().map_err(|_| "无效界面进程".to_string())?;
                let created = created.parse().map_err(|_| "无效进程实例".to_string())?;
                assets
                    .ok_or_else(|| "此版本未包含独立启动器".into())
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
                let journal = machine.load()?.ok_or("恢复记录缺失")?;
                if journal.phase != Phase::Active {
                    return Err("替换尚未完成启用".into());
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

const RECOVERY: &str = "Argus 任务管理器应急恢复\r\n\r\n先运行本目录的 argus-taskmgr-recover.exe，并允许管理员授权。它不依赖 Argus 界面或后台服务。\r\n如果程序也已损坏：Win+R 输入 regedit，以管理员身份打开，定位 HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Image File Execution Options\\taskmgr.exe。核对 Debugger 确实指向本目录的 argus-taskmgr-launcher.exe --task-manager，且 taskmgr-recovery.json 中 before.debugger 为 null，再只删除 Debugger 值。不要删除整个 taskmgr.exe 键，不要删除其他工具的设置。\r\n恢复程序会保留第三方更改和恢复记录；缺少可信备份时停止自动恢复。\r\n";
