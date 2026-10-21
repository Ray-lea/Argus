//! Per-process network traffic from the kernel's own network events (the
//! `Microsoft-Windows-Kernel-Network` ETW provider, which Resource Monitor also
//! reads). Starting an ETW session needs administrator rights or membership in
//! Performance Log Users; without them `NetworkTracker::start` returns None.
//!
//! ETW sessions outlive the process that started them, so a session left by an
//! earlier run is stopped before a new one starts, and `stop_session` lets the
//! host stop it on exit. Each build of the host passes its own session name, so
//! a development build never stops an installed one's session.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_CANCELLED, ERROR_SUCCESS, GetLastError,
};
use windows::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW, EVENT_CONTROL_CODE_ENABLE_PROVIDER,
    EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2, OpenTraceW, PROCESS_TRACE_MODE_EVENT_RECORD,
    PROCESS_TRACE_MODE_REAL_TIME, PROCESSTRACE_HANDLE, ProcessTrace, StartTraceW,
    TRACE_LEVEL_INFORMATION, WNODE_FLAG_TRACED_GUID,
};
use windows::core::{GUID, PCWSTR, PWSTR};

use crate::util::{AlignedBuf, wide};

const KERNEL_NETWORK: GUID = GUID::from_u128(0x7dd42a49_5329_4832_8dfd_43d979153a88);
const KEYWORD_IPV4: u64 = 0x10;
const KEYWORD_IPV6: u64 = 0x20;
/// OpenTraceW's failure value.
const INVALID_TRACE: u64 = u64::MAX;

/// Bytes since the last read, by PID.
type Totals = Mutex<HashMap<u32, (u64, u64)>>;

pub struct NetworkTracker {
    session: CONTROLTRACE_HANDLE,
    trace: PROCESSTRACE_HANDLE,
    totals: Arc<Totals>,
    consumer: Option<JoinHandle<()>>,
    last: Instant,
}

impl NetworkTracker {
    /// Starts the trace session; None without the rights to (or if ETW fails).
    pub fn start(session_name: &str) -> Option<Self> {
        let mut props = Properties::new(session_name);
        let name = wide(session_name);
        let mut session = CONTROLTRACE_HANDLE::default();
        let mut status =
            unsafe { StartTraceW(&mut session, PCWSTR(name.as_ptr()), props.as_mut()) };
        if status == ERROR_ALREADY_EXISTS {
            // Left by a run that did not exit cleanly.
            stop_session(session_name);
            props = Properties::new(session_name);
            status = unsafe { StartTraceW(&mut session, PCWSTR(name.as_ptr()), props.as_mut()) };
        }
        if status != ERROR_SUCCESS {
            // A standard user's local trace may be denied; the host can still
            // obtain the same counters from its authorized service.
            crate::diagnostics::warn(
                "network ETW StartTraceW",
                format!(
                    "{}; elevated={}; service_fallback_possible=true",
                    crate::diagnostics::code(status.0),
                    crate::privilege::is_elevated()
                ),
            );
            return None;
        }
        let enabled = unsafe {
            EnableTraceEx2(
                session,
                &KERNEL_NETWORK,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
                TRACE_LEVEL_INFORMATION as u8,
                KEYWORD_IPV4 | KEYWORD_IPV6,
                0,
                0,
                None,
            )
        };
        if enabled != ERROR_SUCCESS {
            crate::diagnostics::warn(
                "network ETW EnableTraceEx2",
                crate::diagnostics::code(enabled.0),
            );
            stop_session(session_name);
            return None;
        }

        let totals: Arc<Totals> = Arc::default();
        let mut logger = name.clone();
        let mut logfile = EVENT_TRACE_LOGFILEW {
            LoggerName: PWSTR(logger.as_mut_ptr()),
            Context: Arc::as_ptr(&totals) as *mut c_void,
            ..Default::default()
        };
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.Anonymous2.EventRecordCallback = Some(on_event);
        let trace = unsafe { OpenTraceW(&mut logfile) };
        if trace.Value == INVALID_TRACE {
            let error = unsafe { GetLastError() };
            crate::diagnostics::warn("network ETW OpenTraceW", crate::diagnostics::code(error.0));
            stop_session(session_name);
            return None;
        }
        // ProcessTrace delivers events on this thread until the session stops.
        let handle = trace.Value;
        let consumer =
            match std::thread::Builder::new()
                .name("argus-etw".into())
                .spawn(move || unsafe {
                    let result = ProcessTrace(&[PROCESSTRACE_HANDLE { Value: handle }], None, None);
                    if result != ERROR_SUCCESS && result != ERROR_CANCELLED {
                        crate::diagnostics::warn(
                            "network ETW ProcessTrace",
                            crate::diagnostics::code(result.0),
                        );
                    }
                }) {
                Ok(thread) => thread,
                Err(error) => {
                    crate::diagnostics::warn(
                        "network ETW worker creation",
                        crate::diagnostics::io(&error),
                    );
                    unsafe {
                        let _ = CloseTrace(trace);
                    }
                    stop_session(session_name);
                    return None;
                }
            };
        Some(Self {
            session,
            trace,
            totals,
            consumer: Some(consumer),
            last: Instant::now(),
        })
    }

