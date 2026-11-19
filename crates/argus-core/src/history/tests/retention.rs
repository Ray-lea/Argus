use super::*;

const DAY: i64 = 20_737;
const BASE: i64 = DAY * DAY_MS;

fn options(days: Option<u32>, bytes: Option<u64>, detail: HistoryDetail) -> HistoryOptions {
    HistoryOptions {
        limit: HistoryLimit { days, bytes },
        detail,
        ..Default::default()
    }
}
#[test]
fn limits_evict_whole_oldest_days_and_protect_writer_and_today() {
    for (tag, days_limit, bytes, expected) in [
        ("days", Some(3), None, vec![DAY - 2, DAY - 1, DAY]),
        ("bytes", None, Some(150), vec![DAY - 1, DAY]),
        ("both-space", Some(4), Some(105), vec![DAY - 1, DAY]),
        ("both-age", Some(2), Some(500), vec![DAY - 1, DAY]),
        (
            "unlimited",
            None,
            None,
            vec![DAY - 4, DAY - 3, DAY - 2, DAY - 1, DAY],
        ),
        ("today", Some(0), Some(0), vec![DAY]),
    ] {
        let dir = temp_dir(tag);
        fs::create_dir_all(&dir).unwrap();
        for day in DAY - 4..=DAY {
            for ext in ["hist", "proc", "idx", "roll", "measure"] {
                fs::write(day_path(&dir, day).with_extension(ext), [0; 10]).unwrap();
            }
        }
        fs::write(dir.join("process-watch.jsonl"), [0; 5]).unwrap();
        fs::write(dir.join("notes"), [0; 200]).unwrap();
        maintenance::maintain_at(
            &dir,
            options(days_limit, bytes, HistoryDetail::Full),
            DAY,
            DAY,
            || false,
        )
        .unwrap();
        assert_eq!(days(&dir), expected);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), expected.len() * 5 + 2);
        maintenance::maintain_at(
            &dir,
            options(None, Some(0), HistoryDetail::Full),
            DAY + 1,
            DAY,
            || false,
        )
        .unwrap();
        assert_eq!(days(&dir), vec![DAY]);
        fs::remove_dir_all(dir).unwrap();
    }
}

fn sample_day(dir: &Path, day: i64) {
    let mut rec = HistoryRecorder::new(dir.into());
    for n in 0..24 {
        let v = (n % 12) as f32;
        let mut p = proc(7, "app", v, 4096 * (n + 1) as u64);
        p.disk_read = Some(100);
        p.net_received = 20;
        p.threads = n as u32;
        rec.record(&snapshot(day * DAY_MS + n * FRAME_MS, v, vec![p]));
    }
    drop(rec);
}

