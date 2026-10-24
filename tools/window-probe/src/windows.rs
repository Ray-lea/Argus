//! Only the owned process tree is controlled; shared DWM counters are reported separately.

use anyhow::{Context, Result};
use std::{
    collections::HashSet,
    io::{self, BufRead, Write},
    mem::size_of,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use windows::Win32::System::JobObjects::*;
use windows::{
    Win32::{
        Foundation::{CloseHandle, FILETIME, HWND, LPARAM, RECT},
        System::{
            Diagnostics::ToolHelp::*,
            Performance::*,
            ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
            Threading::*,
            WindowsProgramming::QueryProcessCycleTime,
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled},
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, PCWSTR},
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
/// Read-only inspection includes hidden windows; it never changes focus or visibility.
pub fn inspect(pid: u32) -> Result<()> {
    struct Inspection {
        pid: u32,
        windows: Vec<serde_json::Value>,
    }
    unsafe extern "system" fn enumerate(hwnd: HWND, data: LPARAM) -> BOOL {
        let inspection = unsafe { &mut *(data.0 as *mut Inspection) };
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid != inspection.pid {
            return true.into();
        }
        let mut title = [0u16; 256];
        let mut class = [0u16; 128];
        let nt = unsafe { GetWindowTextW(hwnd, &mut title) } as usize;
        let nc = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
        let mut rect = RECT::default();
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        unsafe {
            let _ = GetWindowRect(hwnd, &mut rect);
            let _ = GetWindowPlacement(hwnd, &mut placement);
        }
        let responsive = unsafe {
            SendMessageTimeoutW(
                hwnd,
                WM_NULL,
                Default::default(),
                Default::default(),
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                100,
                None,
            )
        }
        .0 != 0;
        inspection.windows.push(serde_json::json!({"hwnd":hwnd.0 as usize,"thread":thread,"title":String::from_utf16_lossy(&title[..nt]),"class":String::from_utf16_lossy(&class[..nc]),"visible":unsafe{IsWindowVisible(hwnd).as_bool()},"minimized":unsafe{IsIconic(hwnd).as_bool()},"responsive":responsive,"showCommand":placement.showCmd,"rect":[rect.left,rect.top,rect.right,rect.bottom]}));
        true.into()
    }
    let mut inspection = Inspection {
        pid,
        windows: vec![],
    };
    unsafe {
        EnumWindows(
            Some(enumerate),
            LPARAM((&mut inspection as *mut Inspection) as isize),
        )?;
    }
    println!("{}", serde_json::to_string_pretty(&inspection.windows)?);
    Ok(())
}
fn ticks(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

/// Test normal startup on an inactive desktop, without forcing a window to show.
pub fn startup(args: &[String]) -> Result<()> {
    use windows::Win32::System::StationsAndDesktops::*;
    use windows::core::PWSTR;
    anyhow::ensure!(!args.is_empty(), "startup requires an executable");
    struct Desktop(HDESK);
    impl Drop for Desktop {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseDesktop(self.0);
            }
        }
    }
    struct ThreadDesktop(HDESK);
    impl Drop for ThreadDesktop {
        fn drop(&mut self) {
            unsafe {
                let _ = SetThreadDesktop(self.0);
            }
        }
    }
    struct Process(PROCESS_INFORMATION);
    impl Drop for Process {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0.hThread);
                let _ = CloseHandle(self.0.hProcess);
            }
        }
    }
    let name = format!("ArgusStartup-{}-{}", std::process::id(), crate::unix_ms());
    let desktop = Desktop(unsafe {
        CreateDesktopW(
            PCWSTR(wide(&name).as_ptr()),
            PCWSTR::null(),
            None,
            Default::default(),
            DESKTOP_CREATEWINDOW.0
                | DESKTOP_ENUMERATE.0
                | DESKTOP_READOBJECTS.0
                | DESKTOP_WRITEOBJECTS.0
                | DESKTOP_HOOKCONTROL.0,
            None,
        )?
    });
    let original = unsafe { GetThreadDesktop(GetCurrentThreadId())? };
    let mut desktop_name = wide(&format!("WinSta0\\{name}"));
    let quote = |value: &str| {
        let mut out = String::from("\"");
        let mut slashes = 0;
        for ch in value.chars() {
            if ch == '\\' {
                slashes += 1;
                continue;
            }
            out.extend(std::iter::repeat_n(
                '\\',
                if ch == '"' { slashes * 2 + 1 } else { slashes },
            ));
            slashes = 0;
            out.push(ch);
        }
        out.extend(std::iter::repeat_n('\\', slashes * 2));
        out.push('"');
        out
    };
    let mut command = wide(
        &args
            .iter()
            .map(|arg| quote(arg))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        lpDesktop: PWSTR(desktop_name.as_mut_ptr()),
        ..Default::default()
    };
    let mut information = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(wide(&args[0]).as_ptr()),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW | CREATE_SUSPENDED,
            None,
            PCWSTR::null(),
            &startup,
            &mut information,
        )?;
    }
    let process = Process(information);
    let job = Arc::new(Job(unsafe { CreateJobObjectW(None, PCWSTR::null())? }));
    let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )?;
        if let Err(error) = AssignProcessToJobObject(job.0, process.0.hProcess) {
            let _ = TerminateProcess(process.0.hProcess, 1);
            return Err(error.into());
        }
    }
    let monitor = ForegroundMonitor::start(process.0.dwProcessId, job.clone());
    let restore = ThreadDesktop(original);
    unsafe {
        SetThreadDesktop(desktop.0)?;
        ResumeThread(process.0.hThread);
    }
    let started = Instant::now();
    let mut last = None;
    let mut windows = Vec::new();
    loop {
        if unsafe { WaitForSingleObject(process.0.hProcess, 50) }
            == windows::Win32::Foundation::WAIT_OBJECT_0
        {
            break;
        }
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(120),
            "isolated startup timeout"
        );
        anyhow::ensure!(
            !monitor.violated.load(Ordering::Relaxed),
            "test took foreground"
        );
        for pid in job.pids()? {
            let mut search = Search {
                pid,
                hwnd: HWND::default(),
            };
            unsafe {
                let _ = EnumWindows(Some(find), LPARAM((&mut search as *mut Search) as isize));
            }
            if search.hwnd.0.is_null() {
                continue;
            }
            let visible = unsafe { IsWindowVisible(search.hwnd).as_bool() };
            let state = (pid, visible);
            if last != Some(state) {
                let event = serde_json::json!({"pid":pid,"visible":visible,"atMs":started.elapsed().as_millis()});
                println!("{event}");
                windows.push(event);
                last = Some(state);
            }
        }
    }
    let mut code = 0;
    unsafe {
        GetExitCodeProcess(process.0.hProcess, &mut code)?;
    }
    let violated = monitor.violated.load(Ordering::Relaxed);
    drop(restore);
    drop(monitor);
    println!(
        "{}",
        serde_json::json!({"event":"startup-result","exitCode":code,"foregroundViolations":usize::from(violated),"windows":windows})
    );
    anyhow::ensure!(
        code == 0 && !violated,
        "isolated startup did not exit cleanly"
    );
    Ok(())
}

