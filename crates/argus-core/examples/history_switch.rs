//! Times the queries a range switch on the History page makes, against a copy
//! of a real history folder.
//! cargo run -p argus-core --example history_switch --release -- <history folder> [plot width]
use argus_core::history;
use std::{path::PathBuf, time::Instant};

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

fn bucket_for(span: i64, width: f64) -> i64 {
    let target = span as f64 / (width / 2.6).max(60.);
    NICE.into_iter()
        .find(|b| *b as f64 >= target)
        .unwrap_or(NICE[NICE.len() - 1])
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("history folder"));
    let width: f64 = args.next().map_or(1000., |w| w.parse().expect("width"));
    let channels = history::channels(&dir, 0, i64::MAX / 4);
    let now = channels.iter().map(|c| c.last).max().expect("no channels");
    let wanted = [
        "cpu",
        "cpuTemp",
        "memUsed",
        "memTotal",
        "gpu",
        "netReceive",
        "netSend",
    ];
    let keys: Vec<String> = wanted
        .iter()
        .filter(|k| channels.iter().any(|c| c.key == **k))
        .map(|k| k.to_string())
        .collect();
    println!("{} channels, keys {keys:?}", channels.len());
    for (name, span) in [
        ("5m", 5 * MINUTE),
        ("1h", HOUR),
        ("3h", 3 * HOUR),
        ("6h", 6 * HOUR),
        ("1d", DAY),
        ("3d", 3 * DAY),
        ("7d", 7 * DAY),
    ] {
        let bucket = bucket_for(span, width);
        let margin = span / 4 + bucket;
        let (from, to) = (now - span - margin, now + bucket);
        let mut times = Vec::new();
        let mut last = None;
        for _ in 0..7 {
            let start = Instant::now();
            last = Some(history::series(&dir, from, to, bucket, 0, &keys));
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
            "{name:>3}: bucket {:>6} s, {:>5} points, first {first:>7.2} ms, median {:>7.2} ms, max {:>7.2} ms, to_value {encode:.2} ms, {} KB",
            bucket / 1000,
            out.ts.len(),
            times[3],
            times[6],
            bytes / 1024
        );
    }
}
