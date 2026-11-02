//! Read-only cadence probe against the already installed service. Traffic stays
//! on loopback and only this process's counters are printed.
#[path = "support/network_suite.rs"]
mod suite;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use argus_core::{Collector, MetaStore};
use argus_service::{NetworkClient, Operation, Reply};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().len() == 1 || std::env::args().any(|arg| arg == "--suite") {
        return suite::run();
    }
    argus_core::diagnostics::capture_helper_failures();
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
    println!("{}", serde_json::json!({"event":"collector-initializing"}));
    let mut collector = Collector::new(MetaStore::new());
    let mut snapshot = collector.sample();
    println!("{}", serde_json::json!({"event":"collector-ready"}));
    let pid = std::process::id();
    snapshot.processes.retain(|p| p.pid == pid);
    assert_eq!(snapshot.processes.len(), 1);
    let local_requested = std::env::args().any(|arg| arg == "--local");
    let refresh_snapshot = std::env::args().any(|arg| arg == "--collector");
    let raw_local = std::env::args().any(|arg| arg == "--raw-local");
    let dual_local = std::env::args().any(|arg| arg == "--dual-local");
    let alternating = std::env::args().any(|arg| arg == "--alternating");
    assert!(!raw_local || (local_requested && !refresh_snapshot));
    assert!(!dual_local || (local_requested && !refresh_snapshot));
    let collector_local = local_requested
        && refresh_snapshot
        && collector.enable_process_network(&format!("Argus Network Collector Test {pid}"));
    let mut local = (local_requested && !refresh_snapshot)
        .then(|| {
            argus_core::net_process::NetworkTracker::start(&format!(
                "Argus Network Cadence Test {pid}"
            ))
        })
        .flatten();
    if local_requested && local.is_none() && !collector_local {
        println!(
            "{}",
            serde_json::json!({"event":"local-unavailable","elevated":argus_core::privilege::is_elevated(),"failures":argus_core::diagnostics::take_helper_failures()})
        );
        return Ok(());
    }
    let mut peer = dual_local
        .then(|| {
            argus_core::net_process::NetworkTracker::start(&format!(
                "Argus Network Peer Test {pid}"
            ))
        })
        .flatten();
    if dual_local {
        assert!(peer.is_some());
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
            let bytes = if alternating && (start.elapsed().as_millis() / 500) % 2 == 1 {
                8192
            } else {
                2048
            };
            stream.write_all(&[0u8; 8192][..bytes]).unwrap();
            count.fetch_add(bytes as u64, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let start = Instant::now();
    let mut last_raw = start;
    println!(
        "{}",
        serde_json::json!({"event":"start","intervalMs":interval,"pid":pid,"elevated":argus_core::privilege::is_elevated(),"refreshSnapshot":refresh_snapshot,"local":local_requested,"alternating":alternating})
    );
    for step in 1..=(seconds * 1000 / interval) {
        std::thread::sleep(
            (start + Duration::from_millis(step * interval))
                .saturating_duration_since(Instant::now()),
        );
        if refresh_snapshot {
            snapshot = collector.sample();
            snapshot.processes.retain(|p| p.pid == pid);
            assert_eq!(snapshot.processes.len(), 1);
        } else {
            snapshot.process_network = false;
        }
        let t = Instant::now();
        let mut local_raw = serde_json::Value::Null;
        if let Some(local) = &mut local {
            if raw_local {
                let bytes = local.take_bytes();
                let (sent, received) = bytes.get(&pid).copied().unwrap_or_default();
                let elapsed = t.duration_since(last_raw).as_secs_f64().max(0.001);
                last_raw = t;
                snapshot.processes[0].net_sent = (sent as f64 / elapsed) as u64;
                snapshot.processes[0].net_received = (received as f64 / elapsed) as u64;
                local_raw = serde_json::json!({
                    "ownSent":sent,"ownReceived":received,"activePids":bytes.len(),
                    "allBytes":bytes.values().map(|(s,r)| u128::from(*s) + u128::from(*r)).sum::<u128>(),
                });
            } else {
                local.apply_rates(&mut snapshot.processes);
            }
            snapshot.process_network = true;
        } else if !collector_local {
            client.enrich(&mut snapshot, Duration::from_millis(interval));
        }
        let enrich_us = t.elapsed().as_micros();
        let row = &snapshot.processes[0];
        if dual_local && start.elapsed() >= Duration::from_secs(8) {
            peer = None;
        }
        let peer_rate = if let Some(peer) = &mut peer {
            let mut rows = snapshot.processes.clone();
            peer.apply_rates(&mut rows);
            Some((rows[0].net_sent, rows[0].net_received))
        } else {
            None
        };
        let raw = match argus_service::request(Operation::NetworkSnapshot) {
            Ok(Some(Reply::Network(Some(frame)))) => {
                let counter = frame.counters.iter().find(|c| c.0 == pid);
                serde_json::json!({"atUs":frame.elapsed_us,"sent":counter.map(|c|c.3),"received":counter.map(|c|c.4)})
            }
            Ok(None) => serde_json::json!({"available":false,"serviceRunning":false}),
            Ok(Some(Reply::Network(None))) => {
                serde_json::json!({"available":false,"serviceRunning":true,"snapshotUnavailable":true})
            }
            Ok(Some(Reply::Error(error))) => {
                serde_json::json!({"available":false,"serviceError":error.diagnostic()})
            }
            Ok(_) => serde_json::json!({"available":false,"unexpectedReply":true}),
            Err(error) => {
                serde_json::json!({"available":false,"requestFailed":true,"error":argus_core::diagnostics::failure(&error)})
            }
        };
        println!(
            "{}",
            serde_json::json!({"event":"sample","atMs":start.elapsed().as_millis(),"available":snapshot.process_network,"created":row.create_time,"sentRate":row.net_sent,"receivedRate":row.net_received,"written":sent.load(Ordering::Relaxed),"enrichUs":enrich_us,"raw":raw,"localRaw":local_raw,"peerRate":peer_rate,"failures":argus_core::diagnostics::take_helper_failures()})
        );
    }
    sender.join().unwrap();
    receiver.join().unwrap();
    Ok(())
}
