use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use windows::Win32::Foundation::{
    ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_READ_DATA,
    FILE_WRITE_DATA, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows::Win32::System::Threading::{
    CreateEventW, OpenProcess, PROCESS_SYNCHRONIZE, WaitForMultipleObjects, WaitForSingleObject,
};
use windows::core::PCWSTR;

use crate::protocol::{Context, MAX_FRAME, MAX_REPLY, Operation, Reply, Request, execute};
use crate::security;
use crate::util::{Handle, INCOMPATIBLE, Result, wide, win};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const CLIENTS: usize = 8;

trait Frame: Serialize + DeserializeOwned {
    const LIMIT: usize;
}
impl Frame for Request {
    const LIMIT: usize = MAX_FRAME;
}
impl Frame for Reply {
    const LIMIT: usize = MAX_REPLY;
}

pub fn event() -> Result<Handle> {
    unsafe { CreateEventW(None, true, false, None) }
        .map(Handle)
        .map_err(win)
}

fn wait_io(
    pipe: HANDLE,
    ov: &mut OVERLAPPED,
    result: windows::core::Result<()>,
    stop: Option<HANDLE>,
    deadline: Instant,
) -> Result<u32> {
    if let Err(e) = result {
        if e.code() != ERROR_IO_PENDING.to_hresult() {
            return Err(win(e));
        }
        let mut events = vec![ov.hEvent];
        if let Some(stop) = stop {
            events.insert(0, stop);
        }
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u32::MAX as u128 - 1) as u32;
        let r = unsafe { WaitForMultipleObjects(&events, false, timeout) };
        if r.0 != WAIT_OBJECT_0.0 + events.len() as u32 - 1 {
            // Cancel and drain before dropping OVERLAPPED or its buffer.
            unsafe {
                let _ = CancelIoEx(pipe, Some(ov));
                let mut transferred = 0;
                let _ = GetOverlappedResult(pipe, ov, &mut transferred, true);
            }
            return Err(if stop.is_some() && r == WAIT_OBJECT_0 {
                995
            } else {
                1460
            });
        }
    }
    let mut transferred = 0;
    unsafe { GetOverlappedResult(pipe, ov, &mut transferred, false) }.map_err(win)?;
    Ok(transferred)
}

fn transfer(
    pipe: HANDLE,
    data: &mut [u8],
    writing: bool,
    stop: Option<HANDLE>,
    deadline: Instant,
) -> Result<()> {
    let mut offset = 0;
    while offset < data.len() {
        if Instant::now() >= deadline {
            return Err(1460);
        }
        let done = event()?;
        let mut ov = OVERLAPPED {
            hEvent: done.0,
            ..Default::default()
        };
        let result = unsafe {
            if writing {
                WriteFile(pipe, Some(&data[offset..]), None, Some(&mut ov))
            } else {
                ReadFile(pipe, Some(&mut data[offset..]), None, Some(&mut ov))
            }
        };
        let n = wait_io(pipe, &mut ov, result, stop, deadline)? as usize;
        if n == 0 {
            return Err(109);
        }
        offset += n;
    }
    Ok(())
}

fn receive<T: Frame>(pipe: HANDLE, stop: Option<HANDLE>, deadline: Instant) -> Result<T> {
    let mut header = [0; 4];
    transfer(pipe, &mut header, false, stop, deadline)?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > T::LIMIT {
        return Err(87);
    }
    let mut data = vec![0; len];
    transfer(pipe, &mut data, false, stop, deadline)?;
    serde_json::from_slice(&data).map_err(|_| 87)
}

fn send<T: Frame>(pipe: HANDLE, value: &T, stop: Option<HANDLE>, deadline: Instant) -> Result<()> {
    let data = serde_json::to_vec(value).map_err(|_| 87u32)?;
    if data.is_empty() || data.len() > T::LIMIT {
        return Err(87);
    }
    let mut frame = (data.len() as u32).to_le_bytes().to_vec();
    frame.extend(data);
    transfer(pipe, &mut frame, true, stop, deadline)
}

pub fn request(name: &str, expected_pid: u32, operation: Operation) -> Result<Reply> {
    request_with_timeout(name, expected_pid, operation, REQUEST_TIMEOUT)
}

