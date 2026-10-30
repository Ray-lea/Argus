use super::{launch, state::Result};
use crate::{
    security,
    util::{Handle, wide},
};
use argus_core::tr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HANDLE, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::System::Threading::*;
use windows::core::{PCWSTR, PWSTR};

pub const ARG: &str = "--argus-taskmgr-entry";
static PENDING: Mutex<Vec<Request>> = Mutex::new(Vec::new());

/// Early startup has no application logger yet. Keep failures beside the launcher's
/// diagnostic log, with no executable path, user name or activation payload.
pub fn handover_failed(
    operation: &'static str,
    hresult: i32,
    sender: Option<u32>,
    receiver: Option<u32>,
    waited_ms: u64,
) {
    let level = |rid: Option<u32>| {
        rid.map(|n| format!("{n:#x}"))
            .unwrap_or_else(|| "unknown".into())
    };
    super::log_fixed(
        "single-instance handover failed",
        &format!(
            "api={operation}; hresult={:#010x}; sender_integrity={}; receiver_integrity={}; waited_ms={waited_ms}",
            hresult as u32,
            level(sender),
            level(receiver)
        ),
    );
}

fn checked<T>(operation: &'static str, result: Result<T>) -> Result<T> {
    result.inspect_err(|error| super::log_line(operation, error))
}

