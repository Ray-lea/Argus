//! A job-contained helper owns all potentially blocking handle/file queries.
//! The service watches its byte stream without blocking reads or reader threads.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use argus_core::actions::ActionError;
use argus_core::handles::{self, Cursor, HandleLocker, HandleReport, HandleSummary, ScanEvent};
use argus_core::tr;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Pipes::PeekNamedPipe;
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::WaitForSingleObject;

use crate::util::{Handle, io, win};

pub(crate) const WORKER_ARG: &str = "--argus-handle-scan";
const PREFIX: &str = "@argus-handles ";
const TOTAL: Duration = Duration::from_secs(15);
const STARTUP: Duration = Duration::from_secs(3);
const QUERY: Duration = Duration::from_millis(500);
const MAX_RESTARTS: usize = 4;
const MAX_LINE: usize = 32 * 1024;
static SCANNING: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkerRequest {
    paths: Vec<String>,
    after: Option<Cursor>,
}

fn error_text(error: ActionError) -> String {
    match error {
        ActionError::AccessDenied => tr!("service.handles.accessDenied"),
        ActionError::NotFound => tr!("service.handles.targetGone"),
        ActionError::Os(message) => message,
        _ => tr!("service.handles.failed"),
    }
}

fn unavailable(message: String) -> HandleReport {
    HandleReport {
        summary: HandleSummary {
            elevated: argus_core::privilege::is_elevated(),
            error: Some(message),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Local fallback, also used inside the privileged service. Never invokes UAC.
pub fn scan_local(paths: Vec<String>) -> HandleReport {
    if let Err(error) = handles::validate_paths(&paths) {
        return unavailable(error_text(error));
    }
    let Ok(_guard) = SCANNING.try_lock() else {
        return unavailable(tr!("service.handles.busy"));
    };
    let Ok(exe) = std::env::current_exe() else {
        return unavailable(tr!("service.handles.noHelper"));
    };
    run(
        paths,
        |request| {
            let mut command = Command::new(&exe);
            command
                .env(argus_core::i18n::ENV, argus_core::i18n::locale().tag())
                .arg(WORKER_ARG)
                .arg(serde_json::to_string(request).expect("serialize handle request"));
            command
        },
        TOTAL,
        QUERY,
    )
}

/// Invoked before native UI/single-instance initialization. The one-byte gate keeps
/// the worker idle until the parent has assigned its kill-on-close job.
pub(crate) fn worker(argument: &str) -> u32 {
    if argument.len() > 16 * 1024 {
        return 87;
    }
    let Ok(request) = serde_json::from_str::<WorkerRequest>(argument) else {
        return 87;
    };
    if handles::validate_paths(&request.paths).is_err() {
        return 87;
    }
    let mut gate = [0];
    if std::io::stdin().read_exact(&mut gate).is_err() || gate != [1] {
        return 87;
    }
    argus_core::privilege::enable_debug_privilege();
    let mut out = std::io::stdout().lock();
    let mut emit = |event| {
        let Ok(data) = serde_json::to_vec(&event) else {
            return false;
        };
        out.write_all(PREFIX.as_bytes())
            .and_then(|_| out.write_all(&data))
            .and_then(|_| out.write_all(b"\n"))
            .and_then(|_| out.flush())
            .is_ok()
    };
    if !emit(ScanEvent::Ready) {
        return 109;
    }
    if let Err(error) = handles::scan(&request.paths, request.after, &mut emit) {
        emit(ScanEvent::Error(error));
    }
    0
}

struct Worker {
    child: Child,
    output: ChildStdout,
    _job: Handle,
    buffer: Vec<u8>,
}

impl Worker {
    fn spawn(mut command: Command) -> Result<Self, u32> {
        let job = Handle(unsafe { CreateJobObjectW(None, None) }.map_err(win)?);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = 256 * 1024 * 1024;
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        }
        .map_err(win)?;
        let mut directory = [0u16; 1024];
        let n = unsafe { GetSystemDirectoryW(Some(&mut directory)) } as usize;
        if n == 0 || n >= directory.len() {
            return Err(87);
        }
        command
            .current_dir(String::from_utf16_lossy(&directory[..n]))
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(io)?;
        if let Err(error) =
            unsafe { AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())) }
        {
            let _ = child.kill();
            return Err(win(error));
        }
        let output = child.stdout.take().ok_or(6u32)?;
        let mut worker = Self {
            child,
            output,
            _job: job,
            buffer: Vec::new(),
        };
        worker
            .child
            .stdin
            .take()
            .ok_or(6u32)?
            .write_all(&[1])
            .map_err(io)?;
        Ok(worker)
    }

    fn read(&mut self) -> Result<Vec<ScanEvent>, u32> {
        let mut available = 0;
        unsafe {
            PeekNamedPipe(
                HANDLE(self.output.as_raw_handle()),
                None,
                0,
                None,
                Some(&mut available),
                None,
            )
        }
        .map_err(win)?;
        if available == 0 {
            return Ok(Vec::new());
        }
        let mut data = vec![0; (available as usize).min(16 * 1024)];
        let n = self.output.read(&mut data).map_err(io)?;
        if n == 0 {
            return Err(109);
        }
        self.buffer.extend_from_slice(&data[..n]);
        if self.buffer.len() > MAX_LINE {
            return Err(87);
        }
        let mut result = Vec::new();
        while let Some(end) = self.buffer.iter().position(|&b| b == b'\n') {
            let line = &self.buffer[..end];
            if let Some(json) = line.strip_prefix(PREFIX.as_bytes()) {
                result.push(serde_json::from_slice(json).map_err(|_| 87u32)?);
            }
            self.buffer.drain(..=end);
        }
        Ok(result)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        // Some filesystem drivers cannot cancel I/O immediately. Do not wait
        // indefinitely for process teardown; the job remains responsible for it.
        unsafe {
            WaitForSingleObject(HANDLE(self.child.as_raw_handle()), 200);
        }
    }
}

