//! Local, allowlisted interval reports. Source names and process identifiers never enter the body.
use argus_core::{
    history::{IntervalReport, ResourceSummary},
    tr,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub const MAX_BYTES: usize = 1 << 20;
const MAX_ROWS: usize = 1024;
#[cfg(test)]
#[path = "reports/dialog_test.rs"]
mod dialog_test;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    Markdown,
    Csv,
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Csv => "csv",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: u64,
    pub format: Format,
    pub body: Arc<str>,
    pub sha256: String,
}

#[derive(Default)]
pub struct PreviewStore(Mutex<(u64, Option<Preview>)>);
impl PreviewStore {
    pub fn prepare(
        &self,
        report: &IntervalReport,
        format: Format,
    ) -> Result<Preview, &'static str> {
        let body: Arc<str> = render(report, format)?.into();
        let sha256 = Sha256::digest(body.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let mut store = self.0.lock().map_err(|_| "storage")?;
        store.0 = store.0.checked_add(1).ok_or("storage")?;
        let preview = Preview {
            id: store.0,
            format,
            body,
            sha256,
        };
        store.1 = Some(preview.clone());
        Ok(preview)
    }
    /// The caller saves these exact immutable bytes, without re-reading history or locale text.
    pub fn get(&self, id: u64, sha256: &str) -> Result<Preview, &'static str> {
        self.0
            .lock()
            .map_err(|_| "storage")?
            .1
            .as_ref()
            .filter(|preview| preview.id == id && preview.sha256 == sha256)
            .cloned()
            .ok_or("stale")
    }
}

