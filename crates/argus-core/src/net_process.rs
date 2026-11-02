//! Per-process traffic from a private system network ETW session, with the
//! Microsoft-Windows-Kernel-Network manifest provider as a compatibility fallback.
//! Starting a session needs the corresponding ETW rights; ordinary hosts can
//! obtain the same counters from the existing privileged service.
//!
//! ETW sessions outlive the process that started them, so a session left by an
//! earlier run is stopped before a new one starts, and `stop_session` lets the
//! host stop it on exit. Each build of the host passes its own session name, so
//! a development build never stops an installed one's session.

mod flush;

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_CANCELLED, ERROR_SUCCESS, GetLastError,
};
use windows::Win32::System::Com::CoCreateGuid;
use windows::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW, EVENT_CONTROL_CODE_ENABLE_PROVIDER,
    EVENT_RECORD, EVENT_TRACE_CONTROL_QUERY, EVENT_TRACE_CONTROL_STOP,
    EVENT_TRACE_FLAG_NETWORK_TCPIP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, EVENT_TRACE_SYSTEM_LOGGER_MODE, EnableTraceEx2, OpenTraceW,
    PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, PROCESSTRACE_HANDLE,
    ProcessTrace, StartTraceW, TRACE_LEVEL_INFORMATION, WNODE_FLAG_TRACED_GUID,
};
use windows::core::{GUID, PCWSTR, PWSTR};

use crate::util::{AlignedBuf, wide};

const KERNEL_NETWORK: GUID = GUID::from_u128(0x7dd42a49_5329_4832_8dfd_43d979153a88);
const TCP_IP: GUID = GUID::from_u128(0x9a280ac0_c8e0_11d1_84e2_00c04fb998a2);
const UDP_IP: GUID = GUID::from_u128(0xbf3a50c5_a9c9_4988_a005_2df0b7c80f80);
const KEYWORD_IPV4: u64 = 0x10;
const KEYWORD_IPV6: u64 = 0x20;
/// OpenTraceW's failure value.
const INVALID_TRACE: u64 = u64::MAX;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    System,
    Manifest,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::System => "system-network",
            Self::Manifest => "manifest-network",
        }
    }
}

/// Bytes since the last read, by PID.
type Totals = Mutex<HashMap<u32, (u64, u64)>>;

/// Fast readers explicitly flush ETW buffers below the shortest UI cadence.
/// Accept scheduling/IPC jitter around a half-second measurement window.
pub fn rate_window_ready(elapsed: Duration) -> bool {
    elapsed >= Duration::from_millis(450)
}

#[derive(Default)]
struct RateWindow {
    last: Option<Instant>,
    identities: HashMap<u32, i64>,
    values: HashMap<u32, (i64, u64, u64)>,
}

impl RateWindow {
    fn sample(
        &mut self,
        now: Instant,
        identities: impl Iterator<Item = (u32, i64)>,
        take: impl FnOnce() -> HashMap<u32, (u64, u64)>,
    ) {
        let Some(last) = self.last else {
            take();
            self.last = Some(now);
            self.identities = identities.collect();
            return;
        };
        let elapsed = now.saturating_duration_since(last);
        if !rate_window_ready(elapsed) {
            return;
        }
        let current: HashMap<_, _> = identities.collect();
        let rate = |bytes: u64| {
            (u128::from(bytes) * 1_000_000 / elapsed.as_micros()).min(u64::MAX as u128) as u64
        };
        self.values = take()
            .into_iter()
            .filter_map(|(pid, (sent, received))| {
                let &created = current
                    .get(&pid)
                    .filter(|created| self.identities.get(&pid) == Some(*created))?;
                Some((pid, (created, rate(sent), rate(received))))
            })
            .collect();
        self.identities = current;
        self.last = Some(now);
    }

    fn value(&self, pid: u32, created: i64) -> (u64, u64) {
        self.values
            .get(&pid)
            .filter(|value| value.0 == created)
            .map_or((0, 0), |value| (value.1, value.2))
    }
}

pub struct NetworkTracker {
    session: CONTROLTRACE_HANDLE,
    trace: PROCESSTRACE_HANDLE,
    totals: Arc<Totals>,
    consumer: Option<JoinHandle<()>>,
    rates: RateWindow,
    source: Source,
    next_health: Instant,
    lost: (u32, u32),
    flusher: flush::Flush,
}

