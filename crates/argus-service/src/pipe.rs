use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
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
/// For the Ping that asks a service which dropped a request for its protocol.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
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
        argus_core::diagnostics::warn(
            "service IPC frame rejected",
            format!("length={len}; limit={}", T::LIMIT),
        );
        return Err(87);
    }
    let mut data = vec![0; len];
    transfer(pipe, &mut data, false, stop, deadline)?;
    serde_json::from_slice(&data).map_err(|error| {
        argus_core::diagnostics::warn(
            "service IPC JSON rejected",
            format!(
                "category={:?}; line={}; column={}; bytes={len}",
                error.classify(),
                error.line(),
                error.column()
            ),
        );
        87
    })
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

/// `Err(INCOMPATIBLE)` for anything but Ping means the service speaks another
/// protocol and did not run `operation`: services never run a request of
/// another protocol (handle_client), so the caller may run it itself.
pub(crate) fn request_with_timeout(
    name: &str,
    expected_pid: u32,
    operation: Operation,
    timeout: Duration,
) -> Result<Reply> {
    // Keep the SCM-reported process alive as an object while authenticating the
    // pipe endpoint, so a recycled numeric PID cannot authenticate a fake server.
    // Held across the probe below, it also makes both exchanges reach the same
    // process.
    let process =
        Handle(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, expected_pid) }.map_err(win)?);
    // Periodic reads fail quietly; probing each would double their traffic
    // for as long as an old service runs.
    let probe = !matches!(
        operation,
        Operation::Ping
            | Operation::NetworkSnapshot
            | Operation::SensorSnapshot {}
            | Operation::DiskSnapshot {}
    );
    match exchange(name, &process, expected_pid, operation, timeout) {
        // A service of another protocol closes the pipe without a reply, and
        // so may one that failed after running the request. Since the first
        // protocol a service rejects every request but Ping of a protocol not
        // its own before running it (handle_client), so a Ping in the
        // version-one envelope tells the two apart.
        Err(e @ (109 | 232 | 233)) if probe => {
            match exchange(name, &process, expected_pid, Operation::Ping, PROBE_TIMEOUT) {
                Err(INCOMPATIBLE) => Err(INCOMPATIBLE),
                _ => Err(e),
            }
        }
        result => result,
    }
}

fn exchange(
    name: &str,
    process: &Handle,
    expected_pid: u32,
    operation: Operation,
    timeout: Duration,
) -> Result<Reply> {
    let pipe = crate::util::stage("service pipe connect", connect(name))?;
    let mut actual_pid = 0;
    unsafe { GetNamedPipeServerProcessId(pipe.0, &mut actual_pid) }.map_err(win)?;
    if actual_pid != expected_pid || unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 {
        argus_core::diagnostics::warn(
            "service pipe identity rejected",
            format!("expected_pid={expected_pid}; actual_pid={actual_pid}"),
        );
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
    let reply = crate::util::stage(
        "service pipe receive reply",
        receive(pipe.0, None, deadline),
    )?;
    if let Reply::Pong { protocol, .. } = &reply
        && *protocol != crate::PROTOCOL
    {
        argus_core::diagnostics::warn(
            "service protocol mismatch",
            format!("expected={}; actual={protocol}", crate::PROTOCOL),
        );
        return Err(INCOMPATIBLE);
    }
    if let Reply::Pong {
        diagnostics: Some(report),
        ..
    } = &reply
    {
        crate::diagnostics::import(report, expected_pid);
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

fn disconnect(pipe: HANDLE) -> Result<()> {
    match unsafe { DisconnectNamedPipe(pipe) }.map_err(win) {
        // A peer may already have closed an aborted or refused connection.
        Ok(()) | Err(233) => Ok(()),
        Err(code) => Err(code),
    }
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
    let active = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::sync_channel::<ActiveClient>(CLIENTS);
    let worker_stop = stop.clone();
    let workers = spawn_workers(CLIENTS, rx, move |client: ActiveClient| {
        let _ = handle_client(
            client.pipe.as_ref().unwrap().0,
            &owner,
            worker_stop.0,
            &context,
        );
    })?;
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
                Err(109 | 232 | 233) => {
                    // A client can close between CreateNamedPipe and accept.
                    // Reuse this instance to keep the pipe name claimed and
                    // avoid consuming another slot in the bounded instance pool.
                    disconnect(listener.0)?;
                    continue;
                }
                Err(e) => return Err(e),
                Ok(_) => {}
            }
        }
        if active.load(Ordering::Acquire) >= CLIENTS {
            disconnect(listener.0)?;
            continue;
        }
        // Create the next listener before handing off the connected instance;
        // there is never a gap in which another process can claim the pipe name.
        let next = create(name, descriptor, false)?;
        let client = std::mem::replace(&mut listener, next);
        active.fetch_add(1, Ordering::AcqRel);
        tx.try_send(ActiveClient {
            pipe: Some(client),
            active: active.clone(),
        })
        .map_err(|_| 31u32)?;
    })();
    drop(listener);
    drop(tx);
    for worker in workers {
        let _ = worker.join();
    }
    result
}

