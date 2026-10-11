//! Lists the processes using each path given on the command line.
//! `cargo run -p argus-core --example lockers -- C:\Windows\System32\ntdll.dll`

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let t = std::time::Instant::now();
    match argus_core::filelock::find_lockers(&paths) {
        Ok(r) => {
            println!(
                "{} lockers, {} files, truncated {} in {:?}",
                r.lockers.len(),
                r.files_checked,
                r.truncated,
                t.elapsed()
            );
            for l in r.lockers.iter().take(8) {
                println!("  {:>6} {:<10} {}", l.pid, l.kind, l.app_name);
            }
        }
        Err(e) => println!("error: {e:?}"),
    }
}
