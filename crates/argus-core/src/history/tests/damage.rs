use super::*;

const DAY: i64 = 20_737;
const TS: i64 = DAY * DAY_MS;

fn v4_files(dir: &Path) -> [Vec<u8>; 4] {
    let path = day_path(dir, DAY);
    let mut main = MAGIC.to_vec();
    let channel = HistoryChannel {
        key: "cpu".into(),
        unit: "percent".into(),
        first: TS,
        last: TS,
        ..Default::default()
    };
    packet(
        &mut main,
        CHANNEL,
        &serde_json::to_vec(&(0, channel)).unwrap(),
    )
    .unwrap();
    let id = Identity {
        name: "p".into(),
        pid: 7,
        created: 1,
    };
    packet(&mut main, INSTANCE, &serde_json::to_vec(&(0, id)).unwrap()).unwrap();
    packet(&mut main, PRESENCE, &[1]).unwrap();
    let rows = vec![0, 1, 0, 0, 128, 63, 1, 0, 0, 1, 2];
    let mut f = TS.to_le_bytes().to_vec();
    f.extend(1i64.to_le_bytes());
    f.extend((FRAME_MS as u32).to_le_bytes());
    f.extend(0u64.to_le_bytes());
    f.extend((rows.len() as u32).to_le_bytes());
    f.extend(1f32.to_le_bytes());
    packet(&mut main, V4_FRAME, &f).unwrap();
    let mut sys = SystemRoll::default();
    sys.add(TS, &[1.], 1.);
    sys.finish(&mut main).unwrap();
    let mut roll = Roll {
        first: TS,
        end: TS + FRAME_MS,
        recorded: FRAME_MS,
        ..Default::default()
    };
    decode_rows(&rows, |i, m| {
        roll.rows.insert(i, Totals::observation(m, TS, FRAME_MS));
    })
    .unwrap();
    let mut summary = Vec::new();
    roll.write(&mut summary).unwrap();
    let mut cp = Checkpoints::open(path.with_extension("idx"), &main).unwrap();
    cp.commit().unwrap();
    drop(cp);
    [
        main,
        rows,
        fs::read(path.with_extension("idx")).unwrap(),
        summary,
    ]
}

fn exercise(dir: &Path) {
    let keys = ["cpu".into(), "missing".into()];
    std::hint::black_box(super::super::events(dir, TS, TS + ROLL_MS));
    std::hint::black_box(series(dir, TS, TS + ROLL_MS, ROLL_MS, TS, &keys));
    std::hint::black_box(channels(dir, TS, TS + ROLL_MS));
    std::hint::black_box(frame(dir, TS));
    std::hint::black_box(table(dir, TS, TS + ROLL_MS, "cpu", 1));
    std::hint::black_box(usage(dir));
    std::hint::black_box(range(dir));
    std::hint::black_box(legacy_series(dir, TS, TS + ROLL_MS, 4));
    std::hint::black_box(moment(dir, TS));
}

