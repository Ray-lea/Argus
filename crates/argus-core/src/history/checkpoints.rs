//! The `.idx` checkpoints: the frame addresses, metadata and system
//! summaries of the main file's committed ranges, so that an index need
//! not decode every frame.
use super::*;

// Checkpoints contain only frame addresses, metadata and system summaries.
// A run ends at a gap or metadata change. Its two clocks suffice to join it
// to the adjacent runs; reading an older day never decodes every system frame.
pub(super) const FRAME_RUN: u8 = 9;
/// A run of frames each followed by its program counts (`PROGRAMS`): the
/// same record, at a stride that includes them.
pub(super) const FRAME_RUN_COUNTED: u8 = 16;
#[derive(Default)]
struct AddressRun {
    first: Option<Stamp>,
    last: Stamp,
    refs: Vec<u8>,
    proc_end: u64,
    /// Its frames are each followed by their program counts.
    counted: bool,
}
impl AddressRun {
    fn finish(&mut self, bytes: &mut Vec<u8>) {
        let Some(first) = self.first.take() else {
            return;
        };
        let mut body = Vec::with_capacity(48 + self.refs.len());
        for s in [first, self.last] {
            body.extend(s.ts.to_le_bytes());
            body.extend(s.awake.to_le_bytes());
            body.extend((s.span as u32).to_le_bytes());
        }
        body.extend(self.proc_end.to_le_bytes());
        body.append(&mut self.refs);
        bytes.push(if std::mem::take(&mut self.counted) {
            FRAME_RUN_COUNTED
        } else {
            FRAME_RUN
        });
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(body);
    }
}
pub(super) struct Checkpoints {
    pub(super) written: u64,
    out: BufWriter<File>,
    base: usize,
    end: usize,
    pending: Vec<u8>,
    run: AddressRun,
    pub(super) path: PathBuf,
}
impl Checkpoints {
    pub(super) fn open(path: PathBuf, main: &[u8]) -> io::Result<Self> {
        let file = open_writable(&path)?;
        file.set_len(0)?;
        // Rebuild the optional index from the primary's verified record boundaries.
        // A same-size edited index may end in the middle of a primary packet.
        let mut cp = Self {
            written: 0,
            out: BufWriter::new(file),
            base: 5,
            end: 5,
            pending: Vec::new(),
            run: AddressRun::default(),
            path,
        };
        let mut remaining = main.get(5..).ok_or_else(invalid)?;
        while !remaining.is_empty() {
            let (_, p) = next_packet(remaining).ok_or_else(invalid)?;
            let (record, tail) = remaining.split_at(5 + p.len());
            cp.capture(record)?;
            remaining = tail;
        }
        Ok(cp)
    }
    pub(super) fn capture(&mut self, main: &[u8]) -> io::Result<()> {
        let mut summary = false;
        let end = self
            .end
            .checked_add(main.len())
            .filter(|&v| v <= MAX_FILE_BYTES)
            .ok_or_else(invalid)?;
        let used = packets(main, 0, |at, tag, p| {
            if tag == FRAME {
                let Some(f) = Stored::parse(p) else {
                    return false;
                };
                let stamp = Stamp {
                    ts: f.ts,
                    awake: f.awake,
                    span: f.span,
                    ..Default::default()
                };
                if self.run.first.is_some()
                    && self.run.last.ts.checked_add(self.run.last.span) != Some(f.ts)
                {
                    self.run.finish(&mut self.pending);
                }
                self.run.first.get_or_insert(stamp);
                self.run.last = stamp;
                self.run.proc_end = f.offset + f.len as u64;
                self.run
                    .refs
                    .extend((f.ts.rem_euclid(DAY_MS) as u32).to_le_bytes());
                self.run.refs.extend(((self.end + at) as u32).to_le_bytes());
            } else if tag == PROGRAMS {
                // Read with their frame from the main file: copied, every
                // frame would end a run. The run records that its frames
                // have them (`FRAME_RUN_COUNTED`).
                self.run.counted = true;
            } else {
                self.run.finish(&mut self.pending);
                self.pending.extend(&main[at..at + 5 + p.len()]);
                summary |= tag == SYSTEM_ROLL;
            }
            true
        });
        if used != main.len() {
            return Err(invalid());
        }
        self.end = end;
        if summary || self.end - self.base >= 512 * 1024 {
            self.commit()?;
        }
        Ok(())
    }
    pub(super) fn commit(&mut self) -> io::Result<()> {
        if self.base == self.end {
            return Ok(());
        }
        self.run.finish(&mut self.pending);
        let mut body = Vec::with_capacity(8 + self.pending.len());
        body.extend((self.base as u32).to_le_bytes());
        body.extend((self.end as u32).to_le_bytes());
        body.append(&mut self.pending);
        packet(&mut self.out, 1, &body)?;
        self.written += (body.len() + 5) as u64;
        self.out.flush()?;
        self.base = self.end;
        Ok(())
    }
}

impl Index {
    pub(super) fn load_checkpoints(&mut self, path: &Path, main_len: u64) {
        let Some(bytes) = File::open(path.with_extension("idx"))
            .ok()
            .and_then(|mut f| read_bounded(&mut f).ok())
        else {
            return;
        };
        self.checkpoints(&bytes, main_len);
    }

