//! A day's index: its frames' places, catalog, identities, summaries and
//! gaps, kept in a bounded cache between queries.
use super::*;

#[derive(Clone, Copy, Default, Serialize, serde::Deserialize)]
pub(super) struct Stamp {
    pub(super) ts: i64,
    pub(super) span: i64,
    pub(super) boot: i64,
    pub(super) awake: i64,
    pub(super) stop: u8,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Gap {
    pub from: i64,
    pub to: i64,
    pub reason: &'static str,
}
pub(super) fn gap(a: Stamp, b: Stamp) -> Option<Gap> {
    let from = a.ts.checked_add(a.span)?;
    if b.ts <= from {
        return None;
    }
    let reason = if a.stop == 2 {
        "off"
    } else if a.boot == 0 || b.boot == 0 {
        "unknown"
    } else if a.boot.abs_diff(b.boot) > 2000 {
        "shutdown"
    } else if a.awake >= 0
        && b.awake >= a.awake
        && i128::from(b.ts) - i128::from(a.ts) - i128::from(b.awake - a.awake) > 2000
    {
        "sleep"
    } else {
        "notRunning"
    };
    Some(Gap {
        from,
        to: b.ts,
        reason,
    })
}

#[derive(Clone, Default)]
pub(super) struct Index {
    pub(super) day: i64,
    pub(super) version: u8,
    pub(super) valid: usize,
    pub(super) limited: bool,
    pub(super) disordered: bool,
    pub(super) catalog: Catalog,
    pub(super) ids: Vec<Identity>,
    /// Program names by id (`PROGRAM`).
    pub(super) programs: Vec<String>,
    pub(super) frames: Vec<(u32, u32)>,
    pub(super) summaries: Vec<RollRef>,
    pub(super) masks: Vec<(usize, Vec<u8>)>,
    pub(super) labels: Vec<(i64, usize, String)>,
    pub(super) gaps: Vec<Gap>,
    pub(super) first: Option<Stamp>,
    pub(super) last: Option<Stamp>,
    pub(super) boot: i64,
    pub(super) proc_end: u64,
    pub(super) process_bytes: u64,
    pub(super) edges: Option<[Stamp; 2]>,
}
impl Index {
    pub(super) fn gap_start(&self) -> Option<Stamp> {
        self.edges.map(|e| e[0]).or(self.first)
    }
    pub(super) fn gap_end(&self) -> Option<Stamp> {
        self.edges.map(|e| e[1]).or(self.last)
    }
    pub(super) fn stamp(&mut self, s: Stamp, at: usize) {
        if let Some(prev) = self.last
            && let Some(g) = gap(prev, s)
        {
            self.gaps.push(g);
        }
        self.first.get_or_insert(s);
        self.last = Some(s);
        self.frames
            .push((s.ts.rem_euclid(DAY_MS) as u32, at as u32));
    }
    // In a checkpoint, expand runs back to main-file byte positions. Every
    // address and the block end must agree with the catalog's frame width.
    pub(super) fn scan(
        &mut self,
        bytes: &[u8],
        proc_limit: u64,
        checkpoint_end: Option<usize>,
    ) -> usize {
        packets(bytes, 0, |_, tag, p| {
            let at = self.valid;
            let Some(end) = at.checked_add(5 + p.len()) else {
                return false;
            };
            let limit = checkpoint_end.unwrap_or(MAX_FILE_BYTES);
            if tag == FRAME_RUN || tag == FRAME_RUN_COUNTED {
                return checkpoint_end.is_some()
                    && self.address_run(p, limit, tag == FRAME_RUN_COUNTED);
            }
            if end > limit
                || self
                    .record(at, tag, p, proc_limit, checkpoint_end.is_some())
                    .is_none()
            {
                return false;
            }
            self.valid = end;
            true
        })
    }

