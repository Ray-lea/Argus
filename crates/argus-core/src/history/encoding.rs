//! The day files' records: packet framing and tags, variable-length
//! numbers, frames as stored, and the process rows of the `.proc` file.
use super::*;

/// How many programs a frame keeps the process count of (`ProgramCount`).
pub(super) const PROGRAMS_KEPT: usize = 8;

pub(super) const CHANNEL: u8 = 3;
pub(super) const INSTANCE: u8 = 4;
pub(super) const MARKER: u8 = 5;
pub(super) const FRAME: u8 = 6;
pub(super) const SYSTEM_ROLL: u8 = 7;
pub(super) const PRESENCE: u8 = 8;
/// A program's entry in the day's dictionary: `(id, name)`, before the first
/// frame that counts it.
pub(super) const PROGRAM: u8 = 14;
/// The program counts of the frame (or minute) just before, written after
/// every frame since 0.6: how many programs (u16), then `PROGRAMS_KEPT`
/// slots of dictionary id and count (u16 each; a count of 0 ends the list).
/// At a fixed size, frames keep fixed strides in checkpointed runs
/// (`FRAME_RUN_COUNTED`); it is not copied into the checkpoints.
pub(super) const PROGRAMS: u8 = 15;
pub(super) const PROGRAMS_LEN: usize = 2 + PROGRAMS_KEPT * 4;

pub(super) fn packet(out: &mut impl Write, tag: u8, data: &[u8]) -> io::Result<()> {
    out.write_all(&[tag])?;
    out.write_all(
        &u32::try_from(data.len())
            .map_err(|_| invalid())?
            .to_le_bytes(),
    )?;
    out.write_all(data)
}
pub(super) fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid history record")
}
pub(super) fn next_packet(bytes: &[u8]) -> Option<(u8, &[u8])> {
    let (header, rest) = bytes.split_first_chunk::<5>()?;
    let [tag, len @ ..] = *header;
    Some((tag, rest.get(..u32::from_le_bytes(len) as usize)?))
}

pub(super) fn packets(
    bytes: &[u8],
    mut at: usize,
    mut visit: impl FnMut(usize, u8, &[u8]) -> bool,
) -> usize {
    while let Some((tag, payload)) = bytes.get(at..).and_then(next_packet) {
        if !visit(at, tag, payload) {
            break;
        }
        at += 5 + payload.len();
    }
    at
}

pub(super) fn write_marker(
    out: &mut impl Write,
    kind: u8,
    ts: i64,
    boot: i64,
    awake: i64,
) -> io::Result<()> {
    let mut p = vec![kind];
    for v in [ts, boot, awake] {
        p.extend(v.to_le_bytes());
    }
    packet(out, MARKER, &p)
}
pub(super) fn var(out: &mut Vec<u8>, mut v: u64) {
    while v >= 128 {
        out.push(v as u8 | 128);
        v >>= 7;
    }
    out.push(v as u8);
}
// Working sets and video allocations normally contain whole pages. Zero escapes exact unaligned byte counts.
pub(super) fn put_memory(out: &mut Vec<u8>, v: u64) {
    if v.is_multiple_of(4096) {
        var(out, 1 + v / 4096);
    } else {
        var(out, 0);
        var(out, v);
    }
}
fn take_memory(raw: &mut &[u8]) -> Option<u64> {
    match take_var(raw)? {
        0 => take_var(raw),
        n => (n - 1).checked_mul(4096),
    }
}
pub(super) fn take_var(bytes: &mut &[u8]) -> Option<u64> {
    let mut v = 0;
    for shift in (0..64).step_by(7) {
        let (&b, rest) = bytes.split_first()?;
        *bytes = rest;
        if shift == 63 && b > 1 {
            return None;
        }
        v |= u64::from(b & 127) << shift;
        if b < 128 {
            return Some(v);
        }
    }
    None
}

pub(super) fn field<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..)?.first_chunk().copied()
}

