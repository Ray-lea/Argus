//! Summaries that accelerate broad queries: quarter-hour system statistics
//! in the main file (`SystemRoll`), and process totals of up to five
//! minutes in `.roll` (`Roll`, `Totals`).
use super::*;

#[derive(Default, Clone, Copy)]
pub(super) struct Totals {
    pub(super) first: i64,
    pub(super) last: i64,
    pub(super) seen: i64,
    pub(super) cpu: f64,
    pub(super) gpu: f64,
    pub(super) cpu_max: f64,
    pub(super) mem: u64,
    pub(super) mem_sum: f64,
    pub(super) io: f64,
    pub(super) net: f64,
    pub(super) net_ms: i64,
    pub(super) read: Option<f64>,
    pub(super) write: Option<f64>,
    pub(super) disk: f64,
    pub(super) disk_ms: i64,
    pub(super) gpu_mem: Option<u64>,
    pub(super) power: f64,
    pub(super) power_ms: i64,
    pub(super) threads: Option<u32>,
    pub(super) handles: Option<u32>,
}
impl Totals {
    pub(super) fn observation(m: Metrics, ts: i64, span: i64) -> Self {
        let ms = span as f64;
        Self {
            first: ts,
            last: ts,
            seen: span,
            cpu: m.cpu * ms,
            gpu: m.gpu * ms,
            cpu_max: m.cpu,
            mem: m.mem,
            mem_sum: m.mem as f64 * ms,
            io: m.io * ms,
            net: m.net * ms,
            net_ms: if m.net.is_finite() { span } else { 0 },
            read: m.disk_read.map(|v| v * ms),
            write: m.disk_write.map(|v| v * ms),
            disk: m
                .disk_read
                .zip(m.disk_write)
                .map_or(0., |(r, w)| (r + w) * ms),
            disk_ms: if m.disk_read.is_some() && m.disk_write.is_some() {
                span
            } else {
                0
            },
            gpu_mem: m.gpu_mem,
            power: m.power.unwrap_or_default() * ms,
            power_ms: if m.power.is_some() { span } else { 0 },
            threads: m.threads,
            handles: m.handles,
        }
    }
    pub(super) fn merge(&mut self, b: Self) {
        if self.seen == 0 {
            *self = b;
            return;
        }
        self.first = self.first.min(b.first);
        self.last = self.last.max(b.last);
        self.seen += b.seen;
        self.cpu += b.cpu;
        self.gpu += b.gpu;
        self.cpu_max = self.cpu_max.max(b.cpu_max);
        self.mem = self.mem.max(b.mem);
        self.mem_sum += b.mem_sum;
        self.io += b.io;
        self.net_ms += b.net_ms;
        if b.net.is_finite() {
            self.net = if self.net.is_finite() {
                self.net + b.net
            } else {
                b.net
            };
        }
        for (sum, v) in [(&mut self.read, b.read), (&mut self.write, b.write)] {
            if let Some(v) = v {
                *sum.get_or_insert(0.) += v;
            }
        }
        self.disk += b.disk;
        self.disk_ms += b.disk_ms;
        self.gpu_mem = self.gpu_mem.max(b.gpu_mem);
        self.power += b.power;
        self.power_ms += b.power_ms;
        self.threads = self.threads.max(b.threads);
        self.handles = self.handles.max(b.handles);
    }
    pub(super) fn trend(&self, key: &str) -> (f64, i64) {
        match key {
            "mem" => (self.mem_sum, self.seen),
            "disk" => (self.disk, self.disk_ms),
            "net" if self.net.is_nan() => (0., 0),
            "net" => (self.net, self.net_ms),
            "gpu" => (self.gpu, self.seen),
            _ => (self.cpu, self.seen),
        }
    }
    pub(super) fn encode(self, out: &mut Vec<u8>) {
        for v in [
            self.first,
            self.last,
            self.seen,
            self.disk_ms,
            self.power_ms,
        ] {
            var(out, v as u64);
        }
        for v in [
            self.mem,
            self.gpu_mem.map_or(0, |v| v.saturating_add(1)),
            self.threads.map_or(0, |v| u64::from(v) + 1),
            self.handles.map_or(0, |v| u64::from(v) + 1),
        ] {
            var(out, v);
        }
        for v in [
            self.cpu,
            self.gpu,
            self.cpu_max,
            self.mem_sum,
            self.io,
            self.net,
            self.read.unwrap_or(f64::NAN),
            self.write.unwrap_or(f64::NAN),
            self.disk,
            self.power,
        ] {
            out.extend(v.to_le_bytes());
        }
    }
    pub(super) fn decode(raw: &mut &[u8]) -> Option<Self> {
        let mut n = || take_var(raw);
        let first = n()? as i64;
        let last = n()? as i64;
        let seen = n()? as i64;
        let disk_ms = n()? as i64;
        let power_ms = n()? as i64;
        let mem = n()?;
        let gpu_mem = n()?.checked_sub(1);
        let threads = n()?.checked_sub(1).map(u32::try_from).transpose().ok()?;
        let handles = n()?.checked_sub(1).map(u32::try_from).transpose().ok()?;
        let mut f = || {
            let (v, r) = raw.split_at_checked(8)?;
            *raw = r;
            Some(f64::from_le_bytes(v.try_into().ok()?))
        };
        let mut totals = Self {
            net_ms: 0,
            first,
            last,
            seen,
            disk_ms,
            power_ms,
            mem,
            gpu_mem,
            threads,
            handles,
            cpu: finite(f()?)?,
            gpu: finite(f()?)?,
            cpu_max: finite(f()?)?,
            mem_sum: finite(f()?)?,
            io: finite(f()?)?,
            net: f()?,
            read: finite(f()?),
            write: finite(f()?),
            disk: finite(f()?)?,
            power: finite(f()?)?,
        };
        if totals.net.is_infinite() {
            return None;
        }
        if totals.net.is_finite() {
            totals.net_ms = seen;
        }
        Some(totals)
    }
}

