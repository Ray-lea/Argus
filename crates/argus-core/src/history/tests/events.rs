use super::*;
use serde_json::{Value, json};

const DAY: i64 = 20_737;
const BASE: i64 = DAY * DAY_MS;
const M: i64 = MINUTE_MS;
const MB: f64 = 1_048_576.;
const HIGH: [f64; 9] = [85., 95., 90., 85., 100., 75., 15. * MB, 32. * MB, 48. * MB];
const LOW: [f64; 9] = [
    84.,
    99.,
    89.,
    84.,
    100.,
    74.,
    15. * MB - 1.,
    32. * MB,
    48. * MB - 8.,
];
const KINDS: [(&str, i64); 6] = [
    ("cpu", 3),
    ("temp", 2),
    ("mem", 1),
    ("gpu", 15),
    ("net", 3),
    ("disk", 3),
];

struct Day {
    number: i64,
    main: Vec<u8>,
    proc: Vec<u8>,
}
impl Day {
    fn new(number: i64) -> Self {
        let mut main = MAGIC.to_vec();
        for (i, key) in [
            "cpu",
            "cpuPeak",
            "cpuTemp",
            "memUsed",
            "memTotal",
            "gpu",
            "netReceive",
            "diskRead",
            "diskWrite",
        ]
        .into_iter()
        .enumerate()
        {
            let unit = match i {
                3 | 4 => "bytes",
                2 => "celsius",
                6..=8 => "bytesPerSecond",
                _ => "percent",
            };
            let c = HistoryChannel {
                key: key.into(),
                unit: unit.into(),
                first: number * DAY_MS,
                ..Default::default()
            };
            packet(&mut main, CHANNEL, &serde_json::to_vec(&(i, c)).unwrap()).unwrap();
        }
        for (i, name) in ["Build.exe", "build.exe", "video.exe"]
            .into_iter()
            .enumerate()
        {
            let id = Identity {
                name: name.into(),
                pid: i as u32 + 1,
                created: 1,
            };
            packet(&mut main, INSTANCE, &serde_json::to_vec(&(i, id)).unwrap()).unwrap();
        }
        Self {
            number,
            main,
            proc: Vec::new(),
        }
    }
    fn marker(&mut self, code: u8, boot: i64) {
        write_marker(&mut self.main, code, self.number * DAY_MS, boot, 0).unwrap();
    }
    fn frame(&mut self, offset: i64, awake: i64, values: [f64; 9], rows: &[u8]) {
        let mut payload = (self.number * DAY_MS + offset).to_le_bytes().to_vec();
        payload.extend(awake.to_le_bytes());
        payload.extend((FRAME_MS as u32).to_le_bytes());
        payload.extend((self.proc.len() as u64).to_le_bytes());
        payload.extend((rows.len() as u32).to_le_bytes());
        for (i, v) in values.into_iter().enumerate() {
            if matches!(i, 3 | 4) {
                payload.extend(v.to_le_bytes());
            } else {
                payload.extend((v as f32).to_le_bytes());
            }
        }
        packet(&mut self.main, V4_FRAME, &payload).unwrap();
        self.proc.extend(rows);
    }
    fn save(&self, dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        let path = day_path(dir, self.number);
        fs::write(path.with_extension("proc"), &self.proc).unwrap();
        fs::write(path, &self.main).unwrap();
    }
}

fn query(dir: &Path, from: i64, to: i64) -> Vec<Value> {
    let result = super::super::events(dir, from, to).unwrap();
    result
        .events
        .into_iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect()
}
fn of_kind<'a>(values: &'a [Value], kind: &str) -> Vec<&'a Value> {
    values.iter().filter(|e| e["kind"] == kind).collect()
}
fn process_rows() -> Vec<u8> {
    let mut out = Vec::new();
    for (i, (cpu, gpu, mem, net, read, write)) in [
        (30f32, 1f32, 10, 35, 5, 40),
        (25., 2., 20, 25, 0, 25),
        (45., 7., 70, 40, 30, 0),
    ]
    .into_iter()
    .enumerate()
    {
        var(&mut out, i as u64);
        out.push(15);
        out.extend(cpu.to_le_bytes());
        out.extend(gpu.to_le_bytes());
        put_memory(&mut out, mem);
        for v in [999, net, 1, 1, read, write] {
            var(&mut out, v);
        }
    }
    out
}

