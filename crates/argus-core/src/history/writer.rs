//! Writing a day: frames, their process rows and summaries appended to the
//! day's files, and a day opened again after a torn write.
use super::*;

pub(super) struct DayFile {
    pub(super) recorded_until: Option<i64>,
    pub(super) day: i64,
    pub(super) out: BufWriter<File>,
    pub(super) writer: DayWriter,
    _owner: fs::File,
}

impl DayFile {
    pub(super) fn marker(&mut self, kind: u8, ts: i64, boot: i64, awake: i64) -> io::Result<()> {
        let mut bytes = Vec::new();
        write_marker(&mut bytes, kind, ts, boot, awake)?;
        self.out.write_all(&bytes)?;
        self.out.flush()?;
        self.writer.index_append(&bytes);
        Ok(())
    }
}

pub(super) struct DayWriter {
    checkpoints: Option<Checkpoints>,
    presence: Vec<u8>,
    system: SystemRoll,
    roll: Roll,
    roll_network: bool,
    roll_file: BufWriter<File>,
    proc: BufWriter<File>,
    pos: u64,
    catalog: Catalog,
    ids: HashMap<Identity, u32>,
    next_id: u32,
    /// Program ids by lower-case name (`PROGRAM`).
    program_ids: HashMap<String, u32>,
}