struct ActiveClient {
    pipe: Option<Handle>,
    active: Arc<AtomicUsize>,
}

impl Drop for ActiveClient {
    fn drop(&mut self) {
        // Close the server endpoint before making its instance slot available.
        drop(self.pipe.take());
        self.active.fetch_sub(1, Ordering::Release);
    }
}

fn spawn_workers<T: Send + 'static>(
    count: usize,
    rx: mpsc::Receiver<T>,
    run: impl Fn(T) + Send + Sync + 'static,
) -> Result<Vec<std::thread::JoinHandle<()>>> {
    let rx = Arc::new(Mutex::new(rx));
    let run = Arc::new(run);
    (0..count)
        .map(|_| {
            let (rx, run) = (rx.clone(), run.clone());
            std::thread::Builder::new()
                .name("argus-service-ipc".into())
                .spawn(move || {
                    loop {
                        let work = rx.lock().unwrap().recv();
                        let Ok(work) = work else {
                            break;
                        };
                        run(work);
                    }
                })
                .map_err(crate::util::io)
        })
        .collect()
}

fn handle_client(pipe: HANDLE, owner: &str, stop: HANDLE, context: &Context) -> Result<()> {
    let request: Request = receive(pipe, Some(stop), Instant::now() + READ_TIMEOUT)?;
    let pid = security::authenticate(pipe, owner)?;
    // Never run a request of another protocol. A host that finds the pipe
    // closed without a reply asks with a Ping and, hearing another protocol,
    // runs the request itself (request_with_timeout, since 0.2.0): running it
    // here as well, say to serve older hosts, would run it twice.
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
pub(crate) mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::Win32::System::Threading::SetEvent;

    static NEXT: AtomicU32 = AtomicU32::new(0);

    #[test]
    fn persistent_workers_defer_thread_local_cleanup_until_shutdown() {
        use std::cell::RefCell;
        struct DelayedExit(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
        impl Drop for DelayedExit {
            fn drop(&mut self) {
                let _ = self.0.send(());
                let _ = self.1.recv_timeout(Duration::from_secs(3));
            }
        }
        thread_local! { static EXIT: RefCell<Option<DelayedExit>> = const { RefCell::new(None) }; }
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let teardown = Mutex::new(Some(DelayedExit(entered, gate)));
        let (tx, rx) = mpsc::sync_channel(8);
        let (answered, replies) = mpsc::channel();
        let workers = spawn_workers(1, rx, move |()| {
            EXIT.with(|exit| {
                if exit.borrow().is_none() {
                    *exit.borrow_mut() = teardown.lock().unwrap().take();
                }
            });
            let _ = answered.send(std::thread::current().id());
        })
        .unwrap();
        tx.send(()).unwrap();
        let first = replies.recv_timeout(Duration::from_secs(2)).unwrap();
        tx.send(()).unwrap();
        let second = replies.recv_timeout(Duration::from_millis(150));
        let idle = waiting.try_recv().is_err();
        drop(tx);
        let stopping = waiting.recv_timeout(Duration::from_secs(2)).is_ok();
        release.send(()).unwrap();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(second.unwrap(), first);
        assert!(idle, "thread cleanup ran between requests");
        assert!(stopping, "thread cleanup was not joined at shutdown");
    }

    pub(crate) fn name() -> String {
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
            Self::with_context(owner, Context::testing())
        }

        fn with_context(owner: String, context: Context) -> Self {
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
                    serve_with_descriptor(&name, owner, stop, &sd, Arc::new(context), || {
                        tx.send(()).unwrap()
                    })
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
    fn client_closed_before_accept_keeps_the_service_available() {
        let name = name();
        let stop = Arc::new(event().unwrap());
        let (ready, waiting) = std::sync::mpsc::channel();
        let (resume, gate) = std::sync::mpsc::channel();
        let thread = {
            let name = name.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let owner = security::current_sid().unwrap();
                let sd = security::Descriptor::new(&format!("D:P(A;;GA;;;{owner})")).unwrap();
                serve_with_descriptor(
                    &name,
                    owner,
                    stop,
                    &sd,
                    Arc::new(Context::testing()),
                    || {
                        ready.send(()).unwrap();
                        gate.recv_timeout(Duration::from_secs(5)).unwrap();
                    },
                )
            })
        };
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        // Connect and close while the server is deliberately still before accept.
        drop(connect(&name).unwrap());
        resume.send(()).unwrap();
        let reply = request_with_timeout(
            &name,
            std::process::id(),
            Operation::Ping,
            Duration::from_secs(2),
        );
        unsafe {
            SetEvent(stop.0).unwrap();
        }
        let result = thread.join().unwrap();
        assert_eq!(result, Ok(()), "client disconnect terminated the listener");
        assert!(matches!(reply, Ok(Reply::Pong { .. })));
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

    /// Waits for a client on a listening instance.
    fn accept(pipe: &Handle) {
        let connected = event().unwrap();
        let mut ov = OVERLAPPED {
            hEvent: connected.0,
            ..Default::default()
        };
        let result = unsafe { ConnectNamedPipe(pipe.0, Some(&mut ov)) };
        if !result
            .as_ref()
            .is_err_and(|e| e.code() == ERROR_PIPE_CONNECTED.to_hresult())
        {
            wait_io(pipe.0, &mut ov, result, None, Instant::now() + READ_TIMEOUT).unwrap();
        }
    }

    /// Serves `clients` connections as a service of `protocol` that runs
    /// nothing: Ping gets its Pong, any other request is dropped unanswered,
    /// as handle_client drops one of another protocol (or one whose
    /// execution failed). Returns the operations received, in order.
    pub(crate) fn dropping_server(
        name: &str,
        protocol: u32,
        clients: usize,
    ) -> std::thread::JoinHandle<Vec<String>> {
        let name = name.to_string();
        let (ready, listening) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let owner = security::current_sid().unwrap();
            let sd = security::Descriptor::new(&format!("D:P(A;;GA;;;{owner})")).unwrap();
            let mut listener = create(&name, &sd, true).unwrap();
            ready.send(()).unwrap();
            let mut seen = Vec::new();
            for _ in 0..clients {
                accept(&listener);
                // The next instance listens before this one closes, as in serve.
                let next = create(&name, &sd, false).unwrap();
                let client = std::mem::replace(&mut listener, next);
                let request: Request =
                    receive(client.0, None, Instant::now() + READ_TIMEOUT).unwrap();
                let op = format!("{:?}", request.operation);
                seen.push(op.split([' ', '{']).next().unwrap().to_string());
                if matches!(request.operation, Operation::Ping) {
                    let pong = Reply::Pong {
                        version: "0.0.0".into(),
                        protocol,
                        diagnostics: None,
                        sensor_driver_version: None,
                    };
                    send(client.0, &pong, None, Instant::now() + READ_TIMEOUT).unwrap();
                }
            }
            seen
        });
        listening.recv_timeout(Duration::from_secs(5)).unwrap();
        server
    }

    #[test]
    fn an_old_service_that_drops_a_request_is_told_from_a_failure() {
        let terminate = || Operation::Terminate {
            pid: 5,
            create_time: 1,
        };
        // An old service: the Ping after the dropped request reports its
        // protocol, so the request is known not to have run.
        let name = name();
        let server = dropping_server(&name, crate::PROTOCOL - 1, 2);
        assert_eq!(
            request(&name, std::process::id(), terminate()).unwrap_err(),
            INCOMPATIBLE
        );
        assert_eq!(server.join().unwrap(), ["Terminate", "Ping"]);
        // A current service that dropped it may have run it: the error stays.
        let name = self::name();
        let server = dropping_server(&name, crate::PROTOCOL, 2);
        assert_eq!(
            request(&name, std::process::id(), terminate()).unwrap_err(),
            109
        );
        assert_eq!(server.join().unwrap(), ["Terminate", "Ping"]);
        // Periodic reads are not probed.
        let name = self::name();
        let server = dropping_server(&name, crate::PROTOCOL - 1, 1);
        assert_eq!(
            request(&name, std::process::id(), Operation::NetworkSnapshot).unwrap_err(),
            109
        );
        assert_eq!(server.join().unwrap(), ["NetworkSnapshot"]);
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

    /// A waiting process of this test's own, killed when dropped.
    struct Child(std::process::Child);

    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    impl Child {
        fn spawn() -> Self {
            Self(
                std::process::Command::new("cmd.exe")
                    .args(["/d", "/c", "pause"])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .creation_flags(0x08000000)
                    .spawn()
                    .unwrap(),
            )
        }

        fn handle(&self) -> HANDLE {
            HANDLE(self.0.as_raw_handle())
        }

        /// The creation time as requests carry it (Unix ms).
        fn created(&self) -> i64 {
            use windows::Win32::System::Threading::GetProcessTimes;
            let (mut created, mut exited, mut kernel, mut user) = Default::default();
            unsafe {
                GetProcessTimes(
                    self.handle(),
                    &mut created,
                    &mut exited,
                    &mut kernel,
                    &mut user,
                )
                .unwrap();
            }
            let created: windows::Win32::Foundation::FILETIME = created;
            (((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)) as i64
                - 116_444_736_000_000_000)
                / 10_000
        }
    }

    #[test]
    fn requests_of_another_protocol_are_never_run() {
        use windows::Win32::System::Threading::{GetPriorityClass, IDLE_PRIORITY_CLASS};
        let server = TestServer::new(security::current_sid().unwrap());
        let mut child = Child::spawn();
        let (pid, time) = (child.0.id(), child.created());
        let normal = unsafe { GetPriorityClass(child.handle()) };
        assert_ne!(normal, IDLE_PRIORITY_CLASS.0);
        let idle = || Operation::Priority {
            pid,
            create_time: time,
            priority: argus_core::actions::Priority::Idle,
        };
        for protocol in [crate::PROTOCOL - 1, crate::PROTOCOL + 1, 1] {
            for operation in [
                idle(),
                Operation::Terminate {
                    pid,
                    create_time: time,
                },
            ] {
                let raw = server.connect();
                let deadline = Instant::now() + READ_TIMEOUT;
                send(
                    raw.0,
                    &Request {
                        protocol,
                        operation,
                    },
                    None,
                    deadline,
                )
                .unwrap();
                // The pipe closes without a reply once the service is done
                // with the request: it would have run by now.
                assert!(receive::<Reply>(raw.0, None, deadline).is_err());
            }
        }
        assert_eq!(unsafe { GetPriorityClass(child.handle()) }, normal);
        assert!(child.0.try_wait().unwrap().is_none());
        // In this protocol the same request runs.
        assert!(matches!(server.call(idle()).unwrap(), Reply::Priority(_)));
        assert_eq!(
            unsafe { GetPriorityClass(child.handle()) },
            IDLE_PRIORITY_CLASS.0
        );
    }

    #[test]
    fn routes_process_operations_and_rejects_stale_creation_time() {
        use windows::Win32::System::Threading::{GetPriorityClass, IDLE_PRIORITY_CLASS};
        let server = TestServer::new(security::current_sid().unwrap());
        let mut child = Child::spawn();
        let handle = child.handle();
        let time = child.created();
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
            Reply::Pong {
                protocol: crate::PROTOCOL,
                ..
            }
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

    #[test]
    #[ignore = "local broker resource soak; run alone with --test-threads=1"]
    fn resource_soak_requests_disconnects_and_shutdown() {
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
        let handles = || unsafe {
            let mut value = 0;
            GetProcessHandleCount(GetCurrentProcess(), &mut value).unwrap();
            value
        };
        let before = handles();
        println!("broker cold handles={before}");
        let mut settled = None;
        for cycle in 0..6 {
            {
                let server = TestServer::new(security::current_sid().unwrap());
                for _ in 0..50 {
                    assert!(matches!(
                        server.call(Operation::Ping),
                        Ok(Reply::Pong { .. })
                    ));
                }
                let baseline = handles();
                for batch in 0..8 {
                    for _ in 0..500 {
                        drop(server.connect());
                        assert!(matches!(
                            server.call(Operation::Ping),
                            Ok(Reply::Pong { .. })
                        ));
                    }
                    let current = handles();
                    println!(
                        "broker batch={} handles={} baseline={}",
                        batch + 1,
                        current,
                        baseline
                    );
                    assert!(current <= baseline + CLIENTS as u32 * 4);
                }
            }
            let after = handles();
            println!("broker shutdown handles={after} initial={before}");
            println!("broker lifecycle={cycle}");
            if let Some(settled) = settled {
                assert!(after <= settled + 2);
            } else {
                settled = Some(after);
            }
        }
    }

    #[test]
    #[ignore = "read-only load of the already-running authorized broker"]
    fn resource_soak_running_service_reads() {
        use windows::Win32::System::{
            ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
            Threading::{GetProcessHandleCount, PROCESS_QUERY_LIMITED_INFORMATION},
        };
        let pid = crate::install::running_pid()
            .unwrap()
            .expect("existing authorized service required");
        let process =
            Handle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.unwrap());
        let counts = || unsafe {
            let mut handles = 0;
            GetProcessHandleCount(process.0, &mut handles).unwrap();
            let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
            GetProcessMemoryInfo(
                process.0,
                (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            )
            .unwrap();
            (handles, memory.PrivateUsage)
        };
        let mut sensor_states = std::collections::BTreeMap::<String, u32>::new();
        let mut available_network = 0;
        let mut available_disk = 0;
        let mut request_errors = std::collections::BTreeMap::<u32, u32>::new();
        let mut latencies = std::collections::BTreeMap::<String, Vec<u128>>::new();
        let mut no_data_ms = Vec::new();
        let mut settled = None;
        for (name, operation) in [
            ("ping", Operation::Ping),
            ("network", Operation::NetworkSnapshot),
            ("sensors", Operation::SensorSnapshot {}),
            ("disk", Operation::DiskSnapshot {}),
        ] {
            let started = Instant::now();
            let reply =
                request_with_timeout(crate::PIPE_NAME, pid, operation, Duration::from_secs(2));
            println!(
                "{}",
                serde_json::json!({"probe":name,"ms":started.elapsed().as_millis(),"error":reply.as_ref().err(),"ok":reply.is_ok()})
            );
        }
        for batch in 0..8 {
            for _ in 0..200 {
                for operation in [
                    Operation::NetworkSnapshot,
                    Operation::SensorSnapshot {},
                    Operation::DiskSnapshot {},
                ] {
                    let kind = match operation {
                        Operation::NetworkSnapshot => "network",
                        Operation::SensorSnapshot {} => "sensors",
                        _ => "disk",
                    };
                    let started = Instant::now();
                    let reply = match request_with_timeout(
                        crate::PIPE_NAME,
                        pid,
                        operation,
                        Duration::from_secs(2),
                    ) {
                        Ok(reply) => reply,
                        Err(code) => {
                            println!(
                                "{}",
                                serde_json::json!({"request":kind,"elapsedMs":started.elapsed().as_millis(),"error":code})
                            );
                            *request_errors.entry(code).or_default() += 1;
                            assert!(
                                request_errors.values().sum::<u32>() <= 8,
                                "broker read failures: {request_errors:?}"
                            );
                            continue;
                        }
                    };
                    let elapsed = started.elapsed().as_millis();
                    latencies.entry(kind.into()).or_default().push(elapsed);
                    match reply {
                        Reply::Network(frame) => available_network += u32::from(frame.is_some()),
                        Reply::Sensors(frame) => {
                            if frame.state == argus_core::hardware::SensorState::NoData {
                                no_data_ms.push(elapsed);
                            }
                            *sensor_states
                                .entry(format!("{:?}", frame.state))
                                .or_default() += 1
                        }
                        Reply::Disk(frame) => available_disk += u32::from(frame.is_some()),
                        Reply::DiskFailed(_) => {}
                        _ => panic!("unexpected read-only broker reply"),
                    }
                }
            }
            let current = counts();
            println!(
                "{}",
                serde_json::json!({"batch":batch+1,"pid":pid,"handles":current.0,"privateBytes":current.1,"networkReplies":available_network,"diskReplies":available_disk,"sensorStates":sensor_states,"requestErrors":request_errors})
            );
            if let Some((handles, bytes)) = settled {
                assert!(current.0 <= handles + CLIENTS as u32 * 4);
                assert!(current.1 <= bytes + 8 * 1024 * 1024);
            } else {
                settled = Some(current);
            }
        }
        for (kind, mut values) in latencies {
            values.sort_unstable();
            println!(
                "{}",
                serde_json::json!({"latency":kind,"count":values.len(),"medianMs":values[values.len()/2],"p95Ms":values[values.len()*95/100],"maxMs":values.last()})
            );
        }
        println!("{}", serde_json::json!({"sensorNoDataMs":no_data_ms}));
    }

    #[test]
    fn slow_native_reads_leave_the_pipe_responsive_and_keep_recent_snapshots() {
        use argus_core::{
            disk_process::DiskSnapshot,
            hardware::{CpuSensors, SensorState},
        };
        use std::sync::atomic::{AtomicBool, AtomicUsize};
        fn resources() -> (u32, usize) {
            use windows::Win32::System::{
                ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
                Threading::{GetCurrentProcess, GetProcessHandleCount},
            };
            unsafe {
                let mut handles = 0;
                let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
                GetProcessHandleCount(GetCurrentProcess(), &mut handles).unwrap();
                GetProcessMemoryInfo(
                    GetCurrentProcess(),
                    (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                    std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                )
                .unwrap();
                (handles, memory.PrivateUsage)
            }
        }
        let seconds = std::env::var("ARGUS_SERVICE_SOAK_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|s| (5..=300).contains(s));
        let slow = Arc::new(AtomicBool::new(false));
        let reads = Arc::new([
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
        ]);
        let network = {
            let (slow, reads) = (slow.clone(), reads.clone());
            crate::network::NetworkMonitor::with_reader(move || {
                if slow.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(1200));
                }
                let serial = reads[0].fetch_add(1, Ordering::Relaxed) as u64;
                Some(crate::NetworkFrame {
                    epoch: "isolated-test".into(),
                    elapsed_us: serial * 1_200_000,
                    counters: (0..512)
                        .map(|pid| (pid + 42, 1, 0, serial * 100, serial * 200))
                        .collect(),
                })
            })
        };
        let sensors = {
            let (slow, reads) = (slow.clone(), reads.clone());
            crate::sensors::SensorMonitor::with_reader(move || {
                if slow.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(1100));
                }
                reads[1].fetch_add(1, Ordering::Relaxed);
                CpuSensors {
                    state: SensorState::Ready,
                    package_celsius: Some(62.0),
                    cores: (0..32)
                        .map(|core| argus_core::hardware::CpuCoreSensors {
                            core,
                            celsius: Some(62.0),
                            clock_mhz: Some(4200.0),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                }
            })
        };
        let disk = {
            let (slow, reads) = (slow.clone(), reads.clone());
            crate::disk::DiskMonitor::with_reader(move || {
                if slow.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(900));
                }
                let serial = reads[2].fetch_add(1, Ordering::Relaxed) as u64;
                Ok(DiskSnapshot {
                    sampled_at_ms: serial * 900,
                    counters: (0..512)
                        .map(|pid| (pid + 42, 1, serial * 100, serial * 200))
                        .collect(),
                })
            })
        };
        let server = TestServer::with_context(
            security::current_sid().unwrap(),
            Context::with_readers(network, sensors, disk),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let a = matches!(
                server.call(Operation::NetworkSnapshot).unwrap(),
                Reply::Network(Some(_))
            );
            let b = matches!(server.call(Operation::SensorSnapshot {}).unwrap(), Reply::Sensors(frame) if frame.state == SensorState::Ready);
            let c = matches!(
                server.call(Operation::DiskSnapshot {}).unwrap(),
                Reply::Disk(Some(_))
            );
            if a && b && c {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let before: Vec<_> = reads
            .iter()
            .map(|count| count.load(Ordering::Relaxed))
            .collect();
        slow.store(true, Ordering::Relaxed);
        let name = &server.name;
        let began = Instant::now();
        let until = began
            + seconds
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_millis(2600));
        let mut checkpoints = Vec::new();
        let mut next_checkpoint = began + Duration::from_secs(5);
        let samples = std::thread::scope(|scope| {
            let clients: Vec<_> = (0..6)
                .map(|client| {
                    scope.spawn(move || {
                        let mut elapsed = Vec::new();
                        let mut count = 0usize;
                        let mut max_us = 0u128;
                        while Instant::now() < until {
                            let operation = match client % 3 {
                                0 => Operation::NetworkSnapshot,
                                1 => Operation::SensorSnapshot {},
                                _ => Operation::DiskSnapshot {},
                            };
                            let started = Instant::now();
                            let reply = request_with_timeout(
                                name,
                                std::process::id(),
                                operation,
                                Duration::from_millis(500),
                            )
                            .unwrap();
                            let micros = started.elapsed().as_micros();
                            count += 1;
                            max_us = max_us.max(micros);
                            if elapsed.len() < 4096 {
                                elapsed.push(micros);
                            }
                            match reply {
                                Reply::Network(Some(_)) | Reply::Disk(Some(_)) => {}
                                Reply::Sensors(frame) => {
                                    assert_eq!(frame.state, SensorState::Ready)
                                }
                                _ => panic!("recent snapshot lost during a bounded slow read"),
                            }
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        (count, max_us, elapsed)
                    })
                })
                .collect();
            while Instant::now() < until {
                assert!(matches!(
                    request_with_timeout(
                        name,
                        std::process::id(),
                        Operation::Ping,
                        Duration::from_millis(500)
                    ),
                    Ok(Reply::Pong { .. })
                ));
                std::thread::sleep(Duration::from_millis(10));
                if Instant::now() >= next_checkpoint {
                    let (handles, private) = resources();
                    println!(
                        "{}",
                        serde_json::json!({"atMs":began.elapsed().as_millis(),"handles":handles,"privateBytes":private})
                    );
                    checkpoints.push((handles, private));
                    next_checkpoint += Duration::from_secs(5);
                }
            }
            clients
                .into_iter()
                .map(|client| client.join().unwrap())
                .collect::<Vec<_>>()
        });
        let requests: usize = samples.iter().map(|sample| sample.0).sum();
        let max_us = samples.iter().map(|sample| sample.1).max().unwrap();
        let mut samples = samples
            .into_iter()
            .flat_map(|sample| sample.2)
            .collect::<Vec<_>>();
        samples.sort_unstable();
        println!(
            "{}",
            serde_json::json!({"requests":requests,"latencySamples":samples.len(),"p95Us":samples[samples.len()*95/100],"maxUs":max_us,"readsBefore":before,"readsAfter":reads.iter().map(|count|count.load(Ordering::Relaxed)).collect::<Vec<_>>()})
        );
        for (count, before) in reads.iter().zip(before) {
            assert!(count.load(Ordering::Relaxed) > before);
        }
        assert!(samples.len() > 100);
        if let (Some(first), Some(last)) = (checkpoints.first(), checkpoints.last()) {
            assert!(last.0 <= first.0 + 32, "handle growth during cached reads");
            assert!(
                last.1 <= first.1 + 8 * 1024 * 1024,
                "commit growth during cached reads"
            );
        }
    }

    #[test]
    fn cold_readers_coalesce_waiters_and_keep_ping_available() {
        use argus_core::{
            disk_process::DiskSnapshot,
            hardware::{CpuSensors, SensorState},
        };
        use std::sync::atomic::AtomicBool;
        let released = Arc::new(AtomicBool::new(false));
        let (entered, started) = std::sync::mpsc::channel();
        let blocked = || {
            let released = released.clone();
            let entered = entered.clone();
            move || {
                let _ = entered.send(());
                let until = Instant::now() + Duration::from_secs(4);
                while !released.load(Ordering::Relaxed) && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        };
        let wait_network = blocked();
        let wait_sensor = blocked();
        let wait_disk = blocked();
        let server = TestServer::with_context(
            security::current_sid().unwrap(),
            Context::with_readers(
                crate::network::NetworkMonitor::with_reader(move || {
                    wait_network();
                    Some(crate::NetworkFrame {
                        epoch: "cold-test".into(),
                        elapsed_us: 1,
                        counters: vec![],
                    })
                }),
                crate::sensors::SensorMonitor::with_reader(move || {
                    wait_sensor();
                    CpuSensors {
                        state: SensorState::Ready,
                        ..Default::default()
                    }
                }),
                crate::disk::DiskMonitor::with_reader(move || {
                    wait_disk();
                    Ok(DiskSnapshot {
                        sampled_at_ms: 1,
                        counters: vec![],
                    })
                }),
            ),
        );
        let name = &server.name;
        std::thread::scope(|scope| {
            let waiting: Vec<_> = [
                Operation::NetworkSnapshot,
                Operation::SensorSnapshot {},
                Operation::DiskSnapshot {},
            ]
            .into_iter()
            .map(|op| {
                scope.spawn(move || {
                    request_with_timeout(name, std::process::id(), op, Duration::from_secs(2))
                })
            })
            .collect();
            for _ in 0..3 {
                started.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            let start = Instant::now();
            for _ in 0..20 {
                assert!(matches!(
                    request_with_timeout(
                        name,
                        std::process::id(),
                        Operation::NetworkSnapshot,
                        Duration::from_millis(500)
                    ),
                    Ok(Reply::Network(None))
                ));
                assert!(matches!(
                    request_with_timeout(
                        name,
                        std::process::id(),
                        Operation::Ping,
                        Duration::from_millis(500)
                    ),
                    Ok(Reply::Pong { .. })
                ));
            }
            released.store(true, Ordering::Relaxed);
            for client in waiting {
                assert!(client.join().unwrap().is_ok());
            }
            assert!(start.elapsed() < Duration::from_millis(500));
        });
    }
}