pub(super) const ROLL_MS: i64 = 900_000;
pub(super) const PROCESS_ROLL_MS: i64 = 300_000;
#[derive(Default)]
pub(super) struct Roll {
    pub(super) first: i64,
    pub(super) end: i64,
    pub(super) recorded: i64,
    pub(super) rows: HashMap<usize, Totals>,
}
impl Roll {
    pub(super) fn header(p: &[u8], idx: &Index) -> Option<Self> {
        let first = i64::from_le_bytes(field(p, 0)?);
        let end = i64::from_le_bytes(field(p, 8)?);
        let recorded = i64::from_le_bytes(field(p, 16)?);
        if idx.disordered
            || first.div_euclid(DAY_MS) != idx.day
            || end <= first
            || first.div_euclid(PROCESS_ROLL_MS) != (end - 1).div_euclid(PROCESS_ROLL_MS)
            || !(1..=end.checked_sub(first)?).contains(&recorded)
            || first < idx.first?.ts
            || end > idx.last?.ts.checked_add(idx.last?.span)?
            || idx
                .frames
                .binary_search_by_key(&(first.rem_euclid(DAY_MS) as u32), |r| r.0)
                .is_err()
        {
            return None;
        }
        Some(Self {
            first,
            end,
            recorded,
            ..Default::default()
        })
    }
    pub(super) fn read_rows(&mut self, p: &[u8], idx: &Index) -> Option<()> {
        let mut raw = p.get(24..)?;
        while !raw.is_empty() {
            let id = usize::try_from(take_var(&mut raw)?).ok()?;
            let t = Totals::decode(&mut raw)?;
            if id >= idx.ids.len()
                || !(1..=self.recorded).contains(&t.seen)
                || !(0..=t.seen).contains(&t.disk_ms)
                || !(0..=t.seen).contains(&t.power_ms)
                || t.first < self.first
                || t.last < t.first
                || t.last >= self.end
                || self.rows.insert(id, t).is_some()
            {
                return None;
            }
        }
        Some(())
    }
    pub(super) fn write(&mut self, out: &mut impl Write) -> io::Result<u64> {
        if self.recorded == 0 {
            return Ok(0);
        }
        let mut p = Vec::new();
        for v in [self.first, self.end, self.recorded] {
            p.extend(v.to_le_bytes());
        }
        for (&id, t) in &self.rows {
            var(&mut p, id as u64);
            t.encode(&mut p);
        }
        packet(out, 1, &p)?;
        out.flush()?;
        self.recorded = 0;
        self.rows.clear();
        Ok((5 + p.len()) as u64)
    }
}

// Embedded quarter-hour statistics cover finished groups of five-second frames.
// They accelerate broad queries without changing retention or frame detail.

