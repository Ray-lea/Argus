use super::*;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::TaskScheduler::*;
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, Interface};

pub(super) struct Scheduler {
    pub folder: ITaskFolder,
    _apartment: Apartment,
}
struct Apartment(bool);
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}
impl Scheduler {
    pub fn new() -> Result<Self> {
        let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // Tauri can already have initialized this thread as an STA.
        if initialized.is_err() && initialized.0 as u32 != 0x80010106 {
            return Err(initialized.0 as u32);
        }
        let result = (|| unsafe {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
            let empty = VARIANT::default();
            service.Connect(&empty, &empty, &empty, &empty)?;
            service.GetFolder(&BSTR::from("\\"))
        })();
        match result {
            Ok(folder) => Ok(Self {
                folder,
                _apartment: Apartment(initialized.is_ok()),
            }),
            Err(e) => {
                if initialized.is_ok() {
                    unsafe { CoUninitialize() };
                }
                Err(win(e))
            }
        }
    }
    pub fn get(&self, id: &str, sid: &str) -> Result<IRegisteredTask> {
        unsafe { self.folder.GetTask(&BSTR::from(name(id, sid))) }.map_err(win)
    }
    pub fn remove(&self, id: &str, sid: &str) -> Result<()> {
        match unsafe { self.folder.DeleteTask(&BSTR::from(name(id, sid)), 0) }.map_err(win) {
            Err(2) => Ok(()),
            result => result,
        }
    }
    pub fn register(&self, id: &str, sid: &str, image: &Path) -> Result<()> {
        let xml = xml(sid, image);
        let acl = format!("O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;{sid})");
        unsafe {
            self.folder.RegisterTask(
                &BSTR::from(name(id, sid)),
                &BSTR::from(xml),
                TASK_CREATE_OR_UPDATE.0 | TASK_DONT_ADD_PRINCIPAL_ACE.0,
                &VARIANT::from(sid),
                &VARIANT::default(),
                TASK_LOGON_INTERACTIVE_TOKEN,
                &VARIANT::from(acl.as_str()),
            )
        }
        .map_err(win)?;
        Ok(())
    }
}
fn name(id: &str, sid: &str) -> String {
    format!("Argus.{id}.{sid}")
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn xml(sid: &str, image: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.3" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
<Principals><Principal id="Owner"><UserId>{sid}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
<Settings><MultipleInstancesPolicy>Parallel</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><AllowHardTerminate>false</AllowHardTerminate><StartWhenAvailable>false</StartWhenAvailable><RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable><IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings><AllowStartOnDemand>true</AllowStartOnDemand><Enabled>true</Enabled><Hidden>false</Hidden><RunOnlyIfIdle>false</RunOnlyIfIdle><WakeToRun>false</WakeToRun><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Priority>4</Priority></Settings>
<Actions Context="Owner"><Exec><Command>{image}</Command><Arguments>{RUN} $(Arg0)</Arguments><WorkingDirectory>{directory}</WorkingDirectory></Exec></Actions></Task>"#,
        image = escape(&image.to_string_lossy()),
        directory = escape(&image.parent().unwrap().to_string_lossy())
    )
}

pub(super) struct Checked {
    task: IRegisteredTask,
    _scheduler: Scheduler,
}
pub(super) fn checked(id: &str, sid: &str, image: &Path) -> Result<Checked> {
    let scheduler = Scheduler::new()?;
    let task = scheduler.get(id, sid)?;
    unsafe {
        let definition = task.Definition().map_err(win)?;
        let principal = definition.Principal().map_err(win)?;
        let actions = definition.Actions().map_err(win)?;
        let mut user = BSTR::new();
        let mut logon = TASK_LOGON_TYPE::default();
        let mut level = TASK_RUNLEVEL_TYPE::default();
        let mut count = 0;
        principal.UserId(&mut user).map_err(win)?;
        principal.LogonType(&mut logon).map_err(win)?;
        principal.RunLevel(&mut level).map_err(win)?;
        actions.Count(&mut count).map_err(win)?;
        if !task.Enabled().map_err(win)?.as_bool()
            || security::account_sid(&user.to_string())? != sid
            || logon != TASK_LOGON_INTERACTIVE_TOKEN
            || level != TASK_RUNLEVEL_HIGHEST
            || count != 1
        {
            return Err(13);
        }
        let action: IExecAction = actions.get_Item(1).map_err(win)?.cast().map_err(win)?;
        let mut path = BSTR::new();
        let mut arguments = BSTR::new();
        action.Path(&mut path).map_err(win)?;
        action.Arguments(&mut arguments).map_err(win)?;
        if !path
            .to_string()
            .eq_ignore_ascii_case(&image.to_string_lossy())
            || arguments != format!("{RUN} $(Arg0)")
        {
            return Err(13);
        }
    }
    Ok(Checked {
        task,
        _scheduler: scheduler,
    })
}

pub(super) fn run(id: &str, sid: &str, image: &Path, ticket: &str) -> Result<()> {
    let checked = checked(id, sid, image)?;
    unsafe {
        checked.task.RunEx(
            &VARIANT::from(ticket),
            TASK_RUN_USE_SESSION_ID.0,
            session(std::process::id())? as i32,
            &BSTR::new(),
        )
    }
    .map_err(win)?;
    Ok(())
}
