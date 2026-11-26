//! Reading a day's frames back, and the frame nearest a moment.
use super::*;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameProcess {
    pub name: String,
    pub pid: u32,
    pub create_time: Option<i64>,
    pub cpu: f32,
    pub gpu: f32,
    pub mem: u64,
    pub io: f64,
    pub net: f64,
    pub disk_read: Option<f64>,
    pub disk_write: Option<f64>,
    pub gpu_mem: Option<u64>,
    pub power: Option<f64>,
    pub threads: Option<u32>,
    pub handles: Option<u32>,
}
/// How many processes a program ran in a frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProgramCount {
    pub name: String,
    pub count: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct HistoryFrame {
    pub ts: i64,
    pub span: i64,
    pub system: BTreeMap<String, Option<f64>>,
    pub processes: Vec<FrameProcess>,
    /// The programs running the most processes, of all the frame's last
    /// sample had (not only the processes it kept): most first, then by
    /// name. Empty when unknown (frames written before 0.6).
    pub programs: Vec<ProgramCount>,
    /// How many programs that sample had; 0 when unknown.
    #[serde(rename = "programCount")]
    pub program_count: u32,
}

pub(super) struct DayRead {
    pub(super) idx: Arc<Index>,
    base: usize,
    bytes: Vec<u8>,
    proc_base: u64,
    proc: Vec<u8>,
    proc_path: Option<PathBuf>,
    pub(super) range: std::ops::Range<usize>,
}

impl DayRead {
    pub(super) fn open(
        path: &Path,
        day: i64,
        idx: Arc<Index>,
        from: i64,
        to: i64,
        processes: bool,
    ) -> Option<Self> {
        let range = idx.range(day, from, to);
        Self::by_frames(path, idx, range, processes)
    }
    pub(super) fn by_frames(
        path: &Path,
        idx: Arc<Index>,
        range: std::ops::Range<usize>,
        processes: bool,
    ) -> Option<Self> {
        let first = range.start;
        let last = range.end;
        if first >= last {
            return None;
        }
        let base = idx.frames.get(first)?.1 as usize;
        let end = idx.frames.get(last).map_or(idx.valid, |e| e.1 as usize);
        let bytes = read_slice(path, base as u64, end.checked_sub(base)?)?;
        let mut d = Self {
            idx,
            base,
            bytes,
            proc_base: 0,
            proc: Vec::new(),
            proc_path: None,
            range: first..last,
        };
        if processes && d.idx.version == VERSION {
            let side = path.with_extension("proc");
            let slice = d.stored(first).zip(d.stored(last - 1)).and_then(|(a, b)| {
                let len =
                    usize::try_from(b.offset.checked_add(b.len as u64)?.checked_sub(a.offset)?)
                        .ok()?;
                Some((a.offset, read_slice(&side, a.offset, len)?))
            });
            if let Some((base, bytes)) = slice {
                d.proc_base = base;
                d.proc = bytes;
            } else {
                d.proc_path = Some(side);
            }
        }
        Some(d)
    }
    fn bytes(&self, i: usize) -> Option<&[u8]> {
        self.bytes
            .get((self.idx.frames.get(i)?.1 as usize).checked_sub(self.base)?..)
    }
    fn stored(&self, i: usize) -> Option<Stored<'_>> {
        let (tag, payload) = next_packet(self.bytes(i)?)?;
        let f = Stored::parse(payload)?;
        (tag == FRAME
            && f.ts.div_euclid(DAY_MS) == self.idx.day
            && f.ts.rem_euclid(DAY_MS) == i64::from(self.idx.frames.get(i)?.0)
            && f.values.len() <= self.idx.catalog.bytes)
            .then_some(f)
    }
    pub(super) fn metrics(&self, f: &Stored<'_>) -> Option<Vec<(usize, Metrics)>> {
        let single;
        let raw = if let Some(path) = &self.proc_path {
            single = read_slice(path, f.offset, f.len)?;
            &single[..]
        } else {
            let offset = usize::try_from(f.offset.checked_sub(self.proc_base)?).ok()?;
            self.proc.get(offset..offset.checked_add(f.len)?)?
        };
        let mut rows = Vec::new();
        decode_rows(raw, |id, m| rows.push((id, m)))?;
        rows.iter()
            .all(|(id, _)| *id < self.idx.ids.len())
            .then_some(rows)
    }
    /// The program counts recorded with frame (or minute) `i`: the packet
    /// right after it, when there is one.
    fn programs(&self, i: usize) -> (Vec<ProgramCount>, u32) {
        self.programs_packet(i)
            .and_then(|p| decode_programs(p, &self.idx.programs))
            .unwrap_or_default()
    }
    pub(super) fn programs_packet(&self, i: usize) -> Option<&[u8]> {
        let bytes = self.bytes(i)?;
        let (_, first) = next_packet(bytes)?;
        let (tag, p) = next_packet(bytes.get(5 + first.len()..)?)?;
        (tag == PROGRAMS).then_some(p)
    }
    pub(super) fn minute(&self, i: usize) -> Option<Minute<'_>> {
        let (tag, p) = next_packet(self.bytes(i)?)?;
        let m = Minute::parse(p, self.idx.day)?;
        (tag == minute::MINUTE && m.ts.rem_euclid(DAY_MS) == i64::from(self.idx.frames.get(i)?.0))
            .then_some(m)
    }
    pub(super) fn row(&self, i: usize) -> Option<Reading<'_>> {
        if self.idx.version == MINUTE_VERSION {
            let m = self.minute(i)?;
            return Some(Reading::Minute(m.ts, m.bands(self.idx.catalog.rows.len())?));
        }
        Some(Reading::New(self.stored(i)?, &self.idx.catalog))
    }
    fn processes(&self, i: usize) -> Vec<FrameProcess> {
        if self.idx.version == MINUTE_VERSION {
            return self
                .minute(i)
                .map(|m| m.processes(&self.idx))
                .unwrap_or_default();
        }
        self.stored(i)
            .and_then(|f| self.metrics(&f))
            .unwrap_or_default()
            .into_iter()
            .map(|(j, m)| m.process(&self.idx.ids[j]))
            .collect()
    }
}

