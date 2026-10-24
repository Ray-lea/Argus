use super::*;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
pub(super) const STANDARD: &str = "--argus-admin-standard";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    nonce: String,
    pid: u32,
    created: u64,
    hidden: bool,
    updated: bool,
    task_manager: Option<Handshake>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Handshake {
    nonce: String,
    pid: u32,
    created: u64,
    probe: bool,
}
pub(super) fn valid_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}
impl Request {
    fn new(args: &[String]) -> Result<Self> {
        let task_manager =
            if let Some(at) = args.iter().position(|a| a == crate::taskmgr::ready::ARG) {
                match &args[at + 1..] {
                    [nonce, pid, created, mode]
                        if valid_guid(nonce) && matches!(mode.as_str(), "probe" | "show") =>
                    {
                        Some(Handshake {
                            nonce: nonce.clone(),
                            pid: pid.parse().map_err(|_| 87u32)?,
                            created: created.parse().map_err(|_| 87u32)?,
                            probe: mode == "probe",
                        })
                    }
                    _ => return Err(87),
                }
            } else {
                None
            };
        Ok(Self {
            nonce: guid()?,
            pid: std::process::id(),
            created: identity(unsafe { GetCurrentProcess() })?,
            hidden: args.iter().any(|a| a == "--hidden"),
            updated: args.iter().any(|a| a == "--updated"),
            task_manager,
        })
    }
    fn encode(&self) -> Result<String> {
        Ok(serde_json::to_vec(self)
            .map_err(|_| 13u32)?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
    fn decode(token: &str) -> Result<Self> {
        if token.len() > 2048
            || !token.len().is_multiple_of(2)
            || !token.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(87);
        }
        let bytes = (0..token.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&token[i..i + 2], 16).map_err(|_| 87u32))
            .collect::<Result<Vec<_>>>()?;
        let request: Self = serde_json::from_slice(&bytes).map_err(|_| 87u32)?;
        if !valid_guid(&request.nonce)
            || request.pid == 0
            || request.created == 0
            || request
                .task_manager
                .as_ref()
                .is_some_and(|h| !valid_guid(&h.nonce) || h.pid == 0 || h.created == 0)
        {
            return Err(87);
        }
        Ok(request)
    }
    fn args(&self) -> Vec<String> {
        let mut args = vec![STARTED.into(), self.nonce.clone()];
        if self.hidden {
            args.push("--hidden".into());
        }
        if self.updated {
            args.push("--updated".into());
        }
        if let Some(h) = &self.task_manager {
            args.extend([
                crate::taskmgr::ready::ARG.into(),
                h.nonce.clone(),
                h.pid.to_string(),
                h.created.to_string(),
                if h.probe { "probe" } else { "show" }.into(),
            ]);
        }
        args
    }
}
fn name(nonce: &str, suffix: &str) -> Vec<u16> {
    wide(&format!("Local\\Argus.Admin.{nonce}.{suffix}"))
}
fn event(nonce: &str, suffix: &str, create: bool) -> Result<Handle> {
    let name = name(nonce, suffix);
    if create {
        let sd = security::pipe_descriptor(&security::current_sid()?)?;
        let raw =
            unsafe { CreateEventW(Some(&sd.attributes()), true, false, PCWSTR(name.as_ptr())) }
                .map_err(win)?;
        let handle = Handle(raw);
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Err(183);
        }
        Ok(handle)
    } else {
        unsafe {
            OpenEventW(
                EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                false,
                PCWSTR(name.as_ptr()),
            )
        }
        .map(Handle)
        .map_err(win)
    }
}