impl DayWriter {
    pub(super) fn frame(
        &mut self,
        out: &mut impl Write,
        catalog: &Catalog,
        w: &Window,
        picked: &[&ProcAcc],
        groups: RecordGroups,
    ) -> io::Result<u64> {
        let mut metadata = Vec::new();
        let mut summary_bytes = 0;
        let index_before = self.checkpoints.as_ref().map_or(0, |c| c.written);
        if self.system.count > 0
            && self.system.start.div_euclid(ROLL_MS) != w.start.div_euclid(ROLL_MS)
        {
            self.system.finish(&mut metadata)?;
        }
        let mut values = vec![f64::NEG_INFINITY; self.catalog.rows.len()];
        for (i, c) in catalog
            .rows
            .iter()
            .enumerate()
            .filter(|(i, _)| w.present.get(*i) == Some(&true))
        {
            let known = self.catalog.by_key.get(&c.key).copied();
            let changed = known.is_none_or(|j| self.catalog.rows[j].label != c.label);
            let Some(j) = self.catalog.register(
                &c.key,
                &c.unit,
                &c.group,
                &c.label,
                c.device.as_deref(),
                w.start,
            ) else {
                continue;
            };
            if changed {
                let mut row = self.catalog.rows[j].clone();
                row.first = w.start;
                row.last = w.start;
                packet(&mut metadata, CHANNEL, &serde_json::to_vec(&(j, row))?)?;
            }
            values.resize(self.catalog.rows.len(), f64::NEG_INFINITY);
            values[j] = if c.key == "cpuPeak" {
                w.cpu_peak as f64
            } else {
                w.values[i].get().unwrap_or(f64::NAN)
            };
        }
        if self.roll.recorded > 0
            && (self.roll_network != groups.process_network
                || self.roll.first.div_euclid(PROCESS_ROLL_MS)
                    != w.start.div_euclid(PROCESS_ROLL_MS))
        {
            summary_bytes = self.roll.write(&mut self.roll_file)?;
        }
        if self.roll.recorded == 0 {
            self.roll.first = w.start;
            self.roll_network = groups.process_network;
        }
        let mut rows = Vec::new();
        for p in picked {
            let ident = Identity {
                name: p.name.clone(),
                pid: p.pid,
                created: p.create_time,
            };
            let next = self.next_id;
            let id = match self.ids.entry(ident) {
                std::collections::hash_map::Entry::Occupied(e) => *e.get(),
                std::collections::hash_map::Entry::Vacant(_) if next as usize >= MAX_INSTANCES => {
                    continue;
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    packet(
                        &mut metadata,
                        INSTANCE,
                        &serde_json::to_vec(&(next, e.key()))?,
                    )?;
                    self.next_id += 1;
                    *e.insert(next)
                }
            };
            let n = p.divisor(w.samples);
            let encoded_id = id as u64
                + MAX_INSTANCES as u64
                    * (u64::from(!groups.process_network) | (u64::from(!groups.process_gpu) << 1));
            var(&mut rows, encoded_id);
            let vals = [(p.cpu / n) as f32, (p.gpu / n) as f32];
            let mut rates = [
                (p.io, n),
                (p.net, n),
                (p.disk_read.sum, f64::from(p.disk_read.n)),
                (p.disk_write.sum, f64::from(p.disk_write.n)),
            ];
            let mut fractions = 0u8;
            for (i, (rate, count)) in rates.iter_mut().enumerate() {
                if *count > 0.0 {
                    if *rate % *count == 0. {
                        *rate /= *count;
                    } else {
                        fractions |= 1 << i;
                    }
                }
            }
            let flags = u8::from(vals[0] != 0.)
                | u8::from(vals[1] != 0.) << 1
                | u8::from(p.disk_read.n > 0) << 2
                | u8::from(p.disk_write.n > 0) << 3
                | u8::from(p.gpu_mem.is_some()) << 4
                | u8::from(p.power.n > 0) << 5
                | u8::from(p.power.get().is_some_and(|v| v != 0.)) << 6
                | u8::from(fractions != 0) << 7;
            rows.push(flags);
            for v in vals {
                if v != 0. {
                    rows.extend(v.to_le_bytes());
                }
            }
            put_memory(&mut rows, p.mem);
            var(&mut rows, rates[0].0 as u64);
            if groups.process_network {
                var(&mut rows, rates[1].0 as u64);
            }
            if groups.process_gpu {
                var(&mut rows, u64::from(p.threads));
                var(&mut rows, u64::from(p.handles));
            }
            for &(value, count) in &rates[2..] {
                if count > 0. {
                    var(&mut rows, value as u64);
                }
            }
            if let Some(v) = p.gpu_mem {
                put_memory(&mut rows, v);
            }
            if let Some(v) = p.power.get().filter(|&v| v != 0.) {
                rows.extend((v as f32).to_le_bytes());
            }
            if fractions != 0 {
                rows.push(fractions);
                for (i, (_, count)) in rates.iter().enumerate() {
                    if fractions & (1 << i) != 0 {
                        var(&mut rows, *count as u64);
                    }
                }
            }
        }
        // The counts' dictionary entries go before the frame, the counts
        // right after it (`DayRead::programs`).
        let mut counted = Vec::new();
        for (name, count) in &w.programs {
            let key = name.to_lowercase();
            let id = match self.program_ids.get(&key) {
                Some(&id) => id,
                None if self.program_ids.len() >= usize::from(u16::MAX) => continue,
                None => {
                    let id = self.program_ids.len() as u32;
                    packet(&mut metadata, PROGRAM, &serde_json::to_vec(&(id, &**name))?)?;
                    self.program_ids.insert(key, id);
                    id
                }
            };
            counted.push((id, *count));
        }
        let mut frame = Vec::with_capacity(FRAME_PREFIX + values.len() * 4);
        frame.extend(w.start.to_le_bytes());
        frame.extend(w.awake.to_le_bytes());
        frame.extend((w.span() as u32).to_le_bytes());
        frame.extend(self.pos.to_le_bytes());
        frame.extend((rows.len() as u32).to_le_bytes());
        // Match the precision of the individual frame values in the summary.
        for (v, c) in values.iter_mut().zip(&self.catalog.rows) {
            if c.unit != "bytes" {
                *v = (*v as f32) as f64;
            }
        }
        let mut presence = vec![0; values.len().div_ceil(8)];
        for (i, &v) in values.iter().enumerate() {
            if v != f64::NEG_INFINITY {
                presence[i / 8] |= 1 << (i % 8);
            }
        }
        if presence != self.presence {
            packet(&mut metadata, PRESENCE, &presence)?;
            self.presence = presence;
        }
        self.system.add(w.start, &values, w.cpu_peak);
        for (v, c) in values.into_iter().zip(&self.catalog.rows) {
            if c.unit == "bytes" {
                frame.extend(v.to_le_bytes());
            } else {
                frame.extend((v as f32).to_le_bytes());
            }
        }
        packet(&mut metadata, FRAME, &frame)?;
        let mut programs = [0u8; PROGRAMS_LEN];
        let small = |v: u32| u16::try_from(v).unwrap_or(u16::MAX).to_le_bytes();
        programs[..2].copy_from_slice(&small(w.program_count));
        for (slot, (id, count)) in counted.into_iter().enumerate() {
            programs[2 + slot * 4..4 + slot * 4].copy_from_slice(&small(id));
            programs[4 + slot * 4..6 + slot * 4].copy_from_slice(&small(count));
        }
        packet(&mut metadata, PROGRAMS, &programs)?;
        if (w.start + w.span()).rem_euclid(ROLL_MS) == 0 {
            self.system.finish(&mut metadata)?;
        }
        self.proc.write_all(&rows)?;
        self.proc.flush()?;
        out.write_all(&metadata)?;
        out.flush()?;
        self.index_append(&metadata);
        self.pos += rows.len() as u64;
        self.roll.recorded += w.span();
        self.roll.end = w.start + w.span();
        decode_rows(&rows, |id, m| {
            self.roll
                .rows
                .entry(id)
                .or_default()
                .merge(Totals::observation(m, w.start, w.span()))
        });
        let index_bytes = self
            .checkpoints
            .as_ref()
            .map_or(0, |c| c.written.saturating_sub(index_before));
        Ok((metadata.len() + rows.len()) as u64 + index_bytes + summary_bytes)
    }
}

