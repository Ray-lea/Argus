use super::*;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoInitializeSecurity, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::TaskScheduler::*;
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, Interface};

pub(super) struct Scheduler {
    pub folder: ITaskFolder,
    service: ITaskService,
    security_code: Option<u32>,
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
        // The native UI can already have initialized this thread as an STA.
        if initialized.is_err() && initialized.0 as u32 != 0x80010106 {
            return util::stage(
                "administrator COM initialization",
                Err(initialized.0 as u32),
            );
        }
        let result = (|| unsafe {
            // Task Scheduler may impersonate the caller while registering a
            // task. Initialize this before any shell COM object can choose the
            // process defaults. A running GUI may already own those defaults.
            let security = CoInitializeSecurity(
                None,
                -1,
                None,
                None,
                RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
                None,
            );
            let security_code = match security {
                Ok(()) => 0,
                Err(error) if error.code().0 as u32 == 0x80010119 => 0x80010119,
                Err(error) => {
                    return util::stage(
                        "administrator COM security initialization",
                        Err(win(error)),
                    );
                }
            };
            let service: ITaskService = util::stage(
                "administrator TaskScheduler creation",
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(win),
            )?;
            let empty = VARIANT::default();
            util::stage(
                "administrator ITaskService::Connect",
                service.Connect(&empty, &empty, &empty, &empty).map_err(win),
            )?;
            let folder = util::stage(
                "administrator ITaskService::GetFolder",
                service.GetFolder(&BSTR::from("\\")).map_err(win),
            )?;
            Ok((service, folder, security_code))
        })();
        match result {
            Ok((service, folder, security_code)) => Ok(Self {
                folder,
                service,
                security_code: Some(security_code),
                _apartment: Apartment(initialized.is_ok()),
            }),
            Err(e) => {
                if initialized.is_ok() {
                    unsafe { CoUninitialize() };
                }
                Err(e)
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
        let registered = unsafe {
            self.folder.RegisterTask(
                &BSTR::from(name(id, sid)),
                &BSTR::from(xml.as_str()),
                TASK_CREATE_OR_UPDATE.0 | TASK_DONT_ADD_PRINCIPAL_ACE.0,
                &VARIANT::from(sid),
                &VARIANT::default(),
                TASK_LOGON_INTERACTIVE_TOKEN,
                &VARIANT::from(acl.as_str()),
            )
        }
        .map_err(win);
        if registered.is_err() {
            argus_core::diagnostics::warn(
                "administrator task registration context",
                self.registration_context(id, sid, image, &xml),
            );
        }
        util::stage("administrator ITaskFolder::RegisterTask", registered)?;
        Ok(())
    }

    /// Read-only failure facts. Keep identity checks boolean; only the action
    /// path is logged, never task XML, arguments or arbitrary COM descriptions.
    fn registration_context(&self, id: &str, sid: &str, image: &Path, xml: &str) -> String {
        let validation = unsafe {
            self.service
                .NewTask(0)
                .and_then(|task| task.SetXmlText(&BSTR::from(xml)))
        };
        let validation_code = validation.err().map_or(0, |e| e.code().0 as u32);
        let existing = unsafe { self.folder.GetTask(&BSTR::from(name(id, sid))) };
        let existing_code = existing.as_ref().err().map_or(0, |e| e.code().0 as u32);
        let state = existing
            .ok()
            .and_then(|task| unsafe { task.State().ok() })
            .map(|state| state.0);
        let schema = unsafe { self.service.HighestVersion().ok() };
        format!(
            "xml_validation_hresult=0x{validation_code:08x}; existing_task_hresult=0x{existing_code:08x}; existing_state={state:?}; schema={schema:?}; com_security={:?}; elevated={}; principal_matches_caller={}; session={:?}; xml_chars={}; action_chars={}; image_exists={:?}; image={}",
            self.security_code,
            argus_core::privilege::is_elevated(),
            security::current_sid().is_ok_and(|caller| caller == sid),
            session(std::process::id()).ok(),
            xml.encode_utf16().count(),
            image.as_os_str().encode_wide().count(),
            image.try_exists().ok(),
            argus_core::diagnostics::path(image)
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "registers only a temporary task in an explicitly configured disposable VM"]
    fn vm_registration_security() {
        assert_eq!(std::env::var("ARGUS_VM_TEST").as_deref(), Ok("1"));
        let account = std::env::var("ARGUS_VM_ACCOUNT").expect("VM account");
        let sid = security::account_sid(&account).unwrap();
        let mode = std::env::var("ARGUS_VM_COM_MODE").unwrap_or_else(|_| "fixed".into());
        let scheduler = if mode == "fixed" {
            Scheduler::new().unwrap()
        } else {
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
            }
            if mode == "identify" || mode == "anonymous" {
                use windows::Win32::System::Com::{
                    RPC_C_IMP_LEVEL_ANONYMOUS, RPC_C_IMP_LEVEL_IDENTIFY,
                };
                unsafe {
                    CoInitializeSecurity(
                        None,
                        -1,
                        None,
                        None,
                        RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
                        if mode == "identify" {
                            RPC_C_IMP_LEVEL_IDENTIFY
                        } else {
                            RPC_C_IMP_LEVEL_ANONYMOUS
                        },
                        None,
                        EOAC_NONE,
                        None,
                    )
                }
                .unwrap();
            }
            let service: ITaskService =
                unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }.unwrap();
            let empty = VARIANT::default();
            unsafe { service.Connect(&empty, &empty, &empty, &empty) }.unwrap();
            Scheduler {
                folder: unsafe { service.GetFolder(&BSTR::from("\\")) }.unwrap(),
                service,
                security_code: None,
                _apartment: Apartment(true),
            }
        };
        let id = format!("dev.argus.taskmanager.test.com-{}", std::process::id());
        let image = std::env::current_exe().unwrap();
        let registered = scheduler.register(&id, &sid, &image);
        println!("registration mode={mode}; result={registered:?}");
        println!(
            "registration-context={}",
            scheduler.registration_context(&id, &sid, &image, &xml(&sid, &image))
        );
        let malformed = scheduler.registration_context(&id, &sid, &image, "<invalid");
        assert!(!malformed.contains("xml_validation_hresult=0x00000000"));
        assert!(!malformed.contains("<invalid"));
        println!("invalid-xml-context={malformed}");
        let verified = registered
            .as_ref()
            .ok()
            .map(|_| checked(&id, &sid, &image).is_ok());
        let _ = scheduler.remove(&id, &sid);
        if mode == "fixed" {
            registered.unwrap();
            assert_eq!(verified, Some(true));
        } else if registered.is_ok() {
            assert_eq!(verified, Some(true));
        }
    }

    #[test]
    #[ignore = "creates a temporary scheduled task in an explicitly configured disposable VM"]
    fn vm_ambiguous_account_principal() {
        assert_eq!(std::env::var("ARGUS_VM_TEST").as_deref(), Ok("1"));
        let account = std::env::var("ARGUS_VM_ACCOUNT").expect("VM account name");
        let computer = std::env::var("COMPUTERNAME").unwrap();
        assert!(account.eq_ignore_ascii_case(&computer));
        let sid = security::account_sid(&format!("{computer}\\{account}")).unwrap();
        let id = "dev.argus.taskmanager.test.account";
        let image = std::env::current_exe().unwrap();
        let scheduler = Scheduler::new().unwrap();
        scheduler.register(id, &sid, &image).unwrap();
        let result = checked(id, &sid, &image);
        scheduler.remove(id, &sid).unwrap();
        assert!(
            result.is_ok(),
            "normalized task principal rejected: {:?}",
            result.err()
        );
    }
}