fn stamp(ts: i64) -> String {
    match time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ts) * 1_000_000) {
        Ok(t) => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute(),
            t.second(),
            t.millisecond()
        ),
        Err(_) => tr!("format.missing"),
    }
}
fn number(value: Option<f64>) -> String {
    value
        .filter(|value| value.is_finite())
        .map(|value| {
            format!("{value:.6}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        })
        .unwrap_or_else(|| tr!("format.missing"))
}
fn clean(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') {
                ' '
            } else {
                c
            }
        })
        .collect()
}
fn markdown(value: &str) -> String {
    let mut out = String::new();
    for c in clean(value).chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\\' | '|' | '`' | '*' | '_' | '[' | ']' | '{' | '}' | '!' | '#' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}
fn csv(value: &str) -> String {
    let mut value = clean(value);
    if value.trim_start().starts_with(['=', '+', '-', '@']) {
        value.insert(0, '\'');
    }
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[derive(Default)]
struct Aliases {
    known: BTreeMap<(u32, i64), String>,
    next: usize,
}
impl Aliases {
    fn next(&mut self) -> String {
        self.next += 1;
        tr!("history.report.alias", count = self.next)
    }
    fn get(&mut self, pid: u32, created: Option<i64>) -> String {
        let Some(created) = created.filter(|created| *created > 0) else {
            return self.next();
        };
        if let Some(alias) = self.known.get(&(pid, created)) {
            return alias.clone();
        }
        let alias = self.next();
        self.known.insert((pid, created), alias.clone());
        alias
    }
}

// All columns are constructed here from numeric facts and trusted locale strings.
// No serialized source object, arbitrary metadata or name is copied into the report.
type Row = [String; 10];
#[expect(
    clippy::too_many_arguments,
    reason = "Explicit fixed report columns shared by both output formats"
)]
fn row(
    scope: &str,
    resource: &str,
    alias: String,
    from: i64,
    to: i64,
    mean: Option<f64>,
    peak: Option<f64>,
    at: Option<i64>,
    unit: &str,
    coverage: String,
) -> Row {
    [
        argus_core::i18n::text(&format!("history.report.scope.{scope}")),
        argus_core::i18n::text(&format!("history.report.resource.{resource}")),
        alias,
        stamp(from),
        stamp(to),
        number(mean),
        number(peak),
        at.map(stamp).unwrap_or_default(),
        unit.into(),
        coverage,
    ]
}
fn coverage(value: &ResourceSummary, total: usize) -> String {
    tr!(
        "history.report.sourceCoverage",
        valid = value.valid_frames,
        total = total,
        partial = value.partial_frames,
        legacy = value.legacy_frames,
        invalid = value.invalid_frames,
        samples = value
            .sample_coverage
            .as_ref()
            .map(|c| c.valid.to_string())
            .unwrap_or_else(|| tr!("format.missing")),
        expected = value
            .sample_coverage
            .as_ref()
            .map(|c| c.samples.to_string())
            .unwrap_or_else(|| tr!("format.missing")),
        reasons = value.sample_coverage.as_ref().map_or(0, |c| c.missing)
    )
}

pub fn render(report: &IntervalReport, format: Format) -> Result<String, &'static str> {
    let mut rows = Vec::<Row>::new();
    let mut aliases = Aliases::default();
    rows.push(row(
        "system",
        "cpu",
        String::new(),
        report.from,
        report.to,
        report.cpu.mean,
        report.cpu.peak.map(f64::from),
        report.cpu.peak_ts,
        "%",
        tr!(
            "history.report.cpuCoverage",
            valid = report.cpu.valid_frames,
            total = report.expected_frames,
            invalid = report.cpu.invalid_frames
        ),
    ));
    for (key, summary, unit) in [
        ("memory", &report.memory, "B"),
        ("disk", &report.disk, "B/s"),
        ("gpu", &report.gpu, "%"),
        ("network", &report.network, "B/s"),
    ] {
        rows.push(row(
            "system",
            key,
            String::new(),
            report.from,
            report.to,
            summary.mean,
            summary.peak,
            summary.peak_ts,
            unit,
            coverage(summary, report.expected_frames),
        ));
    }
    for leader in &report.leaders {
        let alias = aliases.get(leader.pid, leader.create_time);
        rows.push(row(
            "ranked",
            "cpu",
            alias.clone(),
            leader.first_ts,
            leader.last_ts,
            leader.cpu.mean,
            leader.cpu.peak.map(f64::from),
            leader.cpu.peak_ts,
            "%",
            tr!(
                "history.report.cpuCoverage",
                valid = leader.cpu.valid_frames,
                total = report.recorded_frames,
                invalid = leader.cpu.invalid_frames
            ),
        ));
        rows.push(row(
            "ranked",
            "privateWs",
            alias.clone(),
            leader.first_ts,
            leader.last_ts,
            None,
            Some(leader.private_ws_peak as f64),
            None,
            "B",
            tr!(
                "history.report.listed",
                count = leader.observed_frames,
                total = report.recorded_frames
            ),
        ));
        for (key, summary, unit) in [
            ("io", &leader.io, "B/s"),
            ("gpu", &leader.gpu, "%"),
            ("network", &leader.network, "B/s"),
        ] {
            rows.push(row(
                "ranked",
                key,
                alias.clone(),
                leader.first_ts,
                leader.last_ts,
                summary.mean,
                summary.peak,
                summary.peak_ts,
                unit,
                coverage(summary, report.recorded_frames),
            ));
        }
    }
    for run in &report.observations.runs {
        let alias = aliases.get(run.pid, Some(run.create_time));
        for (key, summary) in [("privateWs", &run.memory), ("privateCommit", &run.commit)] {
            rows.push(row(
                "observation",
                key,
                alias.clone(),
                run.first_ts,
                run.last_ts,
                summary.mean,
                summary.peak.map(|v| v as f64),
                summary.peak_ts,
                "B",
                tr!(
                    "history.report.observationCoverage",
                    valid = summary.samples,
                    missing = summary.missing,
                    gaps = run.gap_events,
                    slots = run.missing_sample_slots,
                    terminal = run.terminal_events
                ),
            ));
        }
    }
    for gap in &report.gaps {
        let mut cells: Row = Default::default();
        cells[0] = tr!("history.report.scope.gap");
        cells[3] = stamp(gap.from);
        cells[4] = stamp(gap.to);
        rows.push(cells);
    }
    if rows.len() > MAX_ROWS {
        return Err("tooLarge");
    }
    let mut notes = vec![
        tr!("history.report.privacy"),
        tr!(
            "history.report.timing",
            step = report.frame_ms,
            sample = report.observations.sample_ms
        ),
        tr!(
            "history.report.extent",
            recorded = report.recorded_frames,
            expected = report.expected_frames,
            missing = report.missing_frames,
            gaps = report.gap_count,
            identities = report.identities_seen,
            omitted = report.omitted_entries,
            unreadable = report.unreadable_files,
            incomplete = report.incomplete_files,
            invalid = report.invalid_frames
        ),
        tr!(
            "history.report.observationExtent",
            from = report
                .observations
                .first_saved_ts
                .map(stamp)
                .unwrap_or_else(|| tr!("format.missing")),
            to = report
                .observations
                .last_saved_ts
                .map(stamp)
                .unwrap_or_else(|| tr!("format.missing")),
            saved = report.observations.saved_points,
            selected = report.observations.selected_points,
            shown = report.observations.runs.len(),
            total = report.observations.runs_seen,
            unreadable = report.observations.unreadable_files,
            damaged = report.observations.damaged_files,
            expired = report.observations.expired_points,
            omittedPoints = report.observations.omitted_points,
            omittedRuns = report.observations.omitted_runs
        ),
        tr!("history.report.cpuNote"),
        tr!("history.interval.resourceNote"),
        tr!("history.interval.memoryNote"),
        tr!("history.interval.memoryCaveat"),
        tr!("history.interval.gpuCaveat"),
        tr!("history.interval.observationNote"),
        tr!("history.report.reasons"),
    ];
    if report.observations.source_files == 0 {
        notes.push(tr!("history.interval.observationEmpty"));
    }
    let headers = [
        "scope", "resource", "instance", "from", "to", "mean", "peak", "peakAt", "unit", "coverage",
    ]
    .map(|key| argus_core::i18n::text(&format!("history.report.columns.{key}")));
    let mut out = match format {
        Format::Markdown => format!(
            "# {}\n\n{} — {}\n\n",
            markdown(&tr!("history.report.title")),
            stamp(report.from),
            stamp(report.to)
        ),
        Format::Csv => String::new(),
    };
    match format {
        Format::Markdown => {
            for note in notes {
                out.push_str(&format!("{}\n\n", markdown(&note)));
            }
            for (scope, start, end) in [
                ("system", 0, 5),
                ("ranked", 5, 5 + report.leaders.len() * 5),
                (
                    "observation",
                    5 + report.leaders.len() * 5,
                    5 + report.leaders.len() * 5 + report.observations.runs.len() * 2,
                ),
                (
                    "gap",
                    5 + report.leaders.len() * 5 + report.observations.runs.len() * 2,
                    rows.len(),
                ),
            ] {
                if start == end {
                    continue;
                }
                out.push_str(&format!(
                    "## {}\n\n",
                    markdown(&argus_core::i18n::text(&format!(
                        "history.report.scope.{scope}"
                    )))
                ));
                let columns = if scope == "gap" {
                    vec![3, 4]
                } else {
                    vec![1, 2, 3, 4, 5, 6, 7, 8, 9]
                };
                out.push_str(&format!(
                    "| {} |\n",
                    columns
                        .iter()
                        .map(|i| markdown(&headers[*i]))
                        .collect::<Vec<_>>()
                        .join(" | ")
                ));
                out.push_str(&format!(
                    "|{}|\n",
                    columns.iter().map(|_| "---").collect::<Vec<_>>().join("|")
                ));
                for row in &rows[start..end] {
                    out.push_str(&format!(
                        "| {} |\n",
                        columns
                            .iter()
                            .map(|i| markdown(&row[*i]))
                            .collect::<Vec<_>>()
                            .join(" | ")
                    ));
                    if out.len() > MAX_BYTES {
                        return Err("tooLarge");
                    }
                }
                out.push('\n');
            }
        }
        Format::Csv => {
            out.push_str(&headers.iter().map(|s| csv(s)).collect::<Vec<_>>().join(","));
            out.push_str("\r\n");
            for note in notes {
                let mut cells: Row = Default::default();
                cells[0] = tr!("history.report.scope.note");
                cells[9] = note;
                rows.push(cells);
            }
            for row in rows {
                out.push_str(&row.iter().map(|s| csv(s)).collect::<Vec<_>>().join(","));
                out.push_str("\r\n");
                if out.len() > MAX_BYTES {
                    return Err("tooLarge");
                }
            }
        }
    }
    if out.len() > MAX_BYTES {
        Err("tooLarge")
    } else {
        Ok(out)
    }
}

pub fn save(preview: &Preview, path: &std::path::Path) -> Result<(), &'static str> {
    use std::{io::Write, os::windows::ffi::OsStrExt};
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };
    let parent = path.parent().ok_or("storage")?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "storage")?
        .as_nanos();
    let temporary = parent.join(format!(
        ".argus-report-{}-{}-{unique}.tmp",
        std::process::id(),
        preview.id
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "storage")?;
    let result = (|| {
        file.write_all(preview.body.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| "storage")?;
        drop(file);
        let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(destination.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|_| "storage")
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{Aliases, Format, PreviewStore, csv, markdown, render, save};
    use argus_core::{
        history::{self, CpuLeader, CpuSummary, IntervalMetric},
        watch::{ObservationRun, SavedMemory, WatchMode, WatchState},
    };
    use sha2::{Digest, Sha256};

    pub(super) fn folder() -> std::path::PathBuf {
        crate::catalog::register();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("argus-report-{}-{unique}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        dir
    }
    fn report(dir: &std::path::Path) -> history::IntervalReport {
        let mut report = history::interval(
            dir,
            1_700_000_000_000,
            1_700_000_010_000,
            IntervalMetric::Cpu,
        )
        .unwrap();
        let secret = "SECRET_NAME_43918 C:\\Users\\SECRET_ACCOUNT\\secret.txt \\host-secret\\share --password=SECRET_TOKEN <img src=x>\n=HYPERLINK(\"https://SECRET_HOST\")\u{202e}";
        report.leaders.push(CpuLeader {
            name: secret.into(),
            pid: 1_309_149_133,
            create_time: Some(1_600_000_000_123),
            first_ts: report.from,
            last_ts: report.from + 5000,
            observed_frames: 2,
            cpu: CpuSummary {
                mean: Some(0.),
                peak: Some(0.),
                peak_ts: Some(report.from),
                valid_frames: 2,
                invalid_frames: 0,
            },
            recorded_cpu_score: 0.,
            private_ws_peak: 123,
            io: report.disk.clone(),
            gpu: report.gpu.clone(),
            network: report.network.clone(),
        });
        report.observations.runs.push(ObservationRun {
            mode: WatchMode::Program,
            name: secret.into(),
            pid: 1_309_149_133,
            create_time: 1_600_000_000_123,
            first_ts: report.from,
            last_ts: report.from + 5000,
            points: 2,
            gap_events: 1,
            missing_sample_slots: 1,
            terminal_events: 1,
            last_recorded_state: WatchState::Exited,
            memory: SavedMemory {
                mean: Some(0.),
                peak: Some(0),
                peak_ts: Some(report.from),
                samples: 1,
                missing: 0,
            },
            commit: SavedMemory {
                mean: None,
                peak: None,
                peak_ts: None,
                samples: 0,
                missing: 1,
            },
        });
        report.observations.runs_seen = 1;
        report.observations.source_files = 1;
        report
    }

    #[test]
    fn aliases_share_only_known_instance_identity_and_never_merge_unknown_frames() {
        crate::catalog::register();
        let mut aliases = Aliases::default();
        let first = aliases.get(17, Some(100));
        assert_eq!(first, aliases.get(17, Some(100)));
        assert_ne!(first, aliases.get(17, Some(200)));
        assert_ne!(aliases.get(17, None), aliases.get(17, None));
        assert_ne!(aliases.get(17, Some(0)), aliases.get(17, Some(0)));
    }

    #[test]
    fn writer_reader_reports_keep_zero_missing_gaps_and_observation_memory_separate() {
        use std::io::Write;
        let dir = folder();
        let start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
            / 5000
            * 5000
            - 60000;
        let created = start - 5000;
        let mut recorder = history::HistoryRecorder::new(dir.clone());
        for i in 0..=5 {
            recorder.record(&argus_core::Snapshot {
                timestamp: start + i * 5000,
                seq: i as u64 + 1,
                process_network: false,
                hardware: Default::default(),
                // New 0.5.0 source fields must never be copied into a report implicitly.
                sessions: vec![argus_core::sessions::LogonSession {
                    id: 17,
                    user: "SECRET_SESSION_USER".into(),
                    domain: "SECRET_SESSION_DOMAIN".into(),
                    current: false,
                    disconnected: false,
                }],
                system: argus_core::SystemStats {
                    cpu: 3.,
                    mem_used: 1 << 30,
                    mem_total: 2 << 30,
                    ..Default::default()
                },
                processes: vec![argus_core::ProcessRow {
                    pid: 17,
                    create_time: created,
                    name: "controlled-observed.exe".into(),
                    session_id: 17,
                    path: Some("C:\\SECRET_IMAGE_PATH\\controlled-observed.exe".into()),
                    description: Some("SECRET_DESCRIPTION".into()),
                    company: Some("SECRET_COMPANY".into()),
                    version: Some("SECRET_VERSION".into()),
                    facts: argus_core::process::Facts {
                        command_line: Some(
                            "C:\\SECRET_IMAGE_PATH\\controlled-observed.exe --token=SECRET_COMMAND"
                                .into(),
                        ),
                        architecture: Some("x64"),
                        elevated: Some(false),
                    },
                    cpu: 3.,
                    private_ws: 1 << 20,
                    ..Default::default()
                }],
            });
        }
        drop(recorder);
        let mut file = std::fs::File::create(dir.join("process-watch.jsonl")).unwrap();
        let mut write = |value: serde_json::Value| {
            serde_json::to_writer(&mut file, &value).unwrap();
            file.write_all(b"\n").unwrap();
        };
        write(
            serde_json::json!({"kind":"header","version":2,"pid":17,"created":created,"name":"controlled-observed.exe"}),
        );
        for (offset, memory, commit, state) in [
            (0, Some(0), Some(0), "recording"),
            (5000, None, Some(10), "recording"),
            (10000, None, None, "gap"),
            (15000, Some(20), Some(30), "recording"),
            (20000, None, None, "exited"),
        ] {
            write(
                serde_json::json!({"kind":"point","ts":start+offset,"memory":memory,"commit":commit,"state":state}),
            );
        }
        drop(file);
        let report = history::interval(&dir, start, start + 25000, IntervalMetric::Cpu).unwrap();
        assert_eq!(report.recorded_frames, 5);
        assert_eq!(report.leaders.len(), 1);
        assert_eq!(report.observations.runs.len(), 1);
        assert_eq!(report.observations.runs[0].memory.mean, Some(10.));
        assert_eq!(report.observations.runs[0].commit.mean, Some(40. / 3.));
        assert_eq!(
            (
                report.observations.runs[0].gap_events,
                report.observations.runs[0].terminal_events
            ),
            (1, 1)
        );
        let store = PreviewStore::default();
        let markdown = store.prepare(&report, Format::Markdown).unwrap();
        let csv = store.prepare(&report, Format::Csv).unwrap();
        assert!(!csv.body.contains("controlled-observed.exe"));
        assert!(!csv.body.contains("SECRET_"));
        assert!(!markdown.body.contains("SECRET_"));
        if let Some(output) = std::env::var_os("ARGUS_TEST_REPORT_REAL_OUTPUT") {
            std::fs::write(output,serde_json::to_vec_pretty(&serde_json::json!({
                "report":report,"md":markdown,"csv":csv,
                "_series":history::series(&dir,start,start+24999,5),
                "_moments":(0..5).map(|i|history::moment(&dir,start+i*5000).unwrap()).collect::<Vec<_>>()
            })).unwrap()).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn both_report_formats_omit_injected_source_names_identities_paths_accounts_hosts_and_commands()
    {
        let dir = folder();
        let report = report(&dir);
        let store = PreviewStore::default();
        let mut exported = serde_json::Map::new();
        for format in [Format::Markdown, Format::Csv] {
            let preview = store.prepare(&report, format).unwrap();
            let encoded = serde_json::to_string(&preview).unwrap();
            for token in [
                "SECRET_",
                "secret.txt",
                "host-secret",
                "password=",
                "HYPERLINK",
                "<img",
                "1309149133",
                "1600000000123",
            ] {
                assert!(!encoded.contains(token), "source leaked into preview");
            }
            assert!(preview.body.len() <= super::MAX_BYTES);
            assert_eq!(
                preview.sha256,
                Sha256::digest(preview.body.as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
            let alias =
                Aliases::default().get(report.leaders[0].pid, report.leaders[0].create_time);
            // Five ranked rows and two observed-memory rows refer to this exact same instance.
            assert_eq!(preview.body.matches(&alias).count(), 7);
            exported.insert(
                format.extension().into(),
                serde_json::to_value(&preview).unwrap(),
            );
        }
        if let Some(path) = std::env::var_os("ARGUS_TEST_REPORT_OUTPUT") {
            std::fs::write(path, serde_json::to_vec_pretty(&exported).unwrap()).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saving_uses_immutable_preview_bytes_and_stale_ids_cannot_select_another_body() {
        let dir = folder();
        let mut report = report(&dir);
        let store = PreviewStore::default();
        let first = store.prepare(&report, Format::Markdown).unwrap();
        let snapshot = store.get(first.id, &first.sha256).unwrap();
        report.cpu.mean = Some(99.);
        let next = store.prepare(&report, Format::Csv).unwrap();
        assert!(store.get(first.id, &first.sha256).is_err());
        assert!(store.get(next.id, &first.sha256).is_err());
        assert!(store.get(0, &next.sha256).is_err());
        let path = dir.join("chosen.md");
        std::fs::write(&path, b"previous user-selected report").unwrap();
        save(&snapshot, &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), first.body.as_bytes());
        save(&store.get(next.id, &next.sha256).unwrap(), &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), next.body.as_bytes());
        let existing_dir = dir.join("existing-dir");
        std::fs::create_dir(&existing_dir).unwrap();
        assert!(save(&next, &existing_dir).is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn delimiters_controls_formulas_and_oversize_inputs_are_bounded() {
        for text in ["=1+1", " +cmd", "\t@formula", "-cmd"] {
            assert!(csv(text).starts_with("\"'"));
        }
        assert_eq!(csv("a,\"b\"\r\nc\u{202e}"), "\"a,\"\"b\"\"  c \"");
        assert_eq!(markdown("[x](a)|<b>\n"), "\\[x\\](a)\\|&lt;b&gt; ");
        let dir = folder();
        let mut report = report(&dir);
        report.leaders = vec![report.leaders[0].clone(); 210];
        assert!(render(&report, Format::Csv).is_err());
        assert!(render(&report, Format::Markdown).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
