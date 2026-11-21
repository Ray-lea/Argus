//! Handle counts, surviving children and idle orphan groups from live process rows.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use crate::ProcessRow;
use series::Series;

mod series;

const RECORD_INTERVAL_MS: i64 = 30_000;
const RETAIN_MS: i64 = 60 * 60_000;
const MIN_SPAN_MS: i64 = 10 * 60_000;
const MAX_MEMBERS: usize = 64;
/// Live children a parent needs for a children finding.
const MIN_CHILDREN: usize = 15;
/// Leftover instances of one program for an orphans finding.
const MIN_ORPHANS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LeakKind {
    Handles,
    Children,
    Orphans,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LeakEvidence {
    /// The count alone is far beyond what processes normally reach.
    Extreme,
    /// The count's floor kept rising through the observation.
    Growing,
    /// Leftover instances: parent gone, no processor time used beyond the idle allowance.
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeakMember {
    pub pid: u32,
    pub create_time: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Leak {
    pub kind: LeakKind,
    pub evidence: LeakEvidence,
    /// The process, accumulating parent, or oldest leftover instance.
    pub pid: u32,
    /// Unix ms, as in ProcessRow.
    pub create_time: i64,
    pub name: Arc<str>,
    pub path: Option<Arc<str>>,
    /// Current handles, live children, or idle leftovers.
    pub count: u64,
    /// Rise between the first and last segment floors, zero without measured growth.
    pub growth: u64,
    /// Observation span in milliseconds.
    pub span_ms: i64,
    /// Unix ms when this finding first appeared during this run.
    pub since: i64,
    /// Up to 48 counts at equally spaced times, oldest first. Empty for orphans.
    pub trend: Vec<u64>,
    /// Up to 64 surviving children created during the observation or idle leftovers, oldest first.
    pub members: Vec<LeakMember>,
}

impl Leak {
    fn new(row: &ProcessRow, kind: LeakKind, evidence: LeakEvidence, count: u64, now: i64) -> Self {
        Self {
            kind,
            evidence,
            pid: row.pid,
            create_time: row.create_time,
            name: row.name.clone(),
            path: row.path.clone(),
            count,
            growth: 0,
            span_ms: 0,
            since: now,
            trend: Vec::new(),
            members: Vec::new(),
        }
    }

    fn with_series(mut self, series: &Series, rise: u32) -> Self {
        self.growth = u64::from(rise);
        self.span_ms = series.span();
        self.trend = series.trend();
        self
    }

    fn identity(&self) -> FindingKey {
        match self.kind {
            LeakKind::Orphans => FindingKey::Path(self.path.as_ref().unwrap().to_lowercase()),
            kind => FindingKey::Process(kind, self.pid, self.create_time),
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
enum FindingKey {
    Process(LeakKind, u32, i64),
    Path(String),
}

#[derive(Default)]
struct Instance {
    created: i64,
    first_cpu: Option<(i64, u64)>,
    handles: Series,
    children: Series,
}

impl Instance {
    /// How long the instance has been idle: processor time beyond the
    /// allowance starts the observation again from now.
    fn idle_span(&mut self, now: i64, cpu_time: u64) -> Option<i64> {
        let (at, cpu) = *self.first_cpu.get_or_insert((now, cpu_time));
        if cpu_time.checked_sub(cpu).is_none_or(|used| used > 1_000) {
            self.first_cpu = Some((now, cpu_time));
            return None;
        }
        let span = now.saturating_sub(at);
        (span >= MIN_SPAN_MS).then_some(span)
    }
}

#[derive(Default)]
pub struct LeakTracker {
    last_recorded: Option<i64>,
    instances: HashMap<u32, Instance>,
    findings: Vec<Leak>,
}

impl LeakTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records at most once per 30 seconds of snapshot time. A clock rollback starts a new observation.
    pub fn observe(&mut self, now_ms: i64, processes: &[ProcessRow]) {
        if let Some(last) = self.last_recorded {
            if now_ms < last {
                self.instances.clear();
                self.findings.clear();
            } else if now_ms.abs_diff(last) < RECORD_INTERVAL_MS as u64 {
                self.follow(processes);
                return;
            }
        }
        self.last_recorded = Some(now_ms);
        let live: HashMap<_, _> = processes
            .iter()
            .filter(|p| !p.stale)
            .map(|p| (p.pid, p))
            .collect();
        self.instances.retain(|pid, state| {
            live.get(pid)
                .is_some_and(|p| p.create_time == state.created)
        });
        let previous_since: HashMap<_, _> = self
            .findings
            .drain(..)
            .map(|f| (f.identity(), f.since))
            .collect();
        let mut children: HashMap<u32, Vec<&ProcessRow>> = HashMap::new();
        let mut orphans: HashMap<String, Vec<(&ProcessRow, i64)>> = HashMap::new();
        for &row in live.values() {
            let parent_alive = live
                .get(&row.ppid)
                .is_some_and(|parent| parent.create_time <= row.create_time);
            if parent_alive && row.ppid != 0 && row.pid != row.ppid {
                children.entry(row.ppid).or_default().push(row);
            }
            // A roster entry may precede the first known creation time and counters.
            if row.counters_pending && row.create_time < 0 {
                continue;
            }
            let state = self.instances.entry(row.pid).or_insert_with(|| Instance {
                created: row.create_time,
                ..Default::default()
            });
            if row.counters_pending {
                continue;
            }
            if let Some(finding) = observe_handles(&mut state.handles, now_ms, row) {
                self.findings.push(finding);
            }
            if let Some(span) = state.idle_span(now_ms, row.cpu_time)
                && !parent_alive
                && !row.app_window
                && !row.windows_process
                && let Some(path) = &row.path
            {
                orphans
                    .entry(path.to_lowercase())
                    .or_default()
                    .push((row, span));
            }
        }
        // Continue established child series even when the last child has exited.
        for (pid, state) in &mut self.instances {
            let family = children.get(pid).map(Vec::as_slice).unwrap_or_default();
            if family.len() < 3 && state.children.is_empty() {
                continue;
            }
            if let Some(finding) = observe_children(&mut state.children, now_ms, live[pid], family)
            {
                self.findings.push(finding);
            }
        }
        for group in orphans.into_values().filter(|g| g.len() >= MIN_ORPHANS) {
            let count = group.len() as u64;
            let span = group.iter().map(|(_, span)| *span).min().unwrap();
            let members = oldest_members(group.into_iter().map(|(row, _)| row));
            let row = live[&members[0].pid];
            let mut finding = Leak::new(row, LeakKind::Orphans, LeakEvidence::Idle, count, now_ms);
            finding.span_ms = span;
            finding.members = members;
            self.findings.push(finding);
        }
        for f in &mut self.findings {
            if let Some(&since) = previous_since.get(&f.identity()) {
                f.since = since;
            }
        }
        sort_findings(&mut self.findings);
    }

    pub fn findings(&self) -> &[Leak] {
        &self.findings
    }

    /// Between records, keeps the findings in step with the process list: a
    /// process that ended (by hand, say) takes its finding with it at the
    /// next sample, not at the next record, and the counts follow the list.
    /// Rules are applied again only when the next record is due.
    fn follow(&mut self, processes: &[ProcessRow]) {
        if self.findings.is_empty() {
            return;
        }
        let live: HashMap<(u32, i64), &ProcessRow> = processes
            .iter()
            .map(|p| ((p.pid, p.create_time), p))
            .collect();
        let alive = |pid: u32, created: i64| live.contains_key(&(pid, created));
        self.findings.retain_mut(|f| match f.kind {
            LeakKind::Handles => match live.get(&(f.pid, f.create_time)) {
                Some(row) => {
                    if !row.stale && !row.counters_pending {
                        f.count = u64::from(row.handles);
                    }
                    true
                }
                None => false,
            },
            LeakKind::Children => {
                if !alive(f.pid, f.create_time) {
                    return false;
                }
                let count = processes
                    .iter()
                    .filter(|p| p.ppid == f.pid && p.pid != f.pid && p.create_time >= f.create_time)
                    .count();
                f.members.retain(|m| alive(m.pid, m.create_time));
                f.count = count as u64;
                count >= MIN_CHILDREN
            }
            LeakKind::Orphans => {
                let listed = f.members.len();
                f.members.retain(|m| alive(m.pid, m.create_time));
                let ended = (listed - f.members.len()) as u64;
                f.count = f.count.saturating_sub(ended);
                // The oldest instance left stands for the group.
                if let Some(first) = f.members.first() {
                    f.pid = first.pid;
                    f.create_time = first.create_time;
                }
                f.count >= MIN_ORPHANS as u64 && !f.members.is_empty()
            }
        });
        sort_findings(&mut self.findings);
    }
}

fn observe_handles(series: &mut Series, now: i64, row: &ProcessRow) -> Option<Leak> {
    series.record(now, row.handles);
    // The current count bounds the last floor and hence the largest possible rise.
    if row.handles < 1_000 {
        return None;
    }
    let (evidence, growth) = if row.handles >= 100_000 && row.pid != 4 {
        (LeakEvidence::Extreme, series.growing(|_| 0).unwrap_or(0))
    } else {
        (
            LeakEvidence::Growing,
            series.growing(|floor| 1_000.max(floor / 2))?,
        )
    };
    Some(
        Leak::new(
            row,
            LeakKind::Handles,
            evidence,
            u64::from(row.handles),
            now,
        )
        .with_series(series, growth),
    )
}

fn observe_children(
    series: &mut Series,
    now: i64,
    row: &ProcessRow,
    family: &[&ProcessRow],
) -> Option<Leak> {
    let count = family.len();
    series.record(now, count as u32);
    if count < MIN_CHILDREN {
        return None;
    }
    let growth = series.growing(|_| 10)?;
    let mut finding = Leak::new(
        row,
        LeakKind::Children,
        LeakEvidence::Growing,
        count as u64,
        now,
    )
    .with_series(series, growth);
    let window = series.start()..=now;
    finding.members = oldest_members(
        family
            .iter()
            .copied()
            .filter(|p| window.contains(&p.create_time)),
    );
    Some(finding)
}

fn oldest_members<'a>(rows: impl Iterator<Item = &'a ProcessRow>) -> Vec<LeakMember> {
    let mut members: Vec<_> = rows
        .map(|p| LeakMember {
            pid: p.pid,
            create_time: p.create_time,
        })
        .collect();
    let order = |m: &LeakMember| (m.create_time, m.pid);
    if members.len() > MAX_MEMBERS {
        members.select_nth_unstable_by_key(MAX_MEMBERS, order);
        members.truncate(MAX_MEMBERS);
        // A large group should not leave its entire allocation in the snapshot.
        members.shrink_to_fit();
    }
    members.sort_unstable_by_key(order);
    members
}

/// By kind, then the largest count first.
fn sort_findings(findings: &mut [Leak]) {
    findings.sort_unstable_by_key(|f| (f.kind as u8, Reverse(f.count), f.pid));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    const START: i64 = 1_800_000_000_000;

    fn row(pid: u32, created: i64) -> ProcessRow {
        ProcessRow {
            pid,
            ppid: 99,
            create_time: created,
            name: "Example.exe".into(),
            path: Some(r"C:\Example.exe".into()),
            threads: 1,
            ..Default::default()
        }
    }

    fn time(step: u32) -> i64 {
        START + i64::from(step) * RECORD_INTERVAL_MS
    }

    fn handles(steps: impl Iterator<Item = u32>, count: impl Fn(u32) -> u32) -> LeakTracker {
        let mut tracker = LeakTracker::new();
        let mut p = row(42, START - 1);
        for i in steps {
            p.handles = count(i);
            tracker.observe(time(i), std::slice::from_ref(&p));
        }
        tracker
    }

    #[test]
    fn growing_handles_need_both_absolute_and_relative_floor_rises() {
        let tracker = handles(0..=20, |i| 2_000 + i * 100);
        let [f] = tracker.findings() else { panic!() };
        assert_eq!(
            (f.kind, f.evidence, f.count),
            (LeakKind::Handles, LeakEvidence::Growing, 4_000)
        );
        assert_eq!(
            (f.growth, f.span_ms, f.since),
            (1_600, MIN_SPAN_MS, time(20))
        );
        assert_eq!((f.trend[0], f.trend[20]), (2_000, 4_000));
        assert!(f.members.is_empty());
        assert!(handles(0..=20, |i| i * 60).findings().is_empty());
        assert!(handles(0..=20, |i| 4_000 + i * 100).findings().is_empty());
    }

    #[test]
    fn rejects_flat_floors_steps_short_spans_and_missing_time_segments() {
        assert!(
            handles(0..=20, |i| 100 + (i % 3) * 4_000)
                .findings()
                .is_empty()
        );
        assert!(
            handles(0..=20, |i| if i < 10 { 100 } else { 6_000 })
                .findings()
                .is_empty()
        );
        assert!(handles(0..20, |i| i * 200).findings().is_empty());
        assert!(
            handles((0..=20).filter(|i| !(8..12).contains(i)), |i| i * 200)
                .findings()
                .is_empty()
        );
    }

    #[test]
    fn extreme_is_immediate_and_supersedes_growth() {
        let mut tracker = LeakTracker::new();
        let mut rows = [row(4, 10), row(42, 10), row(43, 10), row(44, 10)];
        rows[0].handles = 2_500_000;
        rows[1].handles = 100_000;
        rows[2].handles = 99_999;
        rows[3].handles = 2_480_000;
        rows[3].stale = true;
        tracker.observe(START, &rows);
        let [f] = tracker.findings() else { panic!() };
        assert_eq!(
            (f.pid, f.evidence, f.since, f.growth, f.span_ms),
            (42, LeakEvidence::Extreme, START, 0, 0)
        );
        assert_eq!(f.trend, [100_000]);
        let tracker = handles(0..=20, |i| 90_000 + i * 3_000);
        let f = &tracker.findings()[0];
        assert_eq!(
            (f.evidence, f.growth, f.since),
            (LeakEvidence::Extreme, 48_000, time(4))
        );
        let tracker = handles(0..=20, |i| 2_500_000 + i * 10);
        let f = &tracker.findings()[0];
        assert_eq!((f.growth, f.span_ms), (160, MIN_SPAN_MS));
    }

    #[test]
    fn skip_rollback_and_disappearance_control_the_lifetime_of_findings() {
        let mut tracker = LeakTracker::new();
        let mut p = row(40, 1);
        p.handles = 100_000;
        let rows = [p];
        tracker.observe(time(10), &rows);
        // Between records the finding stays while its process does.
        tracker.observe(time(11) - 1, &rows);
        assert_eq!(tracker.findings()[0].since, time(10));
        tracker.observe(time(11), &rows);
        assert_eq!(tracker.findings()[0].span_ms, RECORD_INTERVAL_MS);
        tracker.observe(time(11) - 1, &rows);
        let f = &tracker.findings()[0];
        assert_eq!((f.since, f.span_ms, f.trend.len()), (time(11) - 1, 0, 1));
        tracker.observe(time(12), &[]);
        assert!(tracker.findings().is_empty() && tracker.instances.is_empty());
        tracker.observe(time(13), &rows);
        assert_eq!(
            (tracker.findings()[0].since, tracker.findings()[0].span_ms),
            (time(13), 0)
        );
    }

    #[test]
    fn only_the_last_hour_contributes_even_across_long_gaps_and_offset_rebasing() {
        let mut tracker = handles(0..=160, |i| 100_000 + i);
        let f = &tracker.findings()[0];
        assert_eq!(
            (f.span_ms, f.trend.len(), f.trend[0], f.trend[47]),
            (RETAIN_MS, 48, 100_040, 100_160)
        );
        // A suspension longer than a full 32-bit tick period expires all count samples.
        let mut p = row(42, START - 1);
        p.handles = 250_000;
        let later = time(160) + i64::from(u32::MAX) + 1;
        tracker.observe(later, &[p]);
        assert_eq!(
            (tracker.findings()[0].span_ms, tracker.findings()[0].since),
            (0, START)
        );

        let mut series = Series::default();
        let mut at = 0;
        while at < i64::from(u32::MAX) {
            series.record(at, 100);
            at += RETAIN_MS;
        }
        series.record(at, 200);
        assert_eq!(
            (series.start(), series.span(), series.trend()),
            (at - RETAIN_MS, RETAIN_MS, vec![100, 200])
        );
    }

    #[test]
    fn trend_samples_by_elapsed_time() {
        let steps = [0, 1, 2, 3, 4, 40, 80];
        let tracker = handles(steps.into_iter(), |i| 100_000 + i);
        assert_eq!(
            tracker.findings()[0].trend,
            [
                100_000, 100_004, 100_040, 100_040, 100_040, 100_080, 100_080
            ]
        );
    }

    fn family(count: u32) -> Vec<ProcessRow> {
        let mut rows = vec![row(20, START - 100)];
        for i in 0..count {
            let mut child = row(100 + i, if i < 3 { START - 1 } else { time(i - 2) });
            child.ppid = 20;
            rows.push(child);
        }
        rows
    }

    #[test]
    fn growing_children_report_only_surviving_children_created_in_the_window() {
        let mut tracker = LeakTracker::new();
        for i in 0..=80 {
            let mut rows = family(3 + i);
            rows.retain(|p| p.pid != 106 || i < 40);
            rows.reverse();
            tracker.observe(time(i), &rows);
        }
        let [f] = tracker.findings() else { panic!() };
        assert_eq!(
            (f.kind, f.evidence, f.pid, f.count, f.growth),
            (LeakKind::Children, LeakEvidence::Growing, 20, 82, 63)
        );
        assert_eq!(f.members.len(), 64);
        let pids: Vec<_> = f.members.iter().map(|m| m.pid).collect();
        assert_eq!(
            pids,
            (103..=167).filter(|&pid| pid != 106).collect::<Vec<_>>()
        );
        assert_eq!(f.members[0].create_time, time(1));
    }

    #[test]
    fn stable_families_and_reused_parent_pids_do_not_accumulate_growth() {
        let mut tracker = LeakTracker::new();
        let mut stable = family(30);
        for child in &mut stable[1..] {
            child.create_time = START - 1;
        }
        for i in 0..=20 {
            tracker.observe(time(i), &stable);
        }
        assert!(tracker.findings().is_empty());
        tracker.observe(time(21), &family(0));
        assert_eq!(tracker.instances[&20].children.span(), time(21) - START);

        let mut tracker = LeakTracker::new();
        tracker.observe(time(0), &family(2));
        assert!(tracker.instances[&20].children.is_empty());
        for i in 1..=21 {
            tracker.observe(time(i), &family(3 + i));
        }
        assert_eq!(tracker.findings()[0].kind, LeakKind::Children);
        let mut replaced = family(26);
        replaced[0].create_time = time(22);
        tracker.observe(time(22), &replaced);
        assert!(
            tracker
                .findings()
                .iter()
                .all(|f| f.kind != LeakKind::Children)
        );
        assert!(tracker.instances[&20].children.is_empty());
    }

    #[test]
    fn child_counts_exclude_stale_older_and_self_rows_and_skip_pid_zero() {
        let mut tracker = LeakTracker::new();
        let mut rows = family(4);
        rows[0].ppid = 20;
        rows[1].create_time = START - 200;
        rows[2].stale = true;
        rows.push(row(0, 0));
        for i in 0..3 {
            let mut child = row(200 + i, START);
            child.ppid = 0;
            rows.push(child);
        }
        tracker.observe(time(1), &rows);
        assert!(tracker.instances[&0].children.is_empty());
        assert!(tracker.instances[&20].children.is_empty());
        rows[2].stale = false;
        tracker.observe(time(2), &rows);
        assert_eq!(tracker.instances[&20].children.trend(), [3]);
    }

    #[test]
    fn pid_reuse_and_pending_counters_start_new_handle_and_idle_baselines() {
        let mut tracker = handles(0..=20, |i| 100_000 + i);
        let mut p = row(42, time(21));
        p.counters_pending = true;
        tracker.observe(time(21), std::slice::from_ref(&p));
        assert!(tracker.findings().is_empty());
        assert!(tracker.instances[&42].first_cpu.is_none());
        p.counters_pending = false;
        p.handles = 120_000;
        p.cpu_time = 1_500;
        tracker.observe(time(22), &[p]);
        let f = &tracker.findings()[0];
        assert_eq!(
            (f.since, f.span_ms, f.trend.as_slice()),
            (time(22), 0, [120_000].as_slice())
        );
        assert_eq!(tracker.instances[&42].first_cpu, Some((time(22), 1_500)));
    }

    #[test]
    fn idle_groups_use_parent_instance_path_cpu_and_window() {
        let mut tracker = LeakTracker::new();
        let mut group: Vec<_> = (0..10)
            .map(|i| row(50 + i, START - 10 + i64::from(i)))
            .collect();
        group[0].path = Some(r"c:\EXAMPLE.EXE".into());
        group[5].app_window = true;
        group[6].windows_process = true;
        group[7].path = None;
        group[8].ppid = 90;
        group.push(row(90, START - 100));
        group.last_mut().unwrap().app_window = true;
        // PID 99 has been reused since these instances were created.
        let mut replacement = row(99, START);
        replacement.app_window = true;
        group.push(replacement);
        tracker.observe(time(0), &group);
        group[0].cpu_time = 1_000;
        group[9].cpu_time = 1_001;
        tracker.observe(time(19), &group);
        assert!(tracker.findings().is_empty());
        group.reverse();
        tracker.observe(time(20), &group);
        let [f] = tracker.findings() else { panic!() };
        assert_eq!(
            (f.kind, f.evidence, f.pid, f.count),
            (LeakKind::Orphans, LeakEvidence::Idle, 50, 5)
        );
        assert_eq!((f.growth, f.span_ms, f.since), (0, MIN_SPAN_MS, time(20)));
        assert!(f.trend.is_empty());
        assert_eq!(
            f.members.iter().map(|m| m.pid).collect::<Vec<_>>(),
            [50, 51, 52, 53, 54]
        );
        group.retain(|p| p.pid != 50);
        tracker.observe(time(21), &group);
        assert!(tracker.findings().is_empty());
    }

    #[test]
    fn ended_processes_leave_their_findings_before_the_next_record() {
        let mut tracker = LeakTracker::new();
        let mut agent = row(42, 10);
        agent.handles = 200_000;
        let other = row(43, 10);
        tracker.observe(START, &[agent.clone(), other.clone()]);
        assert_eq!(tracker.findings().len(), 1);
        // Between records the count follows the list...
        agent.handles = 210_000;
        tracker.observe(START + 1_000, &[agent.clone(), other.clone()]);
        assert_eq!(tracker.findings()[0].count, 210_000);
        // ...and an ended process takes its finding with it at once.
        tracker.observe(START + 2_000, std::slice::from_ref(&other));
        assert!(tracker.findings().is_empty());

        // Leftovers: one ended leaves five, led by the next oldest; one more
        // leaves too few for a finding.
        let mut tracker = LeakTracker::new();
        let mut group: Vec<_> = (0..6)
            .map(|i| row(60 + i, START - 100 + i64::from(i)))
            .collect();
        tracker.observe(time(0), &group);
        tracker.observe(time(20), &group);
        assert_eq!(tracker.findings()[0].count, 6);
        group.remove(0);
        tracker.observe(time(20) + 1_000, &group);
        let [f] = tracker.findings() else { panic!() };
        assert_eq!((f.count, f.pid, f.members.len()), (5, 61, 5));
        group.remove(0);
        tracker.observe(time(20) + 2_000, &group);
        assert!(tracker.findings().is_empty());

        // Children: ending some of them updates the count, below the
        // threshold the finding goes.
        let mut tracker = LeakTracker::new();
        let parent = row(70, 1);
        let mut rows = vec![parent];
        for i in 0..16 {
            let mut child = row(100 + i, 2 + i64::from(i));
            child.ppid = 70;
            rows.push(child);
        }
        tracker.last_recorded = Some(START);
        tracker.findings = vec![Leak {
            members: rows[1..]
                .iter()
                .map(|p| LeakMember {
                    pid: p.pid,
                    create_time: p.create_time,
                })
                .collect(),
            ..Leak::new(
                &rows[0],
                LeakKind::Children,
                LeakEvidence::Growing,
                16,
                START,
            )
        }];
        rows.truncate(16);
        tracker.observe(START + 1_000, &rows);
        let [f] = tracker.findings() else { panic!() };
        assert_eq!((f.count, f.members.len()), (15, 15));
        rows.truncate(15);
        tracker.observe(START + 2_000, &rows);
        assert!(tracker.findings().is_empty());
    }

    #[test]
    fn leftovers_are_idle_from_their_last_activity() {
        let mut tracker = LeakTracker::new();
        let mut group: Vec<_> = (0..5).map(|i| row(60 + i, START - 10)).collect();
        tracker.observe(time(0), &group);
        // Busy at first: the idle observation starts again, not never.
        for p in &mut group {
            p.cpu_time = 5_000;
        }
        tracker.observe(time(1), &group);
        tracker.observe(time(20), &group);
        assert!(tracker.findings().is_empty());
        tracker.observe(time(21), &group);
        let [f] = tracker.findings() else { panic!() };
        assert_eq!(
            (f.kind, f.count, f.span_ms),
            (LeakKind::Orphans, 5, MIN_SPAN_MS)
        );
    }

    #[test]
    fn orphan_since_survives_a_change_of_representative_and_resets_below_five() {
        let mut tracker = LeakTracker::new();
        let mut group: Vec<_> = (0..70)
            .map(|i| row(100 + i, START - 100 + i64::from(i)))
            .collect();
        tracker.observe(time(0), &group);
        group.push(row(170, START));
        tracker.observe(time(1), &group);
        tracker.observe(time(20), &group);
        let f = &tracker.findings()[0];
        assert_eq!((f.count, f.members.len(), f.members[63].pid), (70, 64, 163));
        group.remove(0);
        tracker.observe(time(21), &group);
        let f = &tracker.findings()[0];
        assert_eq!(
            (f.pid, f.count, f.since, f.span_ms),
            (101, 70, time(20), MIN_SPAN_MS)
        );
        for p in &mut group[4..] {
            p.app_window = true;
        }
        tracker.observe(time(22), &group);
        assert!(tracker.findings().is_empty());
        group[4].app_window = false;
        tracker.observe(time(23), &group);
        assert_eq!(tracker.findings()[0].since, time(23));
    }

    #[test]
    fn serialization_matches_the_snapshot_contract() {
        let tracker = handles(0..1, |_| 123_456);
        let mut snapshot = crate::Snapshot {
            seq: 1,
            timestamp: START,
            system: Default::default(),
            hardware: Default::default(),
            processes: Vec::new(),
            leaks: Vec::new(),
            sessions: Vec::new(),
            process_network: false,
        };
        assert!(to_value(&snapshot).unwrap().get("leaks").is_none());
        snapshot.leaks = tracker.findings().to_vec();
        assert_eq!(
            to_value(&snapshot).unwrap()["leaks"],
            json!([{
                "kind":"handles", "evidence":"extreme", "pid":42, "createTime": START - 1,
                "name":"Example.exe", "path":r"C:\Example.exe", "count":123456, "growth":0,
                "spanMs":0, "since": START, "trend":[123456], "members":[]
            }])
        );
        assert_eq!(
            to_value([LeakKind::Children, LeakKind::Orphans]).unwrap(),
            json!(["children", "orphans"])
        );
        assert_eq!(
            to_value([LeakEvidence::Growing, LeakEvidence::Idle]).unwrap(),
            json!(["growing", "idle"])
        );
        assert_eq!(
            to_value(LeakMember {
                pid: 5,
                create_time: START
            })
            .unwrap(),
            json!({"pid":5, "createTime":START})
        );
    }

    #[test]
    fn findings_order_is_the_same_for_small_and_large_sets() {
        for n in [12, 65, 300] {
            let mut findings: Vec<_> = (0..n)
                .map(|i| {
                    let kind = [LeakKind::Orphans, LeakKind::Handles, LeakKind::Children][i % 3];
                    Leak::new(
                        &row((n - i) as u32, START),
                        kind,
                        LeakEvidence::Growing,
                        (i % 5) as u64 * 2_000_003,
                        START,
                    )
                })
                .collect();
            sort_findings(&mut findings);
            assert!(findings.is_sorted_by_key(|f| (f.kind as u8, Reverse(f.count), f.pid)));
        }
    }

    #[test]
    #[ignore = "compares findings with handle counts in a live collector snapshot"]
    fn live_snapshot_reports_extreme_handle_counts() {
        let mut collector = crate::Collector::new(crate::MetaStore::new());
        std::thread::sleep(Duration::from_secs(1));
        let snapshot = collector.sample();
        let mut expected = Vec::new();
        let mut measured = 0;
        for p in &snapshot.processes {
            if !p.stale && !p.counters_pending {
                measured += 1;
                if p.pid != 4 && p.handles >= 100_000 {
                    expected.push((Reverse(u64::from(p.handles)), p.pid, p.create_time));
                }
            }
        }
        assert!(measured > 0);
        expected.sort_unstable();
        let actual: Vec<_> = snapshot
            .leaks
            .iter()
            .map(|f| {
                assert_eq!(
                    (f.kind, f.evidence, f.since),
                    (LeakKind::Handles, LeakEvidence::Extreme, snapshot.timestamp)
                );
                (Reverse(f.count), f.pid, f.create_time)
            })
            .collect();
        assert_eq!(actual, expected);
        println!(
            "measured processes={measured}, extreme counts={:?}",
            snapshot.leaks.iter().map(|f| f.count).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore = "measures steady-state observe calls for 650 synthetic process rows"]
    fn observe_cost() {
        let mut rows: Vec<_> = (0..650)
            .map(|i| {
                let mut p = row(100 + i, START - 1);
                if i >= 50 {
                    p.ppid = 100 + i % 30;
                }
                p
            })
            .collect();
        let mut tracker = LeakTracker::new();
        let mut timings = Vec::new();
        for i in 0..1_121 {
            for (j, p) in rows.iter_mut().enumerate() {
                p.handles = match j {
                    0 | 1 => 2_500_000 + i * 30,
                    2..=9 => 1_000 + i * 100,
                    _ => (j as u32 * 71) % 3_000 + i % 17,
                };
            }
            let start = Instant::now();
            tracker.observe(time(i), black_box(&rows));
            let elapsed = start.elapsed();
            if i >= 121 {
                timings.push(elapsed);
            }
        }
        timings.sort_unstable();
        let buckets = 1_000_000;
        let start = Instant::now();
        for _ in 0..buckets {
            black_box(&mut tracker).observe(black_box(time(1_120) + 1), black_box(&rows));
        }
        let skipped_ns = start.elapsed().as_nanos() as f64 / f64::from(buckets);
        let points: usize = tracker
            .instances
            .values()
            .map(|s| s.handles.point_bytes() + s.children.point_bytes())
            .sum();
        println!(
            "record n={} median={:?} p95={:?} mean={:?}, skipped mean={skipped_ns:.2} ns, points={points} bytes, findings={}",
            timings.len(),
            timings[500],
            timings[950],
            timings.iter().sum::<Duration>() / 1_000,
            tracker.findings().len()
        );
    }
}
