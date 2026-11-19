//! A sealed day is a single ARGH v5 file. Metadata uses the same dictionaries
//! as v4. Each minute has independently checksummed zlib streams for its
//! system bands and process totals. Curve queries never inflate processes.
use super::*;
use miniz_oxide::{deflate::compress_to_vec_zlib, inflate::decompress_to_vec_zlib_with_limit};

pub(super) const MINUTE_MAGIC: &[u8; 5] = b"ARGH\x05";
pub(super) const MINUTE_MS: i64 = 60_000;
pub(super) const MINUTE: u8 = 10;
pub(super) const GAP: u8 = 11;
pub(super) const SEALED: u8 = 12;
pub(super) const EDGES: u8 = 13;
pub(super) const SEAL: &[u8] = b"minute-day";

pub(super) fn is_merged(path: &Path) -> io::Result<bool> {
    let mut magic = [0; 5];
    let mut f = fs::File::open(path)?;
    if f.read_exact(&mut magic).is_err() || &magic != MINUTE_MAGIC {
        return Ok(false);
    }
    let mut seal = Vec::new();
    packet(&mut seal, SEALED, SEAL)?;
    f.seek(SeekFrom::End(-(seal.len() as i64)))?;
    let mut tail = vec![0; seal.len()];
    f.read_exact(&mut tail)?;
    if tail != seal {
        return Err(invalid());
    }
    Ok(true)
}

pub(super) struct Minute<'a> {
    pub ts: i64,
    coverage: &'a [u8],
    system: &'a [u8],
    processes: &'a [u8],
}
impl<'a> Minute<'a> {
    pub(super) fn parse(p: &'a [u8], day: i64) -> Option<Self> {
        let ts = i64::from_le_bytes(field(p, 0)?);
        let count = usize::from(u16::from_le_bytes(field(p, 8)?));
        let system_len = u32::from_le_bytes(field(p, 10)?) as usize;
        if ts.div_euclid(DAY_MS) != day || ts.rem_euclid(MINUTE_MS) != 0 || count == 0 {
            return None;
        }
        let (coverage, tail) = p.get(14..)?.split_at_checked(count * 4)?;
        let (system, processes) = tail.split_at_checked(system_len)?;
        let m = Self {
            ts,
            coverage,
            system,
            processes,
        };
        let mut end = ts;
        for (a, b) in m.intervals() {
            if a < end || a >= b || b > ts + MINUTE_MS {
                return None;
            }
            end = b;
        }
        Some(m)
    }
    pub(super) fn process_bytes(&self) -> u64 {
        self.processes.len() as u64
    }
    fn intervals(&self) -> impl Iterator<Item = (i64, i64)> + '_ {
        self.coverage
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[a, b, c, d]| {
                (
                    self.ts + i64::from(u16::from_le_bytes([a, b])),
                    self.ts + i64::from(u16::from_le_bytes([c, d])),
                )
            })
    }
    pub(super) fn bands(&self, count: usize) -> Option<Vec<Band>> {
        let bytes = decompress_to_vec_zlib_with_limit(self.system, count * 28).ok()?;
        if bytes.len() != count * 28 {
            return None;
        }
        bytes
            .as_chunks::<28>()
            .0
            .iter()
            .map(|b| {
                let n = u32::from_le_bytes(field(b, 0)?);
                let sum = f64::from_le_bytes(field(b, 4)?);
                let low = f64::from_le_bytes(field(b, 12)?);
                let high = f64::from_le_bytes(field(b, 20)?);
                if n > MINUTE_MS as u32
                    || !sum.is_finite()
                    || (n > 0 && (!low.is_finite() || !high.is_finite() || low > high))
                {
                    return None;
                }
                Some(Band { n, sum, low, high })
            })
            .collect()
    }
    fn rows(&self, idx: &Index) -> Option<Vec<(usize, MinuteProcess)>> {
        let bytes = decompress_to_vec_zlib_with_limit(
            self.processes,
            idx.ids.len().min(MAX_FRAME_ROWS) * 176,
        )
        .ok()?;
        let mut raw = bytes.as_slice();
        let mut rows = Vec::new();
        let recorded: i64 = self.intervals().map(|(a, b)| b - a).sum();
        while !raw.is_empty() {
            let id = usize::try_from(take_var(&mut raw)?).ok()?;
            let mut t = Totals::decode(&mut raw)?;
            t.first = self.ts.checked_add(t.first)?;
            t.last = self.ts.checked_add(t.last)?;
            let read_ms = i64::try_from(take_var(&mut raw)?).ok()?;
            let write_ms = i64::try_from(take_var(&mut raw)?).ok()?;
            t.net_ms = i64::try_from(take_var(&mut raw)?).ok()?;
            if id >= idx.ids.len()
                || rows.last().is_some_and(|&(last, _)| last >= id)
                || !(1..=recorded).contains(&t.seen)
                || t.first < self.ts
                || t.last < t.first
                || t.last >= self.ts + MINUTE_MS
                || t.read.is_some() != (read_ms > 0)
                || t.write.is_some() != (write_ms > 0)
                || t.net.is_finite() != (t.net_ms > 0)
                || [read_ms, write_ms, t.disk_ms, t.power_ms, t.net_ms]
                    .iter()
                    .any(|v| !(0..=t.seen).contains(v))
            {
                return None;
            }
            rows.push((
                id,
                MinuteProcess {
                    totals: t,
                    read_ms,
                    write_ms,
                },
            ));
        }
        Some(rows)
    }
    pub(super) fn processes(&self, idx: &Index) -> Vec<FrameProcess> {
        self.rows(idx)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, p)| p.metrics().process(&idx.ids[id]))
            .collect()
    }
    pub(super) fn table(&self, q: &mut TableQuery<'_>, idx: &Index, start: i64, end: i64) {
        let rows = self.rows(idx).unwrap_or_default();
        let recorded: i64 = self.intervals().map(|(a, b)| b - a).sum();
        let mut covered = 0;
        for (a, b) in self.intervals() {
            let mut t = a.max(start);
            let finish = b.min(end);
            while t < finish {
                let bin = q.bin(t);
                let next = q.bin_end(bin, finish);
                let offset = covered + t - a;
                q.recorded += next - t;
                for &(id, p) in &rows {
                    let mut totals = p.totals;
                    let fraction = (next - t) as f64 / recorded as f64;
                    // Differences of cumulative integer durations telescope across bins.
                    let scale = |ms| (offset + next - t) * ms / recorded - offset * ms / recorded;
                    totals.seen = scale(totals.seen);
                    totals.net_ms = scale(totals.net_ms);
                    totals.disk_ms = scale(totals.disk_ms);
                    totals.power_ms = scale(totals.power_ms);
                    for v in [
                        &mut totals.cpu,
                        &mut totals.gpu,
                        &mut totals.io,
                        &mut totals.net,
                        &mut totals.mem_sum,
                        &mut totals.power,
                        &mut totals.disk,
                    ] {
                        *v *= fraction;
                    }
                    totals.read = totals.read.map(|v| v * fraction);
                    totals.write = totals.write.map(|v| v * fraction);
                    if totals.seen > 0 {
                        q.add(&idx.ids[id], totals, bin);
                    }
                }
                t = next;
            }
            covered += b - a;
        }
    }
}