pub(super) fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, serde::Deserialize)]
pub(super) struct Identity {
    pub(super) name: Arc<str>,
    pub(super) pid: u32,
    pub(super) created: i64,
}

pub(super) const FRAME_PREFIX: usize = 8 + 8 + 4 + 8 + 4;
pub(super) struct Stored<'a> {
    pub(super) ts: i64,
    pub(super) awake: i64,
    pub(super) span: i64,
    pub(super) offset: u64,
    pub(super) len: usize,
    pub(super) values: &'a [u8],
}
impl<'a> Stored<'a> {
    pub(super) fn parse(b: &'a [u8]) -> Option<Self> {
        let values = b.get(FRAME_PREFIX..)?;
        let span = i64::from(u32::from_le_bytes(field(b, 16)?));
        let offset = u64::from_le_bytes(field(b, 20)?);
        let len = u32::from_le_bytes(field(b, 28)?) as usize;
        offset.checked_add(len as u64)?;
        if !values.len().is_multiple_of(4)
            || values.len() > MAX_CHANNELS * 8
            || !(1..=FRAME_MS).contains(&span)
            || len > MAX_FRAME_ROWS * 160
        {
            return None;
        }
        Some(Self {
            ts: i64::from_le_bytes(field(b, 0)?),
            awake: i64::from_le_bytes(field(b, 8)?),
            span,
            offset,
            len,
            values,
        })
    }
    pub(super) fn value(&self, i: usize, catalog: &Catalog) -> f64 {
        let Some(&at) = catalog.offsets.get(i) else {
            return f64::NEG_INFINITY;
        };
        if catalog.rows[i].unit == "bytes" {
            field(self.values, at).map(f64::from_le_bytes)
        } else {
            field(self.values, at).map(|v| f32::from_le_bytes(v) as f64)
        }
        .unwrap_or(f64::NEG_INFINITY)
    }
}

pub(super) fn decode_rows(mut raw: &[u8], mut visit: impl FnMut(usize, Metrics)) -> Option<()> {
    let mut count = 0;
    while !raw.is_empty() {
        count += 1;
        if count > MAX_FRAME_ROWS {
            return None;
        }
        let encoded = usize::try_from(take_var(&mut raw)?).ok()?;
        let omitted = encoded / MAX_INSTANCES;
        if omitted > 3 {
            return None;
        }
        let id = encoded % MAX_INSTANCES;
        let (&flags, tail) = raw.split_first()?;
        raw = tail;
        if flags & 64 != 0 && flags & 32 == 0 {
            return None;
        }
        let float = |b: &mut &[u8]| -> Option<f64> {
            let (v, r) = b.split_at_checked(4)?;
            *b = r;
            finite(f32::from_le_bytes(v.try_into().ok()?) as f64)
        };
        let cpu = if flags & 1 != 0 { float(&mut raw)? } else { 0. };
        let gpu = if flags & 2 != 0 { float(&mut raw)? } else { 0. };
        let mem = take_memory(&mut raw)?;
        let io = take_var(&mut raw)?;
        let net = if omitted & 1 == 0 {
            take_var(&mut raw)? as f64
        } else {
            f64::NAN
        };
        let mut count = || {
            if omitted & 2 != 0 {
                Some(None)
            } else {
                Some(Some(u32::try_from(take_var(&mut raw)?).ok()?))
            }
        };
        let threads = count()?;
        let handles = count()?;
        let mut opt = |flag| -> Option<Option<u64>> {
            if flags & flag != 0 {
                Some(Some(take_var(&mut raw)?))
            } else {
                Some(None)
            }
        };
        let disk_read = opt(4u8)?;
        let disk_write = opt(8u8)?;
        let gpu_mem = if flags & 16 != 0 {
            Some(take_memory(&mut raw)?)
        } else {
            None
        };
        let power = if flags & 64 != 0 {
            Some(float(&mut raw)?)
        } else {
            (flags & 32 != 0).then_some(0.)
        };
        let mut rates = [
            Some(io as f64),
            Some(net),
            disk_read.map(|v| v as f64),
            disk_write.map(|v| v as f64),
        ];
        if flags & 128 != 0 {
            let (&mask, tail) = raw.split_first()?;
            raw = tail;
            if mask == 0 || mask & !15 != 0 {
                return None;
            }
            for (i, rate) in rates.iter_mut().enumerate() {
                if mask & (1 << i) != 0 {
                    let n = take_var(&mut raw)?;
                    if n == 0 {
                        return None;
                    }
                    *rate.as_mut()? /= n as f64;
                }
            }
        }
        visit(
            id,
            Metrics {
                cpu,
                gpu,
                mem,
                io: rates[0]?,
                net: rates[1]?,
                threads,
                handles,
                disk_read: rates[2],
                disk_write: rates[3],
                gpu_mem,
                power,
            },
        );
    }
    Some(())
}

