//! Read-only cadence probe for an existing, authorized sensor service.
//! Never installs, starts, updates or elevates the service or driver.
use argus_core::hardware::{CpuSensors, SensorState};
use argus_service::{Operation, Reply, SensorClient};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let direct = args.iter().any(|arg| arg == "--direct");
    let mut collector = args
        .iter()
        .any(|arg| arg == "--collector")
        .then(|| argus_core::Collector::new(argus_core::MetaStore::new()));
    let count = args
        .iter()
        .find_map(|arg| arg.parse::<u32>().ok())
        .unwrap_or(60)
        .clamp(1, 300);
    let mut client = SensorClient::spawn();
    let mut states = std::collections::BTreeMap::<String, usize>::new();
    let mut elapsed = Vec::new();
    for sample in 0..count {
        if sample == count / 2 && args.iter().any(|arg| arg == "--pause") {
            std::thread::sleep(Duration::from_secs(20));
        }
        let start = Instant::now();
        let frame = if direct {
            match argus_service::request(Operation::SensorSnapshot {}) {
                Ok(Some(Reply::Sensors(frame))) => *frame,
                _ => CpuSensors::unavailable(SensorState::ServiceUnavailable),
            }
        } else {
            let mut snapshot = argus_core::Snapshot {
                seq: sample as u64,
                timestamp: 0,
                system: Default::default(),
                hardware: Default::default(),
                processes: vec![],
                process_network: false,
            };
            if let Some(collector) = &mut collector {
                snapshot = collector.sample();
            }
            client.enrich(&mut snapshot, true, Duration::from_secs(1));
            snapshot.hardware.cpu_sensors.unwrap_or_default()
        };
        let millis = start.elapsed().as_secs_f64() * 1000.;
        elapsed.push(millis);
        *states.entry(format!("{:?}", frame.state)).or_default() += 1;
        println!(
            "{}",
            serde_json::json!({"sample":sample,"direct":direct,"state":frame.state,"packageCelsius":frame.package_celsius,"packageWatts":frame.package_watts,"cores":frame.cores.len(),"coreTemperatures":frame.cores.iter().filter(|c|c.celsius.is_some()).count(),"coreClocks":frame.cores.iter().filter(|c|c.clock_mhz.is_some()).count(),"coreEffective":frame.cores.iter().filter(|c|c.effective_mhz.is_some()).count(),"coreVid":frame.cores.iter().filter(|c|c.vid.is_some()).count(),"metrics":frame.metrics.len(),"metricKinds":frame.metrics.iter().map(|m|m.kind).collect::<Vec<_>>(),"coreIds":frame.cores.iter().map(|c|c.core).collect::<Vec<_>>(),"elapsedMs":millis})
        );
        std::thread::sleep(Duration::from_secs(1).saturating_sub(start.elapsed()));
    }
    elapsed.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"states":states,"p95Ms":elapsed[(elapsed.len()-1)*95/100],"maxMs":elapsed.last()})
    );
}