    /// Bytes per second each process sent and received since the last call.
    pub fn rates(&mut self) -> HashMap<u32, (u64, u64)> {
        let secs = self.last.elapsed().as_secs_f64().max(0.001);
        self.last = Instant::now();
        let totals = self.take_bytes();
        totals
            .into_iter()
            .map(|(pid, (sent, received))| {
                let rate = |b: u64| (b as f64 / secs) as u64;
                (pid, (rate(sent), rate(received)))
            })
            .collect()
    }

    /// Raw deltas for a service that maintains non-destructive cumulative
    /// counters for multiple readers. Only the tracker owner drains these.
    pub fn take_bytes(&mut self) -> HashMap<u32, (u64, u64)> {
        std::mem::take(&mut *self.totals.lock().unwrap())
    }

    pub fn is_active(&self) -> bool {
        self.consumer
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }
}

impl Drop for NetworkTracker {
    fn drop(&mut self) {
        let mut props = Properties::new("");
        unsafe {
            let _ = ControlTraceW(
                self.session,
                PCWSTR::null(),
                props.as_mut(),
                EVENT_TRACE_CONTROL_STOP,
            );
            let _ = CloseTrace(self.trace);
        }
        if let Some(consumer) = self.consumer.take() {
            let _ = consumer.join();
        }
    }
}

/// Stops the session by name, whoever started it (an earlier run, or this
/// one on exit, when threads do not get to run destructors).
pub fn stop_session(session_name: &str) {
    let mut props = Properties::new(session_name);
    let name = wide(session_name);
    unsafe {
        let _ = ControlTraceW(
            CONTROLTRACE_HANDLE::default(),
            PCWSTR(name.as_ptr()),
            props.as_mut(),
            EVENT_TRACE_CONTROL_STOP,
        );
    }
}

/// EVENT_TRACE_PROPERTIES followed by room for the session name, which ETW
/// copies in.
struct Properties(AlignedBuf);

impl Properties {
    fn new(session_name: &str) -> Self {
        // Room for the longest name ETW may copy back (1024 characters).
        let name_room = (session_name.encode_utf16().count() + 1).max(1024) * 2;
        let size = size_of::<EVENT_TRACE_PROPERTIES>() + name_room;
        let mut buf = AlignedBuf::with_capacity(size);
        let props = unsafe { &mut *(buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES) };
        props.Wnode.BufferSize = size as u32;
        props.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        // QueryPerformanceCounter timestamps.
        props.Wnode.ClientContext = 1;
        props.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        // Deliver at least once a second, so rates track samples.
        props.FlushTimer = 1;
        props.BufferSize = 64;
        props.MinimumBuffers = 4;
        props.MaximumBuffers = 32;
        props.LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        Self(buf)
    }

    fn as_mut(&mut self) -> *mut EVENT_TRACE_PROPERTIES {
        self.0.as_mut_ptr().cast()
    }
}

/// TCP and UDP, IPv4 and IPv6 data events all start with the process ID and
/// the byte count (UInt32 each).
unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    let r = unsafe { &*record };
    if r.EventHeader.ProviderId != KERNEL_NETWORK || r.UserDataLength < 8 {
        return;
    }
    let sent = match r.EventHeader.EventDescriptor.Id {
        10 | 26 | 42 | 58 => true,
        11 | 27 | 43 | 59 => false,
        _ => return,
    };
    let data = unsafe { std::slice::from_raw_parts(r.UserData as *const u8, 8) };
    let pid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as u64;
    let totals = unsafe { &*(r.UserContext as *const Totals) };
    if let Ok(mut t) = totals.lock() {
        let entry = t.entry(pid).or_default();
        if sent {
            entry.0 += size;
        } else {
            entry.1 += size;
        }
    }
}