/// A bootstrap is ready, but no second WebView/profile/history writer exists.
pub struct Prepared {
    commit: Handle,
    _ready: Handle,
    _done: Handle,
}
impl Prepared {
    pub fn commit(self) -> std::result::Result<(), String> {
        unsafe { SetEvent(self.commit.0) }.map_err(|e| util::message(win(e)))
    }
}
pub fn prepare(id: &str, args: &[String], elevated: bool) -> std::result::Result<Prepared, String> {
    util::stage(
        "administrator startup prepare",
        prepare_inner(id, args, elevated),
    )
    .map_err(util::message)
}
fn prepare_inner(id: &str, args: &[String], elevated: bool) -> Result<Prepared> {
    let sid = security::current_sid()?;
    let request = Request::new(args)?;
    let ready = event(&request.nonce, "ready", true)?;
    let commit = event(&request.nonce, "commit", true)?;
    let done = event(&request.nonce, "done", true)?;
    let token = request.encode()?;
    let _child = if elevated {
        let (dir, record) = grant(id, &sid)?.ok_or(2u32)?;
        if outdated(&dir, &record) {
            return Err(util::ADMIN_OUTDATED);
        }
        task::run(id, &sid, &dir.join(record.image), &token)?;
        None
    } else {
        security::allow_process_query(&sid)?;
        let dir = directory(id, &sid)?;
        let image = read_launch(&dir)
            .map(|r| dir.join(r.image))
            .unwrap_or(std::env::current_exe().map_err(io)?);
        Some(
            crate::taskmgr::launch::standard_spawn(&image, &[STANDARD.into(), token])
                .map_err(|error| launch_failure("administrator standard bootstrap", error))?,
        )
    };
    if unsafe { WaitForSingleObject(ready.0, 15000) } != WAIT_OBJECT_0 {
        return Err(1460);
    }
    Ok(Prepared {
        commit,
        _ready: ready,
        _done: done,
    })
}

fn caller(request: &Request) -> Result<Handle> {
    let process = Handle(
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                request.pid,
            )
        }
        .map_err(win)?,
    );
    if identity(process.0)? != request.created
        || session(request.pid)? != session(std::process::id())?
    {
        return Err(5);
    }
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) }.map_err(win)?;
    let token = Handle(token);
    if security::token_user(token.0)? != security::current_sid()? {
        return Err(5);
    }
    if security::sandboxed(token.0)? {
        return Err(5);
    }
    let mut buffer = [0u64; 64];
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            Some(buffer.as_mut_ptr().cast()),
            size_of_val(&buffer) as u32,
            &mut size,
        )
    }
    .map_err(win)?;
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
    let count = unsafe { *GetSidSubAuthorityCount(sid) };
    if count == 0 || unsafe { *GetSidSubAuthority(sid, u32::from(count) - 1) } < 0x2000 {
        return Err(5);
    }
    Ok(process)
}
pub(super) fn broker(id: &str, token: &str) -> Result<()> {
    broker_as(id, token, true)
}
pub(super) fn standard_mode(id: &str, args: &[String]) -> Option<Result<()>> {
    (args.first().is_some_and(|a| a == STANDARD)).then(|| match args {
        [_, token] => broker_as(id, token, false),
        _ => Err(87),
    })
}
fn broker_as(id: &str, token: &str, elevated: bool) -> Result<()> {
    let request = Request::decode(token)?;
    if argus_core::privilege::is_elevated() != elevated {
        return Err(5);
    }
    let current = std::env::current_exe().map_err(io)?;
    if elevated {
        let sid = security::current_sid()?;
        let (dir, record) = grant(id, &sid)?.ok_or(2u32)?;
        if current != dir.join(record.image) {
            return Err(5);
        }
        let _pin = security::protect_directory(&current)?;
    }
    let caller = caller(&request)?;
    let ready = event(&request.nonce, "ready", false)?;
    let commit = event(&request.nonce, "commit", false)?;
    let done = event(&request.nonce, "done", false)?;
    unsafe { SetEvent(ready.0) }.map_err(win)?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        // A fast caller can exit immediately after committing. Observe the
        // commit before checking its lifetime, so that valid request survives.
        if unsafe { WaitForSingleObject(commit.0, 25) } == WAIT_OBJECT_0 {
            break;
        }
        if unsafe { WaitForSingleObject(caller.0, 0) } == WAIT_OBJECT_0
            || Instant::now() >= deadline
        {
            return Err(1460);
        }
    }
    // Commit means the caller will now save/quit. Wait for actual process exit,
    // not merely its window disappearing or a successful IPC response.
    if unsafe { WaitForSingleObject(caller.0, 20000) } != WAIT_OBJECT_0 {
        return Err(1460);
    }
    let mut args = request.args();
    if !elevated {
        args.insert(0, STANDARD_ONCE.into());
    }
    let child = crate::taskmgr::launch::spawn(&current, &args, false)
        .map_err(|error| launch_failure("administrator interface spawn", error))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if unsafe { WaitForSingleObject(done.0, 50) } == WAIT_OBJECT_0 {
            return Ok(());
        }
        if unsafe { WaitForSingleObject(child.0, 0) } == WAIT_OBJECT_0 {
            break;
        }
    }
    // Do not leave a hung second writer behind before recovering a standard UI.
    if unsafe { WaitForSingleObject(child.0, 0) } != WAIT_OBJECT_0 {
        unsafe { TerminateProcess(child.0, 1460) }.map_err(win)?;
        if unsafe { WaitForSingleObject(child.0, 10000) } != WAIT_OBJECT_0 {
            return Err(1460);
        }
    }
    if elevated {
        args.insert(0, FALLBACK.into());
        crate::taskmgr::launch::standard_spawn(&current, &args)
            .map_err(|error| launch_failure("administrator fallback spawn", error))?;
    }
    Err(1460)
}

