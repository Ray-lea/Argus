//! What the history takes on disk, and what a day of it would take.
use super::*;

/// What the recorded history takes on disk.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryUsage {
    /// Size of all day files.
    pub bytes: u64,
    /// Unix ms of the oldest recorded frame.
    pub first: Option<i64>,
    pub days: usize,
    pub breakdown: Breakdown,
    pub limit: HistoryLimit,
    pub estimate: HistoryEstimate,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEstimate {
    pub bytes_per_day: u64,
}

#[derive(Clone, Copy, Default, Serialize, serde::Deserialize)]
pub(super) struct Measurement {
    recording: Recording,
    pub(super) frames: u64,
    pub(super) bytes: u64,
}
impl Measurement {
    pub(super) fn save(&self, dir: &Path, day: i64) {
        if self.frames > 0 {
            let path = day_path(dir, day).with_extension("measure");
            if let Err(e) = serde_json::to_vec(self)
                .map_err(io::Error::other)
                .and_then(|b| fs::write(&path, b))
            {
                crate::diagnostics::file_failure("history size measurement", &path, &e);
            }
        }
    }
    pub(super) fn add(&mut self, recording: Recording, bytes: u64) {
        self.recording = recording;
        self.frames = self.frames.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Breakdown {
    pub system: u64,
    pub processes: u64,
    pub watch: u64,
}

pub fn usage(dir: &Path) -> HistoryUsage {
    usage_with(dir, HistoryOptions::default())
}
pub fn usage_with(dir: &Path, options: HistoryOptions) -> HistoryUsage {
    let _read = maintenance::PUBLICATION
        .read()
        .unwrap_or_else(|e| e.into_inner());
    let mut measurement = live(dir).measurement;
    let live_match = measurement.frames > 0 && measurement.recording == options.recording;
    let mut u = HistoryUsage {
        limit: options.limit,
        ..Default::default()
    };
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries
            .flatten()
            .filter(|e| owned_day(&e.file_name()).is_some())
        {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let counter = if matches!(
                entry.path().extension().and_then(OsStr::to_str),
                Some("proc" | "roll")
            ) {
                &mut u.breakdown.processes
            } else {
                &mut u.breakdown.system
            };
            *counter = counter.saturating_add(meta.len());
        }
    }
    for day in days(dir) {
        let path = day_path(dir, day);
        let Some(idx) = index(&path) else { continue };
        let bytes = metadata(&path).map_or(0, |m| m.len());
        let embedded = idx.process_bytes.min(bytes);
        u.breakdown.system = u.breakdown.system.saturating_sub(embedded);
        u.breakdown.processes = u.breakdown.processes.saturating_add(embedded);
        u.days += 1;
        if u.first.is_none() {
            u.first = idx.first.map(|s| s.ts);
        }
        if live_match || idx.frames.is_empty() {
            continue;
        }

        let measure_path = path.with_extension("measure");
        let saved = metadata(&measure_path)
            .ok()
            .and_then(|m| usize::try_from(m.len()).ok())
            .filter(|len| (1..=1024).contains(len))
            .and_then(|len| read_slice(&measure_path, 0, len))
            .and_then(|b| serde_json::from_slice::<Measurement>(&b).ok())
            .filter(|m| m.frames > 0 && m.bytes > 0);
        let candidate = saved.or_else(|| {
            (idx.version != MINUTE_VERSION).then(|| Measurement {
                recording: Recording::default(),
                frames: idx.frames.len() as u64,
                bytes: ["idx", "proc", "roll"].iter().fold(bytes, |sum, ext| {
                    sum.saturating_add(metadata(path.with_extension(ext)).map_or(0, |m| m.len()))
                }),
            })
        });
        if let Some(candidate) = candidate
            && (candidate.recording == options.recording
                || measurement.frames == 0
                || measurement.recording != options.recording)
        {
            measurement = candidate;
        }
    }
    let baseline = options.recording.fallback_frame_bytes() as f64;
    let frame_bytes = if measurement.frames == 0 {
        baseline
    } else {
        measurement.bytes as f64 / measurement.frames as f64 * baseline
            / measurement.recording.fallback_frame_bytes() as f64
    };
    u.estimate.bytes_per_day = (frame_bytes * 17_280.).round() as u64;
    u.breakdown.watch = crate::watch::WatchRecorder::storage_bytes(dir);
    u.bytes = u
        .breakdown
        .system
        .saturating_add(u.breakdown.processes)
        .saturating_add(u.breakdown.watch);
    u
}
