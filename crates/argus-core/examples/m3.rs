//! Prints services, startup items, connections and a file-lock lookup.
//! `cargo run -p argus-core --example m3 --release`

use std::time::Instant;

use argus_core::{MetaStore, connections, filelock, services::ServiceCatalog, startup};

fn main() {
    let catalog = ServiceCatalog::default();
    let t = Instant::now();
    let svcs = catalog.list().expect("services");
    let cold = t.elapsed();
    let t = Instant::now();
    let _ = catalog.list();
    println!(
        "services: {} (cold {:?}, cached {:?})",
        svcs.len(),
        cold,
        t.elapsed()
    );
    let running = svcs.iter().filter(|s| s.state == "running").count();
    println!(
        "  running {running}, with description {}",
        svcs.iter().filter(|s| s.description.is_some()).count()
    );
    for s in svcs
        .iter()
        .filter(|s| s.name == "Dnscache" || s.name == "Spooler" || s.name == "wuauserv")
    {
        println!("  {s:?}");
    }

    let meta = MetaStore::new();
    let t = Instant::now();
    let items = startup::list(&meta);
    println!("\nstartup items: {} in {:?}", items.len(), t.elapsed());
    for i in &items {
        println!(
            "  [{}] {:<28} enabled={} exists={} publisher={:?}\n      {}",
            i.location, i.name, i.enabled, i.exists, i.publisher, i.command
        );
    }

    let t = Instant::now();
    let conns = connections::list();
    println!("\nconnections: {} in {:?}", conns.len(), t.elapsed());
    let listening = conns.iter().filter(|c| c.state == Some("listen")).count();
    let established = conns
        .iter()
        .filter(|c| c.state == Some("established"))
        .count();
    println!(
        "  tcp listen {listening}, established {established}, udp {}",
        conns.iter().filter(|c| c.protocol == "udp").count()
    );
    for c in conns
        .iter()
        .filter(|c| c.local_port == 445 || c.local_port == 135)
        .take(4)
    {
        println!("  {c:?}");
    }

    let path = std::env::temp_dir().join("argus-lock-test.txt");
    let _held = std::fs::File::create(&path).expect("create temp file");
    let t = Instant::now();
    let report = filelock::find_lockers(&[path.to_string_lossy().into_owned()]).expect("lockers");
    println!("\nfile lock ({:?}): {:?}", t.elapsed(), report);
    println!(
        "  our pid {} found: {}",
        std::process::id(),
        report.lockers.iter().any(|l| l.pid == std::process::id())
    );
}
