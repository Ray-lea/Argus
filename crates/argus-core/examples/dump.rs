//! Prints the top processes by CPU, as a quick sanity check of the collector.
//! `cargo run -p argus-core --example dump --release`

use std::time::{Duration, Instant};

use argus_core::{Collector, MetaStore};

fn main() {
    let t = Instant::now();
    let mut c = Collector::new(MetaStore::new());
    println!("init: {:?}", t.elapsed());
    std::thread::sleep(Duration::from_millis(1000));

    let t = Instant::now();
    let s = c.sample();
    println!("sample: {:?}", t.elapsed());
    let sys = &s.system;
    println!(
        "cpu {:.1}% ({} logical, {} cores reported) | mem {:.2}/{:.2} GiB | commit {:.2}/{:.2} GiB | procs {} threads {} handles {} | up {}s",
        sys.cpu,
        sys.logical_cpus,
        sys.cores.len(),
        sys.mem_used as f64 / 1073741824.0,
        sys.mem_total as f64 / 1073741824.0,
        sys.commit_used as f64 / 1073741824.0,
        sys.commit_limit as f64 / 1073741824.0,
        sys.process_count,
        sys.thread_count,
        sys.handle_count,
        sys.uptime_ms / 1000
    );

    // Let the meta worker resolve descriptions/icons, then sample again.
    std::thread::sleep(Duration::from_millis(1500));
    let t = Instant::now();
    let mut s = c.sample();
    println!("sample (warm): {:?}", t.elapsed());
    let with_path = s.processes.iter().filter(|p| p.path.is_some()).count();
    let with_icon = s.processes.iter().filter(|p| p.icon.is_some()).count();
    let suspended = s.processes.iter().filter(|p| p.suspended).count();
    println!(
        "{} processes, {} with path, {} with icon, {} suspended",
        s.processes.len(),
        with_path,
        with_icon,
        suspended
    );

    s.processes.sort_by_key(|p| std::cmp::Reverse(p.private_ws));
    println!(
        "\n   PID   PPID   CPU%  PrivWS MB   IO KB/s  NAME                         DESCRIPTION"
    );
    for p in s.processes.iter().take(15) {
        println!(
            "{:>6} {:>6} {:>6.2} {:>10.1} {:>9}  {:<28} {}",
            p.pid,
            p.ppid,
            p.cpu,
            p.private_ws as f64 / 1048576.0,
            (p.io_read + p.io_write + p.io_other) / 1024,
            p.name,
            p.description.as_deref().unwrap_or("-")
        );
    }
    for p in s
        .processes
        .iter()
        .filter(|p| p.pid == 4 || p.name.as_ref() == "svchost.exe")
        .take(3)
    {
        println!("pid {} path={:?}", p.pid, p.path);
    }

    let me = std::process::id();
    let d = argus_core::details::query(
        me,
        s.processes
            .iter()
            .find(|p| p.pid == me)
            .and_then(|p| p.path.as_deref()),
    );
    println!("\nself details: {d:?}");
    println!("elevated: {}", argus_core::privilege::is_elevated());
    println!("accent: {:?}", argus_core::theme::accent_colors());
}