#[test]
fn threshold_duration_missing_and_process_attribution_in_raw_and_sealed_days() {
    let dir = temp_dir("event-thresholds");
    let rows = process_rows();
    let mut day = Day::new(DAY);
    for n in 0..48 {
        let v = match n {
            0..16 => LOW,
            16..32 => {
                let mut v = [f64::NAN; 9];
                v[1] = 99.;
                v[3] = 90.;
                v[4] = 0.;
                v[7] = 100. * MB;
                v
            }
            _ => HIGH,
        };
        for offset in [0, FRAME_MS] {
            day.frame(n * M + offset, n * M + offset, v, &rows);
        }
    }
    day.save(&dir);
    let start = BASE + 32 * M;
    for sealed in [false, true] {
        if sealed {
            minute::merge(&dir, DAY, 0).unwrap();
        }
        assert!(query(&dir, BASE, start).is_empty());
        for (kind, n) in KINDS {
            assert!(of_kind(&query(&dir, start, start + n * M - 1), kind).is_empty());
            let result = query(&dir, start, start + n * M);
            let matching = of_kind(&result, kind);
            let [e] = matching.as_slice() else {
                panic!("{kind} {result:?}");
            };
            let (peak, name, mean, share) = match kind {
                "cpu" => (95., "Build.exe", 55., 0.55),
                "temp" => (90., "Build.exe", 55., 0.55),
                "mem" => (85., "video.exe", 70., 0.7),
                "gpu" => (75., "video.exe", 7., 0.7),
                "net" => (15. * MB, "Build.exe", 60., 0.6),
                "disk" => (80. * MB, "Build.exe", 70., 0.7),
                _ => unreachable!(),
            };
            assert_eq!(
                **e,
                json!({"kind":kind,"from":start,"to":start+n*M,"peak":peak,"peakAt":start,
                "top":{"name":name,"mean":mean,"share":share}})
            );
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn only_independently_qualified_runs_merge_and_the_bridge_counts_for_peak_and_top() {
    let dir = temp_dir("event-merge");
    let mut day = Day::new(DAY);
    let rows = process_rows();
    for n in 0..23 {
        // 3 high, 3 low, 3 high, 4 low, 3 high; the last two runs are both too short.
        let high = matches!(n,0..=2|6..=8|13..=15|17..=18|20..=21);
        let mut v = if high { HIGH } else { LOW };
        v[1] = if n == 4 { 100. } else { 95. };
        day.frame(n * M, n * M, v, if n == 4 { &rows } else { &[] });
    }
    day.save(&dir);
    let result = query(&dir, BASE, BASE + 23 * M);
    let cpu = of_kind(&result, "cpu");
    assert_eq!(cpu.len(), 2);
    assert_eq!(
        (cpu[0]["from"].as_i64(), cpu[0]["to"].as_i64()),
        (Some(BASE + 13 * M), Some(BASE + 16 * M))
    );
    assert!(cpu[0]["top"].is_null());
    assert_eq!(
        (cpu[1]["from"].as_i64(), cpu[1]["to"].as_i64()),
        (Some(BASE), Some(BASE + 9 * M))
    );
    assert_eq!(cpu[1]["peak"], 100.);
    assert_eq!(cpu[1]["peakAt"], BASE + 4 * M);
    assert_eq!(cpu[1]["top"]["name"], "Build.exe");
    assert!((cpu[1]["top"]["mean"].as_f64().unwrap() - 55. / 9.).abs() < 1e-10);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn paired_frame_readings_and_sealed_peak_resolution() {
    let dir = temp_dir("event-means");
    let mut day = Day::new(DAY);
    for n in 0..3 {
        let mut a = HIGH;
        a[0] = 70.;
        a[1] = 92.;
        a[2] = f64::NAN;
        a[3] = 50.;
        a[4] = 100.;
        a[7] = 80. * MB;
        a[8] = 0.;
        let mut b = HIGH;
        b[0] = 100.;
        b[1] = 100.;
        b[2] = 94.;
        b[3] = 95.;
        b[4] = 100.;
        b[7] = 0.;
        b[8] = 80. * MB;
        day.frame(n * M, n * M, a, &[]);
        day.frame(n * M + FRAME_MS, n * M + FRAME_MS, b, &[]);
    }
    day.save(&dir);
    for sealed in [false, true] {
        if sealed {
            minute::merge(&dir, DAY, 0).unwrap();
        }
        let e = query(&dir, BASE, BASE + 3 * M);
        assert!(of_kind(&e, "mem").is_empty());
        assert_eq!(of_kind(&e, "disk")[0]["peak"], 80. * MB);
        let cpu = of_kind(&e, "cpu")[0];
        assert_eq!(cpu["peak"], 100.);
        assert_eq!(cpu["peakAt"], if sealed { BASE } else { BASE + FRAME_MS });
        assert_eq!(of_kind(&e, "temp")[0]["peak"], 94.);
        assert!(query(&dir, BASE + 10_000, BASE + M).is_empty());
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn restarted_and_sealed_days_join_at_midnight_and_keep_only_available_channels() {
    for (version, sealed_first) in [(2, false), (3, true), (4, true), (4, false)] {
        let dir = temp_dir(&format!("event-cross-{version}-{sealed_first}"));
        fs::create_dir_all(&dir).unwrap();
        let raw_day = DAY + i64::from(sealed_first);
        let sealed_day = DAY + i64::from(!sealed_first);
        for day in [raw_day, sealed_day] {
            // A day begun by a release writing an older version starts over.
            if day == raw_day && version < 4 {
                fs::write(day_path(&dir, day), [b'A', b'R', b'G', b'H', version]).unwrap();
            }
            let mut rec = HistoryRecorder::new(dir.clone());
            let start = if day == DAY { DAY_MS - 2 * M } else { 0 };
            for f in 0..24 {
                rec.record(&snapshot(
                    day * DAY_MS + start + f * FRAME_MS,
                    90.,
                    vec![proc(3, "x.exe", 40., 4096)],
                ));
            }
            drop(rec);
        }
        minute::merge(&dir, sealed_day, 0).unwrap();
        let midnight = BASE + DAY_MS;
        let result = query(&dir, midnight - 2 * M, midnight + 2 * M);
        assert_eq!(
            result,
            vec![
                json!({"kind":"cpu","from":midnight-2*M,"to":midnight+2*M,"peak":90.,"peakAt":midnight-2*M,
            "top":{"name":"x.exe","share":1.,"mean":40.}})
            ]
        );
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn marker_gaps_keep_exact_duration_reason_and_window_edges() {
    let dir = temp_dir("event-gaps");
    let mut day = Day::new(DAY);
    let mut t = 0;
    day.marker(1, 42);
    day.frame(t, t, LOW, &[]);
    let mut expected = Vec::new();
    for (duration, reason, boot, stop) in [
        (10 * M - 1, "notRunning", 42, 3),
        (10 * M, "notRunning", 42, 3),
        (10 * M, "sleep", 42, 3),
        (10 * M, "shutdown", 9000, 3),
        (10 * M, "off", 9000, 2),
        (10 * M, "unknown", 0, 3),
    ] {
        day.marker(stop, 0);
        day.marker(1, boot);
        let start = t + FRAME_MS;
        t = start + duration;
        let awake = if reason == "sleep" { start } else { t };
        day.frame(t, awake, LOW, &[]);
        if duration >= 10 * M {
            expected.push(json!({"kind":"gap","reason":reason,"from":BASE+start,"to":BASE+t}));
        }
    }
    day.save(&dir);
    expected.reverse();
    for sealed in [false, true] {
        if sealed {
            minute::merge(&dir, DAY, 0).unwrap();
        }
        assert_eq!(query(&dir, BASE - DAY_MS, BASE + DAY_MS), expected);
        assert!(query(&dir, BASE + t - 10 * M + 1, BASE + t).is_empty());
    }
    assert!(query(&dir, BASE - 2 * DAY_MS, BASE - DAY_MS).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn limit_keeps_newest_and_cached_days_refresh_after_append() {
    let dir = temp_dir("event-limit");
    let mut v = LOW;
    v[3] = 85.;
    for d in 0..2 {
        let mut day = Day::new(DAY + d);
        for n in (0..1440).step_by(5) {
            day.frame(n * M, n * M, v, &[]);
        }
        day.save(&dir);
    }
    let run = |to| super::super::events(&dir, BASE, to).unwrap();
    let exact = run(BASE + 2496 * M);
    assert_eq!(exact.events.len(), 500);
    assert!(!exact.truncated);
    let capped = run(BASE + 2 * DAY_MS);
    assert_eq!(capped.events.len(), 500);
    assert!(capped.truncated);
    let start = |e: &HistoryEvent| match e {
        HistoryEvent::Resource { from, .. } => *from,
        _ => panic!(),
    };
    assert_eq!(start(capped.events.first().unwrap()), BASE + 2875 * M);
    assert_eq!(start(capped.events.last().unwrap()), BASE + 380 * M);
    let mut day = Day::new(DAY + 2);
    day.frame(0, 0, LOW, &[]);
    day.save(&dir);
    let start = BASE + 2 * DAY_MS;
    assert!(query(&dir, start, start + DAY_MS).is_empty());
    day.frame(M, M, v, &[]);
    day.save(&dir);
    assert_eq!(query(&dir, start, start + DAY_MS)[0]["from"], start + M);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn damaged_process_blocks_preserve_events_and_damaged_system_minutes_break_runs() {
    let dir = temp_dir("event-corrupt");
    let mut day = Day::new(DAY);
    let rows = process_rows();
    for n in 0..10 {
        day.frame(
            n * M,
            n * M,
            if (3..7).contains(&n) { LOW } else { HIGH },
            &rows,
        );
    }
    day.save(&dir);
    let path = day_path(&dir, DAY);
    let side = path.with_extension("proc");
    fs::write(&side, &day.proc[..3 * rows.len()]).unwrap();
    let cpu = || {
        query(&dir, BASE, BASE + 10 * M)
            .into_iter()
            .filter(|e| e["kind"] == "cpu")
            .collect::<Vec<_>>()
    };
    let raw = cpu();
    assert_eq!(raw.len(), 2);
    assert!(raw[0]["top"].is_null());
    assert_eq!(raw[1]["top"]["name"], "Build.exe");
    fs::write(side, &day.proc).unwrap();
    minute::merge(&dir, DAY, 0).unwrap();
    let sealed = fs::read(&path).unwrap();
    let mut damaged = sealed.clone();
    packets(&sealed, 5, |offset, tag, p| {
        if tag == minute::MINUTE {
            let ts = i64::from_le_bytes(field(p, 0).unwrap());
            let cover = usize::from(u16::from_le_bytes(field(p, 8).unwrap())) * 4;
            let system_len = u32::from_le_bytes(field(p, 10).unwrap()) as usize;
            let start = offset + 5 + 14 + cover;
            if ts >= BASE + 7 * M {
                damaged[start + system_len] ^= 0xff;
            } else if ts == BASE + M {
                damaged[start] ^= 0xff;
            }
        }
        true
    });
    fs::write(&path, damaged).unwrap();
    let sealed = cpu();
    assert_eq!(sealed.len(), 1);
    assert_eq!(sealed[0]["from"], BASE + 7 * M);
    assert!(sealed[0]["top"].is_null());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn frames_written_without_program_counts_have_none() {
    let dir = temp_dir("event-no-programs");
    let mut day = Day::new(DAY);
    day.frame(0, 0, LOW, &[]);
    day.save(&dir);
    let f = frame(&dir, BASE).unwrap();
    assert!(f.programs.is_empty());
    assert_eq!(f.program_count, 0);
    assert_eq!(serde_json::to_value(&f).unwrap()["programs"], json!([]));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn days_of_other_versions_give_no_events() {
    let dir = temp_dir("event-other-version");
    fs::create_dir_all(&dir).unwrap();
    fs::write(day_path(&dir, DAY), b"ARGH\x02\x02\x00\x00\x00\x00").unwrap();
    assert!(query(&dir, BASE, BASE + 3 * M).is_empty());
    assert!(minute::merge(&dir, DAY, 0).is_err());
    assert!(query(&dir, BASE, BASE + 3 * M).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cold_and_cached_dense_days_preserve_order_and_midnight_runs() {
    let dir = temp_dir("event-dense-days");
    for d in 0..4 {
        let mut day = Day::new(DAY + d);
        for n in (0..360).chain(1080..1440) {
            let mut v = LOW;
            v[0] = if n == 0 || n >= 1438 { 90. } else { 10. };
            day.frame(n * M, n * M, v, &[]);
        }
        day.save(&dir);
    }
    let cold = query(&dir, BASE, BASE + 4 * DAY_MS);
    assert!(
        cold.windows(2)
            .all(|w| w[0]["from"].as_i64() >= w[1]["from"].as_i64())
    );
    let cpu = of_kind(&cold, "cpu");
    assert_eq!(cpu.len(), 3);
    for (e, d) in cpu.iter().zip([3, 2, 1]) {
        assert_eq!(e["from"], BASE + d * DAY_MS - 2 * M);
        assert_eq!(e["to"], BASE + d * DAY_MS + M);
    }
    assert_eq!(query(&dir, BASE, BASE + 4 * DAY_MS), cold);
    fs::remove_dir_all(dir).unwrap();
}