    fn checkpoints(&mut self, bytes: &[u8], main_len: u64) {
        let mut bad = false;
        packets(bytes, 0, |_, tag, p| {
            let Some((header, records)) = p.split_first_chunk::<8>() else {
                return false;
            };
            let [lo, hi] = header.as_chunks::<4>().0 else {
                return false;
            };
            let base = u32::from_le_bytes(*lo) as usize;
            let end = u32::from_le_bytes(*hi) as usize;
            if tag != 1 || base != self.valid || end <= base || end as u64 > main_len {
                return false;
            }
            if self.scan(records, u64::MAX, Some(end)) != records.len() || self.valid != end {
                bad = true;
                return false;
            }
            true
        });
        if bad {
            *self = Self {
                day: self.day,
                version: VERSION,
                valid: 5,
                ..Default::default()
            };
        }
    }

    pub(super) fn address_run(&mut self, p: &[u8], limit: usize, counted: bool) -> bool {
        let Some((header, refs)) = p.split_first_chunk::<48>() else {
            return false;
        };
        let (refs, tail) = refs.as_chunks::<8>();
        if refs.is_empty() || !tail.is_empty() || refs.len() > MAX_FRAMES - self.frames.len() {
            return false;
        }
        let stamp = |i| -> Option<Stamp> {
            let ts = i64::from_le_bytes(field(header, i)?);
            let span = i64::from(u32::from_le_bytes(field(header, i + 16)?));
            let awake = i64::from_le_bytes(field(header, i + 8)?);
            (ts.div_euclid(DAY_MS) == self.day && (1..=FRAME_MS).contains(&span)).then_some(Stamp {
                ts,
                span,
                awake,
                boot: self.boot,
                stop: 0,
            })
        };
        let (Some(first), Some(last), Some(proc_end)) = (
            stamp(0),
            stamp(20),
            field(header, 40).map(u64::from_le_bytes),
        ) else {
            return false;
        };
        // Fall back to the primary scan, which can omit individual repeated frames.
        if self.last.is_some_and(|s| first.ts <= s.ts) {
            return false;
        }
        let base = self.valid;
        let width =
            5 + FRAME_PREFIX + self.catalog.bytes + if counted { 5 + PROGRAMS_LEN } else { 0 };
        let Some(end) = refs
            .len()
            .checked_mul(width)
            .and_then(|n| base.checked_add(n))
            .filter(|&n| n <= limit)
        else {
            return false;
        };
        let mut prev = None;
        for (i, pair) in refs.iter().enumerate() {
            let [t, off] = pair.as_chunks::<4>().0 else {
                return false;
            };
            let ts = u32::from_le_bytes(*t) as i64;
            if ts >= DAY_MS
                || u32::from_le_bytes(*off) as usize != base + i * width
                || prev.is_some_and(|a| !(1..=FRAME_MS).contains(&(ts - a)))
                || (i == 0 && ts != first.ts.rem_euclid(DAY_MS))
                || (i == 1 && ts != first.ts.rem_euclid(DAY_MS) + first.span)
                || (i + 1 == refs.len() && ts != last.ts.rem_euclid(DAY_MS))
            {
                return false;
            }
            prev = Some(ts);
        }
        if proc_end < self.proc_end
            || proc_end - self.proc_end > (refs.len() * MAX_FRAME_ROWS * 160) as u64
        {
            return false;
        }
        self.stamp(first, base);
        for (i, r) in refs.iter().enumerate().skip(1) {
            let [t, ..] = r.as_chunks::<4>().0 else {
                return false;
            };
            self.frames
                .push((u32::from_le_bytes(*t), (base + i * width) as u32));
        }
        self.proc_end = proc_end;
        self.last = Some(last);
        self.valid = end;
        // The counts a scan of the primary would have read with each frame.
        if counted {
            self.process_bytes += (refs.len() * (5 + PROGRAMS_LEN)) as u64;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{proc, snapshot, temp_dir};

    #[test]
    fn checkpoints_match_primary_when_missing_or_torn() {
        let dir = temp_dir("index-recovery");
        let base = 20_737 * DAY_MS;
        let mut r = HistoryRecorder::new(dir.clone());
        for i in 0..400 {
            r.record(&snapshot(
                base + i * 5000,
                ((i + 7) % 23) as f32,
                vec![proc(3, "test", 8., 4096)],
            ));
        }
        drop(r);
        let get = || {
            forget(&dir);
            serde_json::to_value(series(
                &dir,
                base,
                base + 2 * ROLL_MS,
                ROLL_MS,
                base,
                &["cpu".into()],
            ))
            .unwrap()
        };
        let expected = get();
        let path = day_path(&dir, 20_737).with_extension("idx");
        let f = OpenOptions::new().write(true).open(&path).unwrap();
        f.set_len(f.metadata().unwrap().len() - 3).unwrap();
        drop(f);
        assert_eq!(get(), expected);
        fs::remove_file(&path).unwrap();
        assert_eq!(get(), expected);
        let mut r = HistoryRecorder::new(dir.clone());
        r.record(&snapshot(base + 3 * ROLL_MS, 1., vec![]));
        drop(r);
        assert_eq!(get(), expected);
        assert_eq!(range(&dir).unwrap().1, base + 3 * ROLL_MS);
        fs::remove_dir_all(dir).unwrap();
    }
}