fn run(
    paths: Vec<String>,
    mut command: impl FnMut(&WorkerRequest) -> Command,
    budget: Duration,
    query_timeout: Duration,
) -> HandleReport {
    let deadline = Instant::now() + budget;
    let mut report = HandleReport::default();
    report.summary.elevated = argus_core::privilege::is_elevated();
    let mut found: BTreeMap<(u32, i64), HandleLocker> = BTreeMap::new();
    let mut after = None;
    'restart: for attempt in 0..=MAX_RESTARTS {
        let mut worker = match Worker::spawn(command(&WorkerRequest {
            paths: paths.clone(),
            after,
        })) {
            Ok(worker) => worker,
            Err(error) => {
                report.summary.error = Some(tr!(
                    "service.handles.startFailed",
                    error = crate::util::message(error)
                ));
                break;
            }
        };
        let mut activity = Instant::now();
        let mut cursor = None;
        let mut checked = 0;
        let mut denied = 0;
        let mut skipped = 0;
        loop {
            let now = Instant::now();
            let timeout = if cursor.is_some() {
                query_timeout
            } else {
                STARTUP
            };
            if now >= deadline || now.duration_since(activity) > timeout {
                report.summary.checked += checked;
                report.summary.denied_processes += denied;
                report.summary.skipped += skipped;
                report.summary.truncated = true;
                if cursor.is_some() && now.duration_since(activity) > timeout {
                    report.summary.timeouts += 1;
                }
                if now < deadline
                    && let Some(current) = cursor
                    && attempt < MAX_RESTARTS
                {
                    after = Some(current);
                    continue 'restart;
                }
                report.summary.error = Some(tr!("service.handles.timeLimit"));
                break 'restart;
            }
            match worker.read() {
                Ok(events) => {
                    for event in events {
                        activity = Instant::now();
                        match event {
                            ScanEvent::Ready => {}
                            ScanEvent::Progress {
                                cursor: current,
                                checked: c,
                                denied: d,
                                skipped: s,
                            } => {
                                report.summary.available = true;
                                cursor = Some(current);
                                checked = c;
                                denied = d;
                                skipped = s;
                            }
                            ScanEvent::Found(locker) => {
                                let key = (locker.pid, locker.create_time);
                                if let Some(existing) = found.get_mut(&key) {
                                    existing.count += locker.count;
                                } else if found.len() < handles::MAX_LOCKERS {
                                    found.insert(key, locker);
                                } else {
                                    report.summary.checked += checked;
                                    report.summary.denied_processes += denied;
                                    report.summary.skipped += skipped;
                                    report.summary.truncated = true;
                                    break 'restart;
                                }
                            }
                            ScanEvent::Done(summary) => {
                                report.summary.available = true;
                                report.summary.elevated = summary.elevated;
                                report.summary.checked += summary.checked;
                                report.summary.denied_processes += summary.denied_processes;
                                report.summary.skipped += summary.skipped;
                                report.summary.truncated |= summary.truncated;
                                break 'restart;
                            }
                            ScanEvent::Error(error) => {
                                report.summary.checked += checked;
                                report.summary.denied_processes += denied;
                                report.summary.skipped += skipped;
                                report.summary.error = Some(error_text(error));
                                break 'restart;
                            }
                        }
                    }
                }
                Err(_) => {
                    report.summary.checked += checked;
                    report.summary.denied_processes += denied;
                    report.summary.skipped += skipped;
                    report.summary.error = Some(tr!("service.handles.helperExited"));
                    break 'restart;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    report.lockers = found.into_values().collect();
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::GetCurrentProcess;

    fn fixture(request: &WorkerRequest, mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "handles::tests::worker_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("ARGUS_HANDLE_FIXTURE", mode)
            .env(
                "ARGUS_HANDLE_REQUEST",
                serde_json::to_string(request).unwrap(),
            );
        command
    }

    #[test]
    #[ignore = "subprocess fixture, invoked by the guarded-worker tests"]
    fn worker_fixture() {
        let Ok(mode) = std::env::var("ARGUS_HANDLE_FIXTURE") else {
            return;
        };
        let request: WorkerRequest =
            serde_json::from_str(&std::env::var("ARGUS_HANDLE_REQUEST").unwrap()).unwrap();
        let mut gate = [0];
        std::io::stdin().read_exact(&mut gate).unwrap();
        assert_eq!(gate, [1]);
        let mut out = std::io::stdout().lock();
        let mut emit = |event: ScanEvent| {
            writeln!(out, "\n{PREFIX}{}", serde_json::to_string(&event).unwrap()).unwrap();
            out.flush().unwrap();
        };
        emit(ScanEvent::Ready);
        if mode == "hang" {
            std::thread::sleep(Duration::from_secs(20));
            return;
        }
        if request.after.is_none() {
            emit(ScanEvent::Progress {
                cursor: Cursor { pid: 10, handle: 1 },
                checked: 0,
                denied: 0,
                skipped: 0,
            });
            emit(ScanEvent::Found(HandleLocker {
                pid: 10,
                create_time: 20,
                name: "fixture".into(),
                path: r"C:\fixture".into(),
                count: 1,
            }));
            emit(ScanEvent::Progress {
                cursor: Cursor { pid: 10, handle: 2 },
                checked: 1,
                denied: 1,
                skipped: 0,
            });
            std::thread::sleep(Duration::from_secs(20));
        } else {
            assert_eq!(request.after, Some(Cursor { pid: 10, handle: 2 }));
            emit(ScanEvent::Found(HandleLocker {
                pid: 10,
                create_time: 20,
                name: "fixture".into(),
                path: r"C:\other".into(),
                count: 1,
            }));
            emit(ScanEvent::Done(HandleSummary {
                available: true,
                checked: 1,
                ..Default::default()
            }));
        }
    }

    #[test]
    fn watchdog_retains_hits_and_resumes_after_a_stalled_handle() {
        let start = Instant::now();
        let report = run(
            vec![r"C:\fixture".into()],
            |r| fixture(r, "resume"),
            Duration::from_secs(5),
            Duration::from_millis(100),
        );
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(report.lockers.len(), 1);
        assert_eq!(report.lockers[0].count, 2);
        assert_eq!(report.summary.checked, 2);
        assert_eq!(report.summary.timeouts, 1);
        assert!(report.summary.incomplete());
        assert!(report.summary.error.is_none());
    }

    #[test]
    fn dropping_a_worker_terminates_its_process() {
        let request = WorkerRequest {
            paths: vec![r"C:\fixture".into()],
            after: None,
        };
        let worker = Worker::spawn(fixture(&request, "hang")).unwrap();
        let mut duplicate = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                HANDLE(worker.child.as_raw_handle()),
                GetCurrentProcess(),
                &mut duplicate,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
            .unwrap();
        }
        let process = Handle(duplicate);
        drop(worker);
        assert_eq!(
            unsafe { WaitForSingleObject(process.0, 2000) },
            WAIT_OBJECT_0
        );
    }

    #[test]
    fn stalled_startup_has_a_total_deadline() {
        let report = run(
            vec![r"C:\fixture".into()],
            |r| fixture(r, "hang"),
            Duration::from_millis(250),
            Duration::from_millis(100),
        );
        assert!(!report.summary.available);
        assert!(report.summary.truncated && report.summary.error.is_some());
    }
}