#[test]
fn corrupt_run_address_reads_the_primary_and_matches_its_frames() {
    let dir = temp_dir("run-address");
    fs::create_dir_all(&dir).unwrap();
    let path = day_path(&dir, DAY);
    let [main, rows, mut idx, _] = v4_files(&dir);
    fs::write(&path, &main).unwrap();
    fs::write(path.with_extension("proc"), &rows).unwrap();
    let expected = || serde_json::to_value(frame(&dir, TS).unwrap()).unwrap();
    let intact = expected();
    let (_, block) = next_packet(&idx).unwrap();
    let mut offset = None;
    packets(&block[8..], 0, |at, tag, _| {
        if tag == FRAME_RUN {
            offset = Some(5 + 8 + at + 5 + 52);
        }
        true
    });
    let at = offset.unwrap();
    for addr in [0, 6, main.len() as u32, u32::MAX] {
        idx[at..at + 4].copy_from_slice(&addr.to_le_bytes());
        fs::write(path.with_extension("idx"), &idx).unwrap();
        forget(&dir);
        assert_eq!(expected(), intact);
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn device_churn_and_a_stalled_clock_have_bounded_live_storage() {
    let dir = temp_dir("bounded-live");
    let mut recorder = HistoryRecorder::new(dir.clone());
    let sample = snapshot(TS, 1., Vec::new());
    recorder.record(&sample);
    let held = live(&dir);
    for _ in 0..MAX_RECENT + 1 {
        recorder.record(&sample);
    }
    assert_eq!(live(&dir).samples.len(), MAX_RECENT);
    assert_eq!(held.samples.len(), 1);
    for i in 0..MAX_CHANNELS + 1 {
        recorder
            .catalog
            .register(&format!("churn-{i}"), "count", "system", "", None, TS);
    }
    assert_eq!(recorder.catalog.rows.len(), MAX_CHANNELS);
    assert_eq!(recorder.catalog.by_key.len(), MAX_CHANNELS);
    recorder.record(&snapshot(TS + DAY_MS, 2., Vec::new()));
    assert!(recorder.catalog.rows.len() < MAX_CHANNELS);
    let pinned = live(&dir);
    recorder.clear().unwrap();
    assert!(live(&dir).samples.is_empty());
    assert!(!pinned.samples.is_empty());
    drop(recorder);
    fs::remove_dir_all(dir).unwrap();
}

struct Random(u64);

#[test]
fn rejects_extreme_query_arguments_and_preserves_negative_dates() {
    let dir = temp_dir("extreme");
    let base = -DAY_MS + FRAME_MS;
    let mut rec = HistoryRecorder::new(dir.clone());
    rec.record(&snapshot(base, 7., vec![proc(7, "p", 1., 4096)]));
    drop(rec);
    let keys = ["cpu".into()];
    for (from, to, bucket) in [
        (base + 1, base, 1000),
        (base, base, 0),
        (base, base, i64::MIN),
        (base, base, i64::MAX),
        (i64::MIN, i64::MAX, 1000),
        (base, base + 100_000 * 1000, 1000),
    ] {
        assert!(
            series(&dir, from, to, bucket, i64::MAX, &keys)
                .ts
                .is_empty()
        );
    }
    for origin in [i64::MIN, i64::MAX] {
        assert_eq!(
            series(&dir, base, base + 1000, 5000, origin, &keys).series["cpu"].mean,
            [Some(7.)]
        );
    }
    assert!(channels(&dir, base + 1, base).is_empty());
    assert!(frame(&dir, i64::MIN).is_none());
    assert!(moment(&dir, i64::MAX).is_none());
    assert!(
        table(&dir, i64::MIN, i64::MAX, "cpu", usize::MAX)
            .processes
            .is_empty()
    );
    assert!(legacy_series(&dir, i64::MIN, i64::MAX, usize::MAX).is_empty());
    assert_eq!(legacy_series(&dir, i64::MIN, i64::MAX, 1)[0].cpu, 7.);
    assert_eq!(
        table(&dir, i64::MIN, i64::MAX, "cpu", 4096).processes[0]
            .trend
            .len(),
        4096
    );
    assert!(day_number("9223372036854775807-01-01").is_none());
    assert!(day_number("2001-02-31").is_none());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn damaged_accelerators_fall_back_and_reopen_keeps_the_committed_prefix() {
    let dir = temp_dir("accelerators");
    fs::create_dir_all(&dir).unwrap();
    let path = day_path(&dir, DAY);
    let [mut main, rows, mut idx, mut roll] = v4_files(&dir);
    // Point the checkpoint end inside a record. Corrupt the per-channel count
    // in the embedded roll and invalidate the last process total in the sidecar.
    idx[9..13].copy_from_slice(&17u32.to_le_bytes());
    let mut summary = 0;
    packets(&main, 5, |at, tag, _| {
        if tag == SYSTEM_ROLL {
            summary = at + 5;
        }
        true
    });
    main[summary + 36..summary + 40].copy_from_slice(&u32::MAX.to_le_bytes());
    roll.extend_from_slice(&[0, 0]);
    let len = (roll.len() - 5) as u32;
    roll[1..5].copy_from_slice(&len.to_le_bytes());
    let committed = main.len();
    packet(&mut main, CHANNEL, br#"[1,{"key":"cut"#).unwrap();
    for (ext, data) in [
        ("hist", &main),
        ("proc", &rows),
        ("idx", &idx),
        ("roll", &roll),
    ] {
        fs::write(path.with_extension(ext), data).unwrap();
    }
    assert_eq!(
        series(&dir, TS, TS + ROLL_MS, ROLL_MS, TS, &["cpu".into()]).series["cpu"].mean,
        [Some(1.)]
    );
    assert_eq!(table(&dir, TS, TS + ROLL_MS, "cpu", 1).processes[0].cpu, 1.);
    drop(open_day(&dir, DAY).unwrap());
    assert_eq!(fs::metadata(&path).unwrap().len(), committed as u64);
    assert_eq!(fs::metadata(path.with_extension("roll")).unwrap().len(), 0);
    let mut r = HistoryRecorder::new(dir.clone());
    r.record(&snapshot(TS + FRAME_MS, 9., vec![]));
    drop(r);
    assert_eq!(frame(&dir, TS + FRAME_MS).unwrap().system["cpu"], Some(9.));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn torn_process_tail_preserves_neighbors_and_drops_incomplete_rows() {
    let dir = temp_dir("proc-tail-neighbor");
    let mut rec = HistoryRecorder::new(dir.clone());
    for n in 0..2 {
        rec.record(&snapshot(
            TS + n * FRAME_MS,
            3.,
            vec![proc(1, "p", 2., 4096), proc(2, "q", 1., 4096)],
        ));
    }
    drop(rec);
    let path = day_path(&dir, DAY).with_extension("proc");
    let mut bytes = fs::read(&path).unwrap();
    let tail = bytes.pop().unwrap();
    fs::write(&path, &bytes).unwrap();
    assert_eq!(frame(&dir, TS).unwrap().processes.len(), 2);
    assert!(frame(&dir, TS + FRAME_MS).unwrap().processes.is_empty());
    bytes.push(tail | 128);
    fs::write(&path, &bytes).unwrap();
    assert!(frame(&dir, TS + FRAME_MS).unwrap().processes.is_empty());
    assert_eq!(
        table(&dir, TS, TS + 2 * FRAME_MS, "cpu", 1).processes[0].seen,
        FRAME_MS
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn oversized_main_is_left_intact() {
    let dir = temp_dir("large-main");
    fs::create_dir_all(&dir).unwrap();
    let path = day_path(&dir, DAY);
    let mut f = fs::File::create(&path).unwrap();
    f.write_all(MAGIC).unwrap();
    f.set_len(MAX_FILE_BYTES as u64 + 1).unwrap();
    drop(f);
    assert!(range(&dir).is_none());
    assert!(open_day(&dir, DAY).is_err());
    assert_eq!(fs::metadata(path).unwrap().len(), MAX_FILE_BYTES as u64 + 1);
    fs::remove_dir_all(dir).unwrap();
}
impl Random {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as usize
    }
    fn damage(&mut self, src: &[u8], case: usize) -> Vec<u8> {
        let mut b = src.to_vec();
        let at = self.next() % b.len();
        match case % 5 {
            0 => b.truncate(at),
            1 => b[at] ^= 1 << (self.next() % 8),
            2 => {
                b.resize(self.next() % 192, 0);
                b.iter_mut().for_each(|v| *v = self.next() as u8);
            }
            3 => {
                let end = (at + 1 + self.next() % 64).min(b.len());
                b[at..end].iter_mut().for_each(|v| *v = self.next() as u8);
            }
            _ => b.extend((0..self.next() % 32 + 1).map(|_| self.next() as u8)),
        }
        b
    }
}

#[test]
fn a_corrupt_process_frame_never_replaces_the_source_day() {
    let dir = temp_dir("merge-invalid");
    fs::create_dir_all(&dir).unwrap();
    let path = day_path(&dir, DAY);
    let [main, rows, ..] = v4_files(&dir);
    let mut invalid_main = MAGIC.to_vec();
    let duplicated = [rows.as_slice(), rows.as_slice()].concat();
    packets(&main, 5, |_, tag, p| {
        let mut payload = p.to_vec();
        if tag == V4_FRAME {
            payload[28..32].copy_from_slice(&(duplicated.len() as u32).to_le_bytes());
        }
        packet(&mut invalid_main, tag, &payload).unwrap();
        true
    });
    fs::write(&path, &invalid_main).unwrap();
    fs::write(path.with_extension("proc"), &duplicated).unwrap();
    assert!(minute::merge(&dir, DAY, 20).is_err());
    assert_eq!(fs::read(&path).unwrap(), invalid_main);
    assert_eq!(fs::read(path.with_extension("proc")).unwrap(), duplicated);
    assert!(!path.with_extension("merge-tmp").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn days_of_other_versions_are_neither_read_nor_merged() {
    let dir = temp_dir("other-version-merge");
    fs::create_dir_all(&dir).unwrap();
    let path = day_path(&dir, DAY);
    fs::write(&path, b"ARGH\x03\x02\x00\x00\x00\x00").unwrap();
    assert!(!minute::is_current(&path));
    assert!(minute::merge(&dir, DAY, 0).is_err());
    assert!(!path.with_extension("merge-tmp").exists());
    maintenance::maintain_at(
        &dir,
        HistoryOptions {
            detail: HistoryDetail::Minute7,
            ..Default::default()
        },
        DAY + 8,
        DAY + 8,
        || false,
    )
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"ARGH\x03\x02\x00\x00\x00\x00");
    assert!(frame(&dir, TS).is_none());
    assert!(table(&dir, TS, TS + 60_000, "cpu", 0).processes.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn damaged_files_through_queries_and_reopen() {
    let disk_only = std::env::var("ARGUS_HISTORY_TEST_REAL_FILES").is_ok_and(|v| v == "1");
    std::thread::scope(|scope| {
        for kind in 2usize..7 {
            for lane in 0..2 {
                scope.spawn(move || {
                    let dir = temp_dir(&format!("damage-{kind}-{lane}"));
                    fs::create_dir_all(&dir).unwrap();
                    let path = day_path(&dir, DAY);
                    let mut files = v4_files(&dir);
                    let which = if kind == 6 { 0 } else { kind.saturating_sub(2) };
                    let paths = ["hist", "proc", "idx", "roll"].map(|ext| path.with_extension(ext));
                    if kind == 6 {
                        for (p, b) in paths.iter().zip(&files) {
                            fs::write(p, b).unwrap();
                        }
                        minute::merge(&dir, DAY, 20).unwrap();
                        files = [fs::read(&path).unwrap(), Vec::new(), Vec::new(), Vec::new()];
                    }
                    let mut rng = Random(0x4217_05bf + (kind * 2 + lane) as u64);
                    for case in (lane..2048).step_by(2) {
                        // The first ten cases per lane use real files. The full corpus
                        // exercises the same query, seek, truncate and reopen code with
                        // memory-backed bytes to keep the debug test fast on Windows.
                        if !disk_only && case / 2 == 10 {
                            test_files::install(&dir, DAY);
                        }
                        for (i, p) in paths.iter().enumerate() {
                            let bytes = if i == which {
                                rng.damage(&files[i], case)
                            } else if kind == 2 && i == 2 {
                                // Exercise primary damage without an intact checkpoint masking it.
                                Vec::new()
                            } else {
                                files[i].clone()
                            };
                            let mut file = open_writable(p).unwrap();
                            file.set_len(0).unwrap();
                            file.write_all(&bytes).unwrap();
                        }
                        INDEXES.lock().unwrap().remove(&path);
                        exercise(&dir);
                        // This is the same repairing open used by HistoryRecorder::open.
                        drop(open_day(&dir, DAY));
                    }
                    forget(&dir);
                    fs::remove_dir_all(dir).unwrap();
                });
            }
        }
    });
}