struct Job(windows::Win32::Foundation::HANDLE);
// Kernel job handles support concurrent calls; Arc keeps the handle alive.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}
impl Job {
    fn pids(&self) -> Result<HashSet<u32>> {
        let mut buffer = vec![0usize; 1025];
        unsafe {
            QueryInformationJobObject(
                Some(self.0),
                JobObjectBasicProcessIdList,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * size_of::<usize>()) as u32,
                None,
            )?;
        }
        let count = unsafe {
            (&*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>()).NumberOfProcessIdsInList
                as usize
        };
        anyhow::ensure!(count < buffer.len(), "job process list overflow");
        Ok(buffer[1..=count].iter().map(|pid| *pid as u32).collect())
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct ForegroundMonitor {
    stop: Arc<AtomicBool>,
    violated: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl ForegroundMonitor {
    fn start(root: u32, job: Arc<Job>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let violated = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let failed = violated.clone();
        let thread = std::thread::spawn(move || {
            let mut previous = HWND::default();
            while !stop_thread.load(Ordering::Relaxed) {
                let tick = Instant::now();
                let hwnd = unsafe { GetForegroundWindow() };
                if hwnd != previous {
                    let mut pid = 0;
                    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
                    if pid == root || job.pids().is_ok_and(|pids| pids.contains(&pid)) {
                        failed.store(true, Ordering::Relaxed);
                        println!(
                            "{}",
                            serde_json::json!({"event":"focusViolation","pid":pid})
                        );
                        unsafe {
                            let _ = TerminateJobObject(job.0, 1);
                        }
                        break;
                    }
                    previous = hwnd;
                }
                std::thread::sleep(Duration::from_millis(50).saturating_sub(tick.elapsed()));
            }
        });
        Self {
            stop,
            violated,
            thread: Some(thread),
        }
    }
}
impl Drop for ForegroundMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Search {
    pid: u32,
    hwnd: HWND,
}
unsafe extern "system" fn find(hwnd: HWND, data: LPARAM) -> BOOL {
    let search = unsafe { &mut *(data.0 as *mut Search) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != search.pid {
        return true.into();
    }
    let mut title = [0u16; 128];
    let mut class = [0u16; 64];
    let nt = unsafe { GetWindowTextW(hwnd, &mut title) } as usize;
    let nc = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
    static METADATA: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let metadata = METADATA.get_or_init(|| {
        serde_json::from_str(include_str!("../../../resources/app.json"))
            .expect("application metadata")
    });
    if String::from_utf16_lossy(&title[..nt])
        .starts_with(metadata["productName"].as_str().expect("product name"))
        && String::from_utf16_lossy(&class[..nc]) == "Zed::Window"
    {
        search.hwnd = hwnd;
        return false.into();
    }
    true.into()
}

struct Gpu {
    query: PDH_HQUERY,
    counters: Vec<(&'static str, PDH_HCOUNTER)>,
    dwm: HashSet<u32>,
}
impl Gpu {
    fn new() -> Result<Self> {
        let mut query = Default::default();
        let rc = unsafe { PdhOpenQueryW(None, 0, &mut query) };
        anyhow::ensure!(rc == 0, "PDH open failed: {rc:#x}");
        let mut counters = Vec::new();
        for (name, path) in [
            ("gpuTime100ns", r"\GPU Engine(*)\Running Time"),
            ("gpuPercent", r"\GPU Engine(*)\Utilization Percentage"),
            ("gpuDedicated", r"\GPU Process Memory(*)\Dedicated Usage"),
            ("gpuShared", r"\GPU Process Memory(*)\Shared Usage"),
        ] {
            let mut counter = Default::default();
            let path = wide(path);
            let rc =
                unsafe { PdhAddEnglishCounterW(query, PCWSTR(path.as_ptr()), 0, &mut counter) };
            anyhow::ensure!(rc == 0, "PDH counter {name} failed: {rc:#x}");
            counters.push((name, counter));
        }
        unsafe { PdhCollectQueryData(query) };
        let mut dwm = HashSet::new();
        unsafe {
            if let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut entry = PROCESSENTRY32W {
                    dwSize: size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                if Process32FirstW(snapshot, &mut entry).is_ok() {
                    loop {
                        let length = entry
                            .szExeFile
                            .iter()
                            .position(|c| *c == 0)
                            .unwrap_or(entry.szExeFile.len());
                        if String::from_utf16_lossy(&entry.szExeFile[..length])
                            .eq_ignore_ascii_case("dwm.exe")
                        {
                            dwm.insert(entry.th32ProcessID);
                        }
                        if Process32NextW(snapshot, &mut entry).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snapshot);
            }
        }
        Ok(Self {
            query,
            counters,
            dwm,
        })
    }
    fn sample(&self, pids: &HashSet<u32>) -> serde_json::Value {
        let tracked: HashSet<_> = pids.union(&self.dwm).copied().collect();
        let pids = &tracked;
        let rc = unsafe { PdhCollectQueryData(self.query) };
        let mut result = serde_json::Map::new();
        result.insert("status".into(), rc.into());
        for (name, counter) in &self.counters {
            if *name == "gpuPercent" {
                let (mut bytes, mut count) = (0, 0);
                unsafe {
                    PdhGetFormattedCounterArrayW(
                        *counter,
                        PDH_FMT_DOUBLE,
                        &mut bytes,
                        &mut count,
                        None,
                    )
                };
                let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
                let rc = unsafe {
                    PdhGetFormattedCounterArrayW(
                        *counter,
                        PDH_FMT_DOUBLE,
                        &mut bytes,
                        &mut count,
                        Some(buffer.as_mut_ptr().cast()),
                    )
                };
                let mut entries = serde_json::Map::new();
                if rc == 0 {
                    for item in unsafe {
                        std::slice::from_raw_parts(
                            buffer.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
                            count as usize,
                        )
                    } {
                        let key = unsafe { item.szName.to_string() }.unwrap_or_default();
                        let pid = key
                            .strip_prefix("pid_")
                            .and_then(|s| s.split('_').next())
                            .and_then(|s| s.parse::<u32>().ok());
                        if pid.is_some_and(|p| pids.contains(&p)) && item.FmtValue.CStatus <= 1 {
                            entries.insert(
                                key,
                                serde_json::json!(unsafe { item.FmtValue.Anonymous.doubleValue }),
                            );
                        }
                    }
                }
                result.insert((*name).into(), entries.into());
                continue;
            }
            let (mut bytes, mut count) = (0, 0);
            unsafe { PdhGetRawCounterArrayW(*counter, &mut bytes, &mut count, None) };
            if bytes == 0 {
                result.insert((*name).into(), serde_json::Value::Null);
                continue;
            }
            let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
            let rc = unsafe {
                PdhGetRawCounterArrayW(
                    *counter,
                    &mut bytes,
                    &mut count,
                    Some(buffer.as_mut_ptr().cast()),
                )
            };
            if rc != 0 {
                result.insert((*name).into(), serde_json::json!({"error":rc}));
                continue;
            }
            let mut entries = serde_json::Map::new();
            for item in unsafe {
                std::slice::from_raw_parts(
                    buffer.as_ptr().cast::<PDH_RAW_COUNTER_ITEM_W>(),
                    count as usize,
                )
            } {
                let key = unsafe { item.szName.to_string() }.unwrap_or_default();
                let pid = key
                    .strip_prefix("pid_")
                    .and_then(|s| s.split('_').next())
                    .and_then(|s| s.parse::<u32>().ok());
                if pid.is_some_and(|p| pids.contains(&p)) && item.RawValue.CStatus <= 1 {
                    entries.insert(key, item.RawValue.FirstValue.into());
                }
            }
            result.insert((*name).into(), entries.into());
        }
        let mut compositor = serde_json::Map::new();
        compositor.insert("pids".into(), serde_json::json!(self.dwm));
        for (name, _) in &self.counters {
            if let Some(entries) = result
                .get_mut(*name)
                .and_then(|value| value.as_object_mut())
            {
                let mut separate = serde_json::Map::new();
                entries.retain(|key, value| {
                    let pid = key
                        .strip_prefix("pid_")
                        .and_then(|s| s.split('_').next())
                        .and_then(|s| s.parse::<u32>().ok());
                    if pid.is_some_and(|pid| self.dwm.contains(&pid)) {
                        separate.insert(key.clone(), value.clone());
                        false
                    } else {
                        true
                    }
                });
                compositor.insert((*name).into(), separate.into());
            }
        }
        result.insert("dwm".into(), compositor.into());
        result.into()
    }
}
impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.query) };
    }
}

fn sample(job: &Job, gpu: Option<&Gpu>) -> Result<serde_json::Value> {
    let pids = job.pids()?;
    let mut processes = Vec::new();
    for &pid in &pids {
        unsafe {
            let Ok(handle) = OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                false,
                pid,
            ) else {
                continue;
            };
            let mut memory = PROCESS_MEMORY_COUNTERS_EX {
                cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                ..Default::default()
            };
            let mut created = FILETIME::default();
            let mut exited = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            let time_ok =
                GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user).is_ok();
            let mut cycles = 0;
            let cycles = QueryProcessCycleTime(handle, &mut cycles)
                .is_ok()
                .then_some(cycles);
            let mem_ok = GetProcessMemoryInfo(
                handle,
                (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            )
            .is_ok();
            let _ = CloseHandle(handle);
            if time_ok && mem_ok {
                processes.push(serde_json::json!({"pid":pid,"created":ticks(created),"cpuMs":(ticks(kernel)+ticks(user)) as f64/10000.,"cpuCycles":cycles,"privateBytes":memory.PrivateUsage,"workingSet":memory.WorkingSetSize}));
            }
        }
    }
    Ok(serde_json::json!({"processes":processes,"gpu":gpu.map(|gpu|gpu.sample(&pids))}))
}

