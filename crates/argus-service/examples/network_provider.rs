//! Bounded comparison of Windows network ETW sources. Records event metadata,
//! counts and this probe's loopback bytes, without recording packet payloads.
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use windows::Win32::Foundation::{ERROR_SUCCESS, GetLastError};
use windows::Win32::System::Com::CoCreateGuid;
use windows::Win32::System::Diagnostics::Etw::*;
use windows::core::{GUID, PCWSTR, PWSTR};

const MANIFEST: GUID = GUID::from_u128(0x7dd42a49_5329_4832_8dfd_43d979153a88);
const TCP: GUID = GUID::from_u128(0x9a280ac0_c8e0_11d1_84e2_00c04fb998a2);
const UDP: GUID = GUID::from_u128(0xbf3a50c5_a9c9_4988_a005_2df0b7c80f80);
const INVALID: u64 = u64::MAX;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Metadata {
    provider: u128,
    id: u16,
    version: u8,
    opcode: u8,
    level: u8,
    keywords: u64,
    task: u16,
    length: u16,
    flags: u16,
}

#[derive(Default)]
struct Received {
    events: u64,
    network_events: u64,
    own_sent: u64,
    own_received: u64,
    metadata: BTreeMap<Metadata, u64>,
}

struct Context {
    pid: u32,
    received: Mutex<Received>,
}

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    if record.is_null() {
        return;
    }
    let event = unsafe { &*record };
    if event.UserContext.is_null() {
        return;
    }
    let context = unsafe { &*(event.UserContext as *const Context) };
    let descriptor = event.EventHeader.EventDescriptor;
    let key = Metadata {
        provider: event.EventHeader.ProviderId.to_u128(),
        id: descriptor.Id,
        version: descriptor.Version,
        opcode: descriptor.Opcode,
        level: descriptor.Level,
        keywords: descriptor.Keyword,
        task: descriptor.Task,
        length: event.UserDataLength,
        flags: event.EventHeader.Flags,
    };
    let Ok(mut received) = context.received.lock() else {
        return;
    };
    received.events += 1;
    if received.metadata.len() < 64 || received.metadata.contains_key(&key) {
        *received.metadata.entry(key).or_default() += 1;
    }
    let direction = if event.EventHeader.ProviderId == MANIFEST {
        match descriptor.Id {
            10 | 26 | 42 | 58 => Some(true),
            11 | 27 | 43 | 59 => Some(false),
            _ => None,
        }
    } else if event.EventHeader.ProviderId == TCP || event.EventHeader.ProviderId == UDP {
        match descriptor.Opcode {
            10 | 26 => Some(true),
            11 | 27 => Some(false),
            _ => None,
        }
    } else {
        None
    };
    let Some(sent) = direction else { return };
    received.network_events += 1;
    if event.UserDataLength < 8 || event.UserData.is_null() {
        return;
    }
    let data = unsafe { std::slice::from_raw_parts(event.UserData.cast::<u8>(), 8) };
    let pid = u32::from_le_bytes(data[..4].try_into().unwrap());
    if pid != context.pid {
        return;
    }
    let bytes = u64::from(u32::from_le_bytes(data[4..].try_into().unwrap()));
    if sent {
        received.own_sent += bytes
    } else {
        received.own_received += bytes
    }
}

struct Properties(Vec<u64>);
impl Properties {
    fn new() -> Self {
        let bytes = size_of::<EVENT_TRACE_PROPERTIES>() + 8192;
        let mut value = Self(vec![0; bytes.div_ceil(8)]);
        let p = value.get();
        p.Wnode.BufferSize = bytes as u32;
        p.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        p.Wnode.ClientContext = 1;
        p.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        p.FlushTimer = 1;
        p.BufferSize = 64;
        p.MinimumBuffers = 4;
        p.MaximumBuffers = 32;
        p.LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        value
    }
    fn get(&mut self) -> &mut EVENT_TRACE_PROPERTIES {
        unsafe { &mut *self.0.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>() }
    }
}

