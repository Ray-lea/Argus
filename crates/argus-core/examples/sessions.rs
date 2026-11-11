//! Signed-in sessions, the processes each one holds, and the enumeration cost.
//! `cargo run -p argus-core --example sessions --release`
//!
//! Account names are printed only with `--names`; otherwise each user shows
//! as a letter, so the output can be shared.

use argus_core::{Collector, MetaStore, sessions};
use std::collections::BTreeMap;
use std::time::Instant;

fn main() {
    let names = std::env::args().any(|argument| argument == "--names");
    let listed = sessions::list();
    let mut users: Vec<(&str, &str)> = Vec::new();
    println!("sessions with a signed-in user: {}", listed.len());
    for session in &listed {
        let identity = (&*session.domain, &*session.user);
        let index = users
            .iter()
            .position(|user| *user == identity)
            .unwrap_or_else(|| {
                users.push(identity);
                users.len() - 1
            });
        let shown = if names {
            format!("{}\\{}", session.domain, session.user)
        } else {
            format!("user {}", (b'A' + (index % 26) as u8) as char)
        };
        println!(
            "  session {:>3}  {shown}{}{}",
            session.id,
            if session.current { "  (current)" } else { "" },
            if session.disconnected {
                "  (disconnected)"
            } else {
                ""
            },
        );
    }
    println!("distinct users: {}", users.len());

    let mut collector = Collector::new(MetaStore::new());
    std::thread::sleep(std::time::Duration::from_millis(500));
    let snapshot = collector.sample();
    let mut by_session: BTreeMap<u32, usize> = BTreeMap::new();
    for process in &snapshot.processes {
        *by_session.entry(process.session_id).or_default() += 1;
    }
    println!("processes by session:");
    for (session, count) in by_session {
        let owner = snapshot
            .sessions
            .iter()
            .find(|listed| listed.id == session)
            .map_or("no user (system)", |_| "signed-in user");
        println!("  session {session:>3}  {count:>4} processes  {owner}");
    }

    // Back to back, then spaced as the sampler reads them: the session
    // manager answers a call after a pause more slowly.
    for (calls, pause) in [(200, 0), (20, 500)] {
        let mut timings: Vec<u128> = (0..calls)
            .map(|_| {
                std::thread::sleep(std::time::Duration::from_millis(pause));
                let start = Instant::now();
                std::hint::black_box(sessions::list());
                start.elapsed().as_micros()
            })
            .collect();
        timings.sort_unstable();
        println!(
            "enumeration every {pause} ms: median {} us, p95 {} us, max {} us over {} calls",
            timings[timings.len() / 2],
            timings[timings.len() * 95 / 100],
            timings[timings.len() - 1],
            timings.len()
        );
    }
}