pub fn acknowledge(args: &[String]) {
    if let Some(at) = args.iter().position(|a| a == STARTED)
        && let Some(nonce) = args.get(at + 1).filter(|s| valid_guid(s))
        && let Ok(done) = event(nonce, "done", false)
    {
        let _ = unsafe { SetEvent(done.0) };
    }
}
pub fn handoff_args() -> Vec<String> {
    identity(unsafe { GetCurrentProcess() })
        .map(|created| {
            vec![
                HANDOFF.into(),
                std::process::id().to_string(),
                created.to_string(),
            ]
        })
        .unwrap_or_default()
}
pub fn trusted_handoff(id: &str, args: &[String]) -> bool {
    let Some(at) = args.iter().position(|a| a == HANDOFF) else {
        return false;
    };
    let [pid, created] = &args[at + 1..] else {
        return false;
    };
    let (Ok(pid), Ok(created)) = (pid.parse::<u32>(), created.parse::<u64>()) else {
        return false;
    };
    (|| -> Result<()> {
        let request = Request {
            nonce: String::new(),
            pid,
            created,
            hidden: false,
            updated: false,
            task_manager: None,
        };
        let process = caller(&request)?;
        let mut image = [0u16; 32768];
        let mut size = image.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                process.0,
                PROCESS_NAME_WIN32,
                PWSTR(image.as_mut_ptr()),
                &mut size,
            )
        }
        .map_err(win)?;
        let path = PathBuf::from(String::from_utf16_lossy(&image[..size as usize]));
        if !is_copy(&path, id) {
            return Err(5);
        }
        let _pin = security::protect_directory(&path)?;
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) }.map_err(win)?;
        let token = Handle(token);
        let mut elevation = TOKEN_ELEVATION::default();
        unsafe {
            GetTokenInformation(
                token.0,
                TokenElevation,
                Some((&raw mut elevation).cast()),
                size_of_val(&elevation) as u32,
                &mut size,
            )
        }
        .map_err(win)?;
        if elevation.TokenIsElevated == 0 {
            return Err(5);
        }
        Ok(())
    })()
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ticket_round_trips_only_fixed_intents() {
        let args = vec![
            "argus.exe".into(),
            "--hidden".into(),
            "--updated".into(),
            crate::taskmgr::ready::ARG.into(),
            guid().unwrap(),
            "123".into(),
            "456".into(),
            "probe".into(),
        ];
        let request = Request::new(&args).unwrap();
        let encoded = request.encode().unwrap();
        let decoded = Request::decode(&encoded).unwrap();
        assert!(decoded.hidden && decoded.updated);
        assert!(decoded.task_manager.as_ref().unwrap().probe);
        assert_eq!(decoded.args()[2..], args[1..]);
        for invalid in [
            "",
            "a",
            "--argus-service",
            "00 00",
            "$(Arg0)",
            &"a".repeat(2050),
        ] {
            assert!(Request::decode(invalid).is_err());
        }
    }
    #[test]
    fn guid_and_handshake_validation_rejects_arguments_and_paths() {
        for invalid in [
            "../a",
            "00000000-0000-0000-0000-000000000000 --hidden",
            "------------------------------------",
            "000000000000000000000000000000000000",
        ] {
            assert!(!valid_guid(invalid));
        }
        assert!(Request::new(&[crate::taskmgr::ready::ARG.into(), "bad".into()]).is_err());
    }
}