pub(super) enum Reading<'a> {
    Minute(i64, Vec<Band>),
    New(Stored<'a>, &'a Catalog),
}
impl Reading<'_> {
    pub(super) fn ts(&self) -> i64 {
        match self {
            Self::Minute(ts, _) => *ts,
            Self::New(f, _) => f.ts,
        }
    }
    pub(super) fn span(&self) -> i64 {
        match self {
            Self::Minute(_, _) => MINUTE_MS,
            Self::New(f, _) => f.span,
        }
    }
    pub(super) fn get(&self, i: usize) -> f64 {
        match self {
            Self::Minute(_, bands) => bands.get(i).map_or(f64::NEG_INFINITY, |b| {
                if b.n > 0 {
                    b.sum / f64::from(b.n)
                } else if b.low == f64::NEG_INFINITY {
                    f64::NEG_INFINITY
                } else {
                    f64::NAN
                }
            }),
            Self::New(f, c) => f.value(i, c),
        }
    }
}

pub(super) fn visit_days(
    dir: &Path,
    from: i64,
    to: i64,
    processes: bool,
    mut visit: impl FnMut(&DayRead),
) {
    if from > to {
        return;
    }
    for day in days(dir)
        .into_iter()
        .filter(|&d| d >= from.div_euclid(DAY_MS) && d <= to.div_euclid(DAY_MS))
    {
        let path = day_path(dir, day);
        if let Some(idx) = index(&path)
            && let Some(d) = DayRead::open(&path, day, idx, from, to, processes)
        {
            visit(&d);
        }
    }
}

pub fn frame(dir: &Path, ts: i64) -> Option<HistoryFrame> {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let mut best = None;
    let mut distance = 60_001;
    visit_days(
        dir,
        ts.saturating_sub(60_000),
        ts.saturating_add(60_000),
        true,
        |d| {
            for i in d.range.clone() {
                let Some(row) = d.row(i) else { continue };
                let diff = row.ts().abs_diff(ts);
                if diff < distance {
                    distance = diff;
                    let (programs, program_count) = d.programs(i);
                    best = Some(HistoryFrame {
                        ts: row.ts(),
                        span: row.span(),
                        system: d
                            .idx
                            .catalog
                            .rows
                            .iter()
                            .enumerate()
                            .filter_map(|(j, c)| {
                                let v = row.get(j);
                                (v != f64::NEG_INFINITY).then(|| (c.key.clone(), finite(v)))
                            })
                            .collect(),
                        processes: d.processes(i),
                        programs,
                        program_count,
                    });
                }
            }
        },
    );
    best
}