#[derive(Clone, Copy, Default)]
struct MinuteProcess {
    totals: Totals,
    read_ms: i64,
    write_ms: i64,
}
impl MinuteProcess {
    fn metrics(self) -> Metrics {
        let t = self.totals;
        let n = t.seen as f64;
        Metrics {
            cpu: t.cpu / n,
            gpu: t.gpu / n,
            mem: t.mem,
            io: t.io / n,
            net: t.net / t.net_ms as f64,
            gpu_mem: t.gpu_mem,
            threads: t.threads,
            handles: t.handles,
            power: (t.power_ms > 0).then(|| t.power / t.power_ms as f64),
            disk_read: t.read.map(|v| v / self.read_ms as f64),
            disk_write: t.write.map(|v| v / self.write_ms as f64),
        }
    }
}

#[derive(Default)]
struct Aggregate {
    ts: i64,
    coverage: Vec<(i64, i64)>,
    bands: Vec<Band>,
    present: Vec<u8>,
    rows: HashMap<usize, MinuteProcess>,
}
impl Aggregate {
    fn add(&mut self, id: usize, m: Metrics, ts: i64, span: i64) -> io::Result<()> {
        let p = self.rows.entry(id).or_default();
        if (p.totals.seen > 0 && p.totals.last >= ts) || !m.cpu.is_finite() || !m.gpu.is_finite() {
            return Err(invalid());
        }
        p.totals.merge(Totals::observation(m, ts, span));
        if m.disk_read.is_some() {
            p.read_ms += span;
        }
        if m.disk_write.is_some() {
            p.write_ms += span;
        }
        Ok(())
    }
    fn write(
        &mut self,
        out: &mut impl Write,
        ids: &[Identity],
        cap: u32,
        peak: Option<usize>,
        prev_mask: &mut Vec<u8>,
    ) -> io::Result<()> {
        if self.coverage.is_empty() {
            return Ok(());
        }
        let mut system = Vec::new();
        for (i, b) in self.bands.iter().enumerate() {
            let present = self.present[i / 8] & (1 << (i % 8)) != 0;
            let sum = if peak == Some(i) && b.n > 0 {
                b.high * f64::from(b.n)
            } else {
                b.sum
            };
            system.extend(b.n.to_le_bytes());
            system.extend(sum.to_le_bytes());
            system.extend((if present { b.low } else { f64::NEG_INFINITY }).to_le_bytes());
            system.extend(b.high.to_le_bytes());
        }
        let system = compress_to_vec_zlib(&system, 6);
        let mut data = self.ts.to_le_bytes().to_vec();
        data.extend((self.coverage.len() as u16).to_le_bytes());
        data.extend((system.len() as u32).to_le_bytes());
        for &(a, b) in &self.coverage {
            data.extend(((a - self.ts) as u16).to_le_bytes());
            data.extend(((b - self.ts) as u16).to_le_bytes());
        }
        data.extend(system);
        let recorded = self.coverage.iter().map(|(a, b)| b - a).sum::<i64>() as f64;
        let mut picked = select(
            self.rows.keys().copied().collect(),
            cap,
            |id, metric| {
                let acc = self.rows[&id];
                let p = acc.metrics();
                match metric {
                    0 => acc.totals.cpu / recorded,
                    1 => p.mem as f64,
                    2 => acc.totals.io / recorded,
                    3 => acc.totals.gpu / recorded,
                    4 => finite(acc.totals.net).unwrap_or_default() / recorded,
                    _ => p.disk_read.unwrap_or_default() + p.disk_write.unwrap_or_default(),
                }
            },
            |id| (ids[id].pid, ids[id].created),
        );
        picked.sort_unstable();
        let mut rows = Vec::new();
        for id in picked {
            let p = &self.rows[&id];
            var(&mut rows, id as u64);
            let mut totals = p.totals;
            totals.first -= self.ts;
            totals.last -= self.ts;
            totals.encode(&mut rows);
            var(&mut rows, p.read_ms as u64);
            var(&mut rows, p.write_ms as u64);
            var(&mut rows, p.totals.net_ms as u64);
        }
        data.extend(compress_to_vec_zlib(&rows, 6));
        if self.present != *prev_mask {
            packet(out, PRESENCE, &self.present)?;
            prev_mask.clone_from(&self.present);
        }
        packet(out, MINUTE, &data)?;
        self.coverage.clear();
        self.rows.clear();
        self.bands.fill(Band::default());
        self.present.fill(0);
        Ok(())
    }
}