pub(crate) fn request_with_timeout(
    name: &str,
    expected_pid: u32,
    operation: Operation,
    timeout: Duration,
) -> Result<Reply> {
    // Keep the SCM-reported process alive as an object while authenticating the
    // pipe endpoint, so a recycled numeric PID cannot authenticate a fake server.
    let process =
        Handle(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, expected_pid) }.map_err(win)?);
    let pipe = connect(name)?;
    let mut actual_pid = 0;
    unsafe { GetNamedPipeServerProcessId(pipe.0, &mut actual_pid) }.map_err(win)?;
    if actual_pid != expected_pid || unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 {
        return Err(5);
    }
    let deadline = Instant::now() + timeout;
    // Keep the version-one discovery envelope so an older service can report
    // its protocol version and the settings page can ask for an update.
    let protocol = if matches!(operation, Operation::Ping) {
        1
    } else {
        crate::PROTOCOL
    };
    send(
        pipe.0,
        &Request {
            protocol,
            operation,
        },
        None,
        deadline,
    )?;
    let reply = receive(pipe.0, None, deadline)?;
    if let Reply::Pong { protocol, .. } = &reply
        && *protocol != crate::PROTOCOL
    {
        return Err(INCOMPATIBLE);
    }
    Ok(reply)
}

fn connect(name: &str) -> Result<Handle> {
    let name = wide(name);
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let result = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                FILE_READ_DATA.0 | FILE_WRITE_DATA.0 | 0x100080,
                Default::default(),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                None,
            )
        };
        match result {
            Ok(h) => return Ok(Handle(h)),
            Err(e) if e.code() == windows::Win32::Foundation::ERROR_PIPE_BUSY.to_hresult() => {
                let remaining = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis() as u32;
                if remaining == 0 {
                    return Err(1460);
                }
                // Nothing has been sent yet, so waiting for a listener cannot
                // repeat an operation. Never reconnect after sending a request.
                unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), remaining) }
                    .ok()
                    .map_err(win)?;
            }
            Err(e) => return Err(win(e)),
        }
    }
}

fn create(name: &str, sd: &security::Descriptor, first: bool) -> Result<Handle> {
    let mut mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(wide(name).as_ptr()),
            mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            CLIENTS as u32 + 1,
            MAX_FRAME as u32 + 4,
            MAX_FRAME as u32 + 4,
            1000,
            Some(&sd.attributes()),
        )
    };
    if handle.is_invalid() {
        return Err(unsafe { GetLastError() }.0);
    }
    Ok(Handle(handle))
}

pub fn serve(name: &str, owner: String, stop: Arc<Handle>, ready: impl FnOnce()) -> Result<()> {
    let descriptor = security::pipe_descriptor(&owner)?;
    serve_with_descriptor(
        name,
        owner,
        stop,
        &descriptor,
        Arc::new(Context::new()),
        ready,
    )
}

fn serve_with_descriptor(
    name: &str,
    owner: String,
    stop: Arc<Handle>,
    descriptor: &security::Descriptor,
    context: Arc<Context>,
    ready: impl FnOnce(),
) -> Result<()> {
    let mut listener = create(name, descriptor, true)?;
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    ready();
    let result = (|| loop {
        if unsafe { WaitForSingleObject(stop.0, 0) } == WAIT_OBJECT_0 {
            return Ok(());
        }
        let connected = event()?;
        let mut ov = OVERLAPPED {
            hEvent: connected.0,
            ..Default::default()
        };
        let r = unsafe { ConnectNamedPipe(listener.0, Some(&mut ov)) };
        if !r
            .as_ref()
            .is_err_and(|e| e.code() == ERROR_PIPE_CONNECTED.to_hresult())
        {
            match wait_io(
                listener.0,
                &mut ov,
                r,
                Some(stop.0),
                Instant::now() + Duration::from_secs(86400),
            ) {
                Err(995) => return Ok(()),
                Err(1460) => continue,
                Err(e) => return Err(e),
                Ok(_) => {}
            }
        }
        for i in (0..workers.len()).rev() {
            if workers[i].is_finished() {
                let _ = workers.swap_remove(i).join();
            }
        }
        if workers.len() >= CLIENTS {
            unsafe { DisconnectNamedPipe(listener.0) }.map_err(win)?;
            continue;
        }
        // Create the next listener before handing off the connected instance;
        // there is never a gap in which another process can claim the pipe name.
        let next = create(name, descriptor, false)?;
        let client = std::mem::replace(&mut listener, next);
        let owner = owner.clone();
        let stop = stop.clone();
        let context = context.clone();
        workers.push(std::thread::spawn(move || {
            let client = client;
            let _ = handle_client(client.0, &owner, stop.0, &context);
        }));
    })();
    drop(listener);
    for worker in workers {
        let _ = worker.join();
    }
    result
}

