//! Verify the additional process counters and measure snapshot sampling cost.
//! `cargo run -p argus-core --example process_columns --release`

use argus_core::{Collector, MetaStore};
use std::time::{Duration, Instant};

fn main() {
    let mut collector = Collector::new(MetaStore::new());
    let mut timings = Vec::new();
    let mut result = None;
    for _ in 0..12 {
        std::thread::sleep(Duration::from_millis(500));
        let start = Instant::now();
        let sample = collector.sample();
        timings.push(start.elapsed().as_secs_f64() * 1000.);
        let own = sample
            .processes
            .iter()
            .find(|process| process.pid == std::process::id())
            .expect("self process present");
        assert!(own.counters.peak_working_set >= own.working_set);
        let split = own.counters.cpu_user_time + own.counters.cpu_kernel_time;
        assert!(own.cpu_time >= split && own.cpu_time - split <= 1);
        assert!(own.counters.virtual_size >= own.commit);
        let json = serde_json::to_value(own).expect("serializable process");
        assert_eq!(
            json["ioReadTotal"].as_u64(),
            Some(own.counters.io_read_total)
        );
        assert!(json.get("counters").is_none());
        result = Some(
            serde_json::json!({"processCount":sample.processes.len(),"counters":own.counters}),
        );
    }
    timings.sort_by(f64::total_cmp);
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"elevated":argus_core::privilege::is_elevated(),"samplingMs":timings,"medianMs":timings[timings.len()/2],"result":result})).unwrap());
}
