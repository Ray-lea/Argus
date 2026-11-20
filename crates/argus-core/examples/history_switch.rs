//! Times the queries a range switch on the History page makes, against a copy
//! of a real history folder.
//! cargo run -p argus-core --example history_switch --release -- <history folder> [plot width] [--cold <range>]
//! With `--cold 30d` (or `all`, `7d`, …) that range is asked first, in a
//! process that has read nothing yet (every day index comes from disk), and
//! then again; `all` reaches back to the earliest day file.
use argus_core::history;
use std::{
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const SECOND: i64 = 1_000;
const MINUTE: i64 = 60 * SECOND;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const NICE: [i64; 16] = [
    SECOND,
    2 * SECOND,
    5 * SECOND,
    10 * SECOND,
    15 * SECOND,
    30 * SECOND,
    MINUTE,
    2 * MINUTE,
    5 * MINUTE,
    10 * MINUTE,
    15 * MINUTE,
    30 * MINUTE,
    HOUR,
    2 * HOUR,
    3 * HOUR,
    6 * HOUR,
];
const RANGES: [(&str, i64); 8] = [
    ("5m", 5 * MINUTE),
    ("1h", HOUR),
    ("3h", 3 * HOUR),
    ("6h", 6 * HOUR),
    ("1d", DAY),
    ("3d", 3 * DAY),
    ("7d", 7 * DAY),
    ("30d", 30 * DAY),
];
const WANTED: [&str; 7] = [
    "cpu",
    "cpuTemp",
    "memUsed",
    "memTotal",
    "gpu",
    "netReceive",
    "netSend",
];

fn bucket_for(span: i64, width: f64) -> i64 {
    let target = span as f64 / (width / 2.6).max(60.);
    NICE.into_iter()
        .find(|b| *b as f64 >= target)
        .unwrap_or(NICE[NICE.len() - 1])
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The earliest day file's UTC midnight, from its name (`YYYY-MM-DD.hist`).
fn earliest_day(dir: &Path) -> Option<i64> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let date = name.strip_suffix(".hist")?;
            let mut parts = date.split('-').map(|p| p.parse::<i64>().ok());
            let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
            Some(days_from_civil(y, m, d) * DAY)
        })
        .min()
}

fn time(dir: &Path, name: &str, span: i64, now: i64, width: f64, keys: &[String], runs: usize) {
    let bucket = bucket_for(span, width);
    let margin = span / 4 + bucket;
    let (from, to) = (now - span - margin, now + bucket);
    let mut times = Vec::new();
    let mut last = None;
    for _ in 0..runs {
        let start = Instant::now();
        last = Some(history::series(dir, from, to, bucket, 0, keys));
        times.push(start.elapsed().as_secs_f64() * 1e3);
    }
    let out = last.unwrap();
    let start = Instant::now();
    let value = serde_json::to_value(&out).unwrap();
    let encode = start.elapsed().as_secs_f64() * 1e3;
    let bytes = value.to_string().len();
    let first = times[0];
    times.sort_by(f64::total_cmp);
    println!(
        "{name:>3}: bucket {:>6} s, {:>5} points, first {first:>8.2} ms, median {:>8.2} ms, max {:>8.2} ms, to_value {encode:.2} ms, {} KB",
        bucket / 1000,
        out.ts.len(),
        times[times.len() / 2],
        times[times.len() - 1],
        bytes / 1024
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(args.first().expect("history folder"));
    let width: f64 = args
        .get(1)
        .filter(|w| !w.starts_with("--"))
        .map_or(1000., |w| w.parse().expect("width"));
    let cold = args
        .iter()
        .position(|a| a == "--cold")
        .and_then(|i| args.get(i + 1));
    if let Some(cold) = cold {
        // Nothing read before: the keys are asked for whether recorded or not.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_millis() as i64;
        let keys: Vec<String> = WANTED.iter().map(|k| k.to_string()).collect();
        let span = match cold.as_str() {
            "all" => now - earliest_day(&dir).expect("no day files"),
            name => {
                RANGES
                    .iter()
                    .find(|(n, _)| *n == name)
                    .expect("a range: 5m 1h 3h 6h 1d 3d 7d 30d all")
                    .1
            }
        };
        time(&dir, cold, span, now, width, &keys, 1);
        time(&dir, cold, span, now, width, &keys, 5);
        return;
    }
    let channels = history::channels(&dir, 0, i64::MAX / 4);
    let now = channels.iter().map(|c| c.last).max().expect("no channels");
    let keys: Vec<String> = WANTED
        .iter()
        .filter(|k| channels.iter().any(|c| c.key == **k))
        .map(|k| k.to_string())
        .collect();
    println!("{} channels, keys {keys:?}", channels.len());
    for (name, span) in RANGES {
        time(&dir, name, span, now, width, &keys, 7);
    }
}