#[test]
fn closed_minute_day_serves_all_query_contracts() {
    let dir = temp_dir("minute-contract");
    sample_day(&dir, DAY);
    let raw_table = table(&dir, BASE, BASE + 120_000, "cpu", 2);
    minute::merge(&dir, DAY, 20).unwrap();
    let path = day_path(&dir, DAY);
    assert!(minute::is_merged(&path).unwrap());
    for ext in ["idx", "proc", "roll"] {
        assert!(!path.with_extension(ext).exists());
    }
    let keys = ["cpu".into(), "cpuPeak".into(), "memUsed".into()];
    let query = series(&dir, BASE + 1200, BASE + 119_999, 1000, 123, &keys);
    assert_eq!(query.merged_days, [BASE]);
    assert_eq!(query.span, [60_000; 2]);
    assert_eq!(query.ts, [BASE, BASE + 60_000]);
    assert_eq!(query.series["cpu"].mean, [Some(5.5); 2]);
    assert_eq!(query.series["cpu"].low, [Some(0.); 2]);
    assert_eq!(query.series["cpu"].high, [Some(11.); 2]);
    assert_eq!(query.series["cpuPeak"].mean, [Some(11.); 2]);
    let broad = series(&dir, BASE, BASE + 119_999, 120_000, BASE, &keys);
    assert_eq!(broad.merged_days, [BASE]);
    assert_eq!(broad.series["cpu"].high, [Some(11.)]);
    let nearest = frame(&dir, BASE + 34_000).unwrap();
    assert_eq!((nearest.ts, nearest.span), (BASE + 60_000, 60_000));
    assert_eq!(nearest.processes[0].threads, Some(23));
    assert_eq!(nearest.processes[0].cpu, 5.5);
    assert_eq!(nearest.processes[0].disk_read, Some(100.));
    assert_eq!(moment(&dir, BASE).unwrap().system.cpu, 5.5);
    assert_eq!(
        legacy_series(&dir, BASE, BASE + 120_000, 2)[1].cpu_peak,
        11.
    );
    assert!(
        channels(&dir, BASE + 1, BASE + 4000)
            .iter()
            .any(|c| c.key == "cpu")
    );
    let merged = table(&dir, BASE, BASE + 120_000, "cpu", 2);
    assert_eq!(
        serde_json::to_value(merged).unwrap(),
        serde_json::to_value(raw_table).unwrap()
    );
    assert_eq!(
        table(&dir, BASE + 500, BASE + 2500, "cpu", 4).recorded,
        2000
    );
    assert!(usage(&dir).breakdown.processes > 0);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recovery_before_and_after_atomic_publication_and_age_boundaries() {
    let dir = temp_dir("merge-recovery");
    for day in [DAY - 31, DAY - 30, DAY - 8, DAY - 7, DAY - 1, DAY] {
        sample_day(&dir, day);
    }
    let closed = day_path(&dir, DAY - 31);
    let raw = fs::read(&closed).unwrap();
    let budget = usage(&dir).bytes;
    fs::write(dir.join(".argus-history-maintenance.lock"), []).unwrap();
    fs::write(closed.with_extension("merge-tmp"), b"partial").unwrap();
    assert_eq!(frame(&dir, (DAY - 31) * DAY_MS).unwrap().span, FRAME_MS);
    assert_eq!(raw, fs::read(&closed).unwrap());
    let run = |detail| {
        maintenance::maintain_at(&dir, options(None, None, detail), DAY, DAY, || false).unwrap()
    };
    maintenance::maintain_at(
        &dir,
        options(None, Some(budget), HistoryDetail::Minute30),
        DAY,
        DAY,
        || false,
    )
    .unwrap();
    assert!(!closed.with_extension("merge-tmp").exists());
    assert!(minute::is_merged(&closed).unwrap());
    assert!(!minute::is_merged(&day_path(&dir, DAY - 30)).unwrap());
    fs::write(closed.with_extension("proc"), b"stale").unwrap();
    let published = fs::read(&closed).unwrap();
    assert_eq!(
        frame(&dir, (DAY - 31) * DAY_MS).unwrap().processes[0].pid,
        7
    );
    run(HistoryDetail::Minute7);
    assert_eq!(published, fs::read(&closed).unwrap());
    assert!(!closed.with_extension("proc").exists());
    assert!(minute::is_merged(&day_path(&dir, DAY - 8)).unwrap());
    for day in [DAY - 7, DAY - 1, DAY] {
        assert!(!minute::is_merged(&day_path(&dir, day)).unwrap());
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn move_validates_and_rolls_back_collisions_and_failed_settings_writes() {
    let root = temp_dir("move-rollback");
    let old = root.join("old");
    let new = std::env::var_os("ARGUS_HISTORY_TEST_SECOND_VOLUME")
        .map(PathBuf::from)
        .map(|p| p.join(format!("argus-history-move-{}", std::process::id())))
        .unwrap_or_else(|| root.join("new"));
    fs::create_dir_all(&old).unwrap();
    fs::write(root.join("file"), b"keep").unwrap();
    for bad in [
        old.clone(),
        root.clone(),
        old.join("child"),
        root.join("file"),
        PathBuf::from("relative"),
    ] {
        assert!(validate_directory(&old, &bad).is_err());
    }
    assert!(!old.join("child").exists());
    validate_directory(&old, &new).unwrap();
    let files = [
        "2026-01-01.hist",
        "2026-01-01.idx",
        "2026-01-01.proc",
        "2026-01-01.roll",
        "2026-01-01.measure",
        "process-watch.jsonl",
        "program-watch.v1.bin",
    ];
    for name in files {
        fs::write(old.join(name), name).unwrap();
    }
    fs::write(old.join("unrelated"), b"keep").unwrap();
    fs::write(new.join(files[5]), b"existing").unwrap();
    assert!(move_records(&old, &new, || Ok(())).is_err());
    assert_eq!(fs::read(new.join(files[5])).unwrap(), b"existing");
    fs::remove_file(new.join(files[5])).unwrap();
    assert!(move_records(&old, &new, || Err::<(), _>(invalid())).is_err());
    for name in files {
        assert_eq!(fs::read(old.join(name)).unwrap(), name.as_bytes());
    }
    assert_eq!(fs::read_dir(&new).unwrap().count(), 0);
    move_records(&old, &new, || Ok(())).unwrap();
    for name in files {
        assert_eq!(fs::read(new.join(name)).unwrap(), name.as_bytes());
    }
    assert_eq!(fs::read_dir(&old).unwrap().count(), 1);
    fs::remove_dir_all(new).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recording_profile_changes_split_the_pending_frame_and_preserve_old_readings() {
    let dir = temp_dir("switch-profile");
    let mut rec = HistoryRecorder::new(dir.clone());
    let rows: Vec<_> = (1..=80)
        .map(|i| {
            let mut p = proc(i, "proc", 1., 4096);
            p.disk_read = Some(20);
            p.gpu_dedicated = Some(100);
            p.net_received = 500;
            p
        })
        .collect();
    rec.record(&snapshot(BASE, 1., rows.clone()));
    let groups = RecordGroups {
        cpu_extra: false,
        sensors: false,
        memory_detail: false,
        counts: false,
        disks: false,
        nics: false,
        gpus: false,
        battery_detail: false,
        process_disk: false,
        process_network: false,
        process_gpu: false,
    };
    rec.configure(Recording {
        groups,
        processes: 0,
    });
    rec.record(&snapshot(BASE + 1000, 2., rows.clone()));
    rec.record(&snapshot(BASE + FRAME_MS, 3., rows));
    drop(rec);
    let before = frame(&dir, BASE).unwrap();
    assert!(before.processes.len() <= 20 && before.system.contains_key("cpuKernel"));
    assert_eq!(before.processes[0].disk_read, Some(20.));
    let after = frame(&dir, BASE + 1000).unwrap();
    assert_eq!(after.processes.len(), 80);
    assert!(!after.system.contains_key("cpuKernel"));
    assert!(!after.system.contains_key("batteryVoltage"));
    assert!(after.system.contains_key("batteryRate"));
    let p = serde_json::to_value(&after.processes[0]).unwrap();
    for key in ["diskRead", "net", "gpuMem", "threads", "handles"] {
        assert!(p[key].is_null());
    }
    let u = usage_with(
        &dir,
        HistoryOptions {
            recording: Recording {
                groups,
                processes: 0,
            },
            ..Default::default()
        },
    );
    let measured: Measurement =
        serde_json::from_slice(&fs::read(day_path(&dir, DAY).with_extension("measure")).unwrap())
            .unwrap();
    assert_eq!(
        u.estimate.bytes_per_day,
        (measured.bytes as f64 / measured.frames as f64 * 17_280.).round() as u64
    );
    assert!(u.estimate.bytes_per_day > 0);
    minute::merge(&dir, DAY, 0).unwrap();
    let merged = frame(&dir, BASE).unwrap();
    let retained = merged
        .processes
        .iter()
        .find(|p| p.pid == before.processes[0].pid)
        .unwrap();
    assert_eq!(retained.net, 500.);
    let totals = table(&dir, BASE, BASE + 60_000, "net", 1);
    let retained = totals
        .processes
        .iter()
        .find(|p| p.pid == before.processes[0].pid)
        .unwrap();
    assert_eq!(retained.trend, [Some(500.)]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn caps_keep_leaders_from_each_resource_and_all_measured_instances() {
    let rows: Vec<_> = (0..120).collect();
    let resource = |i: usize, k: usize| if i / 20 == k { (i % 20 + 1) as f64 } else { 0. };
    for (cap, want) in [(20, 20), (40, 40), (0, 120)] {
        let picked = select(rows.clone(), cap, resource, |i| (i as u32, 0));
        assert_eq!(picked.len(), want);
        assert!((0..6).all(|k| picked.contains(&(k * 20 + 19))));
    }
}