#[derive(Default)]
pub(super) struct SystemRoll {
    pub(super) start: i64,
    last: i64,
    pub(super) count: u32,
    peak: i64,
    max: f64,
    bands: Vec<Band>,
}
impl SystemRoll {
    pub(super) fn add(&mut self, ts: i64, values: &[f64], peak: f32) {
        if self.count == 0 {
            self.start = ts;
            self.max = f64::NEG_INFINITY;
        }
        self.count += 1;
        self.last = ts;
        if f64::from(peak) > self.max {
            self.max = f64::from(peak);
            self.peak = ts;
        }
        self.bands.resize(values.len(), Band::default());
        for (b, &v) in self.bands.iter_mut().zip(values) {
            b.add(v);
        }
    }
    pub(super) fn finish(&mut self, out: &mut Vec<u8>) -> io::Result<()> {
        if self.count == 0 {
            return Ok(());
        }
        let mut bytes = Vec::new();
        for v in [self.start, self.last, self.peak] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend(self.count.to_le_bytes());
        bytes.extend(self.max.to_le_bytes());
        for b in &self.bands {
            bytes.extend(b.n.to_le_bytes());
            for v in [b.sum, b.low, b.high] {
                bytes.extend(v.to_le_bytes());
            }
        }
        packet(out, SYSTEM_ROLL, &bytes)?;
        self.count = 0;
        self.bands.clear();
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct RollRef {
    pub(super) first: usize,
    pub(super) last: usize,
    pub(super) start_ts: i64,
    pub(super) last_ts: i64,
    pub(super) peak: i64,
    pub(super) max: f64,
    pub(super) bands: Box<[Band]>,
}
impl RollRef {
    pub(super) fn parse(p: &[u8], idx: &Index) -> Option<Self> {
        let start_ts = i64::from_le_bytes(field(p, 0)?);
        let last_ts = i64::from_le_bytes(field(p, 8)?);
        let peak = i64::from_le_bytes(field(p, 16)?);
        let count = u32::from_le_bytes(field(p, 24)?);
        let max = finite(f64::from_le_bytes(field(p, 28)?))?;
        let last = idx.frames.len();
        let first = last.checked_sub(count as usize)?;
        let frames = idx.frames.get(first..last)?;
        let (values, tail) = p.get(36..)?.as_chunks::<28>();
        if !tail.is_empty()
            || values.len() != idx.catalog.rows.len()
            || start_ts.div_euclid(DAY_MS) != idx.day
            || last_ts.div_euclid(DAY_MS) != idx.day
            || start_ts.div_euclid(ROLL_MS) != last_ts.div_euclid(ROLL_MS)
            || !(start_ts..=last_ts).contains(&peak)
            || i64::from(frames.first()?.0) != start_ts.rem_euclid(DAY_MS)
            || i64::from(frames.last()?.0) != last_ts.rem_euclid(DAY_MS)
            || frames
                .binary_search_by_key(&(peak.rem_euclid(DAY_MS) as u32), |e| e.0)
                .is_err()
            || idx.summaries.last().is_some_and(|r| r.last > first)
        {
            return None;
        }
        let mut bands = Vec::with_capacity(values.len());
        for v in values {
            let n = u32::from_le_bytes(field(v, 0)?);
            let sum = finite(f64::from_le_bytes(field(v, 4)?))?;
            let low = f64::from_le_bytes(field(v, 12)?);
            let high = f64::from_le_bytes(field(v, 20)?);
            if n > count
                || (n > 0 && (!low.is_finite() || !high.is_finite() || low > high))
                || (n == 0 && (sum != 0. || low != f64::INFINITY || high != f64::NEG_INFINITY))
            {
                return None;
            }
            bands.push(Band { n, sum, low, high });
        }
        Some(Self {
            first,
            last,
            start_ts,
            last_ts,
            peak,
            max,
            bands: bands.into_boxed_slice(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{proc, snapshot, temp_dir};

    #[test]
    fn system_summaries_match_frames_and_skip_process_file() {
        let dir = temp_dir("system-roll");
        let base = 20_737 * DAY_MS;
        let mut r = HistoryRecorder::new(dir.clone());
        for i in 0..400 {
            let mut s = snapshot(
                base + i * 5000,
                (i % 19) as f32,
                vec![proc(17, "busy", 12., 1 << 20)],
            );
            s.system.mem_used = (1u64 << 35) + i as u64;
            r.record(&s);
        }
        drop(r);
        fs::remove_file(day_path(&dir, 20_737).with_extension("proc")).unwrap();
        let keys = ["cpuPeak".into(), "memUsed".into(), "missing".into()];
        let result = series(&dir, base, base + 2 * ROLL_MS - 1, ROLL_MS, base, &keys);
        assert_eq!(result.ts, [base, base + ROLL_MS]);
        assert_eq!(result.peak[0], base + 18 * FRAME_MS);
        let memory = &result.series["memUsed"];
        assert_eq!(memory.low[0], Some((1u64 << 35) as f64));
        assert_eq!(memory.high[0], Some(((1u64 << 35) + 179) as f64));
        assert_eq!(memory.mean[0], Some((1u64 << 35) as f64 + 89.5));
        assert_eq!(result.series["missing"].high, [None, None]);
        let offset = series(
            &dir,
            base + 1000,
            base + ROLL_MS + 999,
            ROLL_MS,
            base + 1000,
            &keys,
        );
        assert_eq!(
            offset.series["memUsed"].mean[0],
            Some((1u64 << 35) as f64 + 90.5)
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
