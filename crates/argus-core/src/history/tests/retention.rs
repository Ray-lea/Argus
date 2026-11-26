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
        fs::write(dir.join(crate::watch::FILE_NAME), [0; 5]).unwrap();
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
        assert_eq!(fs::read_dir(&dir).unwrap().count(), expected.len() * 5 + 3);
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

#[test]
fn custom_storage_preserves_unowned_sidecars() {
    let root = temp_dir("directory-ownership");
    let default = root.join("default");
    let picked = root.join("shared");
    fs::create_dir(&picked).unwrap();
    let day = maintenance::today();
    let foreign = [
        day_path(&picked, day - 40).with_extension("idx"),
        day_path(&picked, day).with_extension("proc"),
    ];
    for p in &foreign {
        fs::write(p, [0, 8, 255, 3]).unwrap();
    }
    let actual = select_directory(&default, &default, Some(&picked)).unwrap();
    assert_eq!(actual, picked.join("Argus History"));
    assert_eq!(prepare_directory(&default, Some(&picked)).unwrap(), actual);
    let owned_old = day_path(&actual, day - 40).with_extension("idx");
    fs::write(&owned_old, b"expired").unwrap();
    for path in [&picked, &actual] {
        let mut rec = HistoryRecorder::start(path.clone());
        rec.record(&snapshot(day * DAY_MS, 1., vec![]));
        drop(rec);
        maintain(path, HistoryOptions::default(), day).unwrap();
    }
    assert!(!owned_old.exists());
    assert!(clear(&picked).is_err());
    assert!(move_records(&picked, &actual, || Ok(())).is_err());
    assert!(day_path(&actual, day).is_file());
    for p in foreign {
        assert_eq!(fs::read(p).unwrap(), [0, 8, 255, 3]);
    }
    let empty = root.join("empty");
    assert_eq!(prepare_directory(&default, Some(&empty)).unwrap(), empty);
    fs::write(empty.join("notes"), b"keep").unwrap();
    assert_eq!(prepare_directory(&default, Some(&empty)).unwrap(), empty);
    fs::create_dir(&default).unwrap();
    fs::write(default.join("old"), b"keep").unwrap();
    assert_eq!(prepare_directory(&default, None).unwrap(), default);
    fs::remove_dir_all(root).unwrap();
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
    prepare_directory(&old, None).unwrap();
    select_directory(&old, &old, Some(&new)).unwrap();
    let files = [
        "2026-01-01.hist",
        "2026-01-01.idx",
        "2026-01-01.proc",
        "2026-01-01.roll",
        "2026-01-01.measure",
        crate::watch::FILE_NAME,
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
    assert_eq!(fs::read_dir(&new).unwrap().count(), 1);
    move_records(&old, &new, || Ok(())).unwrap();
    for name in files {
        assert_eq!(fs::read(new.join(name)).unwrap(), name.as_bytes());
    }
    assert_eq!(fs::read_dir(&old).unwrap().count(), 2);
    fs::remove_dir_all(new).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recovery_discards_only_unpublished_partial_copies() {
    let dest = temp_dir("incomplete-copy");
    sample_day(&dest, DAY - 1);
    let kept = day_path(&dest, DAY - 1);
    let before = fs::read(&kept).unwrap();
    let stage = dest.join(".argus-history-copy");
    prepare_directory(&stage, None).unwrap();
    fs::write(day_path(&stage, DAY), b"partial").unwrap();
    prepare_directory(&dest, None).unwrap();
    assert!(!stage.exists());
    assert!(!day_path(&dest, DAY).exists());
    assert_eq!(fs::read(kept).unwrap(), before);
    fs::remove_dir_all(dest).unwrap();
}

fn volume_id(path: &Path) -> u32 {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::GetFileInformationByHandle};
    let marker = owned_directory(path).unwrap();
    let mut info = Default::default();
    unsafe { GetFileInformationByHandle(HANDLE(marker.as_raw_handle()), &mut info) }.unwrap();
    info.dwVolumeSerialNumber
}

fn interrupt_after_primary(tag: &str, destination_root: &Path, same_volume: bool) {
    let source = temp_dir(tag);
    let dest = destination_root.join(format!("{tag}-dest-{}", std::process::id()));
    prepare_directory(&dest, None).unwrap();
    assert_eq!(volume_id(&source) == volume_id(&dest), same_volume);
    let day = maintenance::today();
    sample_day(&source, day);
    sample_day(&dest, day - 1);
    let existing = day_path(&dest, day - 1);
    let existing_bytes = fs::read(&existing).unwrap();
    let ts = day * DAY_MS + FRAME_MS;
    let expected = serde_json::to_value(frame(&source, ts).unwrap()).unwrap();
    let primary = day_path(&source, day);
    let processes = fs::read(primary.with_extension("proc")).unwrap();
    use std::os::windows::process::CommandExt;
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0)
        .args([
            "--exact",
            "history::tests::retention::migration_exit_child",
            "--ignored",
        ])
        .env("ARGUS_MOVE_TEST_SOURCE", &source)
        .env("ARGUS_MOVE_TEST_DEST", &dest)
        .env("ARGUS_MOVE_TEST_PRIMARY", primary.file_name().unwrap())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(86));
    assert!(day_path(&dest, day).is_file());
    assert!(!day_path(&dest, day).with_extension("proc").exists());
    assert_eq!(fs::read(primary.with_extension("proc")).unwrap(), processes);
    // The saved setting still selects the source. The destination finishes
    // publication when next opened, even with the original source offline.
    for path in [&source, &dest] {
        prepare_directory(path, None).unwrap();
        drop(HistoryRecorder::start(path.clone()));
        assert_eq!(
            serde_json::to_value(frame(path, ts).unwrap()).unwrap(),
            expected
        );
        assert_eq!(
            fs::read(day_path(path, day).with_extension("proc")).unwrap(),
            processes
        );
    }
    assert_eq!(fs::read(&existing).unwrap(), existing_bytes);
    fs::remove_dir_all(source).unwrap();
    fs::remove_dir_all(dest).unwrap();
}

