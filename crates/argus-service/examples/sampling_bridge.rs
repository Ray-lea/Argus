//! Measures the work added on the sampling thread, without installing a
//! service, changing settings, or configuring any process rules.
use std::sync::Arc;
use std::time::{Duration, Instant};

use argus_core::{Collector, MetaStore};
use argus_service::{NetworkClient, RuleRunner};

fn main() {
    println!("service: {:?}", argus_service::status());
    let mut collector = Collector::new(MetaStore::new());
    let rules = Arc::new(RuleRunner::spawn(argus_core::privilege::is_elevated()));
    let mut network = NetworkClient::spawn();
    let mut collection = Vec::new();
    let mut bridge = Vec::new();
    let mut count = 0;
    let mut measured = false;
    for i in 0..22 {
        std::thread::sleep(Duration::from_secs(1));
        let start = Instant::now();
        let mut snapshot = collector.sample();
        let collect = start.elapsed();
        let start = Instant::now();
        network.enrich(&mut snapshot, Duration::from_secs(1));
        rules.observe(&snapshot.processes);
        let extra = start.elapsed();
        count = snapshot.processes.len();
        measured = snapshot.process_network;
        if i >= 2 {
            collection.push(collect.as_secs_f64() * 1000.0);
            bridge.push(extra.as_secs_f64() * 1000.0);
        }
    }
    collection.sort_by(f64::total_cmp);
    bridge.sort_by(f64::total_cmp);
    println!("processes: {count}, service network available: {measured}");
    println!(
        "collection median {:.3} ms, max {:.3} ms",
        (collection[9] + collection[10]) / 2.0,
        collection[19]
    );
    println!(
        "bridge median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
        (bridge[9] + bridge[10]) / 2.0,
        bridge[18],
        bridge[19]
    );
}
