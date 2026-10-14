//! Ordinary-permission telemetry check. Does not request the service or open
//! a kernel device; exercises the disabled bridge, CPU snapshot serialization,
//! and vendor GPU SDKs.
use argus_core::{Collector, MetaStore};
use argus_service::SensorClient;
use std::time::{Duration, Instant};

fn main() {
    use argus_core::hardware::{
        CpuCoreSensors, CpuMetric, CpuMetricKind, CpuSensorSource, CpuSensors,
    };
    let frame = CpuSensors {
        cores: (0..16)
            .map(|core| CpuCoreSensors {
                core,
                group: 0,
                processor: (core * 2) as u8,
                celsius: Some(61.0),
                clock_mhz: Some(5500.0),
                effective_mhz: Some(1500.0),
                watts: Some(4.5),
                vid: Some(1.2),
                multiplier: Some(55.0),
                c0_percent: Some(27.0),
                ..Default::default()
            })
            .collect(),
        metrics: vec![
            CpuMetric {
                kind: CpuMetricKind::CoreVoltage,
                value: 1.21,
                source: CpuSensorSource::Svi3
            };
            55
        ],
        ..Default::default()
    };
    let mut wire = Vec::new();
    let mut size = 0;
    for i in 0..1200 {
        let start = Instant::now();
        let copy = std::hint::black_box(frame.clone());
        size = std::hint::black_box(serde_json::to_vec(&copy).unwrap()).len();
        if i >= 200 {
            wire.push(start.elapsed().as_secs_f64() * 1_000_000.0);
        }
    }
    wire.sort_by(f64::total_cmp);
    println!(
        "16-core/55-metric snapshot clone+serialize: median {:.3} us, p95 {:.3} us, {size} bytes",
        wire[500], wire[950]
    );
    let mut collector = Collector::new(MetaStore::new());
    let mut sensors = SensorClient::spawn();
    let mut collect = Vec::new();
    let mut bridge = Vec::new();
    for i in 0..22 {
        std::thread::sleep(Duration::from_secs(1));
        let start = Instant::now();
        let mut snapshot = collector.sample();
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        sensors.enrich(&mut snapshot, false, Duration::from_secs(1));
        if i >= 2 {
            collect.push(elapsed);
            bridge.push(start.elapsed().as_secs_f64() * 1_000_000.0);
        }
        if i == 21 {
            for (index, gpu) in snapshot.hardware.gpus.iter().enumerate() {
                println!(
                    "GPU {index}: source {:?}, power {:?}, limit {:?}, temperature {:?}",
                    gpu.power_source, gpu.board_power, gpu.board_power_limit, gpu.temperature
                );
            }
        }
    }
    collect.sort_by(f64::total_cmp);
    bridge.sort_by(f64::total_cmp);
    println!(
        "collection median {:.3} ms, p95 {:.3} ms; disabled sensor bridge median {:.3} us, p95 {:.3} us",
        (collect[9] + collect[10]) / 2.0,
        collect[18],
        (bridge[9] + bridge[10]) / 2.0,
        bridge[18]
    );
}
