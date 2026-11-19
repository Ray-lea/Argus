//! Measures live recording and optionally replays real snapshots into 30 full UTC days.
//! cargo +stable-x86_64-pc-windows-msvc run -p argus-core --example history --release -- 60 [--month | --merge-day]
use argus_core::{
    Collector, MetaStore, Snapshot,
    history::{self, FRAME_MS, HistoryRecorder},
};
use std::{
    fs, io,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const DAY: i64 = 86_400_000;
fn measure<T>(run: impl Fn() -> T) -> T {
    let mut times = Vec::new();
    let mut last = None;
    for _ in 0..7 {
        let start = Instant::now();
        last = Some(run());
        times.push(start.elapsed().as_secs_f64() * 1e3);
    }
    let cold = times[0];
    times.sort_by(f64::total_cmp);
    println!(
        "first {cold:.3} ms, median {:.3} ms, max {:.3} ms",
        times[3], times[6]
    );
    last.unwrap()
}
fn replay(root: &Path, mut snapshots: Vec<Snapshot>, merge_day: bool) -> io::Result<()> {
    use argus_core::hardware::{BatteryInfo, BatteryStats, CpuFanReading, CpuSensors};
    let Some(first) = snapshots.first() else {
        return Err(io::Error::other("no replay samples"));
    };
    let day_count = if merge_day { 1 } else { 30 };
    let base = first.timestamp.div_euclid(DAY) * DAY - 30 * DAY;
    // Make optional readings measurable on desktops without batteries or service sensors.
    // Disjoint leaders exercise all six resource rankings.
    for s in &mut snapshots {
        s.hardware.cpu_sensors = Some(CpuSensors {
            core_voltage: Some(1.1),
            board_fans: vec![CpuFanReading {
                channel: 1,
                rpm: 1250.,
            }],
            ..Default::default()
        });
        if s.hardware.batteries.is_empty() {
            s.hardware.batteries.push(BatteryStats {
                info: std::sync::Arc::new(BatteryInfo {
                    id: "bench".into(),
                    name: String::new(),
                    manufacturer: String::new(),
                    serial: String::new(),
                    chemistry: String::new(),
                    designed_capacity: None,
                    full_capacity: None,
                    cycle_count: None,
                    manufactured: None,
                }),
                percent: 82.,
                capacity: None,
                rate: Some(-5000),
                voltage: Some(12000),
                state: "discharging",
                plugged_in: false,
                critical: false,
                remaining: None,
                temperature: Some(36.),
            });
        }
        s.processes.truncate(40);
        for (j, p) in s.processes.iter_mut().enumerate() {
            p.counters_pending = false;
            p.stale = false;
            p.sample_reused = false;
            p.cpu = if j < 6 { 2. + j as f32 } else { 0. };
            p.private_ws = if (6..12).contains(&j) {
                1 << 30
            } else {
                1 << 20
            };
            p.io_read = if (12..15).contains(&j) { 1 << 24 } else { 0 };
            p.io_write = 0;
            p.io_other = 0;
            p.gpu = if (15..18).contains(&j) { 10. } else { 0. };
            p.net_sent = if (18..21).contains(&j) { 1 << 20 } else { 0 };
            p.net_received = 0;
            p.disk_read = Some(if (21..24).contains(&j) { 8 << 20 } else { 0 });
            p.disk_write = Some(1024);
            p.gpu_dedicated = Some(1 << 20);
            p.power = Some(p.cpu / 100.);
        }
    }
    let dir = root.join("month");
    fs::create_dir(&dir)?;
    std::thread::scope(|scope| -> io::Result<()> {
        let mut jobs = Vec::new();
        for lane in 0..4.min(day_count) {
            let dir = &dir;
            let mut samples = snapshots.clone();
            jobs.push(scope.spawn(move || -> io::Result<()> {
                for day in (lane..day_count).step_by(4) {
                    let work = dir.join(format!("day-{day}"));
                    fs::create_dir(&work)?;
                    let mut rec = HistoryRecorder::new(work.clone());
                    for i in 0..DAY / FRAME_MS {
                        let n = samples.len();
                        let s = &mut samples[i as usize % n];
                        s.timestamp = base + day * DAY + i * FRAME_MS;
                        s.system.uptime_ms = (s.timestamp - base + DAY) as u64;
                        rec.record(s);
                    }
                    drop(rec);
                    for file in fs::read_dir(&work)? {
                        let file = file?;
                        fs::rename(file.path(), dir.join(file.file_name()))?;
                    }
                    fs::remove_dir(work)?;
                }
                Ok(())
            }));
        }
        for job in jobs {
            job.join()
                .map_err(|_| io::Error::other("replay worker stopped"))??;
        }
        Ok(())
    })?;
    if merge_day {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        let raw = history::usage(&dir).bytes;
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) }
            .map_err(io::Error::other)?;
        for trial in 1..=3 {
            let copy = root.join(format!("merge-{trial}"));
            fs::create_dir(&copy)?;
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                fs::copy(entry.path(), copy.join(entry.file_name()))?;
            }
            let start = Instant::now();
            history::maintain(
                &copy,
                history::HistoryOptions {
                    limit: history::HistoryLimit {
                        days: None,
                        bytes: None,
                    },
                    ..Default::default()
                },
                i64::MIN,
            )?;
            let elapsed = start.elapsed();
            let merged = history::usage(&copy).bytes;
            let points = history::series(&copy, base, base + DAY - 1, 5000, 0, &["cpu".into()]);
            if points.span.len() != 1440 || points.span.iter().any(|&s| s != 60000) {
                return Err(io::Error::other("incomplete minute replay"));
            }
            println!(
                "merge: 17280 frames -> {} minutes, {} -> {} bytes, {:.3}% retained, {:.3} ms/day",
                points.span.len(),
                raw,
                merged,
                100. * merged as f64 / raw as f64,
                elapsed.as_secs_f64() * 1000.
            );
        }
        return Ok(());
    }
    let end = base + 30 * DAY;
    let keys = [
        "cpu",
        "cpuPeak",
        "memUsed",
        "diskRead",
        "diskWrite",
        "netReceive",
        "netSend",
        "gpu",
    ]
    .map(str::to_owned);
    for (label, width, bucket) in [
        ("1h", 3_600_000, 10_000),
        ("7d", 7 * DAY, 900_000),
        ("30d", 30 * DAY, 3_600_000),
    ] {
        print!("series {label} 8 keys: ");
        let points = measure(|| history::series(&dir, end - width, end - 1, bucket, 0, &keys));
        println!("{} points", points.ts.len());
    }
    let all: Vec<_> = history::channels(&dir, base, base + FRAME_MS)
        .into_iter()
        .map(|c| c.key)
        .collect();
    print!("series 30d, all {} channels: ", all.len());
    measure(|| history::series(&dir, base, end - 1, 3_600_000, 0, &all));
    print!("table 30d, 96 bins: ");
    let t = measure(|| history::table(&dir, base, end, "disk", 96));
    println!("{} instances, {} ms", t.processes.len(), t.recorded);
    print!("table 30d, cropped boundaries, 97 bins: ");
    measure(|| history::table(&dir, base + 1234, end - 2345, "cpu", 97));
    print!("channel catalog 30d: ");
    measure(|| history::channels(&dir, base, end - 1));
    let u = history::usage(&dir);
    println!(
        "full replay: {} days, {:.3} decimal MB/day, system={}, processes={}",
        u.days,
        u.bytes as f64 / 30e6,
        u.breakdown.system,
        u.breakdown.processes
    );
    Ok(())
}
fn main() -> io::Result<()> {
    let secs = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(60)
        .max(1);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let root = std::env::temp_dir().join(format!("argus-history-{}-{stamp}", std::process::id()));
    fs::create_dir(&root)?;
    let dir = root.join("live");
    let mut collector = Collector::new(MetaStore::new());
    let mut rec = HistoryRecorder::new(dir.clone());
    let mut times = Vec::new();
    let mut snaps = Vec::new();
    let mut last = 0;
    let mut baseline = None;
    let mut writes = Vec::new();
    let mut samples = Vec::new();
    let mut collection = Vec::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_secs(1));
        let sampling = Instant::now();
        let s = collector.sample();
        collection.push(sampling.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        rec.record(&s);
        let cost = t.elapsed().as_secs_f64() * 1e6;
        if last != 0 {
            if last / FRAME_MS != s.timestamp / FRAME_MS {
                writes.push(cost);
            } else {
                samples.push(cost);
            }
        }
        times.push(cost);
        last = s.timestamp;
        if start.elapsed().as_secs() >= 15 && baseline.is_none() {
            baseline = history::range(&dir).map(|(_, t)| (t, history::usage(&dir).bytes));
        }
        if snaps.len() < 12 && times.len() > 1 {
            snaps.push(s);
        }
    }
    if times.is_empty() {
        return Err(io::Error::other("no snapshots collected"));
    }
    times.sort_by(f64::total_cmp);
    println!(
        "record: {} samples; mean {:.1} us, median {:.1} us, max {:.1} us",
        times.len(),
        times.iter().sum::<f64>() / times.len() as f64,
        times[times.len() / 2],
        times.last().unwrap()
    );
    let keys = [
        "cpu",
        "cpuPeak",
        "memUsed",
        "diskRead",
        "diskWrite",
        "netReceive",
        "netSend",
        "gpu",
    ]
    .map(str::to_owned);
    for (label, mut values) in [("in-memory sample", samples), ("frame boundary", writes)] {
        values.sort_by(f64::total_cmp);
        if !values.is_empty() {
            println!(
                "{label}: n={}, median {:.1} us, max {:.1} us",
                values.len(),
                values[values.len() / 2],
                values[values.len() - 1]
            );
        }
    }
    collection.sort_by(f64::total_cmp);
    println!(
        "collector median {:.2} ms; recorder p95 {:.1} us",
        collection[collection.len() / 2],
        times[(times.len() - 1) * 95 / 100]
    );
    if let (Some((a, bytes)), Some((_, b))) = (baseline, history::range(&dir))
        && b > a
    {
        println!(
            "live steady frame growth {:.3} decimal MB/day",
            (history::usage(&dir).bytes - bytes) as f64 * DAY as f64 / (b - a) as f64 / 1e6
        );
    }
    print!("incremental live (5 seconds): ");
    measure(|| history::series(&dir, last - 5000, last, 1000, 0, &keys));
    drop(rec);
    let u = history::usage(&dir);
    let r = history::range(&dir).ok_or_else(|| io::Error::other("no recorded frames"))?;
    let n = r.1.div_euclid(FRAME_MS) - r.0.div_euclid(FRAME_MS) + 1;
    println!(
        "live: {n} frames, {} bytes including dictionaries, pending frame and summaries",
        u.bytes
    );
    let merge_day = std::env::args().any(|a| a == "--merge-day");
    if merge_day || std::env::args().any(|a| a == "--month") {
        replay(&root, snaps, merge_day)?;
    }
    fs::remove_dir_all(root)
}
