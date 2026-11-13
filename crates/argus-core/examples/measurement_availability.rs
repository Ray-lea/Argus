//! Aggregate-only live evidence: no process names, paths, addresses or commands.
use argus_core::{Collector, MetaStore};
use serde_json::json;
use std::time::Duration;

fn main() {
    let mut collector = Collector::new(MetaStore::new());
    for _ in 0..5 {
        std::thread::sleep(Duration::from_millis(600));
        let snapshot = collector.sample();
        println!(
            "{}",
            json!({
                "sequence":snapshot.seq,
                "gpu":snapshot.hardware.gpus.iter().map(|g|json!({"state":g.util_state,"value":g.util_state.valid.then_some(g.util)})).collect::<Vec<_>>(),
                "networkState":snapshot.hardware.network_state,
                "adapters":snapshot.hardware.nics.iter().map(|n|json!({"state":n.rate_state,"rx":n.rate_state.valid.then_some(n.rx),"tx":n.rate_state.valid.then_some(n.tx)})).collect::<Vec<_>>(),
                "disks":snapshot.hardware.disks.iter().map(|d|json!({"state":d.counter_state,"read":d.read,"write":d.write})).collect::<Vec<_>>(),
                "knownIdleGpuProcesses":snapshot.processes.iter().filter(|p|p.gpu_state.valid && p.gpu==0.).count(),
                "knownIdleIoProcesses":snapshot.processes.iter().filter(|p|p.io_valid && !p.sample_reused && !p.stale && p.io_read==0 && p.io_write==0 && p.io_other==0).count()
            })
        );
    }
}
