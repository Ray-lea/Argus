//! Compare the collector's resident GPU memory with native budget usage.
//! `cargo run -p argus-core --example gpu_memory --release -- <PID>`

use argus_core::gpu_process::GpuProcessTracker;
use serde_json::{Value, json};
use windows::Wdk::Graphics::Direct3D::*;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION};

fn native_usage(pid: u32) -> Value {
    let process = match unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) } {
        Ok(handle) => handle,
        Err(error) => return json!({"openError":error.code().0}),
    };
    let mut list = D3DKMT_ENUMADAPTERS2::default();
    let mut status = unsafe { D3DKMTEnumAdapters2(&mut list) }.0;
    let mut results = Vec::new();
    if status >= 0 && list.NumAdapters <= 64 {
        let mut adapters = vec![D3DKMT_ADAPTERINFO::default(); list.NumAdapters as usize];
        list.pAdapters = adapters.as_mut_ptr();
        status = unsafe { D3DKMTEnumAdapters2(&mut list) }.0;
        if status >= 0 {
            adapters.truncate(list.NumAdapters as usize);
            for adapter in adapters {
                let mut groups = Vec::new();
                for group in [
                    D3DKMT_MEMORY_SEGMENT_GROUP_LOCAL,
                    D3DKMT_MEMORY_SEGMENT_GROUP_NON_LOCAL,
                ] {
                    let mut memory = D3DKMT_QUERYVIDEOMEMORYINFO {
                        hProcess: process,
                        hAdapter: adapter.hAdapter,
                        MemorySegmentGroup: group,
                        ..Default::default()
                    };
                    let status = unsafe { D3DKMTQueryVideoMemoryInfo(&mut memory) }.0;
                    groups.push(json!({"group":group.0,"status":status,
                        "usage":(status>=0).then_some(memory.CurrentUsage),
                        "budget":(status>=0).then_some(memory.Budget)}));
                }
                results.push(json!({"luid":format!("{:08x}:{:08x}",adapter.AdapterLuid.HighPart as u32,adapter.AdapterLuid.LowPart),"groups":groups}));
                unsafe {
                    let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER {
                        hAdapter: adapter.hAdapter,
                    });
                }
            }
        }
    }
    unsafe {
        let _ = CloseHandle(process);
    }
    json!({"enumerationStatus":status,"adapters":results})
}

fn main() {
    let pid = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(std::process::id());
    let mut tracker = GpuProcessTracker::new();
    let mut samples = Vec::new();
    for _ in 0..3 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let start = std::time::Instant::now();
        let readings = tracker
            .as_mut()
            .map(GpuProcessTracker::sample)
            .unwrap_or_default();
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        let available = tracker
            .as_ref()
            .map(GpuProcessTracker::memory_available)
            .unwrap_or([false; 2]);
        let reading = readings.get(&pid);
        samples.push(json!({"sampleMs":elapsed,"available":available,
            "dedicated":reading.map(|v|v.dedicated).unwrap_or(available[0].then_some(0)),
            "shared":reading.map(|v|v.shared).unwrap_or(available[1].then_some(0)),
            "native":native_usage(pid)}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"pid":pid,
        "elevated":argus_core::privilege::is_elevated(),"samples":samples}))
        .unwrap()
    );
}