pub fn run(args: &[String]) -> Result<()> {
    anyhow::ensure!(
        args.len() >= 2 && args.iter().any(|a| a == "--hidden"),
        "owned test must start hidden"
    );
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let gpu = Gpu::new()?;
    let job = Arc::new(Job(unsafe { CreateJobObjectW(None, None)? }));
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )?;
    }
    let mut child = Command::new(&args[0])
        .args(&args[1..])
        .creation_flags(0x08000000)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("launch owned test")?;
    let root = child.id();
    let start = Instant::now();
    use std::os::windows::io::AsRawHandle;
    if let Err(error) = unsafe {
        AssignProcessToJobObject(
            job.0,
            windows::Win32::Foundation::HANDLE(child.as_raw_handle()),
        )
    } {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }
    let monitor = ForegroundMonitor::start(root, job.clone());
    println!(
        "{}",
        serde_json::json!({"event":"started","pid":root,"unixMs":crate::unix_ms()})
    );
    #[cfg(feature = "capture")]
    {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
        let mut adapters = Vec::new();
        let mut index = 0;
        while let Ok(adapter) = unsafe { factory.EnumAdapters1(index) } {
            let desc = unsafe { adapter.GetDesc1()? };
            let len = desc
                .Description
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(desc.Description.len());
            adapters.push(serde_json::json!({"name":String::from_utf16_lossy(&desc.Description[..len]),"luid":format!("0x{:08X}_0x{:08X}",desc.AdapterLuid.HighPart as u32,desc.AdapterLuid.LowPart),"dedicatedBytes":desc.DedicatedVideoMemory}));
            index += 1;
        }
        println!(
            "{}",
            serde_json::json!({"event":"hardware","adapters":adapters})
        );
    }
    io::stdout().flush()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in io::stdin()
            .lock()
            .lines()
            .map_while(std::result::Result::ok)
        {
            if tx.send(line).is_err() {
                break;
            }
        }
        let _ = tx.send("stop".to_owned());
    });
    let mut search = Search {
        pid: root,
        hwnd: HWND::default(),
    };
    let mut measured = Instant::now();
    let mut closing = None;
    #[cfg(target_arch = "x86_64")]
    let mut stack_sampler = None;
    #[cfg(feature = "capture")]
    let mut observer: Option<crate::frames::Observer> = None;
    loop {
        anyhow::ensure!(
            !monitor.violated.load(Ordering::Relaxed),
            "owned test became foreground; result rejected"
        );
        if let Some(status) = child.try_wait()? {
            anyhow::ensure!(
                status.success() && !monitor.violated.load(Ordering::Relaxed),
                "owned test exited unsuccessfully: {status}"
            );
            println!(
                "{}",
                serde_json::json!({"event":"exited","code":status.code(),"focusViolations":0})
            );
            break;
        }
        unsafe {
            if search.hwnd.0.is_null() {
                let _ = EnumWindows(Some(find), LPARAM((&mut search as *mut Search) as isize));
                if search.hwnd.0.is_null() {
                    for pid in job.pids()? {
                        search.pid = pid;
                        let _ =
                            EnumWindows(Some(find), LPARAM((&mut search as *mut Search) as isize));
                        if !search.hwnd.0.is_null() {
                            break;
                        }
                    }
                }
                if !search.hwnd.0.is_null() {
                    #[cfg(target_arch = "x86_64")]
                    if let Ok(path) = std::env::var("ARGUS_PROTOTYPE_STACK_FILE") {
                        let thread = GetWindowThreadProcessId(search.hwnd, None);
                        stack_sampler =
                            Some(crate::stacks::Sampler::start(search.pid, thread, path));
                    }
                    // Display-only runs must not consume clicks on the user's desktop.
                    let style = GetWindowLongPtrW(search.hwnd, GWL_EXSTYLE);
                    SetWindowLongPtrW(
                        search.hwnd,
                        GWL_EXSTYLE,
                        (style | WS_EX_NOACTIVATE.0 as isize) & !(WS_EX_APPWINDOW.0 as isize),
                    );
                    let _ = EnableWindow(search.hwnd, false);
                    anyhow::ensure!(
                        GetWindowLongPtrW(search.hwnd, GWL_EXSTYLE) & WS_EX_NOACTIVATE.0 as isize
                            != 0
                            && !IsWindowEnabled(search.hwnd).as_bool(),
                        "test window input isolation failed"
                    );
                    SetWindowPos(
                        search.hwnd,
                        Some(HWND_BOTTOM),
                        0,
                        0,
                        0,
                        0,
                        SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                    )?;
                    let mut rect = RECT::default();
                    GetClientRect(search.hwnd, &mut rect)?;
                    println!(
                        "{}",
                        serde_json::json!({"event":"shown","pid":search.pid,"hwnd":search.hwnd.0 as usize,"width":rect.right,"height":rect.bottom,"atMs":start.elapsed().as_millis()})
                    );
                }
            }
            while let Ok(command) = rx.try_recv() {
                if let Some(tag) = command.strip_prefix("mark ") {
                    let mut value = sample(&job, Some(&gpu))?;
                    value["event"] = "mark".into();
                    value["tag"] = tag.into();
                    value["atMs"] = serde_json::json!(start.elapsed().as_secs_f64() * 1000.);
                    println!("{value}");
                    io::stdout().flush()?;
                    continue;
                }
                if let Some(json) = command.strip_prefix("app ") {
                    if let Some(input) = child.stdin.as_mut() {
                        writeln!(input, "{json}")?;
                        input.flush()?;
                    }
                    continue;
                }
                if search.hwnd.0.is_null() {
                    continue;
                }
                if command == "stop" {
                    let _ = PostMessageW(
                        Some(search.hwnd),
                        WM_CLOSE,
                        Default::default(),
                        Default::default(),
                    );
                    closing = Some(Instant::now());
                }
                #[cfg(feature = "capture")]
                if command == "watch" {
                    observer = Some(crate::frames::Observer::start(search.hwnd));
                }
                if command == "begin-move" || command == "end-move" {
                    let message = if command == "begin-move" {
                        WM_ENTERSIZEMOVE
                    } else {
                        WM_EXITSIZEMOVE
                    };
                    let _ = SendMessageTimeoutW(
                        search.hwnd,
                        message,
                        windows::Win32::Foundation::WPARAM(0),
                        LPARAM(0),
                        SMTO_ABORTIFHUNG,
                        1000,
                        None,
                    );
                    println!(
                        "{}",
                        serde_json::json!({"event":command,"atMs":start.elapsed().as_secs_f64()*1000.})
                    );
                }
                if command == "hide" {
                    let _ = ShowWindow(search.hwnd, SW_HIDE);
                }
                if command == "show" {
                    SetWindowPos(
                        search.hwnd,
                        Some(HWND_BOTTOM),
                        0,
                        0,
                        0,
                        0,
                        SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                    )?;
                }
                if let Some(value) = command.strip_prefix("size ") {
                    let size: Vec<i32> = value
                        .split_whitespace()
                        .map(str::parse)
                        .collect::<std::result::Result<_, _>>()?;
                    let mut client = RECT::default();
                    let mut outer = RECT::default();
                    GetClientRect(search.hwnd, &mut client)?;
                    GetWindowRect(search.hwnd, &mut outer)?;
                    SetWindowPos(
                        search.hwnd,
                        Some(HWND_BOTTOM),
                        0,
                        0,
                        size[0] + outer.right - outer.left - client.right,
                        size[1] + outer.bottom - outer.top - client.bottom,
                        SWP_NOACTIVATE | SWP_NOMOVE,
                    )?;
                    GetClientRect(search.hwnd, &mut client)?;
                    println!(
                        "{}",
                        serde_json::json!({"event":"resized","width":client.right,"height":client.bottom})
                    );
                }
                if let Some(value) = command.strip_prefix("move ") {
                    let delta: Vec<i32> = value
                        .split_whitespace()
                        .map(str::parse)
                        .collect::<std::result::Result<_, _>>()?;
                    anyhow::ensure!(
                        delta.len() == 2 && delta.iter().all(|d| d.abs() <= 32),
                        "bounded owned-window movement required"
                    );
                    let mut rect = RECT::default();
                    GetWindowRect(search.hwnd, &mut rect)?;
                    SetWindowPos(
                        search.hwnd,
                        Some(HWND_BOTTOM),
                        rect.left + delta[0],
                        rect.top + delta[1],
                        0,
                        0,
                        SWP_NOACTIVATE | SWP_NOSIZE,
                    )?;
                }
            }
        }
        if measured.elapsed() >= Duration::from_millis(500) {
            let mut value = sample(&job, None)?;
            #[cfg(feature = "capture")]
            if let Some(observer) = &observer {
                value["capture"] = observer.sample();
            }
            value["atMs"] = serde_json::json!(start.elapsed().as_secs_f64() * 1000.);
            value["event"] = "sample".into();
            println!("{value}");
            io::stdout().flush()?;
            measured = Instant::now();
        }
        if closing.is_some_and(|at: Instant| at.elapsed() > Duration::from_secs(8)) {
            child.kill()?;
            child.wait()?;
            break;
        }
        if start.elapsed() > Duration::from_secs(600) {
            child.kill()?;
            child.wait()?;
            anyhow::bail!("owned test timeout");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    #[cfg(target_arch = "x86_64")]
    drop(stack_sampler);
    Ok(())
}

#[cfg(feature = "capture")]
pub fn capture(pid: u32, path: &str) -> Result<()> {
    use windows::{
        Graphics::{
            Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
            DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        },
        Win32::{
            Graphics::{
                Direct3D::{D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0},
                Direct3D11::*,
                Dxgi::IDXGIDevice,
            },
            System::WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
                RO_INIT_MULTITHREADED, RoInitialize,
            },
        },
        core::Interface,
    };
    let mut search = Search {
        pid,
        hwnd: HWND::default(),
    };
    unsafe {
        let _ = EnumWindows(Some(find), LPARAM((&mut search as *mut Search) as isize));
    }
    anyhow::ensure!(!search.hwnd.0.is_null(), "owned Argus window missing");
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED)?;
    }
    let (mut device, mut context) = (None, None);
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device = device.unwrap();
    let context = context.unwrap();
    let dxgi: IDXGIDevice = device.cast()?;
    let runtime: IDirect3DDevice =
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
    let factory: IGraphicsCaptureItemInterop =
        windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item: GraphicsCaptureItem = unsafe { factory.CreateForWindow(search.hwnd)? };
    let size = item.Size()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &runtime,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        size,
    )?;
    let session = pool.CreateCaptureSession(&item)?;
    session.SetIsCursorCaptureEnabled(false)?;
    session.StartCapture()?;
    let start = Instant::now();
    let mut image = None;
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(frame) = pool.TryGetNextFrame() {
            let size = frame.ContentSize()?;
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { texture.GetDesc(&mut desc) };
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging))? };
            let staging = staging.unwrap();
            unsafe { context.CopyResource(&staging, &texture) };
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
            let (w, h) = (size.Width as usize, size.Height as usize);
            let stride = mapped.RowPitch as usize;
            let bytes =
                unsafe { std::slice::from_raw_parts(mapped.pData as *const u8, stride * h) };
            let mut rgb = Vec::with_capacity(w * h * 3);
            for y in 0..h {
                for x in 0..w {
                    let i = y * stride + x * 4;
                    rgb.extend_from_slice(&[bytes[i + 2], bytes[i + 1], bytes[i]]);
                }
            }
            unsafe { context.Unmap(&staging, 0) };
            frame.Close()?;
            image = Some((w, h, rgb));
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    session.Close()?;
    pool.Close()?;
    let (w, h, data) = image.context("window capture timed out")?;
    let mut encoder = png::Encoder::new(std::fs::File::create(path)?, w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&data)?;
    println!("{}", serde_json::json!({"width":w,"height":h,"path":path}));
    Ok(())
}
