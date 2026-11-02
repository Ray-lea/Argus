//! Verify the additional process counters and measure snapshot sampling cost.
//! `cargo run -p argus-core --example process_columns --release`

use argus_core::{Collector, MetaStore};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::FILETIME,
    System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::{GetCurrentProcess, GetProcessIoCounters, GetProcessTimes, IO_COUNTERS},
    },
};

fn native_counters() -> [u64; 7] {
    let process = unsafe { GetCurrentProcess() };
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let mut io = IO_COUNTERS::default();
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    unsafe {
        GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user)
            .expect("own process times");
        GetProcessIoCounters(process, &mut io).expect("own process I/O");
        GetProcessMemoryInfo(
            process,
            (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            memory.cb,
        )
        .expect("own process memory");
    }
    let ms = |time: FILETIME| {
        ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) / 10_000
    };
    [
        ms(user),
        ms(kernel),
        memory.PeakWorkingSetSize as u64,
        u64::from(memory.PageFaultCount),
        io.ReadTransferCount,
        io.WriteTransferCount,
        io.OtherTransferCount,
    ]
}

fn main() {
    let mut collector = Collector::new(MetaStore::new());
    let mut timings = Vec::new();
    let mut result = None;
    for _ in 0..12 {
        std::thread::sleep(Duration::from_millis(500));
        let before = native_counters();
        let start = Instant::now();
        let sample = collector.sample();
        timings.push(start.elapsed().as_secs_f64() * 1000.);
        let after = native_counters();
        let own = sample
            .processes
            .iter()
            .find(|process| process.pid == std::process::id())
            .expect("self process present");
        assert!(own.counters.peak_working_set >= own.working_set);
        let split = own.counters.cpu_user_time + own.counters.cpu_kernel_time;
        assert!(own.cpu_time >= split && own.cpu_time - split <= 1);
        assert!(own.counters.virtual_size >= own.commit);
        let counters = &own.counters;
        let sampled = [
            counters.cpu_user_time,
            counters.cpu_kernel_time,
            counters.peak_working_set,
            u64::from(counters.page_faults),
            counters.io_read_total,
            counters.io_write_total,
            counters.io_other_total,
        ];
        for index in 0..sampled.len() {
            assert!(
                (before[index]..=after[index]).contains(&sampled[index]),
                "native counter {index}: {} <= {} <= {}",
                before[index],
                sampled[index],
                after[index]
            );
        }
        let json = serde_json::to_value(own).expect("serializable process");
        assert_eq!(
            json["ioReadTotal"].as_u64(),
            Some(own.counters.io_read_total)
        );
        assert!(json.get("counters").is_none());
        result = Some(
            serde_json::json!({"processCount":sample.processes.len(),"counters":own.counters,
                "nativeBefore":before,"nativeAfter":after,"validatedFields":["cpuUserTime","cpuKernelTime","peakWorkingSet","pageFaults","ioReadTotal","ioWriteTotal","ioOtherTotal"]}),
        );
    }
    timings.sort_by(f64::total_cmp);
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"elevated":argus_core::privilege::is_elevated(),"samplingMs":timings,"medianMs":timings[timings.len()/2],"result":result})).unwrap());
}