pub(super) fn parse_gap(p: &[u8], day: i64) -> Option<Gap> {
    let from = i64::from_le_bytes(field(p, 0)?);
    let to = i64::from_le_bytes(field(p, 8)?);
    if p.len() != 17 || from.div_euclid(DAY_MS) != day || to.div_euclid(DAY_MS) != day || from >= to
    {
        return None;
    }
    let reason = *["off", "unknown", "shutdown", "sleep", "notRunning"].get(usize::from(p[16]))?;
    Some(Gap { from, to, reason })
}

pub(super) fn merge(dir: &Path, day: i64, cap: u32) -> io::Result<()> {
    let path = day_path(dir, day);
    let tmp = path.with_extension("merge-tmp");
    let mut primary = fs::File::open(&path)?;
    let source_meta = primary.metadata()?;
    let len = usize::try_from(source_meta.len()).map_err(|_| invalid())?;
    if !(5..=MAX_FILE_BYTES).contains(&len) {
        return Err(invalid());
    }
    let mut magic = [0; 5];
    primary.read_exact(&mut magic)?;
    let mut idx = Index {
        day,
        version: magic[4],
        valid: 5,
        ..Default::default()
    };
    if &magic == MAGIC {
        let mut bytes = Vec::new();
        (&mut primary).take(len as u64).read_to_end(&mut bytes)?;
        idx.scan(
            &bytes,
            fs::metadata(path.with_extension("proc"))?.len(),
            None,
        );
    } else if &magic == LEGACY_MAGIC || &magic == MAGIC_V3 {
        idx.legacy(&mut primary, len)?;
    } else {
        return Err(invalid());
    }
    drop(primary);
    if idx.valid != len || idx.limited || idx.frames.is_empty() {
        return Err(invalid());
    }
    let idx = Arc::new(idx);
    let result = (|| {
        let mut out = BufWriter::new(fs::File::create_new(&tmp)?);
        out.write_all(MINUTE_MAGIC)?;
        if idx.labels.is_empty() {
            for (i, c) in idx.catalog.rows.iter().enumerate() {
                packet(&mut out, CHANNEL, &serde_json::to_vec(&(i, c))?)?;
            }
        } else {
            for (ts, i, label) in &idx.labels {
                let mut c = idx.catalog.rows[*i].clone();
                c.first = align(*ts, 0, MINUTE_MS);
                c.label.clone_from(label);
                packet(&mut out, CHANNEL, &serde_json::to_vec(&(i, c))?)?;
            }
        }
        let mut ids = idx.ids.clone();
        for (i, id) in ids.iter().enumerate() {
            packet(&mut out, INSTANCE, &serde_json::to_vec(&(i, id))?)?;
        }
        let mut legacy_ids = HashMap::new();
        let mut agg = Aggregate {
            bands: vec![Band::default(); idx.catalog.rows.len()],
            present: vec![0; idx.catalog.rows.len().div_ceil(8)],
            ..Default::default()
        };
        let peak = idx.catalog.by_key.get("cpuPeak").copied();
        let mut mask = Vec::new();
        for start in (0..idx.frames.len()).step_by(128) {
            let d = DayRead::by_frames(
                &path,
                idx.clone(),
                start..(start + 128).min(idx.frames.len()),
                true,
            )
            .ok_or_else(invalid)?;
            for i in d.range.clone() {
                let row = d.row(i).ok_or_else(invalid)?;
                let ts = row.ts();
                let day_end = (day + 1) * DAY_MS;
                let next = idx
                    .frames
                    .get(i + 1)
                    .map_or(day_end, |f| day * DAY_MS + i64::from(f.0));
                if next < ts {
                    return Err(invalid());
                }
                let frame_end = (ts + row.span()).min(next);
                let rows = if let Reading::New(f, _) = &row {
                    d.metrics(f).ok_or_else(invalid)?
                } else {
                    let mut rows = Vec::new();
                    for p in d.processes(i) {
                        let id = Identity {
                            name: p.name.into(),
                            pid: p.pid,
                            created: p.create_time.unwrap_or_default(),
                        };
                        let local = if let Some(&i) = legacy_ids.get(&id) {
                            i
                        } else {
                            if ids.len() >= MAX_INSTANCES {
                                return Err(invalid());
                            }
                            let i = ids.len();
                            packet(&mut out, INSTANCE, &serde_json::to_vec(&(i, &id))?)?;
                            ids.push(id.clone());
                            legacy_ids.insert(id, i);
                            i
                        };
                        rows.push((
                            local,
                            Metrics {
                                cpu: p.cpu.into(),
                                gpu: p.gpu.into(),
                                mem: p.mem,
                                io: p.io,
                                net: p.net,
                                ..Default::default()
                            },
                        ));
                    }
                    rows
                };
                let mut ts = ts;
                while ts < frame_end {
                    let slot = align(ts, 0, MINUTE_MS);
                    let end = frame_end.min(slot + MINUTE_MS);
                    if slot != agg.ts {
                        agg.write(&mut out, &ids, cap, peak, &mut mask)?;
                        agg.ts = slot;
                        if out.stream_position()? > MAX_FILE_BYTES as u64 {
                            return Err(invalid());
                        }
                    }
                    if let Some(last) = agg.coverage.last_mut().filter(|v| v.1 == ts) {
                        last.1 = end;
                    } else {
                        agg.coverage.push((ts, end));
                    }
                    for (j, b) in agg.bands.iter_mut().enumerate() {
                        let v = row.get(j);
                        if v != f64::NEG_INFINITY {
                            agg.present[j / 8] |= 1 << (j % 8);
                        }
                        b.add(v);
                        if !b.sum.is_finite() {
                            return Err(invalid());
                        }
                    }
                    for &(id, metrics) in &rows {
                        agg.add(id, metrics, ts, end - ts)?;
                    }
                    ts = end;
                }
            }
        }
        agg.write(&mut out, &ids, cap, peak, &mut mask)?;
        for g in &idx.gaps {
            let mut data = g.from.to_le_bytes().to_vec();
            data.extend(g.to.to_le_bytes());
            data.push(match g.reason {
                "off" => 0,
                "shutdown" => 2,
                "sleep" => 3,
                "notRunning" => 4,
                _ => 1,
            });
            packet(&mut out, GAP, &data)?;
        }
        packet(
            &mut out,
            EDGES,
            &serde_json::to_vec(&[
                idx.first.ok_or_else(invalid)?,
                idx.last.ok_or_else(invalid)?,
            ])?,
        )?;
        packet(&mut out, SEALED, SEAL)?;
        out.flush()?;
        if out.get_ref().metadata()?.len() > MAX_FILE_BYTES as u64 {
            return Err(invalid());
        }
        out.get_ref().sync_all()?;
        drop(out);
        let _publish = maintenance::PUBLICATION
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let current = fs::metadata(&path)?;
        if current.len() != source_meta.len()
            || current.modified()? != source_meta.modified()?
            || current.created()? != source_meta.created()?
        {
            return Err(invalid());
        }
        use windows::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        maintenance::rename_file(
            &tmp,
            &path,
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )?;
        forget(dir);
        maintenance::remove_sidecars(&path)
    })();
    if result.is_err() {
        let _ = maintenance::remove_if_exists(&tmp);
    }
    result
}