pub fn range(dir: &Path) -> Option<(i64, i64)> {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let days = days(dir);
    let edge = |day: &i64| index(&day_path(dir, *day));
    Some((
        days.iter().find_map(|d| edge(d)?.first.map(|s| s.ts))?,
        days.iter()
            .rev()
            .find_map(|d| edge(d)?.last.map(|s| s.ts))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{proc, snapshot, temp_dir};

    #[test]
    fn frames_keep_program_counts_of_every_process_in_their_last_sample() {
        let dir = temp_dir("programs");
        let day = 20_737;
        let base = day * DAY_MS;
        let path = day_path(&dir, day);
        let mut processes = Vec::new();
        let mut pid = 100;
        let mut add = |name: &str, n: u32| {
            for _ in 0..n {
                pid += 1;
                processes.push(proc(pid, name, 0., 1024));
            }
        };
        // Two spellings are one program, under the first seen.
        add("svchost.exe", 5);
        add("Chrome.exe", 3);
        add("chrome.exe", 2);
        for name in [
            "i.exe", "b.exe", "a.exe", "h.exe", "c.exe", "g.exe", "d.exe", "f.exe", "e.exe",
        ] {
            add(name, 1);
        }
        let mut rec = HistoryRecorder::new(dir.clone());
        // Only the frame's last sample counts.
        rec.record(&snapshot(base, 1., vec![proc(1, "early.exe", 0., 1024)]));
        rec.record(&snapshot(base + 1000, 1., processes.clone()));
        rec.record(&snapshot(base + FRAME_MS, 1., processes[..5].to_vec()));
        rec.record(&snapshot(base + 2 * FRAME_MS, 1., Vec::new()));
        drop(rec);
        let counts = |list: &[(&str, u32)]| {
            list.iter()
                .map(|&(name, count)| ProgramCount {
                    name: name.into(),
                    count,
                })
                .collect::<Vec<_>>()
        };
        let check = |dir: &Path| {
            let first = frame(dir, base).unwrap();
            assert_eq!(first.program_count, 11);
            assert_eq!(
                first.programs,
                counts(&[
                    ("Chrome.exe", 5),
                    ("svchost.exe", 5),
                    ("a.exe", 1),
                    ("b.exe", 1),
                    ("c.exe", 1),
                    ("d.exe", 1),
                    ("e.exe", 1),
                    ("f.exe", 1),
                ])
            );
            let second = frame(dir, base + FRAME_MS).unwrap();
            assert_eq!(second.programs, counts(&[("svchost.exe", 5)]));
            let json = serde_json::to_value(&second).unwrap();
            assert_eq!(json["programCount"], 1);
            assert_eq!(
                json["programs"],
                serde_json::json!([{"name": "svchost.exe", "count": 5}])
            );
            let empty = frame(dir, base + 2 * FRAME_MS).unwrap();
            assert!(empty.programs.is_empty());
            assert_eq!(empty.program_count, 0);
        };
        check(&dir);
        // Recording on in the same day keeps the dictionary.
        let mut rec = HistoryRecorder::new(dir.clone());
        rec.record(&snapshot(
            base + 3 * FRAME_MS,
            1.,
            vec![
                proc(9, "SVCHOST.EXE", 0., 1024),
                proc(10, "new.exe", 0., 1024),
            ],
        ));
        rec.record(&snapshot(base + 4 * FRAME_MS, 1., Vec::new()));
        drop(rec);
        let later = counts(&[("new.exe", 1), ("svchost.exe", 1)]);
        assert_eq!(frame(&dir, base + 3 * FRAME_MS).unwrap().programs, later);
        // The checkpoints hold the dictionary but not the counts of every
        // frame, whose runs they mark; read through them and without them.
        let mut tags = Vec::new();
        packets(
            &fs::read(path.with_extension("idx")).unwrap(),
            0,
            |_, _, p| {
                packets(p.get(8..).unwrap_or_default(), 0, |_, tag, _| {
                    tags.push(tag);
                    true
                });
                true
            },
        );
        assert!(tags.contains(&PROGRAM) && tags.contains(&FRAME_RUN_COUNTED));
        assert!(!tags.contains(&PROGRAMS));
        forget(&dir);
        check(&dir);
        let through = index(&path).unwrap().process_bytes;
        fs::remove_file(path.with_extension("idx")).unwrap();
        forget(&dir);
        check(&dir);
        assert_eq!(index(&path).unwrap().process_bytes, through);
        // A merged minute keeps the counts of its last frame that had some.
        minute::merge(&dir, day, 0).unwrap();
        forget(&dir);
        let merged = frame(&dir, base).unwrap();
        assert_eq!((merged.programs, merged.program_count), (later, 2));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn finds_moments_across_midnight() {
        let dir = temp_dir("midnight");
        let midnight = 20_738 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        // One sample per frame: 23:59:50, 23:59:55, 00:00:00, 00:00:05 (UTC).
        for i in -2..=2 {
            rec.record(&snapshot(
                midnight + i * 5_000,
                1.0,
                vec![proc(1, "a.exe", 1.0, 1 << 20)],
            ));
        }
        drop(rec);
        assert_eq!(days(&dir), vec![20_737, 20_738]);
        // The closest frame is in the other day's file.
        assert_eq!(moment(&dir, midnight - 2_000).unwrap().system.ts, midnight);
        assert_eq!(
            moment(&dir, midnight + 30_000).unwrap().system.ts,
            midnight + 10_000
        );
        assert_eq!(
            moment(&dir, midnight - 60_000).unwrap().system.ts,
            midnight - 10_000
        );
        assert!(moment(&dir, midnight + 70_001).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
