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
    pub fn exists(&self, id: &str, sid: &str) -> Result<bool> {
        match self.get(id, sid) {
            Ok(_) => Ok(true),
            Err(2) => Ok(false),
            Err(code) => Err(code),
        }
    }
    pub fn remove_all(&self, id: &str) -> Result<()> {
        if !valid_id(id) {
            return Err(87);
        }
        let tasks = unsafe { self.folder.GetTasks(TASK_ENUM_HIDDEN.0) }.map_err(win)?;
        let mut owners = Vec::new();
        for index in 1..=unsafe { tasks.Count() }.map_err(win)? {
            let task = unsafe { tasks.get_Item(&VARIANT::from(index)) }.map_err(win)?;
            let name = unsafe { task.Name() }.map_err(win)?.to_string();
            if let Some(sid) = task_owner(&name, id) {
                owners.push(sid.to_owned());
            }
        }
        // Collect first: deleting tasks changes the collection indices.
        for sid in owners {
            self.remove(id, &sid)?;
        }
        Ok(())
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
fn task_owner<'a>(name: &'a str, id: &str) -> Option<&'a str> {
    let sid = name.strip_prefix(&format!("Argus.{id}."))?;
    security::validate_sid(sid).ok()?;
    Some(sid)
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
    let task = util::stage("administrator task lookup", scheduler.get(id, sid))?;
    let definition = util::stage("administrator task definition", unsafe {
        task.Definition().map_err(win)
    })?;
    let enabled = util::stage("administrator task enabled query", unsafe {
        task.Enabled().map_err(win)
    })?;
    validate_definition(&definition, enabled.as_bool(), sid, image)?;
    Ok(Checked {
        task,
        _scheduler: scheduler,
    })
}

fn require(stage: &'static str, valid: bool) -> Result<()> {
    util::stage(stage, if valid { Ok(()) } else { Err(13) })
}