#[derive(Clone, Copy, Default)]
pub(super) struct Metrics {
    pub(super) cpu: f64,
    pub(super) gpu: f64,
    pub(super) mem: u64,
    pub(super) io: f64,
    pub(super) net: f64,
    pub(super) threads: Option<u32>,
    pub(super) handles: Option<u32>,
    pub(super) disk_read: Option<f64>,
    pub(super) disk_write: Option<f64>,
    pub(super) gpu_mem: Option<u64>,
    pub(super) power: Option<f64>,
}
impl Metrics {
    pub(super) fn process(self, id: &Identity) -> FrameProcess {
        FrameProcess {
            name: id.name.to_string(),
            pid: id.pid,
            create_time: (id.created > 0).then_some(id.created),
            cpu: self.cpu as f32,
            gpu: self.gpu as f32,
            mem: self.mem,
            io: self.io,
            net: self.net,
            threads: self.threads,
            handles: self.handles,
            disk_read: self.disk_read,
            disk_write: self.disk_write,
            gpu_mem: self.gpu_mem,
            power: self.power,
        }
    }
}

pub(super) fn decode_programs(p: &[u8], names: &[String]) -> Option<(Vec<ProgramCount>, u32)> {
    let (total, slots) = p.split_first_chunk::<2>()?;
    if p.len() != PROGRAMS_LEN {
        return None;
    }
    let mut out = Vec::new();
    for slot in slots.as_chunks::<4>().0 {
        let count = u32::from(u16::from_le_bytes([slot[2], slot[3]]));
        if count == 0 {
            break;
        }
        let id = usize::from(u16::from_le_bytes([slot[0], slot[1]]));
        out.push(ProgramCount {
            name: names.get(id)?.clone(),
            count,
        });
    }
    Some((out, u32::from(u16::from_le_bytes(*total))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{proc, snapshot, temp_dir};

    #[test]
    fn process_rates_preserve_fractional_means_and_independent_divisors() {
        let dir = temp_dir("fractional");
        let base = 20_737 * DAY_MS;
        let mut rec = HistoryRecorder::new(dir.clone());
        for i in 0..5 {
            let mut p = proc(7, "task", 2., 4097);
            p.io_read = u64::from(i == 0);
            p.net_received = u64::from(i > 1);
            p.disk_read = (i < 2).then_some(i as u64);
            p.disk_write = None;
            p.gpu_dedicated = Some(4097);
            p.power = Some(0.);
            rec.record(&snapshot(base + i * 1000, 0., vec![p]));
        }
        drop(rec);
        let f = frame(&dir, base).unwrap();
        let p = &f.processes[0];
        assert_eq!(
            (p.io, p.net, p.disk_read, p.disk_write, p.power),
            (0.2, 0.6, Some(0.5), None, Some(0.))
        );
        assert_eq!(p.gpu_mem, Some(4097));
        let q = table(&dir, base, base + FRAME_MS, "net", 1);
        assert_eq!((q.processes[0].io, q.processes[0].net), (1, Some(3)));
        fs::remove_dir_all(dir).unwrap();
    }
}