#[derive(Clone)]
struct Request {
    id: String,
    pid: u32,
    created: u64,
    probe: bool,
}
fn event_name(id: &str, suffix: &str) -> Vec<u16> {
    wide(&format!("Local\\Argus.Taskmgr.{id}.{suffix}"))
}
fn valid_id(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

pub fn queue(args: &[String]) -> bool {
    let Some(at) = args.iter().position(|s| s == ARG) else {
        return false;
    };
    if let [id, pid, created, mode] = &args[at + 1..]
        && valid_id(id)
        && let (Ok(pid), Ok(created)) = (pid.parse::<u32>(), created.parse::<u64>())
        && pid != 0
        && created != 0
        && matches!(mode.as_str(), "show" | "probe")
    {
        let mut pending = PENDING.lock().unwrap();
        if pending.len() < 8 {
            pending.push(Request {
                id: id.clone(),
                pid,
                created,
                probe: mode == "probe",
            });
        }
    }
    true
}

pub struct Completion {
    show: bool,
    parent: Handle,
    done: Handle,
}
impl Completion {
    pub fn should_show(&self) -> bool {
        self.show && unsafe { WaitForSingleObject(self.parent.0, 0) } != WAIT_OBJECT_0
    }
    pub fn acknowledge(self) {
        unsafe {
            let _ = SetEvent(self.done.0);
        }
    }
}

/// Called by the frontend only after it is mounted, and on each explicit
/// activation request. An expired parent cannot make a late UI steal focus.
pub fn complete() -> Vec<Completion> {
    let requests = std::mem::take(&mut *PENDING.lock().unwrap());
    requests
        .into_iter()
        .filter_map(|r| complete_one(r).ok())
        .collect()
}

fn complete_one(request: Request) -> Result<Completion> {
    let expected = checked(
        "interface handshake: launcher directory",
        crate::install::directory().map_err(crate::util::message),
    )?
    .join(super::LAUNCHER);
    complete_with(request, &expected)
}

fn complete_with(request: Request, expected: &std::path::Path) -> Result<Completion> {
    let parent = Handle(checked(
        "interface handshake: OpenProcess",
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                request.pid,
            )
        }
        .map_err(|e| e.to_string()),
    )?);
    if checked(
        "interface handshake: GetProcessTimes",
        launch::identity(parent.0),
    )? != request.created
    {
        super::log_fixed(
            "interface handshake rejected",
            "reason=parent identity mismatch",
        );
        return Err("expired request".into());
    }
    let mut image = [0u16; 32768];
    let mut len = image.len() as u32;
    checked(
        "interface handshake: QueryFullProcessImageNameW",
        unsafe {
            QueryFullProcessImageNameW(
                parent.0,
                PROCESS_NAME_WIN32,
                PWSTR(image.as_mut_ptr()),
                &mut len,
            )
        }
        .map_err(|e| e.to_string()),
    )?;
    if !String::from_utf16_lossy(&image[..len as usize])
        .eq_ignore_ascii_case(&expected.to_string_lossy())
    {
        super::log_fixed(
            "interface handshake rejected",
            "reason=unexpected launcher image",
        );
        return Err("unexpected launcher".into());
    }
    let open = |suffix, operation| -> Result<Handle> {
        Ok(Handle(checked(
            operation,
            unsafe {
                OpenEventW(
                    EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                    false,
                    PCWSTR(event_name(&request.id, suffix).as_ptr()),
                )
            }
            .map_err(|e| e.to_string()),
        )?))
    };
    let ready = open("ready", "interface handshake: OpenEventW ready")?;
    let commit = open("commit", "interface handshake: OpenEventW commit")?;
    let done = open("done", "interface handshake: OpenEventW done")?;
    checked(
        "interface handshake: SetEvent ready",
        unsafe { SetEvent(ready.0) }.map_err(|e| e.to_string()),
    )?;
    if !launch::wait(commit.0, parent.0, Duration::from_secs(2)) {
        super::log_fixed(
            "interface handshake rejected",
            &format!(
                "reason=commit not received; parent_exited={}",
                unsafe { WaitForSingleObject(parent.0, 0) } == WAIT_OBJECT_0
            ),
        );
        return Err("expired request".into());
    }
    Ok(Completion {
        show: !request.probe,
        parent,
        done,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum ReadyWait {
    Ready,
    TimedOut,
    ChildExited(u32),
}

fn wait_for_ready(
    ready: HANDLE,
    child: HANDLE,
    timeout: Duration,
) -> windows::core::Result<ReadyWait> {
    let deadline = Instant::now() + timeout;
    loop {
        match unsafe { WaitForSingleObject(ready, 25) } {
            WAIT_OBJECT_0 => return Ok(ReadyWait::Ready),
            WAIT_FAILED => return Err(windows::core::Error::from_thread()),
            _ => {}
        }
        if unsafe { WaitForSingleObject(child, 0) } == WAIT_OBJECT_0 {
            let mut code = 0;
            unsafe { GetExitCodeProcess(child, &mut code) }?;
            // Zero is a successful handover, not proof that the UI is ready.
            if code != 0 {
                return Ok(ReadyWait::ChildExited(code));
            }
        }
        if Instant::now() >= deadline {
            return Ok(ReadyWait::TimedOut);
        }
    }
}

pub fn activate(image: &std::path::Path, owner: &str, probe: bool) -> Result<()> {
    let started = Instant::now();
    let mut phase = "verify account";
    let mut child = None;
    let result = (|| {
        if security::current_sid().map_err(crate::util::message)? != owner {
            return Err(tr!("taskManager.wrongAccount"));
        }
        let id = super::id()?;
        let sd = security::pipe_descriptor(owner).map_err(crate::util::message)?;
        let event = |suffix| -> Result<Handle> {
            Ok(Handle(
                unsafe {
                    CreateEventW(
                        Some(&sd.attributes()),
                        true,
                        false,
                        PCWSTR(event_name(&id, suffix).as_ptr()),
                    )
                }
                .map_err(|e| e.to_string())?,
            ))
        };
        phase = "CreateEventW ready";
        let ready = event("ready")?;
        phase = "CreateEventW commit";
        let commit = event("commit")?;
        phase = "CreateEventW done";
        let done = event("done")?;
        let parent = unsafe { GetCurrentProcess() };
        phase = "create interface process";
        child = Some(launch::standard_spawn(
            image,
            &[
                ARG.into(),
                id,
                std::process::id().to_string(),
                launch::identity(parent)?.to_string(),
                if probe { "probe" } else { "show" }.into(),
            ],
        )?);
        phase = "wait for interface ready";
        match wait_for_ready(ready.0, child.as_ref().unwrap().0, Duration::from_secs(10))
            .map_err(|e| e.to_string())?
        {
            ReadyWait::Ready => {}
            ReadyWait::TimedOut => return Err(tr!("taskManager.uiTimeout")),
            ReadyWait::ChildExited(code) => {
                return Err(tr!("taskManager.uiExited", code = format!("{code:#010x}")));
            }
        }
        phase = "SetEvent commit";
        unsafe { SetEvent(commit.0) }.map_err(|e| e.to_string())?;
        phase = "wait for interface completion";
        if unsafe { WaitForSingleObject(done.0, 2000) } != WAIT_OBJECT_0 {
            return Err(tr!("taskManager.uiNoResponse"));
        }
        Ok(())
    })();
    if let Err(error) = &result {
        let child_state = child
            .as_ref()
            .map(|child| {
                let mut code = 0;
                match unsafe { GetExitCodeProcess(child.0, &mut code) } {
                    Ok(()) => format!(
                        "running={}, exit_code={code:#010x}",
                        unsafe { WaitForSingleObject(child.0, 0) } != WAIT_OBJECT_0
                    ),
                    Err(error) => format!("query failed {:#010x}", error.code().0 as u32),
                }
            })
            .unwrap_or_else(|| "not created".into());
        super::log_fixed(
            "interface activation failed",
            &format!(
                "stage={phase}; reason={}; probe={probe}; launcher_elevated={}; child=({child_state}); elapsed_ms={}",
                argus_core::diagnostics::failure(error),
                argus_core::privilege::is_elevated(),
                started.elapsed().as_millis()
            ),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;

    #[test]
    #[ignore = "subprocess fixture for interface_exit_is_distinct_from_readiness"]
    fn exit_fixture() {
        std::process::exit(
            std::env::var("ARGUS_READY_EXIT_CODE")
                .unwrap()
                .parse()
                .unwrap(),
        );
    }

    #[test]
    fn interface_exit_is_distinct_from_readiness() {
        for (code, signal, expected) in [
            (0, true, ReadyWait::Ready),
            (0, false, ReadyWait::TimedOut),
            (5, false, ReadyWait::ChildExited(5)),
        ] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "taskmgr::ready::tests::exit_fixture",
                    "--ignored",
                ])
                .env("ARGUS_READY_EXIT_CODE", code.to_string())
                .creation_flags(0x08000000)
                .spawn()
                .unwrap();
            child.wait().unwrap();
            let event = Handle(unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.unwrap());
            let raw = event.0.0 as usize;
            let worker = signal.then(|| {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(50));
                    unsafe { SetEvent(HANDLE(raw as *mut _)) }.unwrap();
                })
            });
            let result = wait_for_ready(
                event.0,
                HANDLE(child.as_raw_handle()),
                Duration::from_millis(150),
            );
            if let Some(worker) = worker {
                worker.join().unwrap();
            }
            assert_eq!(result.unwrap(), expected);
        }
    }
    #[test]
    fn invalid_and_expired_requests_cannot_activate_a_window() {
        assert!(queue(&[ARG.into(), "bad".into()]));
        assert!(!queue(&["--hidden".into()]));
        assert!(complete().is_empty());
        assert!(!valid_id("../escaped"));
        let request = Request {
            id: super::super::id().unwrap(),
            pid: std::process::id(),
            created: 1,
            probe: true,
        };
        assert!(complete_with(request, &std::env::current_exe().unwrap()).is_err());
    }

    #[test]
    #[ignore = "child fixture used by readiness_handshake_requires_commit"]
    fn ready_fixture() {
        let value = std::env::var("ARGUS_READY_FIXTURE").unwrap();
        let parts: Vec<_> = value.split('|').collect();
        let request = Request {
            id: parts[0].into(),
            pid: parts[1].parse().unwrap(),
            created: parts[2].parse().unwrap(),
            probe: true,
        };
        let expected =
            std::path::PathBuf::from(std::env::var_os("ARGUS_READY_FIXTURE_LAUNCHER").unwrap());
        let completion = complete_with(request, &expected).unwrap();
        assert!(!completion.should_show());
        completion.acknowledge();
    }

    #[test]
    fn readiness_handshake_requires_commit() {
        let id = super::super::id().unwrap();
        let sd = security::pipe_descriptor(&security::current_sid().unwrap()).unwrap();
        let create = |suffix| {
            Handle(
                unsafe {
                    CreateEventW(
                        Some(&sd.attributes()),
                        true,
                        false,
                        PCWSTR(event_name(&id, suffix).as_ptr()),
                    )
                }
                .unwrap(),
            )
        };
        let ready = create("ready");
        let commit = create("commit");
        let done = create("done");
        // A Cargo target directory may be a junction. Use the parent's reported
        // image name, not the child's launch spelling of the same executable.
        let mut image = [0u16; 32768];
        let mut image_len = image.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                GetCurrentProcess(),
                PROCESS_NAME_WIN32,
                PWSTR(image.as_mut_ptr()),
                &mut image_len,
            )
            .unwrap();
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .env(
                "ARGUS_READY_FIXTURE_LAUNCHER",
                String::from_utf16_lossy(&image[..image_len as usize]),
            )
            .args([
                "--exact",
                "taskmgr::ready::tests::ready_fixture",
                "--ignored",
            ])
            .env(
                "ARGUS_READY_FIXTURE",
                format!(
                    "{id}|{}|{}",
                    std::process::id(),
                    launch::identity(unsafe { GetCurrentProcess() }).unwrap()
                ),
            )
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let arrived = unsafe { WaitForSingleObject(ready.0, 5000) };
        let premature = unsafe { WaitForSingleObject(done.0, 0) };
        unsafe {
            SetEvent(commit.0).unwrap();
        }
        let completed = unsafe { WaitForSingleObject(done.0, 3000) };
        if completed != WAIT_OBJECT_0 {
            let _ = child.kill();
        }
        let exit = child.wait().unwrap();
        assert_eq!(arrived, WAIT_OBJECT_0);
        assert_ne!(premature, WAIT_OBJECT_0);
        assert_eq!(completed, WAIT_OBJECT_0);
        assert!(exit.success());
    }
}
