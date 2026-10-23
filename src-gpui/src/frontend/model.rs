//! Renderer-independent view state and deterministic process identity/grouping.
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

pub fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.)
}
pub fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
pub fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// Preserve slower background sampling while separating time spans with no recordings.
pub fn history_points(records: &[Value], metric: &str, bucket_ms: f64) -> Vec<(f64, Option<f64>)> {
    let max_cadence = argus_app::settings::BACKGROUND_INTERVALS
        .iter()
        .copied()
        .max()
        .unwrap() as f64
        + argus_core::history::FRAME_MS as f64;
    let mut points = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        let at = number(record, "ts");
        if index > 0 {
            let previous = number(&records[index - 1], "ts");
            let before = if index > 1 {
                previous - number(&records[index - 2], "ts")
            } else {
                0.
            };
            let after = records
                .get(index + 1)
                .map(|next| number(next, "ts") - at)
                .unwrap_or(0.);
            let cadence = before.max(after).min(max_cadence);
            if at - previous > bucket_ms.max(cadence) * 2.5 {
                points.push(((at + previous) / 2., None));
            }
        }
        points.push((at, record[metric].as_f64()));
    }
    points
}
pub fn values(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}
pub fn identity(p: &Value) -> String {
    format!("{}:{}", p["pid"], p["createTime"])
}
pub fn shared_host(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "msedgewebview2.exe"
            | "svchost.exe"
            | "dllhost.exe"
            | "rundll32.exe"
            | "conhost.exe"
            | "runtimebroker.exe"
            | "applicationframehost.exe"
            | "taskhostw.exe"
            | "taskhost.exe"
            | "wudfhost.exe"
            | "node.exe"
            | "python.exe"
            | "pythonw.exe"
            | "java.exe"
            | "javaw.exe"
            | "dotnet.exe"
            | "wscript.exe"
            | "cscript.exe"
            | "cmd.exe"
            | "powershell.exe"
            | "pwsh.exe"
    )
}
pub fn application_key(p: &Value) -> String {
    if !p["webView"].is_null() {
        return format!(
            "g:webview:{}:{}",
            p["webView"]["browserPid"], p["webView"]["browserCreateTime"]
        );
    }
    let path = text(p, "path");
    if path.is_empty() || shared_host(text(p, "name")) {
        return format!("g:instance:{}", identity(p));
    }
    let path = path.replace('/', "\\").to_lowercase();
    let path = path
        .strip_prefix(r"\\?\unc\")
        .map(|s| format!(r"\\{s}"))
        .unwrap_or_else(|| path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned());
    format!("g:image:{path}")
}
pub fn title(p: &Value) -> String {
    if !p["webView"].is_null() {
        let web = &p["webView"];
        let host = web["hostDescription"]
            .as_str()
            .or_else(|| web["hostName"].as_str());
        if let Some(host) = host {
            return argus_core::tr!("processes.webView.host", name = host);
        }
    }
    let description = text(p, "description");
    if description.is_empty() {
        text(p, "name").to_owned()
    } else {
        description.to_owned()
    }
}
#[derive(Clone)]
pub struct Row {
    pub key: String,
    pub raw: Value,
    pub title: String,
    pub detail: String,
    pub members: Vec<Value>,
    pub depth: usize,
    pub expanded: bool,
    pub expandable: bool,
    pub section: Option<String>,
    pub mark_key: Option<String>,
    visible_members: Option<Vec<Value>>,
}
impl Row {
    pub fn process(raw: Value, depth: usize) -> Self {
        let title = title(&raw);
        let detail = if title != text(&raw, "name") {
            text(&raw, "name").into()
        } else {
            String::new()
        };
        Self {
            key: identity(&raw),
            title,
            detail,
            members: vec![raw.clone()],
            raw,
            depth,
            expanded: false,
            expandable: false,
            section: None,
            mark_key: None,
            visible_members: None,
        }
    }
    pub fn metric(&self, key: &str) -> Option<f64> {
        let key = match key {
            "mem" => "privateWs",
            other => other,
        };
        if key == "io" {
            return Some(
                ["ioRead", "ioWrite", "ioOther"]
                    .iter()
                    .map(|k| self.members.iter().map(|p| number(p, k)).sum::<f64>())
                    .sum(),
            );
        }
        if key == "net" {
            return Some(
                self.members
                    .iter()
                    .map(|p| number(p, "netReceived") + number(p, "netSent"))
                    .sum(),
            );
        }
        let readings: Vec<_> = self
            .members
            .iter()
            .filter_map(|p| p[key].as_f64())
            .collect();
        if readings.is_empty() {
            None
        } else {
            Some(readings.iter().sum())
        }
    }
}
pub struct ProcessOptions<'a> {
    pub mode: &'a str,
    pub query: &'a str,
    pub sort: &'a str,
    pub descending: bool,
    pub expanded: &'a HashSet<String>,
    pub collapsed: &'a HashSet<String>,
    pub sections: &'a HashSet<String>,
}
pub fn process_rows(processes: &[Value], options: ProcessOptions<'_>) -> Vec<Row> {
    let query = options.query.to_lowercase();
    let matches = |p: &Value| {
        query.is_empty()
            || [
                text(p, "name"),
                text(p, "description"),
                text(p, "path"),
                text(p, "user"),
            ]
            .iter()
            .any(|s| s.to_lowercase().contains(&query))
            || p["pid"].to_string().contains(&query)
    };
    let compare = |a: &Row, b: &Row| {
        let order = match options.sort {
            "name" => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            "pid" => number(&a.raw, "pid").total_cmp(&number(&b.raw, "pid")),
            key => match (a.metric(key), b.metric(key)) {
                (Some(a), Some(b)) => {
                    let scale = match key {
                        "cpu" | "gpu" => 10.,
                        "mem" => 1. / 104858.,
                        "io" | "diskRead" | "diskWrite" | "net" => 1. / 1024.,
                        _ => 1.,
                    };
                    (a * scale).round().total_cmp(&(b * scale).round())
                }
                (None, None) => std::cmp::Ordering::Equal,
                (None, _) => return std::cmp::Ordering::Greater,
                (_, None) => return std::cmp::Ordering::Less,
            },
        };
        let order = if options.descending {
            order.reverse()
        } else {
            order
        };
        order
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            .then_with(|| a.key.cmp(&b.key))
    };
    let mut top = Vec::new();
    if options.mode == "apps" {
        let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for p in processes {
            groups
                .entry(application_key(p))
                .or_default()
                .push(p.clone());
        }
        for (key, mut members) in groups {
            if !members.iter().any(matches) {
                continue;
            }
            members.sort_by(|a, b| number(a, "createTime").total_cmp(&number(b, "createTime")));
            let mut row = Row::process(members[0].clone(), 0);
            let shown = if !query.is_empty()
                && !row.title.to_lowercase().contains(&query)
                && !text(&row.raw, "name").to_lowercase().contains(&query)
            {
                Some(
                    members
                        .iter()
                        .filter(|p| matches(p))
                        .cloned()
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            };
            if members.len() > 1 {
                row.key = key;
                row.members = members;
                row.expandable = true;
                row.expanded = options.expanded.contains(&row.key)
                    || shown
                        .as_ref()
                        .is_some_and(|items| items.len() < row.members.len());
                row.visible_members = shown;
            }
            top.push(row);
        }
    } else {
        let mut tree_visible = HashSet::new();
        if options.mode == "tree" && !query.is_empty() {
            let by_pid: HashMap<_, _> = processes
                .iter()
                .filter_map(|p| p["pid"].as_u64().map(|pid| (pid, p)))
                .collect();
            for process in processes.iter().filter(|p| matches(p)) {
                let mut current = process;
                loop {
                    if !tree_visible.insert(identity(current)) {
                        break;
                    }
                    let Some(parent) = current["ppid"].as_u64().and_then(|pid| by_pid.get(&pid))
                    else {
                        break;
                    };
                    if number(parent, "createTime") > number(current, "createTime") {
                        break;
                    }
                    current = parent;
                }
            }
        }
        top = processes
            .iter()
            .filter(|p| {
                if options.mode == "tree" && !query.is_empty() {
                    tree_visible.contains(&identity(p))
                } else {
                    matches(p)
                }
            })
            .cloned()
            .map(|p| Row::process(p, 0))
            .collect();
    }
    top.sort_by(compare);
    if options.mode == "tree" {
        let ids: HashSet<u64> = processes.iter().filter_map(|p| p["pid"].as_u64()).collect();
        let mut children: HashMap<u64, Vec<Row>> = HashMap::new();
        let mut roots = Vec::new();
        for row in top {
            let parent = row.raw["ppid"].as_u64().unwrap_or(0);
            let pid = row.raw["pid"].as_u64().unwrap_or(0);
            let parent_is_newer = processes
                .iter()
                .find(|p| p["pid"].as_u64() == Some(parent))
                .is_some_and(|p| number(p, "createTime") > number(&row.raw, "createTime"));
            if parent == pid || !ids.contains(&parent) || parent_is_newer {
                roots.push(row);
            } else {
                children.entry(parent).or_default().push(row);
            }
        }
        fn visit(
            mut row: Row,
            depth: usize,
            visible: bool,
            children: &mut HashMap<u64, Vec<Row>>,
            out: &mut Vec<Row>,
            collapsed: &HashSet<String>,
            seen: &mut HashSet<String>,
        ) {
            if !seen.insert(row.key.clone()) {
                return;
            }
            row.depth = depth;
            let descendants = children
                .remove(&row.raw["pid"].as_u64().unwrap_or(0))
                .unwrap_or_default();
            row.expandable = !descendants.is_empty();
            row.expanded = !collapsed.contains(&row.key);
            let expanded = row.expanded;
            if visible {
                out.push(row);
            }
            for child in descendants {
                visit(
                    child,
                    depth + 1,
                    visible && expanded,
                    children,
                    out,
                    collapsed,
                    seen,
                );
            }
        }
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let expanded_for_search = HashSet::new();
        for row in roots {
            visit(
                row,
                0,
                true,
                &mut children,
                &mut out,
                if query.is_empty() {
                    options.collapsed
                } else {
                    &expanded_for_search
                },
                &mut seen,
            );
        }
        for (_, rows) in children {
            for row in rows {
                if seen.insert(row.key.clone()) {
                    out.push(row);
                }
            }
        }
        return out;
    }
    let mut out = Vec::new();
    if options.sort == "name" {
        for section in ["apps", "background", "windows"] {
            let grouped: Vec<_> = top
                .iter()
                .filter(|r| {
                    let app = r.members.iter().any(|p| boolean(p, "appWindow"));
                    let system = r.members.iter().all(|p| boolean(p, "windowsProcess"));
                    match section {
                        "apps" => app,
                        "windows" => !app && system,
                        _ => !app && !system,
                    }
                })
                .cloned()
                .collect();
            if grouped.is_empty() {
                continue;
            }
            let mut heading = Row::process(json!({}), 0);
            heading.key = format!("s:{section}");
            heading.section = Some(section.into());
            heading.title = argus_core::i18n::text(&format!("processes.section.{section}"));
            heading.members = grouped.iter().flat_map(|r| r.members.clone()).collect();
            heading.expandable = true;
            heading.expanded = !options.sections.contains(section) || !query.is_empty();
            let expanded = heading.expanded;
            out.push(heading);
            if expanded {
                append_groups(grouped, &compare, &mut out);
            }
        }
    } else {
        append_groups(top, &compare, &mut out);
    }
    out
}
fn append_groups(
    rows: Vec<Row>,
    compare: &impl Fn(&Row, &Row) -> std::cmp::Ordering,
    out: &mut Vec<Row>,
) {
    for row in rows {
        let children = if row.expanded && row.expandable {
            let mut children: Vec<_> = row
                .visible_members
                .as_ref()
                .unwrap_or(&row.members)
                .iter()
                .cloned()
                .map(|p| Row::process(p, 1))
                .collect();
            children.sort_by(compare);
            children
        } else {
            Vec::new()
        };
        out.push(row);
        out.extend(children);
    }
}

pub struct Timeline {
    pub samples: HashMap<String, VecDeque<(f64, Option<f64>)>>,
}

/// Device histories survive collection reordering and never follow a reused array slot.
pub fn series_key(snapshot: &Value, path: &str) -> String {
    let mut parts = path.split('.');
    if parts.next() != Some("hardware") {
        return path.into();
    }
    let Some(collection) = parts.next() else {
        return path.into();
    };
    if !matches!(collection, "disks" | "nics" | "gpus" | "batteries") {
        return path.into();
    }
    let Some(index) = parts.next().and_then(|p| p.parse::<usize>().ok()) else {
        return path.into();
    };
    let item = &snapshot["hardware"][collection][index];
    let identity = if collection == "disks" {
        item["info"]["index"].to_string()
    } else {
        text(&item["info"], "id").into()
    };
    let remainder = parts.collect::<Vec<_>>().join(".");
    format!("hardware.{collection}.{identity}.{remainder}")
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            samples: HashMap::new(),
        }
    }
    pub fn push(&mut self, snapshot: &Value, interval: f64) {
        let now = number(snapshot, "timestamp");
        if !now.is_finite() {
            return;
        }
        let last = self.samples.values().find_map(|v| v.back().map(|p| p.0));
        if last == Some(now) {
            return;
        }
        if last.is_some_and(|previous| now < previous) {
            self.samples.clear();
        }
        let mut seen = HashSet::new();
        fn flatten(prefix: &str, value: &Value, out: &mut Vec<(String, Option<f64>)>) {
            match value {
                Value::Number(n) => out.push((prefix.into(), n.as_f64())),
                Value::Null => out.push((prefix.into(), None)),
                Value::Array(a) => {
                    for (i, v) in a.iter().enumerate() {
                        flatten(&format!("{prefix}.{i}"), v, out)
                    }
                }
                Value::Object(o) => {
                    for (k, v) in o {
                        flatten(&format!("{prefix}.{k}"), v, out)
                    }
                }
                _ => {}
            }
        }
        let mut metrics = Vec::new();
        flatten("system", &snapshot["system"], &mut metrics);
        flatten("hardware", &snapshot["hardware"], &mut metrics);
        for (key, _) in &mut metrics {
            *key = series_key(snapshot, key);
        }
        let hardware = &snapshot["hardware"];
        metrics.extend([
            (
                "totals.disk".into(),
                Some(
                    values(&hardware["disks"])
                        .iter()
                        .map(|d| number(d, "read") + number(d, "write"))
                        .sum(),
                ),
            ),
            (
                "totals.network".into(),
                Some(
                    values(&hardware["nics"])
                        .iter()
                        .map(|n| number(n, "rx") + number(n, "tx"))
                        .sum(),
                ),
            ),
            (
                "totals.gpu".into(),
                values(&hardware["gpus"])
                    .iter()
                    .filter_map(|g| g["util"].as_f64())
                    .reduce(f64::max),
            ),
        ]);
        let mut groups: HashMap<String, (f64, f64)> = HashMap::new();
        for p in values(&snapshot["processes"]) {
            let key = identity(p);
            metrics.push((format!("process.{key}.cpu"), p["cpu"].as_f64()));
            metrics.push((format!("process.{key}.mem"), p["privateWs"].as_f64()));
            let sum = groups.entry(application_key(p)).or_default();
            sum.0 += number(p, "cpu");
            sum.1 += number(p, "privateWs");
        }
        for (key, (cpu, mem)) in groups {
            metrics.push((format!("group.{key}.cpu"), Some(cpu)));
            metrics.push((format!("group.{key}.mem"), Some(mem)));
        }
        let oldest = now - interval * 64. - 250.;
        for (key, value) in metrics {
            seen.insert(key.clone());
            if value.is_some() || self.samples.contains_key(&key) {
                let list = self.samples.entry(key).or_default();
                if let Some(previous) = list.back().map(|p| p.0)
                    && now - previous > interval * 2.5
                {
                    list.push_back((previous + interval, None));
                }
                list.push_back((now, value));
                while list.len() > 2 && list[1].0 < oldest {
                    list.pop_front();
                }
            }
        }
        self.samples.retain(|key, list| {
            if !seen.contains(key) {
                list.push_back((now, None));
                while list.len() > 2 && list[1].0 < oldest {
                    list.pop_front();
                }
            }
            list.iter().any(|(_, v)| v.is_some())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_hosts_never_merge_unrelated_instances() {
        let a = json!({"pid":1,"createTime":1,"name":"node.exe","path":"C:/node.exe"});
        let b = json!({"pid":2,"createTime":1,"name":"node.exe","path":"C:/node.exe"});
        assert_ne!(application_key(&a), application_key(&b));
    }
    #[test]
    fn browser_processes_share_the_recorded_browser_identity() {
        let a = json!({"pid":1,"createTime":2,"webView":{"browserPid":10,"browserCreateTime":100}});
        let b = json!({"pid":2,"createTime":3,"webView":{"browserPid":10,"browserCreateTime":100}});
        assert_eq!(application_key(&a), application_key(&b));
    }
    #[test]
    fn extended_unc_paths_share_one_application_identity() {
        let a =
            json!({"pid":1,"createTime":1,"name":"app.exe","path":r"\\?\UNC\server\share\app.exe"});
        let b = json!({"pid":2,"createTime":1,"name":"app.exe","path":r"\\server\share\app.exe"});
        assert_eq!(application_key(&a), application_key(&b));
    }
    #[test]
    fn searching_a_group_member_reveals_the_matching_instance() {
        let processes = vec![
            json!({"pid":10,"createTime":1,"name":"app.exe","path":"C:/app.exe"}),
            json!({"pid":42,"createTime":2,"name":"app.exe","path":"C:/app.exe"}),
        ];
        let empty = HashSet::new();
        let rows = process_rows(
            &processes,
            ProcessOptions {
                mode: "apps",
                query: "42",
                sort: "cpu",
                descending: true,
                expanded: &empty,
                collapsed: &empty,
                sections: &empty,
            },
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].members.len(), 2);
        assert_eq!(rows[1].raw["pid"], 42);
    }
    #[test]
    fn dense_samples_preserve_the_displayed_time_window() {
        let mut timeline = Timeline::new();
        for index in 0..700 {
            timeline.push(
                &json!({"timestamp":index*100,"system":{"cpu":index},"processes":[]}),
                1000.,
            );
        }
        assert!(timeline.samples["system.cpu"].front().unwrap().0 <= 6000.);
        assert!(timeline.samples["system.cpu"].len() > 600);
    }
    #[test]
    fn history_keeps_background_cadence_but_breaks_recording_outages() {
        let frames = [0, 30_000, 60_000, 600_000, 630_000].map(|ts| json!({"ts":ts,"cpu":10.}));
        let points = super::history_points(&frames, "cpu", 7200.);
        assert_eq!(points.iter().filter(|p| p.1.is_none()).count(), 1);
        assert_eq!(points[3], (330_000., None));
    }
}