#[test]
#[ignore = "child process for the migration interruption tests"]
fn migration_exit_child() {
    let Some(source) = std::env::var_os("ARGUS_MOVE_TEST_SOURCE").map(PathBuf::from) else {
        return;
    };
    let dest = PathBuf::from(std::env::var_os("ARGUS_MOVE_TEST_DEST").unwrap());
    let primary = std::env::var_os("ARGUS_MOVE_TEST_PRIMARY").unwrap();
    migration::move_with(
        &source,
        &dest,
        || Ok(()),
        |name| {
            if name == primary {
                std::process::exit(86);
            }
        },
    )
    .unwrap();
}

#[test]
fn interrupted_move_on_same_volume() {
    interrupt_after_primary("interrupted-same", &std::env::temp_dir(), true);
}

#[test]
#[ignore = "requires ARGUS_HISTORY_TEST_SECOND_VOLUME on a second writable volume"]
fn interrupted_move_across_volumes() {
    let second = PathBuf::from(std::env::var_os("ARGUS_HISTORY_TEST_SECOND_VOLUME").unwrap());
    interrupt_after_primary("interrupted-cross", &second, false);
}

#[test]
fn rollback_during_recording_and_after_restart_keeps_sorted_frames() {
    let dir = temp_dir("rollback-clock");
    let write = |samples: &[i64]| {
        let mut rec = HistoryRecorder::new(dir.clone());
        for &n in samples {
            rec.record(&snapshot(
                BASE + n * FRAME_MS,
                n as f32,
                vec![proc(1, "p", 1., 4096)],
            ));
        }
    };
    write(&[3, 4, 2, 5]);
    write(&[1, 2, 3, 4, 5, 6]);
    let idx = index(&day_path(&dir, DAY)).unwrap();
    assert!(!idx.disordered);
    assert_eq!(
        idx.frames
            .iter()
            .map(|f| i64::from(f.0))
            .collect::<Vec<_>>(),
        [3, 4, 5, 6].map(|n| n * FRAME_MS)
    );
    assert_eq!(
        frame(&dir, BASE + 4 * FRAME_MS).unwrap().system["cpu"],
        Some(4.)
    );
    minute::merge(&dir, DAY, 20).unwrap();
    assert_eq!(frame(&dir, BASE).unwrap().system["cpu"], Some(4.5));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_disordered_day_queries_and_merges_past_the_overlap() {
    let dir = temp_dir("rollback-existing");
    sample_day(&dir, DAY);
    let path = day_path(&dir, DAY);
    let mut bytes = fs::read(&path).unwrap();
    let mut frames = Vec::new();
    packets(&bytes, 5, |at, tag, _| {
        if tag == FRAME {
            frames.push(at + 5);
        }
        true
    });
    bytes[frames[12]..frames[12] + 8].copy_from_slice(&(BASE + 2 * FRAME_MS).to_le_bytes());
    fs::write(&path, bytes).unwrap();
    fs::remove_file(path.with_extension("idx")).unwrap();
    let mut cp = Checkpoints::open(path.with_extension("idx"), &fs::read(&path).unwrap()).unwrap();
    cp.commit().unwrap();
    drop(cp);
    let idx = index(&path).unwrap();
    assert!(idx.disordered);
    assert_eq!(idx.frames.len(), 23);
    let query = series(
        &dir,
        BASE + 11 * FRAME_MS,
        BASE + 14 * FRAME_MS,
        1000,
        BASE,
        &["cpu".into()],
    );
    assert_eq!(query.ts, [11, 13, 14].map(|n| BASE + n * FRAME_MS));
    assert_eq!(
        table(&dir, BASE, BASE + 2 * 60_000, "cpu", 1).recorded,
        23 * FRAME_MS
    );
    assert_eq!(
        frame(&dir, BASE + 13 * FRAME_MS).unwrap().system["cpu"],
        Some(1.)
    );
    minute::merge(&dir, DAY, 20).unwrap();
    assert_eq!(frame(&dir, BASE + 60_000).unwrap().system["cpu"], Some(6.));
    fs::remove_dir_all(dir).unwrap();
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

#[test]
fn prunes_old_days() {
    let dir = temp_dir("prune");
    fs::create_dir_all(&dir).unwrap();
    for day in [20_729, 20_730, 20_736] {
        fs::write(day_path(&dir, day), MAGIC).unwrap();
    }
    maintenance::maintain_at(
        &dir,
        HistoryOptions {
            limit: HistoryLimit {
                days: Some(7),
                bytes: None,
            },
            detail: HistoryDetail::Full,
            ..Default::default()
        },
        20_737,
        20_737,
        || false,
    )
    .unwrap();
    assert_eq!(days(&dir), vec![20_736]);
    let _ = fs::remove_dir_all(&dir);
}
