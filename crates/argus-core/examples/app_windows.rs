//! Prints the processes listed under "Apps" (they own a taskbar window) and
//! how long finding them takes.
//! `cargo run -p argus-core --example app_windows --release`

use std::collections::HashMap;
use std::time::Instant;

use argus_core::{Collector, MetaStore, app_windows};

fn main() {
    let names: HashMap<u32, String> = Collector::new(MetaStore::new())
        .sample()
        .processes
        .iter()
        .map(|p| {
            (
                p.pid,
                p.description.as_deref().unwrap_or(&p.name).to_string(),
            )
        })
        .collect();

    let mut times = Vec::new();
    let mut owners = Default::default();
    for _ in 0..50 {
        let t = Instant::now();
        owners = app_windows::owners();
        times.push(t.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "owners(): median {:.3} ms, max {:.3} ms over {} runs",
        times[times.len() / 2],
        times[times.len() - 1],
        times.len()
    );

    let mut apps: Vec<_> = owners
        .iter()
        .map(|pid| (names.get(pid).cloned().unwrap_or_else(|| "?".into()), *pid))
        .collect();
    apps.sort();
    println!("{} app processes:", apps.len());
    for (name, pid) in apps {
        println!("  {pid:>6}  {name}");
    }
}
