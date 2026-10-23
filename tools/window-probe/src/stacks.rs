//! Opt-in stack samples of the owned GPUI process. No locals or raw memory are saved.
use std::{
    io::Write,
    mem::size_of,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        System::{
            Diagnostics::{Debug::*, ToolHelp::*},
            ProcessStatus::GetModuleBaseNameW,
            Threading::*,
        },
    },
    core::*,
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct Resume(HANDLE);
impl Drop for Resume {
    fn drop(&mut self) {
        unsafe {
            ResumeThread(self.0);
        }
    }
}

pub struct Sampler {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Sampler {
    pub fn start(pid: u32, main: u32, path: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            if let Err(error) = sample(pid, main, &path, &stopping) {
                eprintln!("owned stack sampling failed: {error:#}");
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

unsafe extern "system" fn unwind_table(process: HANDLE, address: u64) -> *mut std::ffi::c_void {
    unsafe { SymFunctionTableAccess64(process, address) }
}
unsafe extern "system" fn module_base(process: HANDLE, address: u64) -> u64 {
    unsafe { SymGetModuleBase64(process, address) }
}

fn sample(pid: u32, main: u32, path: &str, stop: &AtomicBool) -> anyhow::Result<()> {
    let process =
        Handle(unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)? });
    unsafe {
        SymSetOptions(
            SYMOPT_DEFERRED_LOADS
                | SYMOPT_UNDNAME
                | SYMOPT_FAIL_CRITICAL_ERRORS
                | SYMOPT_NO_PROMPTS,
        );
        SymInitializeW(process.0, w!(""), true)?;
    }
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)? });
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut threads = Vec::new();
    if unsafe { Thread32First(snapshot.0, &mut entry) }.is_ok() {
        loop {
            if entry.th32OwnerProcessID == pid
                && let Ok(thread) = unsafe {
                    OpenThread(
                        THREAD_GET_CONTEXT | THREAD_SUSPEND_RESUME | THREAD_QUERY_INFORMATION,
                        false,
                        entry.th32ThreadID,
                    )
                }
            {
                let handle = Handle(thread);
                let description = unsafe { GetThreadDescription(handle.0) };
                let name = description
                    .as_ref()
                    .ok()
                    .and_then(|v| unsafe { v.to_string() }.ok())
                    .unwrap_or_default();
                if let Ok(description) = description {
                    unsafe {
                        let _ = LocalFree(Some(HLOCAL(description.0.cast())));
                    }
                }
                if entry.th32ThreadID == main
                    || name == "prototype-frames"
                    || name == "VSyncProvider"
                {
                    threads.push((
                        entry.th32ThreadID,
                        if entry.th32ThreadID == main {
                            "main".to_owned()
                        } else {
                            name
                        },
                        handle,
                    ));
                }
            }
            if unsafe { Thread32Next(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    let start = Instant::now();
    while !stop.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(40) {
        let tick = Instant::now();
        for (tid, name, thread) in &threads {
            let at = crate::unix_ms();
            let pause = Instant::now();
            if unsafe { SuspendThread(thread.0) } == u32::MAX {
                continue;
            }
            let resume = Resume(thread.0);
            let mut context = CONTEXT {
                ContextFlags: CONTEXT_FULL_AMD64,
                ..Default::default()
            };
            let mut addresses = Vec::new();
            if unsafe { GetThreadContext(thread.0, &mut context) }.is_ok() {
                let mut frame = STACKFRAME64 {
                    AddrPC: ADDRESS64 {
                        Offset: context.Rip,
                        Mode: AddrModeFlat,
                        ..Default::default()
                    },
                    AddrStack: ADDRESS64 {
                        Offset: context.Rsp,
                        Mode: AddrModeFlat,
                        ..Default::default()
                    },
                    AddrFrame: ADDRESS64 {
                        Offset: context.Rbp,
                        Mode: AddrModeFlat,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                addresses.push(context.Rip);
                for _ in 0..15 {
                    if !unsafe {
                        StackWalk64(
                            0x8664,
                            process.0,
                            thread.0,
                            &mut frame,
                            (&mut context as *mut CONTEXT).cast(),
                            None,
                            Some(unwind_table),
                            Some(module_base),
                            None,
                        )
                    }
                    .as_bool()
                        || frame.AddrPC.Offset == 0
                    {
                        break;
                    }
                    addresses.push(frame.AddrPC.Offset);
                }
            }
            drop(resume);
            let pause_ms = pause.elapsed().as_secs_f64() * 1000.;
            let frames:Vec<_>=addresses.into_iter().map(|address|{
                let base=unsafe{SymGetModuleBase64(process.0,address)};
                let mut module=[0u16;260];
                let len=unsafe{GetModuleBaseNameW(process.0,Some(HMODULE(base as *mut _)),&mut module)} as usize;
                let mut storage=vec![0u64;(size_of::<SYMBOL_INFO>()+1024).div_ceil(8)];
                let symbol=storage.as_mut_ptr().cast::<SYMBOL_INFO>();
                unsafe{(*symbol).SizeOfStruct=size_of::<SYMBOL_INFO>() as u32;(*symbol).MaxNameLen=1024;}
                let name=if unsafe{SymFromAddr(process.0,address,None,symbol)}.is_ok(){Some(unsafe{String::from_utf8_lossy(std::slice::from_raw_parts((*symbol).Name.as_ptr().cast::<u8>(),(*symbol).NameLen as usize))}.into_owned())}else{None};
                serde_json::json!({"module":String::from_utf16_lossy(&module[..len]),"offset":format!("{:#x}",address.saturating_sub(base)),"symbol":name})
            }).collect();
            writeln!(
                out,
                "{}",
                serde_json::json!({"unixMs":at,"thread":name,"tid":tid,"pauseMs":pause_ms,"frames":frames})
            )?;
        }
        out.flush()?;
        std::thread::sleep(Duration::from_millis(10).saturating_sub(tick.elapsed()));
    }
    unsafe {
        let _ = SymCleanup(process.0);
    }
    Ok(())
}
