//! Read-only cadence probe against the already installed service. Traffic stays
//! on loopback and only this process's counters are printed.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use argus_core::{Collector, MetaStore};
use argus_service::{NetworkClient, Operation, Reply};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let interval = std::env::args()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(500);
    assert!((100..=5000).contains(&interval));
    let seconds = std::env::args()
        .nth(2)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(17);
    assert!((16..=120).contains(&seconds));
    let mut collector = Collector::new(MetaStore::new());
    let mut snapshot = collector.sample();
    let pid = std::process::id();
    snapshot.processes.retain(|p| p.pid == pid);
    assert_eq!(snapshot.processes.len(), 1);
    let local_requested = std::env::args().any(|arg| arg == "--local");
    let mut local = local_requested
        .then(|| {
            argus_core::net_process::NetworkTracker::start(&format!(
                "Argus Network Cadence Test {pid}"
            ))
        })
        .flatten();
    if local_requested && local.is_none() {
        println!(
            "{}",
            serde_json::json!({"event":"local-unavailable","elevated":argus_core::privilege::is_elevated()})
        );
        return Ok(());
    }
    let mut client = NetworkClient::spawn();
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let sent = Arc::new(AtomicU64::new(0));
    let count = sent.clone();
    let receiver = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0u8; 8192];
        while stream.read(&mut buffer).unwrap_or(0) != 0 {}
    });
    let sender = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_nodelay(true).unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(12) {
            stream.write_all(&[0u8; 2048]).unwrap();
            count.fetch_add(2048, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let start = Instant::now();
    println!(
        "{}",
        serde_json::json!({"event":"start","intervalMs":interval,"pid":pid,"elevated":argus_core::privilege::is_elevated()})
    );
    for step in 1..=(seconds * 1000 / interval) {
        std::thread::sleep(
            (start + Duration::from_millis(step * interval))
                .saturating_duration_since(Instant::now()),
        );
        snapshot.process_network = false;
        let t = Instant::now();
        if let Some(local) = &mut local {
            local.apply_rates(&mut snapshot.processes);
            snapshot.process_network = true;
        } else {
            client.enrich(&mut snapshot, Duration::from_millis(interval));
        }
        let enrich_us = t.elapsed().as_micros();
        let row = &snapshot.processes[0];
        let raw = match argus_service::request(Operation::NetworkSnapshot) {
            Ok(Some(Reply::Network(Some(frame)))) => {
                let counter = frame.counters.iter().find(|c| c.0 == pid);
                serde_json::json!({"atUs":frame.elapsed_us,"sent":counter.map(|c|c.3),"received":counter.map(|c|c.4)})
            }
            Ok(_) => serde_json::json!({"available":false}),
            Err(_) => serde_json::json!({"available":false,"requestFailed":true}),
        };
        println!(
            "{}",
            serde_json::json!({"event":"sample","atMs":start.elapsed().as_millis(),"available":snapshot.process_network,"sentRate":row.net_sent,"receivedRate":row.net_received,"written":sent.load(Ordering::Relaxed),"enrichUs":enrich_us,"raw":raw})
        );
    }
    sender.join().unwrap();
    receiver.join().unwrap();
    Ok(())
}