    fn record(
        &mut self,
        at: usize,
        tag: u8,
        p: &[u8],
        proc_limit: u64,
        checkpoint: bool,
    ) -> Option<()> {
        let capped = match tag {
            CHANNEL => p.len() > MAX_METADATA_BYTES || self.labels.len() >= MAX_FRAMES,
            INSTANCE => p.len() > MAX_METADATA_BYTES || self.ids.len() >= MAX_INSTANCES,
            PROGRAM => p.len() > MAX_METADATA_BYTES || self.programs.len() >= MAX_INSTANCES,
            minute::MINUTE if self.version == MINUTE_VERSION => self.frames.len() >= 1440,
            minute::GAP if self.version == MINUTE_VERSION => self.gaps.len() >= MAX_FRAMES,
            FRAME => self.frames.len() >= MAX_FRAMES,
            PRESENCE => self.masks.len() >= MAX_FRAMES,
            _ => false,
        };
        if capped {
            self.limited = true;
            return None;
        }
        match tag {
            CHANNEL => {
                let (i, c) = serde_json::from_slice::<(usize, HistoryChannel)>(p).ok()?;
                if i == MAX_CHANNELS && i == self.catalog.rows.len() {
                    self.limited = true;
                    return None;
                }
                if i > self.catalog.rows.len()
                    || self
                        .catalog
                        .rows
                        .get(i)
                        .is_some_and(|old| old.key != c.key || old.unit != c.unit)
                    || self.catalog.by_key.get(&c.key).is_some_and(|&j| j != i)
                {
                    return None;
                }
                let j = self.catalog.register(
                    &c.key,
                    &c.unit,
                    &c.group,
                    &c.label,
                    c.device.as_deref(),
                    c.first,
                )?;
                self.catalog.rows[j].service = c.service;
                self.labels.push((c.first, j, c.label));
            }
            INSTANCE => {
                let (i, id) = serde_json::from_slice::<(usize, Identity)>(p).ok()?;
                if i != self.ids.len() {
                    return None;
                }
                self.ids.push(id);
                self.process_bytes += (5 + p.len()) as u64;
            }
            PROGRAM => {
                let (i, name) = serde_json::from_slice::<(usize, String)>(p).ok()?;
                if i != self.programs.len() {
                    return None;
                }
                self.programs.push(name);
                self.process_bytes += (5 + p.len()) as u64;
            }
            // Read with the frame before it.
            PROGRAMS if p.len() == PROGRAMS_LEN => self.process_bytes += (5 + p.len()) as u64,
            MARKER if p.len() == 25 => match p[0] {
                1 => self.boot = i64::from_le_bytes(field(p, 9)?),
                2 | 3 => {
                    if let Some(s) = &mut self.last {
                        s.stop = p[0];
                    }
                }
                _ => return None,
            },
            PRESENCE if p.len() == self.catalog.rows.len().div_ceil(8) => {
                self.masks.push((self.frames.len(), p.to_vec()));
            }
            SYSTEM_ROLL => {
                if !self.disordered
                    && let Some(r) = RollRef::parse(p, self)
                {
                    self.summaries.push(r);
                }
            }
            minute::MINUTE if self.version == MINUTE_VERSION => {
                let minute = Minute::parse(p, self.day)?;
                if self.last.is_some_and(|s| s.ts >= minute.ts) {
                    return None;
                }
                let stamp = Stamp {
                    ts: minute.ts,
                    span: MINUTE_MS,
                    ..Default::default()
                };
                self.first.get_or_insert(stamp);
                self.last = Some(stamp);
                self.frames
                    .push((stamp.ts.rem_euclid(DAY_MS) as u32, at as u32));
                self.process_bytes += minute.process_bytes();
            }
            minute::EDGES if self.version == MINUTE_VERSION && p.len() <= 512 => {
                let edges: [Stamp; 2] = serde_json::from_slice(p).ok()?;
                if edges[1].ts < edges[0].ts
                    || edges.iter().any(|s| {
                        s.ts.div_euclid(DAY_MS) != self.day
                            || !(1..=FRAME_MS).contains(&s.span)
                            || s.stop > 3
                    })
                {
                    return None;
                }
                self.edges = Some(edges);
            }
            minute::SEALED if self.version == MINUTE_VERSION && p == minute::SEAL => (),
            minute::GAP if self.version == MINUTE_VERSION => {
                let g = minute::parse_gap(p, self.day)?;
                if self.gaps.last().is_some_and(|prev| prev.to > g.from) {
                    return None;
                }
                self.gaps.push(g);
            }
            FRAME if self.version != MINUTE_VERSION && !checkpoint => {
                let f = Stored::parse(p)?;
                let end = f.offset.checked_add(f.len as u64)?;
                if f.offset != self.proc_end
                    || end > proc_limit
                    || f.values.len() != self.catalog.bytes
                    || f.ts.div_euclid(DAY_MS) != self.day
                {
                    return None;
                }
                self.proc_end = end;
                // Keep physical offsets for the following records, but expose
                // the first pass through a repeated wall-clock interval.
                if self.last.is_some_and(|s| f.ts <= s.ts) {
                    self.disordered = true;
                    self.summaries.clear();
                    return Some(());
                }
                self.stamp(
                    Stamp {
                        ts: f.ts,
                        span: f.span,
                        boot: self.boot,
                        awake: f.awake,
                        stop: 0,
                    },
                    at,
                );
            }
            _ => return None,
        }
        Some(())
    }