/// A failed check keeps ERROR_INVALID_DATA while naming only the rejected
/// invariant. Principal names, task XML and action arguments stay out of logs.
fn validate_definition(
    definition: &ITaskDefinition,
    enabled: bool,
    sid: &str,
    image: &Path,
) -> Result<()> {
    unsafe {
        let principal = util::stage(
            "administrator task principal query",
            definition.Principal().map_err(win),
        )?;
        let actions = util::stage(
            "administrator task actions query",
            definition.Actions().map_err(win),
        )?;
        let mut user = BSTR::new();
        let mut logon = TASK_LOGON_TYPE::default();
        let mut level = TASK_RUNLEVEL_TYPE::default();
        let mut count = 0;
        util::stage(
            "administrator task principal user query",
            principal.UserId(&mut user).map_err(win),
        )?;
        util::stage(
            "administrator task logon query",
            principal.LogonType(&mut logon).map_err(win),
        )?;
        util::stage(
            "administrator task run level query",
            principal.RunLevel(&mut level).map_err(win),
        )?;
        util::stage(
            "administrator task action count query",
            actions.Count(&mut count).map_err(win),
        )?;
        require("administrator task enabled validation", enabled)?;
        let owner = util::stage(
            "administrator task principal SID resolution",
            security::account_sid(&user.to_string()),
        )?;
        require("administrator task principal validation", owner == sid)?;
        require(
            "administrator task logon validation",
            logon == TASK_LOGON_INTERACTIVE_TOKEN,
        )?;
        require(
            "administrator task run level validation",
            level == TASK_RUNLEVEL_HIGHEST,
        )?;
        require("administrator task action count validation", count == 1)?;
        let action: IExecAction = util::stage(
            "administrator task executable action query",
            actions
                .get_Item(1)
                .and_then(|action| action.cast())
                .map_err(win),
        )?;
        let mut path = BSTR::new();
        let mut arguments = BSTR::new();
        util::stage(
            "administrator task action path query",
            action.Path(&mut path).map_err(win),
        )?;
        util::stage(
            "administrator task action arguments query",
            action.Arguments(&mut arguments).map_err(win),
        )?;
        require(
            "administrator task action path validation",
            path.to_string()
                .eq_ignore_ascii_case(&image.to_string_lossy()),
        )?;
        require(
            "administrator task action arguments validation",
            arguments == format!("{RUN} $(Arg0)"),
        )?;
    }
    Ok(())
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
    fn task_validation_identifies_each_mismatch_without_logging_task_contents() {
        use std::os::windows::process::CommandExt;

        // The logger and its bounded failure ring are process-wide. Keep this
        // assertion independent of failures emitted by concurrently run tests.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "admin::task::tests::task_validation_fixture",
                "--ignored",
                "--nocapture",
            ])
            .creation_flags(CREATE_NO_WINDOW.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "isolated diagnostic logger fixture, run by the parent test"]
    fn task_validation_fixture() {
        use argus_core::diagnostics;

        // Parse and edit definitions in memory: never connect to the scheduler,
        // register a task, launch a process or require administrator rights.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }.unwrap();
        let _apartment = Apartment(true);
        let service: ITaskService =
            unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }.unwrap();
        let sid = "S-1-5-21-1-2-3-1001";
        let image = Path::new(r"C:\Private & Unicode 目录\argus.exe");
        let xml = xml(sid, image);
        diagnostics::capture_helper_failures();

        for field in [
            "enabled",
            "principal",
            "logon",
            "run level",
            "action count",
            "action path",
            "action arguments",
            "action type",
        ] {
            let definition = unsafe { service.NewTask(0) }.unwrap();
            unsafe { definition.SetXmlText(&BSTR::from(xml.as_str())) }.unwrap();
            assert_eq!(validate_definition(&definition, true, sid, image), Ok(()));
            // Windows paths are case insensitive; escaping and Unicode do not
            // make an otherwise matching action invalid.
            assert_eq!(
                validate_definition(
                    &definition,
                    true,
                    sid,
                    Path::new(&image.to_string_lossy().to_uppercase())
                ),
                Ok(())
            );
            unsafe {
                let principal = definition.Principal().unwrap();
                let actions = definition.Actions().unwrap();
                match field {
                    "enabled" => {}
                    "principal" => principal
                        .SetUserId(&BSTR::from("S-1-5-21-1-2-3-1002"))
                        .unwrap(),
                    "logon" => principal.SetLogonType(TASK_LOGON_PASSWORD).unwrap(),
                    "run level" => principal.SetRunLevel(TASK_RUNLEVEL_LUA).unwrap(),
                    "action count" => actions.Clear().unwrap(),
                    "action path" => actions
                        .get_Item(1)
                        .unwrap()
                        .cast::<IExecAction>()
                        .unwrap()
                        .SetPath(&BSTR::from(r"C:\Private\different.exe"))
                        .unwrap(),
                    "action arguments" => actions
                        .get_Item(1)
                        .unwrap()
                        .cast::<IExecAction>()
                        .unwrap()
                        .SetArguments(&BSTR::from("--private-argument=do-not-log"))
                        .unwrap(),
                    "action type" => {
                        actions.Clear().unwrap();
                        actions.Create(TASK_ACTION_COM_HANDLER).unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            let (stage, code) = if field == "action type" {
                // Preserve the native E_NOINTERFACE instead of relabeling a
                // non-executable action as a generic validation mismatch.
                (
                    "administrator task executable action query:".into(),
                    0x80004002,
                )
            } else {
                (format!("administrator task {field} validation:"), 13)
            };
            assert_eq!(
                validate_definition(&definition, field != "enabled", sid, image),
                Err(code)
            );
            let failures = diagnostics::captured_failures();
            let failure = failures
                .iter()
                .find(|entry| entry.message.contains(&stage))
                .unwrap_or_else(|| panic!("missing validation stage for {field}"));
            assert!(failure.message.contains(&format!("code={code} ")));
            for entry in failures
                .iter()
                .filter(|entry| entry.message.contains("administrator task "))
            {
                for private in [
                    sid,
                    "1002",
                    "Private",
                    "目录",
                    "different.exe",
                    "do-not-log",
                    "<Task",
                ] {
                    assert!(!entry.message.contains(private));
                }
            }
        }
    }

    #[test]
    fn removal_names_are_scoped_to_the_exact_application_and_a_user_sid() {
        let id = "dev.argus.taskmanager";
        let sid = "S-1-5-21-1-2-3-1001";
        assert_eq!(task_owner(&name(id, sid), id), Some(sid));
        for other in [
            name("dev.argus.taskmanager.test", sid),
            name(id, "S-1-5-18"),
            name(id, "S-1-5-32-544"),
            format!("{}-extra", name(id, sid)),
            format!("Other.{}", name(id, sid)),
        ] {
            assert_eq!(task_owner(&other, id), None);
        }
    }

    #[test]
    #[ignore = "creates and removes two isolated, non-elevated tasks without running them"]
    fn ordinary_user_revokes_tasks_without_runtime_directories() {
        assert!(!argus_core::privilege::is_elevated());
        let scheduler = Scheduler::new().unwrap();
        let sid = security::current_sid().unwrap();
        let id = format!("dev.argus.taskmanager.test.revoke-{}", guid().unwrap());
        let other = format!("{id}-other");
        struct Tasks<'a>(&'a Scheduler, String, String, String);
        impl Drop for Tasks<'_> {
            fn drop(&mut self) {
                let _ = self.0.remove(&self.1, &self.3);
                let _ = self.0.remove(&self.2, &self.3);
            }
        }
        let _cleanup = Tasks(&scheduler, id.clone(), other.clone(), sid.clone());
        let image = std::env::current_exe().unwrap();
        let xml = xml(&sid, &image).replace("HighestAvailable", "LeastPrivilege");
        for id in [&id, &other] {
            unsafe {
                scheduler.folder.RegisterTask(
                    &BSTR::from(name(id, &sid)),
                    &BSTR::from(xml.as_str()),
                    TASK_CREATE.0,
                    &VARIANT::from(sid.as_str()),
                    &VARIANT::default(),
                    TASK_LOGON_INTERACTIVE_TOKEN,
                    &VARIANT::default(),
                )
            }
            .unwrap();
        }
        assert!(scheduler.exists(&id, &sid).unwrap());
        scheduler.remove_all(&id).unwrap();
        scheduler.remove_all(&id).unwrap();
        assert!(!scheduler.exists(&id, &sid).unwrap());
        assert!(scheduler.exists(&other, &sid).unwrap());
    }

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