impl DayWriter {
    fn index_append(&mut self, bytes: &[u8]) {
        if let Some(cp) = &mut self.checkpoints
            && let Err(e) = cp.capture(bytes)
        {
            crate::diagnostics::file_failure("history index append", &cp.path, &e);
            self.checkpoints = None;
        }
    }
}

impl Drop for DayWriter {
    fn drop(&mut self) {
        if let Some(cp) = &mut self.checkpoints
            && let Err(e) = cp.commit()
        {
            crate::diagnostics::file_failure("history index close", &cp.path, &e);
        }
        if let Err(e) = self.roll.write(&mut self.roll_file) {
            crate::diagnostics::warn(
                "history process summary",
                format!("win32={:?}", e.raw_os_error()),
            );
        }
    }
}

pub(super) fn open_day(dir: &Path, day: i64) -> io::Result<DayFile> {
    let owner = owned_directory(dir)?;
    let path = day_path(dir, day);
    let mut file = open_writable(&path)?;
    let mut bytes = read_bounded(&mut file)?;
    // A day begun by a release writing another version (v1/v2, no longer
    // read) starts over in this one, as an empty or torn header does.
    let other = bytes.starts_with(b"ARGH")
        && bytes.len() >= 5
        && !bytes.starts_with(MAGIC)
        && !bytes.starts_with(MINUTE_MAGIC);
    if (bytes.len() < 5 && MAGIC.starts_with(&bytes)) || other {
        if other {
            maintenance::remove_sidecars(&path)?;
        }
        file.set_len(0)?;
        file.rewind()?;
        file.write_all(MAGIC)?;
        bytes = MAGIC.to_vec();
    }
    let mut idx = Index {
        day,
        valid: 5,
        ..Default::default()
    };
    if !bytes.starts_with(MAGIC) {
        return Err(invalid());
    }
    let writer = {
        let side = path.with_extension("proc");
        let mut proc = open_writable(&side)?;
        idx.scan(&bytes[5..], proc.metadata()?.len(), None);
        if idx.limited {
            return Err(invalid());
        }
        proc.set_len(idx.proc_end)?;
        proc.seek(SeekFrom::End(0))?;
        let mut roll_file = open_writable(&path.with_extension("roll"))?;
        let rolls = if roll_file.metadata()?.len() > MAX_FILE_BYTES as u64 {
            Vec::new()
        } else {
            read_bounded(&mut roll_file)?
        };
        let valid = packets(&rolls, 0, |_, tag, p| {
            tag == 1
                && Roll::header(p, &idx)
                    .and_then(|mut roll| roll.read_rows(p, &idx))
                    .is_some()
        });
        roll_file.set_len(valid as u64)?;
        roll_file.seek(SeekFrom::End(0))?;
        let idx_path = path.with_extension("idx");
        let checkpoints = Checkpoints::open(idx_path.clone(), &bytes[..idx.valid])
            .inspect_err(|e| crate::diagnostics::file_failure("history index open", &idx_path, e))
            .ok();
        DayWriter {
            checkpoints,
            presence: Vec::new(),
            system: SystemRoll::default(),
            roll: Roll::default(),
            roll_network: true,
            roll_file: BufWriter::new(roll_file),
            proc: BufWriter::new(proc),
            pos: idx.proc_end,
            catalog: idx.catalog,
            next_id: idx.ids.len() as u32,
            program_ids: idx
                .programs
                .iter()
                .enumerate()
                .map(|(i, name)| (name.to_lowercase(), i as u32))
                .collect(),
            ids: idx
                .ids
                .into_iter()
                .enumerate()
                .map(|(i, id)| (id, i as u32))
                .collect(),
        }
    };
    file.set_len(idx.valid as u64)?;
    file.seek(SeekFrom::End(0))?;
    Ok(DayFile {
        _owner: owner,
        recorded_until: idx.last.map(|s| s.ts + s.span),
        out: BufWriter::new(file),
        writer,
        day,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{proc, snapshot, temp_dir};

    #[test]
    fn days_of_other_versions_are_not_read_and_restart_when_recorded() {
        let base = 20_737 * DAY_MS;
        for version in [1u8, 2] {
            let dir = temp_dir(&format!("other-version-{version}"));
            fs::create_dir_all(&dir).unwrap();
            let path = day_path(&dir, 20_737);
            let mut old = b"ARGH".to_vec();
            old.push(version);
            old.extend([2, 0, 0, 0, 0, 1, 2, 3]);
            fs::write(&path, &old).unwrap();
            fs::write(path.with_extension("idx"), [1, 2, 3]).unwrap();
            assert!(frame(&dir, base).is_none());
            assert!(moment(&dir, base).is_none());
            assert!(range(&dir).is_none());
            assert!(channels(&dir, base, base + DAY_MS).is_empty());
            assert!(legacy_series(&dir, base, base + DAY_MS, 10).is_empty());
            let mut rec = HistoryRecorder::new(dir.clone());
            rec.record(&snapshot(base, 3., vec![proc(7, "a.exe", 1., 1024)]));
            rec.record(&snapshot(base + FRAME_MS, 1., Vec::new()));
            drop(rec);
            assert!(fs::read(&path).unwrap().starts_with(MAGIC));
            assert_eq!(frame(&dir, base).unwrap().processes[0].name, "a.exe");
            assert_ne!(
                fs::read(path.with_extension("idx")).ok(),
                Some(vec![1, 2, 3])
            );
            fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn cuts_a_torn_record_and_keeps_appending() {
        let dir = temp_dir("torn");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..11 {
            rec.record(&snapshot(
                base + i * 1000,
                5.0,
                vec![proc(1, "a.exe", 1.0, 1 << 20)],
            ));
        }
        drop(rec);
        let path = day_path(&dir, 20_737);
        let len = fs::metadata(&path).unwrap().len();
        // A crash mid-write leaves half a frame behind.
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[FRAME, 1, 2, 3])
            .unwrap();
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 20..26 {
            rec.record(&snapshot(
                base + i * 1000,
                6.0,
                vec![proc(1, "a.exe", 1.0, 1 << 20)],
            ));
        }
        drop(rec);
        assert!(fs::metadata(&path).unwrap().len() > len);
        let points = legacy_series(&dir, base, base + 60_000, 60);
        assert_eq!(
            points.iter().map(|p| p.ts).collect::<Vec<_>>(),
            vec![
                base,
                base + 5_000,
                base + 10_000,
                base + 20_000,
                base + 25_000
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovery_matches_process_tail_and_discards_incomplete_rollup() {
        let dir = temp_dir("torn-pair");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..3 {
            rec.record(&snapshot(
                base + i * FRAME_MS,
                2.,
                vec![proc(7, "task", 3., 12345)],
            ));
        }
        drop(rec);
        let p = day_path(&dir, 20_737).with_extension("proc");
        let file = OpenOptions::new().write(true).open(&p).unwrap();
        file.set_len(file.metadata().unwrap().len() - 1).unwrap();
        drop(file);
        let mut rec = HistoryRecorder::new(dir.clone());
        rec.record(&snapshot(base + 30_000, 7., vec![proc(8, "new", 2., 999)]));
        drop(rec);
        assert_eq!(
            legacy_series(&dir, base, base + 30_000, 20)
                .iter()
                .map(|p| p.ts)
                .collect::<Vec<_>>(),
            [base, base + 5000, base + 30_000]
        );
        assert_eq!(frame(&dir, base + 30_000).unwrap().processes[0].mem, 999);
        let expected = table(&dir, base, base + ROLL_MS, "cpu", 1);
        fs::remove_file(day_path(&dir, 20_737).with_extension("roll")).unwrap();
        assert_eq!(
            serde_json::to_value(expected).unwrap(),
            serde_json::to_value(table(&dir, base, base + ROLL_MS, "cpu", 1)).unwrap()
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