struct Session {
    control: CONTROLTRACE_HANDLE,
    trace: PROCESSTRACE_HANDLE,
    thread: Option<JoinHandle<u32>>,
    context: Option<Arc<Context>>,
}
impl Session {
    fn stop(&mut self) -> u32 {
        if self.control.Value != 0 {
            unsafe {
                let _ = ControlTraceW(
                    self.control,
                    PCWSTR::null(),
                    Properties::new().get(),
                    EVENT_TRACE_CONTROL_STOP,
                );
            }
            self.control.Value = 0;
        }
        if self.trace.Value != INVALID {
            unsafe {
                let _ = CloseTrace(self.trace);
            }
            self.trace.Value = INVALID;
        }
        self.thread
            .take()
            .map_or(0, |thread| thread.join().unwrap_or(INVALID as u32))
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

fn traffic() -> Result<(u64, u64)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let reader = std::thread::spawn(move || -> io::Result<u64> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut buffer = [0; 8192];
        let mut bytes = 0;
        loop {
            let count = stream.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
        }
        Ok(bytes)
    });
    let mut stream = TcpStream::connect(address)?;
    stream.set_nodelay(true)?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let begin = Instant::now();
    let mut bytes = 0;
    while begin.elapsed() < Duration::from_secs(6) {
        stream.write_all(&[0; 2048])?;
        bytes += 2048;
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(stream);
    Ok((bytes, reader.join().unwrap()?))
}

fn measure(mode: &str) -> Result<Value> {
    let mut properties = Properties::new();
    if mode == "system-network" {
        properties.get().LogFileMode |= EVENT_TRACE_SYSTEM_LOGGER_MODE;
        properties.get().Wnode.Guid = unsafe { CoCreateGuid()? };
        properties.get().EnableFlags = EVENT_TRACE_FLAG_NETWORK_TCPIP;
    }
    let mut name: Vec<u16> = format!("Argus ETW Provider Test {} {mode}", std::process::id())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut session = Session {
        control: CONTROLTRACE_HANDLE::default(),
        trace: PROCESSTRACE_HANDLE { Value: INVALID },
        thread: None,
        context: None,
    };
    let status = unsafe {
        StartTraceW(
            &mut session.control,
            PCWSTR(name.as_ptr()),
            properties.get(),
        )
    };
    if status != ERROR_SUCCESS {
        session.control.Value = 0;
        return Ok(json!({"mode":mode,"stage":"StartTraceW","code":status.0}));
    }
    let mut enabled = 0;
    if mode != "system-network" {
        let all = mode == "manifest-all";
        let status = unsafe {
            EnableTraceEx2(
                session.control,
                &MANIFEST,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
                if all { 5 } else { 4 },
                if all { u64::MAX } else { 0x30 },
                0,
                if mode == "manifest-original" { 0 } else { 5000 },
                None,
            )
        };
        enabled = status.0;
        if status != ERROR_SUCCESS {
            return Ok(json!({"mode":mode,"stage":"EnableTraceEx2","code":status.0}));
        }
    }
    let context = Arc::new(Context {
        pid: std::process::id(),
        received: Mutex::default(),
    });
    session.context = Some(context.clone());
    let mut logfile = EVENT_TRACE_LOGFILEW {
        LoggerName: PWSTR(name.as_mut_ptr()),
        Context: Arc::as_ptr(&context) as *mut c_void,
        ..Default::default()
    };
    logfile.Anonymous1.ProcessTraceMode =
        PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
    logfile.Anonymous2.EventRecordCallback = Some(on_event);
    session.trace = unsafe { OpenTraceW(&mut logfile) };
    if session.trace.Value == INVALID {
        let code = unsafe { GetLastError() }.0;
        return Ok(json!({"mode":mode,"stage":"OpenTraceW","code":code}));
    }
    let trace = session.trace.Value;
    session.thread = Some(std::thread::spawn(move || unsafe {
        ProcessTrace(&[PROCESSTRACE_HANDLE { Value: trace }], None, None).0
    }));
    std::thread::sleep(Duration::from_millis(300));
    let (written, read) = traffic()?;
    std::thread::sleep(Duration::from_millis(1500));
    let query = unsafe {
        ControlTraceW(
            session.control,
            PCWSTR::null(),
            properties.get(),
            EVENT_TRACE_CONTROL_QUERY,
        )
    };
    let p = properties.get();
    let statistics = json!({"queryCode":query.0,"buffersWritten":p.BuffersWritten,"eventsLost":p.EventsLost,"realtimeBuffersLost":p.RealTimeBuffersLost,"bufferCount":p.NumberOfBuffers,"mode":p.LogFileMode,"flags":p.EnableFlags.0});
    let consumer = session.stop();
    let received = context.received.lock().unwrap();
    let metadata: Vec<_> = received.metadata.iter().map(|(m,count)| json!({"provider":format!("{:?}",GUID::from_u128(m.provider)),"id":m.id,"version":m.version,"opcode":m.opcode,"level":m.level,"keywords":m.keywords,"task":m.task,"length":m.length,"flags":m.flags,"count":count})).collect();
    Ok(
        json!({"mode":mode,"enableCode":enabled,"consumerCode":consumer,"written":written,"read":read,"events":received.events,"networkEvents":received.network_events,"ownSent":received.own_sent,"ownReceived":received.own_received,"statistics":statistics,"metadata":metadata}),
    )
}

fn run() -> Result<()> {
    let root = std::env::current_exe()?.parent().unwrap().to_path_buf();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("network-provider.log"))?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let out = root.join(format!("network-provider-{stamp}.json"));
    println!("Network provider comparison; results: {}", out.display());
    writeln!(
        log,
        "START {stamp}; elevated={}",
        argus_core::privilege::is_elevated()
    )?;
    log.flush()?;
    let mut results = Vec::new();
    for mode in [
        "manifest-original",
        "manifest-sync",
        "manifest-all",
        "system-network",
    ] {
        println!("Running {mode}");
        let result = measure(mode)?;
        println!("{result}");
        writeln!(log, "{result}")?;
        log.flush()?;
        results.push(result);
        fs::write(&out, serde_json::to_vec_pretty(&results)?)?;
    }
    println!("COMPLETE: {}", out.display());
    writeln!(log, "COMPLETE: {}", out.display())?;
    Ok(())
}

fn main() -> Result<()> {
    let result = run();
    if let Err(error) = &result {
        eprintln!("Provider comparison stopped: {error}");
    }
    if io::stdin().is_terminal() && !std::env::args().any(|arg| arg == "--no-wait") {
        println!("Press Enter to close.");
        let _ = io::stdin().read_line(&mut String::new());
    }
    result
}