    pub(super) fn range(&self, day: i64, from: i64, to: i64) -> std::ops::Range<usize> {
        let start = day * DAY_MS;
        self.frames
            .partition_point(|&(t, _)| start + i64::from(t) < from)
            ..self
                .frames
                .partition_point(|&(t, _)| start + i64::from(t) <= to)
    }
}

pub(super) struct CachedIndex {
    generation: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    len: u64,
    cost: usize,
    /// When it was last read (`INDEX_USES`), to drop the least recently used first.
    used: u64,
    idx: Arc<Index>,
}
/// What the day indexes kept between queries may cost. A day's index is about
/// 0.6 MiB in the current format and 2.2 MiB in the older ones: a week of the
/// older ones fits, so that a week's range read again is not read from disk.
const INDEX_CACHE_BYTES: usize = 24 * 1024 * 1024;
static INDEX_USES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CLEAR_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(super) static INDEXES: LazyLock<Mutex<HashMap<PathBuf, CachedIndex>>> =
    LazyLock::new(Default::default);

pub(super) fn index(path: &Path) -> Option<Arc<Index>> {
    let generation = CLEAR_GENERATION.load(std::sync::atomic::Ordering::Acquire);
    let meta = history_read(path, metadata(path))?;
    let len = meta.len();
    if len > MAX_FILE_BYTES as u64 {
        return None;
    }
    let modified = meta.modified().ok();
    let created = meta.created().ok();
    let mut cache = INDEXES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = cache.get_mut(path)
        && c.generation == generation
        && c.created == created
        && c.modified == modified
        && c.len == len
    {
        c.used = INDEX_USES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Some(c.idx.clone());
    }
    let mut idx = cache
        .remove(path)
        .filter(|c| c.generation == generation && c.len < len && c.created == created)
        .map(|c| c.idx)
        .unwrap_or_default();
    drop(cache);
    let mut f = history_read(path, File::open(path))?;
    let data = Arc::make_mut(&mut idx);
    if data.valid == 0 {
        let mut magic = [0; 5];
        f.read_exact(&mut magic).ok()?;
        // Other versions (the v1/v2 days of earlier releases) are not read.
        if &magic != MAGIC && &magic != MINUTE_MAGIC {
            return None;
        }
        data.day = day_of(path.file_name()?)?;
        data.version = magic[4];
        data.valid = 5;
        if data.version == VERSION {
            data.load_checkpoints(path, len);
        }
    }
    f.seek(SeekFrom::Start(data.valid as u64)).ok()?;
    let mut tail = Vec::new();
    history_read(
        path,
        f.take(len.saturating_sub(data.valid as u64))
            .read_to_end(&mut tail),
    )?;
    data.scan(&tail, u64::MAX, None);
    // Bound uncommon catalogs and churn too (`INDEX_CACHE_BYTES`).
    let cost = |i: &Index| {
        i.frames.capacity() * 8
            + i.masks.iter().map(|m| 32 + m.1.capacity()).sum::<usize>()
            + i.summaries.capacity() * std::mem::size_of::<RollRef>()
            + i.summaries
                .iter()
                .map(|r| r.bands.len() * std::mem::size_of::<Band>())
                .sum::<usize>()
            + i.gaps.capacity() * std::mem::size_of::<Gap>()
            + i.ids.iter().map(|id| 64 + id.name.len()).sum::<usize>()
            + i.programs.iter().map(|n| 32 + n.len()).sum::<usize>()
            + i.catalog
                .rows
                .iter()
                .map(|c| {
                    256 + c.key.len() * 2
                        + c.unit.len()
                        + c.label.len()
                        + c.group.len()
                        + c.device.as_ref().map_or(0, String::len)
                })
                .sum::<usize>()
            + i.labels.iter().map(|(_, _, s)| 48 + s.len()).sum::<usize>()
    };
    let size = cost(&idx).max(512);
    let mut cache = INDEXES.lock().unwrap_or_else(|e| e.into_inner());
    if size <= INDEX_CACHE_BYTES
        && generation == CLEAR_GENERATION.load(std::sync::atomic::Ordering::Acquire)
    {
        // The days read longest ago go first: a long range read once leaves
        // the recent days that the shorter ranges read.
        let mut total = cache
            .iter()
            .filter(|(p, _)| p.as_path() != path)
            .map(|(_, c)| c.cost)
            .sum::<usize>();
        while total + size > INDEX_CACHE_BYTES {
            let Some(oldest) = cache
                .iter()
                .filter(|(p, _)| p.as_path() != path)
                .min_by_key(|(_, c)| c.used)
                .map(|(p, _)| p.clone())
            else {
                break;
            };
            if let Some(dropped) = cache.remove(&oldest) {
                total = total.saturating_sub(dropped.cost);
            }
        }
        cache.insert(
            path.into(),
            CachedIndex {
                generation,
                modified,
                cost: size,
                created,
                len,
                used: INDEX_USES.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                idx: idx.clone(),
            },
        );
    }
    Some(idx)
}
pub(super) fn forget(dir: &Path) {
    CLEAR_GENERATION.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    if let Ok(mut cache) = INDEXES.try_lock() {
        cache.retain(|p, _| p.parent() != Some(dir));
    }
}