fn handle_client(pipe: HANDLE, owner: &str, stop: HANDLE, context: &Context) -> Result<()> {
    let request: Request = receive(pipe, Some(stop), Instant::now() + READ_TIMEOUT)?;
    let pid = security::authenticate(pipe, owner)?;
    if request.protocol != crate::PROTOCOL && !matches!(request.operation, Operation::Ping) {
        return Err(INCOMPATIBLE);
    }
    if unsafe { WaitForSingleObject(stop, 0) } == WAIT_OBJECT_0 {
        return Err(995);
    }
    let reply = execute(request.operation, context, pid);
    send(pipe, &reply, Some(stop), Instant::now() + READ_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::Win32::System::Threading::SetEvent;

    static NEXT: AtomicU32 = AtomicU32::new(0);

    fn name() -> String {
        format!(
            r"\\.\pipe\Argus.Test.{}.{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )
    }

    struct TestServer {
        name: String,
        stop: Arc<Handle>,
        thread: Option<std::thread::JoinHandle<Result<()>>>,
    }

    impl TestServer {
        fn new(owner: String) -> Self {
            let name = name();
            let stop = Arc::new(event().unwrap());
            let (tx, rx) = std::sync::mpsc::channel();
            let thread = {
                let name = name.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    // The test server is not System. Give its own account the
                    // right to create instances, independently of the owner
                    // token the production authenticator is asked to verify.
                    let sid = security::current_sid().unwrap();
                    let sd = security::Descriptor::new(&format!("D:P(A;;GA;;;{sid})")).unwrap();
                    serve_with_descriptor(
                        &name,
                        owner,
                        stop,
                        &sd,
                        Arc::new(Context::testing()),
                        || tx.send(()).unwrap(),
                    )
                })
            };
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Self {
                name,
                stop,
                thread: Some(thread),
            }
        }

        fn call(&self, op: Operation) -> Result<Reply> {
            request(&self.name, std::process::id(), op)
        }

        fn connect(&self) -> Handle {
            connect(&self.name).unwrap()
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            unsafe {
                SetEvent(self.stop.0).unwrap();
            }
            self.thread.take().unwrap().join().unwrap().unwrap();
        }
    }

    #[test]
    fn production_acl_allows_client_but_not_another_server() {
        let name = name();
        let owner = security::current_sid().unwrap();
        let sd = security::pipe_descriptor(&owner).unwrap();
        let server = create(&name, &sd, true).unwrap();
        if !argus_core::privilege::is_elevated() {
            assert!(matches!(create(&name, &sd, false), Err(5)));
        }
        let client =
            std::thread::spawn(move || request(&name, std::process::id(), Operation::Ping));
        let done = event().unwrap();
        let mut ov = OVERLAPPED {
            hEvent: done.0,
            ..Default::default()
        };
        let result = unsafe { ConnectNamedPipe(server.0, Some(&mut ov)) };
        if !result
            .as_ref()
            .is_err_and(|e| e.code() == ERROR_PIPE_CONNECTED.to_hresult())
        {
            wait_io(
                server.0,
                &mut ov,
                result,
                None,
                Instant::now() + READ_TIMEOUT,
            )
            .unwrap();
        }
        let stop = event().unwrap();
        handle_client(server.0, &owner, stop.0, &Context::testing()).unwrap();
        assert!(matches!(
            client.join().unwrap().unwrap(),
            Reply::Pong { .. }
        ));
    }

    #[test]
    fn rejects_wrong_account_malformed_frames_and_protocol() {
        let wrong = TestServer::new("S-1-5-18".into());
        assert!(wrong.call(Operation::Ping).is_err());
        let server = TestServer::new(security::current_sid().unwrap());
        assert!(matches!(
            server
                .call(Operation::FindHandles {
                    paths: vec![r"\\server\share\file".into()]
                })
                .unwrap(),
            Reply::Error(_)
        ));
        assert!(matches!(
            server.call(Operation::NetworkSnapshot).unwrap(),
            Reply::Network(None)
        ));
        let raw = server.connect();
        let deadline = Instant::now() + READ_TIMEOUT;
        transfer(
            raw.0,
            &mut (MAX_FRAME as u32 + 1).to_le_bytes(),
            true,
            None,
            deadline,
        )
        .unwrap();
        assert!(receive::<Reply>(raw.0, None, deadline).is_err());
        let raw = server.connect();
        send(
            raw.0,
            &Request {
                protocol: 99,
                operation: Operation::NetworkSnapshot,
            },
            None,
            deadline,
        )
        .unwrap();
        assert!(receive::<Reply>(raw.0, None, deadline).is_err());
        assert!(matches!(
            server.call(Operation::Ping).unwrap(),
            Reply::Pong { .. }
        ));
    }

    #[test]
    fn stalled_client_does_not_block_other_clients_or_stop() {
        let server = TestServer::new(security::current_sid().unwrap());
        let _stalled = server.connect();
        assert!(matches!(
            server.call(Operation::Ping).unwrap(),
            Reply::Pong { .. }
        ));
        let start = Instant::now();
        drop(server);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn client_limit_does_not_take_down_listener() {
        let server = TestServer::new(security::current_sid().unwrap());
        let stalled: Vec<_> = (0..CLIENTS).map(|_| server.connect()).collect();
        assert!(server.call(Operation::Ping).is_err());
        drop(stalled);
        // Workers observe broken pipes asynchronously. All retries here are
        // read-only probes; the production client never replays mutations.
        let start = Instant::now();
        loop {
            if matches!(server.call(Operation::Ping), Ok(Reply::Pong { .. })) {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn reply_survives_a_delayed_reader() {
        let server = TestServer::new(security::current_sid().unwrap());
        let raw = server.connect();
        let deadline = Instant::now() + READ_TIMEOUT;
        send(
            raw.0,
            &Request {
                protocol: crate::PROTOCOL,
                operation: Operation::Ping,
            },
            None,
            deadline,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(matches!(
            receive::<Reply>(raw.0, None, deadline).unwrap(),
            Reply::Pong { .. }
        ));
    }

    #[test]
    fn network_reply_crosses_pipe_buffers_without_expanding_request_limit() {
        let name = name();
        let owner = security::current_sid().unwrap();
        let sd = security::pipe_descriptor(&owner).unwrap();
        let server = create(&name, &sd, true).unwrap();
        let client = std::thread::spawn(move || {
            request(&name, std::process::id(), Operation::NetworkSnapshot)
        });
        let connected = event().unwrap();
        let mut ov = OVERLAPPED {
            hEvent: connected.0,
            ..Default::default()
        };
        let result = unsafe { ConnectNamedPipe(server.0, Some(&mut ov)) };
        if !result
            .as_ref()
            .is_err_and(|e| e.code() == ERROR_PIPE_CONNECTED.to_hresult())
        {
            wait_io(
                server.0,
                &mut ov,
                result,
                None,
                Instant::now() + READ_TIMEOUT,
            )
            .unwrap();
        }
        let request: Request = receive(server.0, None, Instant::now() + READ_TIMEOUT).unwrap();
        assert!(matches!(request.operation, Operation::NetworkSnapshot));
        security::authenticate(server.0, &owner).unwrap();
        let frame = crate::NetworkFrame {
            epoch: "large".into(),
            elapsed_us: 1_000_000,
            counters: (0..8192)
                .map(|pid| (pid, 1, 0, 1_000_000, 2_000_000))
                .collect(),
        };
        let reply = Reply::Network(Some(frame));
        assert!(serde_json::to_vec(&reply).unwrap().len() > MAX_FRAME);
        send(server.0, &reply, None, Instant::now() + READ_TIMEOUT).unwrap();
        let Reply::Network(Some(frame)) = client.join().unwrap().unwrap() else {
            panic!("unexpected reply")
        };
        assert_eq!(frame.counters.len(), 8192);
    }

    #[test]
    fn routes_process_operations_and_rejects_stale_creation_time() {
        use windows::Win32::System::Threading::{
            GetPriorityClass, GetProcessTimes, IDLE_PRIORITY_CLASS,
        };
        let server = TestServer::new(security::current_sid().unwrap());
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Child(
            std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "pause"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(0x08000000)
                .spawn()
                .unwrap(),
        );
        let handle = HANDLE(child.0.as_raw_handle());
        let (mut created, mut exited, mut kernel, mut user) = Default::default();
        unsafe {
            GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user).unwrap();
        }
        let created: windows::Win32::Foundation::FILETIME = created;
        let time = (((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
            as i64
            - 116_444_736_000_000_000)
            / 10_000;
        let pid = child.0.id();
        assert!(matches!(
            server
                .call(Operation::Terminate {
                    pid,
                    create_time: time + 1
                })
                .unwrap(),
            Reply::Error(argus_core::actions::ActionError::NotFound)
        ));
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(matches!(
            server
                .call(Operation::Priority {
                    pid,
                    create_time: time,
                    priority: argus_core::actions::Priority::Idle
                })
                .unwrap(),
            Reply::Priority(_)
        ));
        assert_eq!(unsafe { GetPriorityClass(handle) }, IDLE_PRIORITY_CLASS.0);
        assert!(matches!(
            server
                .call(Operation::ApplyRule {
                    pid,
                    create_time: time + 1,
                    priority: Some(argus_core::actions::Priority::AboveNormal),
                    efficiency: Some(true)
                })
                .unwrap(),
            Reply::Error(argus_core::actions::ActionError::NotFound)
        ));
        assert_eq!(unsafe { GetPriorityClass(handle) }, IDLE_PRIORITY_CLASS.0);
        assert!(matches!(
            server
                .call(Operation::ApplyRule {
                    pid,
                    create_time: time,
                    priority: Some(argus_core::actions::Priority::AboveNormal),
                    efficiency: Some(true)
                })
                .unwrap(),
            Reply::Done
        ));
        assert_eq!(
            unsafe { GetPriorityClass(handle) },
            windows::Win32::System::Threading::ABOVE_NORMAL_PRIORITY_CLASS.0
        );
        assert!(matches!(
            server
                .call(Operation::Terminate {
                    pid,
                    create_time: time
                })
                .unwrap(),
            Reply::Done
        ));
        assert!(!child.0.wait().unwrap().success());
    }

    #[test]
    fn local_pipe_authentication_protocol_and_stop() {
        let name = format!(r"\\.\pipe\Argus.Test.{}", std::process::id());
        let stop = Arc::new(event().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = {
            let name = name.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let owner = security::current_sid().unwrap();
                // The test server runs as this account, not LocalSystem. It
                // needs server-instance access; production grants that only
                // to System and elevated administrators.
                let descriptor =
                    security::Descriptor::new(&format!("D:P(A;;GA;;;{owner})")).unwrap();
                serve_with_descriptor(
                    &name,
                    owner,
                    stop,
                    &descriptor,
                    Arc::new(Context::testing()),
                    || tx.send(()).unwrap(),
                )
            })
        };
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            request(&name, std::process::id(), Operation::Ping).unwrap(),
            Reply::Pong { protocol: 4, .. }
        ));
        assert!(request(&name, 4, Operation::Ping).is_err());
        assert!(matches!(
            request(
                &name,
                std::process::id(),
                Operation::Terminate {
                    pid: 5,
                    create_time: 0
                }
            )
            .unwrap(),
            Reply::Error(_)
        ));
        unsafe {
            SetEvent(stop.0).unwrap();
        }
        thread.join().unwrap().unwrap();
    }
}