impl NetworkTracker {
    /// Starts the trace session; None without the rights to (or if ETW fails).
    pub fn start(session_name: &str) -> Option<Self> {
        Self::start_source(session_name, Source::System)
            .or_else(|| Self::start_source(session_name, Source::Manifest))
    }

    fn start_source(session_name: &str, source: Source) -> Option<Self> {
        let mut props = Properties::for_source(session_name, source)?;
        let name = wide(session_name);
        let mut session = CONTROLTRACE_HANDLE::default();
        let mut status =
            unsafe { StartTraceW(&mut session, PCWSTR(name.as_ptr()), props.as_mut()) };
        if status == ERROR_ALREADY_EXISTS {
            // Left by a run that did not exit cleanly.
            stop_session(session_name);
            props = Properties::for_source(session_name, source)?;
            status = unsafe { StartTraceW(&mut session, PCWSTR(name.as_ptr()), props.as_mut()) };
        }
        if status != ERROR_SUCCESS {
            // A standard user's local trace may be denied; the host can still
            // obtain the same counters from its authorized service.
            let elevated = crate::privilege::is_elevated();
            let detail = format!(
                "{}; source={}; elevated={elevated}; service_fallback_possible=true",
                crate::diagnostics::code(status.0),
                source.name(),
            );
            if status == ERROR_ACCESS_DENIED && !elevated {
                log::info!(target: "argus_diagnostics", "network ETW local trace unavailable: {detail}");
            } else {
                crate::diagnostics::warn("network ETW StartTraceW", detail);
            }
            return None;
        }
        let enabled = if source == Source::Manifest {
            unsafe {
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
            }
        } else {
            ERROR_SUCCESS
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
        let mut flusher = match flush::Flush::new(session.Value) {
            Ok(flusher) => flusher,
            Err(error) => {
                crate::diagnostics::warn(
                    "network ETW flush worker creation",
                    crate::diagnostics::io(&error),
                );
                unsafe {
                    let _ = CloseTrace(trace);
                }
                stop_session(session_name);
                return None;
            }
        };
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
                    flusher.shutdown();
                    unsafe {
                        let _ = CloseTrace(trace);
                    }
                    stop_session(session_name);
                    return None;
                }
            };
        log::info!("network ETW source ready: {}", source.name());
        Some(Self {
            session,
            trace,
            totals,
            consumer: Some(consumer),
            rates: RateWindow::default(),
            source,
            next_health: Instant::now() + Duration::from_secs(5),
            lost: (0, 0),
            flusher,
        })
    }

    /// Apply a complete network measurement, retaining it between faster UI
    /// samples. Process identity prevents reuse of a previous PID's reading.
    pub fn apply_rates(&mut self, processes: &mut [crate::process::ProcessRow]) {
        self.flusher.observe_read();
        self.report_health();
        let totals = &self.totals;
        self.rates.sample(
            Instant::now(),
            processes.iter().map(|p| (p.pid, p.create_time)),
            || std::mem::take(&mut *totals.lock().unwrap()),
        );
        for process in processes {
            (process.net_sent, process.net_received) =
                self.rates.value(process.pid, process.create_time);
        }
    }

    /// Raw deltas for a service that maintains non-destructive cumulative
    /// counters for multiple readers. Only the tracker owner drains these.
    pub fn take_bytes(&mut self) -> HashMap<u32, (u64, u64)> {
        self.flusher.observe_read();
        self.report_health();
        std::mem::take(&mut *self.totals.lock().unwrap())
    }

    fn report_health(&mut self) {
        let now = Instant::now();
        if now < self.next_health {
            return;
        }
        self.next_health = now + Duration::from_secs(30);
        let mut properties = Properties::new("");
        let status = unsafe {
            ControlTraceW(
                self.session,
                PCWSTR::null(),
                properties.as_mut(),
                EVENT_TRACE_CONTROL_QUERY,
            )
        };
        if status != ERROR_SUCCESS {
            crate::diagnostics::warn(
                "network ETW session query",
                crate::diagnostics::code(status.0),
            );
            return;
        }
        let properties = unsafe { &*properties.as_mut() };
        let lost = (properties.EventsLost, properties.RealTimeBuffersLost);
        if lost != self.lost {
            crate::diagnostics::warn(
                "network ETW event loss",
                format!(
                    "source={}; events_lost={}; realtime_buffers_lost={}",
                    self.source.name(),
                    lost.0,
                    lost.1,
                ),
            );
            self.lost = lost;
        }
    }

    pub fn is_active(&self) -> bool {
        self.consumer
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }
}

