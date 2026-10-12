use super::{launch, state::Result};
use crate::{
    security,
    util::{Handle, wide},
};
use argus_core::tr;
use std::sync::Mutex;
use std::time::Duration;
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::System::Threading::*;
use windows::core::{PCWSTR, PWSTR};

pub const ARG: &str = "--argus-taskmgr-entry";
static PENDING: Mutex<Vec<Request>> = Mutex::new(Vec::new());

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
    let expected = crate::install::directory()
        .map_err(crate::util::message)?
        .join(super::LAUNCHER);
    complete_with(request, &expected)
}

fn complete_with(request: Request, expected: &std::path::Path) -> Result<Completion> {
    let parent = Handle(
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                request.pid,
            )
        }
        .map_err(|e| e.to_string())?,
    );
    if launch::identity(parent.0)? != request.created {
        return Err("expired request".into());
    }
    let mut image = [0u16; 32768];
    let mut len = image.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            parent.0,
            PROCESS_NAME_WIN32,
            PWSTR(image.as_mut_ptr()),
            &mut len,
        )
    }
    .map_err(|e| e.to_string())?;
    if !String::from_utf16_lossy(&image[..len as usize])
        .eq_ignore_ascii_case(&expected.to_string_lossy())
    {
        return Err("unexpected launcher".into());
    }
    let open = |suffix| -> Result<Handle> {
        Ok(Handle(
            unsafe {
                OpenEventW(
                    EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                    false,
                    PCWSTR(event_name(&request.id, suffix).as_ptr()),
                )
            }
            .map_err(|e| e.to_string())?,
        ))
    };
    let ready = open("ready")?;
    let commit = open("commit")?;
    let done = open("done")?;
    unsafe { SetEvent(ready.0) }.map_err(|e| e.to_string())?;
    if !launch::wait(commit.0, parent.0, Duration::from_secs(2)) {
        return Err("expired request".into());
    }
    Ok(Completion {
        show: !request.probe,
        parent,
        done,
    })
}

pub fn activate(image: &std::path::Path, owner: &str, probe: bool) -> Result<()> {
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
    let ready = event("ready")?;
    let commit = event("commit")?;
    let done = event("done")?;
    let parent = unsafe { GetCurrentProcess() };
    let _child = launch::standard_spawn(
        image,
        &[
            ARG.into(),
            id,
            std::process::id().to_string(),
            launch::identity(parent)?.to_string(),
            if probe { "probe" } else { "show" }.into(),
        ],
    )?;
    if unsafe { WaitForSingleObject(ready.0, 10000) } != WAIT_OBJECT_0 {
        return Err(tr!("taskManager.uiTimeout"));
    }
    unsafe { SetEvent(commit.0) }.map_err(|e| e.to_string())?;
    if unsafe { WaitForSingleObject(done.0, 2000) } != WAIT_OBJECT_0 {
        return Err(tr!("taskManager.uiNoResponse"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let completion = complete_with(request, &std::env::current_exe().unwrap()).unwrap();
        assert!(!completion.should_show());
        completion.acknowledge();
    }

    #[test]
    fn readiness_handshake_requires_commit() {
        use std::os::windows::process::CommandExt;
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
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
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