impl Drop for NetworkTracker {
    fn drop(&mut self) {
        self.flusher.shutdown();
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
    fn for_source(session_name: &str, source: Source) -> Option<Self> {
        let mut properties = Self::new(session_name);
        if source == Source::System {
            let guid = unsafe { CoCreateGuid() }
                .inspect_err(|error| {
                    crate::diagnostics::windows_failure("network ETW session identity", error);
                })
                .ok()?;
            let value = unsafe { &mut *properties.as_mut() };
            // A private system logger has its own GUID, separate from the
            // reserved NT Kernel Logger and other profiling sessions.
            value.Wnode.Guid = guid;
            value.LogFileMode |= EVENT_TRACE_SYSTEM_LOGGER_MODE;
            value.EnableFlags = EVENT_TRACE_FLAG_NETWORK_TCPIP;
        }
        Some(properties)
    }

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
        // Slow readers use the native timer; fast readers also flush asynchronously.
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
    if record.is_null() {
        return;
    }
    let r = unsafe { &*record };
    if r.UserDataLength < 8 || r.UserData.is_null() || r.UserContext.is_null() {
        return;
    }
    let Some(sent) = direction(
        r.EventHeader.ProviderId,
        r.EventHeader.EventDescriptor.Id,
        r.EventHeader.EventDescriptor.Opcode,
        r.EventHeader.EventDescriptor.Version,
    ) else {
        return;
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

fn direction(provider: GUID, id: u16, opcode: u8, version: u8) -> Option<bool> {
    if provider == KERNEL_NETWORK {
        match id {
            10 | 26 | 42 | 58 => Some(true),
            11 | 27 | 43 | 59 => Some(false),
            _ => None,
        }
    } else if (provider == TCP_IP || provider == UDP_IP) && version >= 2 {
        match opcode {
            10 | 26 => Some(true),
            11 | 27 => Some(false),
            _ => None,
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_session_uses_an_independent_identity_and_only_network_flags() {
        let mut a = Properties::for_source("Argus test a", Source::System).unwrap();
        let mut b = Properties::for_source("Argus test b", Source::System).unwrap();
        let a = unsafe { &*a.as_mut() };
        let b = unsafe { &*b.as_mut() };
        assert_ne!(a.Wnode.Guid, GUID::default());
        assert_ne!(a.Wnode.Guid, b.Wnode.Guid);
        assert_ne!(
            a.Wnode.Guid,
            GUID::from_u128(0x9e814aad_3204_11d2_9a82_006008a86939)
        );
        assert_eq!(
            a.LogFileMode,
            EVENT_TRACE_REAL_TIME_MODE | EVENT_TRACE_SYSTEM_LOGGER_MODE
        );
        assert_eq!(a.EnableFlags, EVENT_TRACE_FLAG_NETWORK_TCPIP);
        let mut fallback = Properties::for_source("Argus fallback", Source::Manifest).unwrap();
        let fallback = unsafe { &*fallback.as_mut() };
        assert_eq!(fallback.LogFileMode, EVENT_TRACE_REAL_TIME_MODE);
        assert_eq!(fallback.EnableFlags.0, 0);
    }

    #[test]
    fn both_sources_decode_tcp_udp_ipv4_ipv6_without_counting_retransmits() {
        let totals = Totals::default();
        let mut payload = [42u32.to_le_bytes(), 512u32.to_le_bytes()].concat();
        let mut record = EVENT_RECORD {
            UserContext: (&totals as *const Totals).cast_mut().cast(),
            UserData: payload.as_mut_ptr().cast(),
            UserDataLength: payload.len() as u16,
            ..Default::default()
        };
        for (provider, version, ids) in [
            (KERNEL_NETWORK, 0, vec![10, 11, 26, 27, 42, 43, 58, 59]),
            (TCP_IP, 2, vec![10, 11, 26, 27]),
            (UDP_IP, 2, vec![10, 11, 26, 27]),
        ] {
            totals.lock().unwrap().clear();
            for id in &ids {
                record.EventHeader.ProviderId = provider;
                record.EventHeader.EventDescriptor.Version = version;
                record.EventHeader.EventDescriptor.Id =
                    if provider == KERNEL_NETWORK { *id } else { 0 };
                record.EventHeader.EventDescriptor.Opcode = *id as u8;
                unsafe { on_event(&mut record) };
            }
            let expected = (ids.len() / 2) as u64 * 512;
            assert_eq!(totals.lock().unwrap().get(&42), Some(&(expected, expected)));
            for opcode in [12, 13, 14, 15, 18, 28, 29, 30, 34] {
                record.EventHeader.EventDescriptor.Id = opcode;
                record.EventHeader.EventDescriptor.Opcode = opcode as u8;
                unsafe { on_event(&mut record) };
            }
            assert_eq!(totals.lock().unwrap().get(&42), Some(&(expected, expected)));
        }
    }

    #[test]
    fn unrelated_and_incomplete_events_do_not_create_process_counters() {
        assert_eq!(direction(GUID::default(), 10, 10, 2), None);
        assert_eq!(direction(TCP_IP, 0, 10, 1), None);
        let totals = Totals::default();
        let mut payload = [42u32.to_le_bytes(), 512u32.to_le_bytes()].concat();
        let mut record = EVENT_RECORD {
            UserContext: (&totals as *const Totals).cast_mut().cast(),
            UserData: payload.as_mut_ptr().cast(),
            UserDataLength: 7,
            ..Default::default()
        };
        record.EventHeader.ProviderId = TCP_IP;
        record.EventHeader.EventDescriptor.Version = 2;
        record.EventHeader.EventDescriptor.Opcode = 10;
        unsafe { on_event(&mut record) };
        record.UserDataLength = 8;
        record.UserData = std::ptr::null_mut();
        unsafe { on_event(&mut record) };
        record.UserData = payload.as_mut_ptr().cast();
        record.UserContext = std::ptr::null_mut();
        unsafe {
            on_event(&mut record);
            on_event(std::ptr::null_mut());
        }
        assert!(totals.lock().unwrap().is_empty());
    }

    #[test]
    fn fast_reads_keep_bytes_until_the_network_window_and_idle_returns_zero() {
        let mut rates = RateWindow::default();
        let at = Instant::now();
        let ids = [(42, 10)];
        rates.sample(at, ids.into_iter(), HashMap::new);
        rates.sample(at + Duration::from_millis(250), ids.into_iter(), || {
            panic!("early read drained the ETW buffer")
        });
        rates.sample(at + Duration::from_millis(500), ids.into_iter(), || {
            HashMap::from([(42, (1000, 2000))])
        });
        assert_eq!(rates.value(42, 10), (2000, 4000));
        rates.sample(at + Duration::from_millis(750), ids.into_iter(), || {
            panic!("early read drained the ETW buffer")
        });
        assert_eq!(rates.value(42, 10), (2000, 4000));
        rates.sample(at + Duration::from_secs(1), ids.into_iter(), || {
            HashMap::from([(42, (2000, 4000))])
        });
        assert_eq!(rates.value(42, 10), (4000, 8000));
        rates.sample(
            at + Duration::from_millis(1500),
            ids.into_iter(),
            HashMap::new,
        );
        assert_eq!(rates.value(42, 10), (0, 0));
    }

    #[test]
    fn jitter_and_longer_sampling_use_the_actual_elapsed_time() {
        let mut rates = RateWindow::default();
        let at = Instant::now();
        let ids = [(42, 10)];
        rates.sample(at, ids.into_iter(), HashMap::new);
        rates.sample(at + Duration::from_millis(999), ids.into_iter(), || {
            HashMap::from([(42, (999_000, 1_998_000))])
        });
        assert_eq!(rates.value(42, 10), (1_000_000, 2_000_000));
        rates.sample(at + Duration::from_millis(5999), ids.into_iter(), || {
            HashMap::from([(42, (5000, 10000))])
        });
        assert_eq!(rates.value(42, 10), (1000, 2000));
    }

    #[test]
    fn retained_rates_do_not_cross_process_instances() {
        let mut rates = RateWindow::default();
        let at = Instant::now();
        rates.sample(at, [(42, 10)].into_iter(), HashMap::new);
        rates.sample(at + Duration::from_secs(1), [(42, 10)].into_iter(), || {
            HashMap::from([(42, (1000, 2000))])
        });
        assert_eq!(rates.value(42, 10), (1000, 2000));
        assert_eq!(rates.value(42, 20), (0, 0));
        rates.sample(at + Duration::from_secs(2), [(42, 20)].into_iter(), || {
            HashMap::from([(42, (90000, 180000))])
        });
        assert_eq!(rates.value(42, 20), (0, 0));
        rates.sample(at + Duration::from_secs(3), [(42, 20)].into_iter(), || {
            HashMap::from([(42, (1000, 2000))])
        });
        assert_eq!(rates.value(42, 20), (1000, 2000));
        assert_eq!(rates.value(42, 10), (0, 0));
    }
}
