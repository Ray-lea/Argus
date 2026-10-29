//! Renderer-independent view state and deterministic process identity/grouping.
use super::collation::{self, SortKey};
use super::gpu_usage::{self, GpuUsage};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};

pub fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.)
}
/// Available grid space and the minimum dimensions of a processor card.
pub struct CoreGrid {
    pub count: usize,
    pub width: f32,
    pub height: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    pub header_height: f32,
    pub gap: f32,
}
/// Prefer readable plots and full rows; keep a near-equal layout while resizing.
pub fn core_grid_columns(size: &CoreGrid, previous: usize) -> usize {
    if size.count <= 1 || size.width <= 0. {
        return 1;
    }
    let limit = (((size.width + size.gap) / (size.cell_width + size.gap)).floor() as usize)
        .max(1)
        .min(size.count);
    let score = |columns: usize| {
        let rows = size.count.div_ceil(columns);
        let width = (size.width - (columns - 1) as f32 * size.gap) / columns as f32;
        let height = (size.height - (rows - 1) as f32 * size.gap) / rows as f32;
        if height < size.cell_height - 0.5 {
            return 0.;
        }
        let plot = (width / 1.8).min(height - size.header_height);
        let empty = 1. - size.count as f32 / (rows * columns) as f32;
        plot * (1. - empty * 0.15)
    };
    let mut best = limit;
    let mut best_score = 0.;
    for columns in 1..=limit {
        let value = score(columns);
        if value > best_score {
            best = columns;
            best_score = value;
        }
    }
    if previous > 0 && previous <= limit {
        let current = score(previous);
        // A small dead band prevents one-pixel drags from repeatedly moving cores.
        if current > 0. && best_score <= current * 1.08 {
            return previous;
        }
    }
    // When no arrangement fits vertically, minimize the height and allow scrolling.
    best
}
pub fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
pub fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
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
/// "WebView2: <host app>" for a WebView2 process, from the app that hosts it.
fn web_title(p: &Value) -> Option<String> {
    let web = &p["webView"];
    let host = web["hostDescription"]
        .as_str()
        .or_else(|| web["hostName"].as_str())
        .filter(|host| !host.is_empty())?;
    Some(argus_core::tr!("processes.webView.host", name = host))
}
/// A process's own title: its WebView2 host, its description, or its name.
pub fn title(p: &Value) -> String {
    web_title(p).unwrap_or_else(|| {
        let description = text(p, "description");
        if description.is_empty() {
            text(p, "name").to_owned()
        } else {
            description.to_owned()
        }
    })
}
fn web_role(p: &Value) -> Option<String> {
    let role = text(&p["webView"], "role");
    (!role.is_empty() && role != "unknown")
        .then(|| argus_core::i18n::text(&format!("processes.webView.role.{role}")))
}
/// The text beside a process titled by its executable name or WebView2 host.
fn process_detail(p: &Value) -> String {
    match web_role(p) {
        Some(role) => format!("{} · {role}", text(p, "name")),
        None if web_title(p).is_some() => text(p, "name").to_owned(),
        None => text(p, "description").to_owned(),
    }
}
/// "Service Host: DNS Client" for a service host, which its description does not tell apart.
fn service_title(p: &Value, hosted: &HashMap<u64, Vec<String>>) -> Option<String> {
    if !text(p, "name").eq_ignore_ascii_case("svchost.exe") {
        return None;
    }
    let names = p["pid"]
        .as_u64()
        .and_then(|pid| hosted.get(&pid))
        .filter(|names| !names.is_empty())?;
    Some(argus_core::tr!(
        "processes.serviceHost",
        names = summarize_services(names)
    ))
}
#[derive(Clone)]
pub struct Row {
    pub key: String,
    pub raw: Value,
    pub title: String,
    /// The title's collation key; set where process lists are built.
    sort_name: SortKey,
    pub detail: String,
    pub members: Vec<Value>,
    pub depth: usize,
    pub expanded: bool,
    pub expandable: bool,
    pub section: Option<String>,
    pub mark_key: Option<String>,
    /// The user's note and pin from the row marks.
    pub note: Option<String>,
    pub pinned: bool,
    visible_members: Option<Vec<Value>>,
    /// An app group's or section's GPU usage, aggregated per physical engine;
    /// a process row reads its own.
    gpu: Option<GpuUsage>,
}
impl Row {
    fn new(raw: Value, depth: usize, title: String, detail: String, sort_name: SortKey) -> Self {
        Self {
            key: identity(&raw),
            sort_name,
            title,
            detail,
            members: vec![raw.clone()],
            raw,
            depth,
            expanded: false,
            expandable: false,
            section: None,
            mark_key: None,
            note: None,
            pinned: false,
            visible_members: None,
            gpu: None,
        }
    }
    pub fn process(raw: Value, depth: usize) -> Self {
        let title = title(&raw);
        let detail = if title != text(&raw, "name") {
            text(&raw, "name").into()
        } else {
            String::new()
        };
        Self::new(raw, depth, title, detail, SortKey::default())
    }
    /// A process row of the process list, ordered by `title` when sorted by name.
    fn labeled(raw: Value, depth: usize, title: String, detail: String) -> Self {
        let sort_name = collation::key(&title);
        Self::new(raw, depth, title, detail, sort_name)
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
        if key == "gpu"
            && let Some(usage) = &self.gpu
        {
            return Some(usage.gpu);
        }
        self.members
            .iter()
            .filter_map(|p| p[key].as_f64())
            .reduce(|sum, value| sum + value)
    }
    /// The GPU usage a heading folds in: a group's aggregate or the process's own reading.
    fn gpu_usage(&self) -> Cow<'_, GpuUsage> {
        match &self.gpu {
            Some(usage) => Cow::Borrowed(usage),
            None => Cow::Owned(GpuUsage::of_process(&self.raw)),
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
    /// Display names of the services each service-hosting process runs, by PID.
    pub hosted: &'a HashMap<u64, Vec<String>>,
}
/// "DNS Client, DHCP Client and 3 more services": at most two names.
pub fn summarize_services(names: &[String]) -> String {
    let separator = argus_core::i18n::text("common.listSeparator");
    if names.len() <= 2 {
        names.join(&separator)
    } else {
        argus_core::tr!(
            "processes.servicesSummary",
            names = names[..2].join(&separator),
            count = names.len()
        )
    }
}
/// The rows of the process table in the original's arrangement: the apps view
/// titles a row by its description, the tree and list by the executable name.
pub fn process_rows(processes: &[Value], options: ProcessOptions<'_>) -> Vec<Row> {
    let mut rows = group_rows(processes, &options);
    // A service host's description says nothing; the services it runs do. App
    // rows keep their exe name; other hosts list their services as detail.
    if !options.hosted.is_empty() {
        for row in &mut rows {
            if row.section.is_some() || row.members.len() > 1 {
                continue;
            }
            let Some(names) = row.raw["pid"]
                .as_u64()
                .and_then(|pid| options.hosted.get(&pid))
            else {
                continue;
            };
            if !names.is_empty() && text(&row.raw, "name").eq_ignore_ascii_case("svchost.exe") {
                row.detail = text(&row.raw, "name").to_owned();
            } else if !(options.mode == "apps" && row.depth == 0) {
                row.detail = summarize_services(names);
            }
        }
    }
    rows
}
fn pid(row: &Row) -> u64 {
    row.raw["pid"].as_u64().unwrap_or(0)
}
/// The original's comparator: names collate in the interface language with
/// the PID breaking ties; figures compare at their displayed precision, then
/// by name in ascending order whatever the direction.
fn compare_rows(a: &Row, b: &Row, sort: &str, descending: bool) -> Ordering {
    let direct = |order: Ordering| if descending { order.reverse() } else { order };
    let by_name = || {
        a.sort_name
            .cmp(&b.sort_name)
            .then_with(|| pid(a).cmp(&pid(b)))
    };
    let order = match sort {
        "name" => direct(by_name()),
        "pid" => direct(pid(a).cmp(&pid(b))),
        key => {
            let scale = match key {
                "cpu" | "gpu" => 10.,
                "mem" => 1. / 104858.,
                "io" | "diskRead" | "diskWrite" | "net" => 1. / 1024.,
                _ => 1.,
            };
            // Missing readings (a disk interval not measured yet) sort last either way.
            match (a.metric(key), b.metric(key)) {
                (Some(a), Some(b)) => direct((a * scale).round().total_cmp(&(b * scale).round())),
                (None, None) => Ordering::Equal,
                (None, _) => Ordering::Greater,
                (_, None) => Ordering::Less,
            }
            .then_with(by_name)
        }
    };
    order.then_with(|| a.key.cmp(&b.key))
}
fn group_rows(processes: &[Value], options: &ProcessOptions<'_>) -> Vec<Row> {
    let query = options.query.trim().to_lowercase();
    let numeric = !query.is_empty() && query.bytes().all(|b| b.is_ascii_digit());
    let matches = |p: &Value| {
        let contains = |value: &str| value.to_lowercase().contains(&query);
        query.is_empty()
            || (numeric && p["pid"].to_string().starts_with(&query))
            || contains(text(p, "name"))
            || contains(text(p, "description"))
            || contains(text(p, "company"))
            || contains(text(p, "path"))
            || contains(text(&p["webView"], "hostName"))
            || contains(text(&p["webView"], "hostDescription"))
            || web_role(p).is_some_and(|role| contains(&role))
            || p["pid"]
                .as_u64()
                .and_then(|pid| options.hosted.get(&pid))
                .is_some_and(|names| names.iter().any(|name| contains(name)))
    };
    let compare = |a: &Row, b: &Row| compare_rows(a, b, options.sort, options.descending);
    // The tree and list name a process by its executable, its description beside it.
    let named_by_executable = |p: &Value, depth: usize| {
        let name = text(p, "name");
        match service_title(p, options.hosted) {
            Some(service) => Row::labeled(p.clone(), depth, service, name.to_owned()),
            None => Row::labeled(
                p.clone(),
                depth,
                web_title(p).unwrap_or_else(|| name.to_owned()),
                process_detail(p),
            ),
        }
    };
    let mut top = Vec::new();
    match options.mode {
        "apps" => {
            let mut order = Vec::new();
            let mut groups: HashMap<String, Vec<&Value>> = HashMap::new();
            for p in processes {
                let key = application_key(p);
                groups
                    .entry(key)
                    .or_insert_with_key(|key| {
                        order.push(key.clone());
                        Vec::new()
                    })
                    .push(p);
            }
            for key in order {
                let mut members = groups.remove(&key).unwrap_or_default();
                // The earliest-started member stands for the app (the details panel
                // reads the first member too).
                members.sort_by(|a, b| number(a, "createTime").total_cmp(&number(b, "createTime")));
                let Some(&first) = members.first() else {
                    continue;
                };
                let name = text(first, "name");
                let label = service_title(first, options.hosted).unwrap_or_else(|| title(first));
                let detail =
                    if !text(first, "description").is_empty() || !first["webView"].is_null() {
                        name.to_owned()
                    } else {
                        String::new()
                    };
                // A search that names the app shows all of it; otherwise only the matches.
                let shown = if !query.is_empty()
                    && !label.to_lowercase().contains(&query)
                    && !name.to_lowercase().contains(&query)
                {
                    let shown: Vec<Value> = members
                        .iter()
                        .filter(|p| matches(p))
                        .map(|p| (*p).clone())
                        .collect();
                    if shown.is_empty() {
                        continue;
                    }
                    Some(shown)
                } else {
                    None
                };
                if members.len() == 1 {
                    let detail = if first["webView"].is_null() {
                        detail
                    } else {
                        process_detail(first)
                    };
                    top.push(Row::labeled(first.clone(), 0, label, detail));
                    continue;
                }
                let mut row = Row::labeled(first.clone(), 0, label, detail);
                row.key = key;
                // Every member counts, also those a search leaves out.
                row.gpu = Some(gpu_usage::aggregate(
                    members.iter().map(|p| GpuUsage::of_process(p)),
                ));
                row.members = members.into_iter().cloned().collect();
                row.expandable = true;
                row.expanded = options.expanded.contains(&row.key)
                    || shown
                        .as_ref()
                        .is_some_and(|items| items.len() < row.members.len());
                row.visible_members = shown;
                top.push(row);
            }
        }
        "tree" => return tree_rows(processes, options, &query, &matches, &named_by_executable),
        _ => {
            top = processes
                .iter()
                .filter(|p| matches(p))
                .map(|p| named_by_executable(p, 0))
                .collect();
        }
    }
    top.sort_by(compare);
    let mut out = Vec::new();
    // Like Windows Task Manager: sorted by name, apps come first under their
    // own heading. Other sorts keep one list.
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
            // Top-level rows only: a group's usage already includes its members.
            heading.gpu = Some(gpu_usage::aggregate(grouped.iter().map(Row::gpu_usage)));
            heading.raw["entryCount"] = json!(grouped.len());
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
/// The process tree: children under the process that started them, each level
/// sorted on its own. While searching, the ancestors of matches stay in place.
fn tree_rows(
    processes: &[Value],
    options: &ProcessOptions<'_>,
    query: &str,
    matches: &impl Fn(&Value) -> bool,
    named: &impl Fn(&Value, usize) -> Row,
) -> Vec<Row> {
    let by_pid: HashMap<u64, &Value> = processes
        .iter()
        .filter_map(|p| p["pid"].as_u64().map(|pid| (pid, p)))
        .collect();
    // A parent must predate its child; otherwise the parent PID was reused.
    let parent_of = |p: &Value| {
        p["ppid"]
            .as_u64()
            .and_then(|ppid| by_pid.get(&ppid).copied())
            .filter(|parent| {
                parent["pid"] != p["pid"] && number(parent, "createTime") <= number(p, "createTime")
            })
    };
    let mut children: HashMap<String, Vec<&Value>> = HashMap::new();
    let mut roots = Vec::new();
    for p in processes {
        match parent_of(p) {
            Some(parent) => children.entry(identity(parent)).or_default().push(p),
            None => roots.push(p),
        }
    }
    let searching = !query.is_empty();
    let mut visible = HashSet::new();
    if searching {
        for process in processes.iter().filter(|p| matches(p)) {
            let mut current = Some(process);
            while let Some(p) = current {
                if !visible.insert(identity(p)) {
                    break;
                }
                current = parent_of(p);
            }
        }
    }
    struct Walk<'a, F> {
        children: &'a HashMap<String, Vec<&'a Value>>,
        visible: &'a HashSet<String>,
        searching: bool,
        collapsed: &'a HashSet<String>,
        named: &'a F,
        sort: &'a str,
        descending: bool,
        seen: HashSet<String>,
        out: Vec<Row>,
    }
    impl<F: Fn(&Value, usize) -> Row> Walk<'_, F> {
        fn level(&mut self, list: &[&Value], depth: usize) {
            let mut level: Vec<Row> = list
                .iter()
                .filter(|p| !self.searching || self.visible.contains(&identity(p)))
                .map(|p| {
                    let mut row = (self.named)(p, depth);
                    row.expandable = self.children.get(&row.key).is_some_and(|c| !c.is_empty());
                    row.expanded =
                        row.expandable && (self.searching || !self.collapsed.contains(&row.key));
                    row
                })
                .collect();
            level.sort_by(|a, b| compare_rows(a, b, self.sort, self.descending));
            for row in level {
                if !self.seen.insert(row.key.clone()) {
                    continue;
                }
                let (key, expanded) = (row.key.clone(), row.expanded);
                self.out.push(row);
                if expanded && let Some(list) = self.children.get(&key) {
                    self.level(list, depth + 1);
                }
            }
        }
    }
    let mut walk = Walk {
        children: &children,
        visible: &visible,
        searching,
        collapsed: options.collapsed,
        named,
        sort: options.sort,
        descending: options.descending,
        seen: HashSet::new(),
        out: Vec::new(),
    };
    walk.level(&roots, 0);
    walk.out
}
fn append_groups(rows: Vec<Row>, compare: &impl Fn(&Row, &Row) -> Ordering, out: &mut Vec<Row>) {
    for row in rows {
        let children = if row.expanded && row.expandable {
            let mut children: Vec<_> = row
                .visible_members
                .as_ref()
                .unwrap_or(&row.members)
                .iter()
                .map(|p| {
                    let label = web_role(p).unwrap_or_else(|| text(p, "name").into());
                    let detail = if p["webView"].is_null() {
                        String::new()
                    } else {
                        title(p)
                    };
                    Row::labeled(p.clone(), 1, label, detail)
                })
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
    /// Sample times of the process and app histories, oldest first.
    times: VecDeque<f64>,
    /// CPU and private working set of every live process, by PID and creation time.
    processes: HashMap<(u64, i64), Usage>,
    /// The same per app (group key): totals of whichever members ran at the time.
    groups: HashMap<String, Usage>,
    /// Counts pushes; usage not updated in the latest one belongs to an exited process.
    tick: u64,
}

/// Rolling CPU and memory of one process or app, aligned with the end of `Timeline::times`.
#[derive(Default)]
struct Usage {
    /// Newest last; NaN marks an unknown sample.
    cpu: VecDeque<f64>,
    mem: VecDeque<f64>,
    /// The app a process belongs to, when it can share an app row with others.
    /// A process without one is checked again in case its path or WebView
    /// details arrive later.
    app: Option<String>,
    origin: (usize, bool),
    tick: u64,
}
impl Usage {
    fn push(&mut self, cpu: f64, mem: f64, keep: usize) {
        self.cpu.push_back(cpu);
        self.mem.push_back(mem);
        while self.cpu.len() > keep {
            self.cpu.pop_front();
            self.mem.pop_front();
        }
    }
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
            times: VecDeque::new(),
            processes: HashMap::new(),
            groups: HashMap::new(),
            tick: 0,
        }
    }
    /// A series by key: device metrics, or `process.<identity>.cpu|mem` and
    /// `group.<app key>.cpu|mem`, which every live process and app keeps from
    /// its first sample, so a newly selected row's charts are already full.
    pub fn points(&self, key: &str) -> Option<Vec<(f64, Option<f64>)>> {
        let points = |usage: &Usage, metric: &str| {
            let values = match metric {
                "cpu" => &usage.cpu,
                "mem" => &usage.mem,
                _ => return None,
            };
            let skip = self.times.len().saturating_sub(values.len());
            Some(
                self.times
                    .iter()
                    .skip(skip)
                    .zip(values)
                    .map(|(&at, &value)| (at, (!value.is_nan()).then_some(value)))
                    .collect(),
            )
        };
        if let Some(rest) = key.strip_prefix("process.") {
            // The identity is `pid:createTime`, as the rows key processes.
            let (id, metric) = rest.rsplit_once('.')?;
            let (pid, created) = id.split_once(':')?;
            let usage = self
                .processes
                .get(&(pid.parse().ok()?, created.parse().ok()?))?;
            return points(usage, metric);
        }
        if let Some(rest) = key.strip_prefix("group.") {
            let (id, metric) = rest.rsplit_once('.')?;
            return points(self.groups.get(id)?, metric);
        }
        self.samples
            .get(key)
            .map(|list| list.iter().copied().collect())
    }
    pub fn push(&mut self, snapshot: &Value, interval: f64) {
        let now = number(snapshot, "timestamp");
        if !now.is_finite() {
            return;
        }
        let last = self
            .samples
            .values()
            .find_map(|v| v.back().map(|p| p.0))
            .or_else(|| self.times.back().copied());
        if last == Some(now) {
            return;
        }
        if last.is_some_and(|previous| now < previous) {
            self.samples.clear();
            self.times.clear();
            self.processes.clear();
            self.groups.clear();
        }
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
                        if matches!(k.as_str(), "info" | "threads" | "boardSensors" | "limits")
                            || (prefix == "hardware.cpuSensors" && k == "cores")
                        {
                            continue;
                        }
                        flatten(&format!("{prefix}.{k}"), v, out)
                    }
                }
                _ => {}
            }
        }
        let mut metrics = Vec::with_capacity(128);
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
        let oldest = now - interval * 64. - 250.;
        // Samples further apart than the interval (the slower cadence while the
        // window is minimized or in the tray, a pause, a missed sample) join up,
        // as in the original; only an unknown reading breaks a curve.
        let mut record = |key: String, value: Option<f64>| {
            if value.is_some() || self.samples.contains_key(&key) {
                let list = self.samples.entry(key).or_default();
                list.push_back((now, value));
                while list.len() > 2 && list[1].0 < oldest {
                    list.pop_front();
                }
            }
        };
        for (key, value) in metrics {
            record(key, value);
        }
        self.push_usage(snapshot, now, oldest);
        self.samples.retain(|_, list| {
            if list.back().map(|p| p.0) != Some(now) {
                list.push_back((now, None));
                while list.len() > 2 && list[1].0 < oldest {
                    list.pop_front();
                }
            }
            list.iter().any(|(_, v)| v.is_some())
        });
    }
    /// Records every process and app, as the original kept them; exited ones are dropped.
    fn push_usage(&mut self, snapshot: &Value, now: f64, oldest: f64) {
        // Sparse samples join up here too (see `push`).
        self.times.push_back(now);
        while self.times.len() > 2 && self.times[1] < oldest {
            self.times.pop_front();
        }
        let keep = self.times.len();
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        for p in values(&snapshot["processes"]) {
            let (Some(pid), Some(created)) = (p["pid"].as_u64(), p["createTime"].as_i64()) else {
                continue;
            };
            let usage = self.processes.entry((pid, created)).or_default();
            if usage.tick == tick {
                continue;
            }
            usage.tick = tick;
            let fresh = usage.cpu.is_empty();
            if fresh || usage.app.is_none() {
                let origin = (text(p, "path").len(), p["webView"].is_null());
                if fresh || usage.origin != origin {
                    let app = application_key(p);
                    usage.app = (!app.starts_with("g:instance:")).then_some(app);
                    usage.origin = origin;
                }
            }
            usage.push(
                p["cpu"].as_f64().unwrap_or(f64::NAN),
                p["privateWs"].as_f64().unwrap_or(f64::NAN),
                keep,
            );
        }
        self.processes.retain(|_, usage| usage.tick == tick);
        // An app's figures add up its processes' latest samples; unknown ones count as zero.
        let latest =
            |values: &VecDeque<f64>| values.back().copied().filter(|v| !v.is_nan()).unwrap_or(0.);
        let mut totals: HashMap<&str, (f64, f64)> = HashMap::new();
        for usage in self.processes.values() {
            if let Some(app) = &usage.app {
                let sum = totals.entry(app).or_default();
                sum.0 += latest(&usage.cpu);
                sum.1 += latest(&usage.mem);
            }
        }
        for (app, (cpu, mem)) in totals {
            if !self.groups.contains_key(app) {
                self.groups.insert(app.to_owned(), Usage::default());
            }
            if let Some(usage) = self.groups.get_mut(app) {
                usage.tick = tick;
                usage.push(cpu, mem, keep);
            }
        }
        self.groups.retain(|_, usage| usage.tick == tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rows_of(
        processes: &[Value],
        mode: &str,
        query: &str,
        sort: &str,
        descending: bool,
        hosted: &HashMap<u64, Vec<String>>,
    ) -> Vec<Row> {
        let empty = HashSet::new();
        process_rows(
            processes,
            ProcessOptions {
                mode,
                query,
                sort,
                descending,
                expanded: &empty,
                collapsed: &empty,
                sections: &empty,
                hosted,
            },
        )
    }
    fn pids(rows: &[Row]) -> Vec<u64> {
        rows.iter()
            .filter(|row| row.section.is_none())
            .map(pid)
            .collect()
    }
    #[test]
    fn service_hosts_sort_by_the_services_they_run() {
        // Titles come from the interface catalogs, in whatever language is current.
        crate::catalog::register();
        let host = |pid: u64| json!({"pid":pid,"createTime":1,"name":"svchost.exe","description":"Host Process for Windows Services","path":"C:/Windows/System32/svchost.exe"});
        let processes = vec![host(10), host(20)];
        let hosted = HashMap::from([
            (10, vec!["Zeta Service".to_owned()]),
            (20, vec!["Alpha Service".to_owned()]),
        ]);
        for mode in ["apps", "list", "tree"] {
            let rows = rows_of(&processes, mode, "", "name", false, &hosted);
            assert_eq!(pids(&rows), [20, 10], "{mode}");
            let row = rows.iter().find(|row| pid(row) == 20).unwrap();
            assert!(row.title.contains("Alpha Service"), "{mode}");
            assert_eq!(row.detail, "svchost.exe", "{mode}");
        }
    }
    #[test]
    fn equal_names_follow_the_pid_in_the_sort_direction() {
        let processes = vec![
            json!({"pid":5,"createTime":1,"name":"same.exe"}),
            json!({"pid":3,"createTime":1,"name":"same.exe"}),
            json!({"pid":4,"createTime":1,"name":"other.exe"}),
        ];
        let none = HashMap::new();
        assert_eq!(
            pids(&rows_of(&processes, "list", "", "name", false, &none)),
            [4, 3, 5]
        );
        assert_eq!(
            pids(&rows_of(&processes, "list", "", "name", true, &none)),
            [5, 3, 4]
        );
    }
    #[test]
    fn equal_figures_keep_names_ascending_in_either_direction() {
        let processes = vec![
            json!({"pid":1,"createTime":1,"name":"b.exe","cpu":1.0}),
            json!({"pid":2,"createTime":1,"name":"a.exe","cpu":1.02}),
            json!({"pid":3,"createTime":1,"name":"c.exe","cpu":5.0}),
        ];
        let none = HashMap::new();
        assert_eq!(
            pids(&rows_of(&processes, "list", "", "cpu", true, &none)),
            [3, 2, 1]
        );
        assert_eq!(
            pids(&rows_of(&processes, "list", "", "cpu", false, &none)),
            [2, 1, 3]
        );
    }
    #[test]
    fn views_title_processes_like_the_original() {
        let processes = vec![
            json!({"pid":1,"createTime":1,"name":"app.exe","description":"App","path":"C:/app.exe"}),
            json!({"pid":2,"createTime":2,"name":"msedgewebview2.exe","path":"C:/wv/msedgewebview2.exe","webView":{"browserPid":2,"browserCreateTime":2,"hostName":"app.exe","hostDescription":"App","role":"browser"}}),
        ];
        let none = HashMap::new();
        let apps = rows_of(&processes, "apps", "", "pid", false, &none);
        assert_eq!(
            (apps[0].title.as_str(), apps[0].detail.as_str()),
            ("App", "app.exe")
        );
        // A lone WebView2 process names its role beside the executable.
        assert!(apps[1].detail.starts_with("msedgewebview2.exe · "));
        for mode in ["list", "tree"] {
            let rows = rows_of(&processes, mode, "", "pid", false, &none);
            assert_eq!(
                (rows[0].title.as_str(), rows[0].detail.as_str()),
                ("app.exe", "App")
            );
            assert_eq!(rows[1].title, apps[1].title);
            assert_eq!(rows[1].detail, apps[1].detail);
        }
    }
    #[test]
    fn the_tree_expands_parents_only_and_keeps_ancestors_of_matches() {
        let processes = vec![
            json!({"pid":1,"ppid":0,"createTime":1,"name":"root.exe"}),
            json!({"pid":2,"ppid":1,"createTime":2,"name":"child.exe"}),
            json!({"pid":3,"ppid":2,"createTime":3,"name":"leaf.exe"}),
            // Its parent PID was reused by a newer process: a root of its own.
            json!({"pid":4,"ppid":5,"createTime":4,"name":"orphan.exe"}),
            json!({"pid":5,"ppid":0,"createTime":9,"name":"newer.exe"}),
        ];
        let none = HashMap::new();
        let rows = rows_of(&processes, "tree", "", "name", false, &none);
        assert_eq!(pids(&rows), [5, 4, 1, 2, 3]);
        assert_eq!(
            rows.iter().map(|row| row.depth).collect::<Vec<_>>(),
            [0, 0, 0, 1, 2]
        );
        assert!(rows[2].expandable && rows[2].expanded);
        assert!(!rows[4].expandable && !rows[4].expanded);
        let found = rows_of(&processes, "tree", "leaf", "name", false, &none);
        assert_eq!(pids(&found), [1, 2, 3]);
    }
    #[test]
    fn expanded_members_sort_by_their_displayed_names() {
        let processes = vec![
            json!({"pid":1,"createTime":1,"name":"zeta.exe","description":"Product","path":"C:/App.exe"}),
            json!({"pid":2,"createTime":2,"name":"alpha.exe","description":"Product","path":"C:/App.exe"}),
        ];
        let empty = HashSet::new();
        let expanded = HashSet::from([application_key(&processes[0])]);
        let rows = process_rows(
            &processes,
            ProcessOptions {
                mode: "apps",
                query: "",
                sort: "name",
                descending: false,
                expanded: &expanded,
                collapsed: &empty,
                sections: &empty,
                hosted: &HashMap::new(),
            },
        );
        let members = rows.iter().filter(|row| row.depth == 1).collect::<Vec<_>>();
        assert_eq!(members.len(), 2);
        assert!(
            members
                .windows(2)
                .all(|pair| pair[0].title.to_lowercase() <= pair[1].title.to_lowercase())
        );
    }
    #[test]
    fn expanded_apps_distinguish_members_and_bound_gpu_totals() {
        let processes = vec![
            json!({"pid":1,"createTime":1,"name":"app.exe","description":"Product","path":"C:/app.exe","gpu":70,"gpuEngine":"GPU 0 - 3D","gpuEngines":[{"id":"1:0:0","name":"GPU 0 - 3D","util":70}]}),
            json!({"pid":2,"createTime":2,"name":"app.exe","description":"Product","path":"C:/app.exe","gpu":50,"gpuEngine":"GPU 0 - 3D","gpuEngines":[{"id":"1:0:0","name":"GPU 0 - 3D","util":50}]}),
        ];
        let empty = HashSet::new();
        let expanded = HashSet::from([application_key(&processes[0])]);
        let rows = process_rows(
            &processes,
            ProcessOptions {
                mode: "apps",
                query: "",
                sort: "cpu",
                descending: true,
                expanded: &expanded,
                collapsed: &empty,
                sections: &empty,
                hosted: &HashMap::new(),
            },
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].metric("gpu"), Some(100.));
        assert_eq!(rows[1].title, text(&rows[1].raw, "name"));
        assert!(rows[1].detail.is_empty());
        assert_eq!(rows[1].depth, 1);
        assert_ne!(rows[1].key, rows[2].key);
    }
    #[test]
    fn app_groups_and_headings_take_the_busiest_physical_engine() {
        let process = |pid: u64, path: &str, engines: Value| {
            json!({"pid":pid,"createTime":pid,"name":"app.exe","description":"Product","path":path,
                "gpu":values(&engines).iter().map(|e| number(e, "util")).fold(0., f64::max),
                "gpuEngines":engines})
        };
        let processes = vec![
            process(
                1,
                "C:/a.exe",
                json!([{"id":"1:0:0","name":"GPU 0 - 3D","util":40}]),
            ),
            process(
                2,
                "C:/a.exe",
                json!([{"id":"2:0:3","name":"GPU 1 - Video Decode","util":35}]),
            ),
            process(
                3,
                "C:/b.exe",
                json!([{"id":"1:0:0","name":"GPU 0 - 3D","util":30}]),
            ),
        ];
        let rows = rows_of(&processes, "apps", "", "name", false, &HashMap::new());
        let group = rows.iter().find(|row| row.members.len() == 2).unwrap();
        assert_eq!(group.metric("gpu"), Some(40.));
        // The heading folds the group in by engine: 40% + 30% on one 3D node.
        assert_eq!(rows[0].section.as_deref(), Some("background"));
        assert_eq!(rows[0].metric("gpu"), Some(70.));
        // Rows labelled with one busy engine add up on it, as the original's heading does.
        let labeled = |pid: u64, gpu: f64| json!({"pid":pid,"createTime":1,"name":format!("{pid}.exe"),"gpu":gpu,"gpuEngine":"GPU 0 - 3D"});
        let rows = rows_of(
            &[labeled(1, 40.), labeled(2, 40.), labeled(3, 30.)],
            "list",
            "",
            "name",
            false,
            &HashMap::new(),
        );
        assert_eq!(rows[0].metric("gpu"), Some(100.));
    }
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
                hosted: &HashMap::new(),
            },
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].members.len(), 2);
        assert_eq!(rows[1].raw["pid"], 42);
    }
    #[test]
    fn every_process_and_app_keeps_history_from_its_first_sample() {
        let mut timeline = Timeline::new();
        let process = |pid: u64, cpu: f64| json!({"pid":pid,"createTime":1,"name":"app.exe","path":"C:/app.exe","cpu":cpu,"privateWs":1000});
        for index in 0..6u64 {
            let mut processes = vec![process(1, index as f64)];
            if index < 2 {
                processes.push(process(2, 1.));
            }
            // Two samples are missed: the histories join the samples around
            // them, as the original does (a minimized window samples less often).
            if !matches!(index, 3 | 4) {
                timeline.push(
                    &json!({"timestamp":index*1000,"system":{"cpu":1},"processes":processes}),
                    1000.,
                );
            }
        }
        let cpu = timeline.points("process.1:1.cpu").unwrap();
        assert_eq!(cpu.first(), Some(&(0., Some(0.))));
        assert_eq!(cpu.last(), Some(&(5000., Some(5.))));
        assert_eq!(cpu[3], (5000., Some(5.)));
        assert!(cpu.iter().all(|(_, value)| value.is_some()));
        let total = timeline.points("system.cpu").unwrap();
        assert_eq!(total.len(), 4);
        assert!(total.iter().all(|(_, value)| value.is_some()));
        // An exited process is dropped; its app adds up whichever members ran.
        assert!(timeline.points("process.2:1.cpu").is_none());
        let app = timeline.points("group.g:image:c:\\app.exe.cpu").unwrap();
        assert_eq!(app[1], (1000., Some(2.)));
        assert_eq!(app[2], (2000., Some(2.)));
        assert_eq!(
            timeline.points("group.g:image:c:\\app.exe.mem").unwrap()[0],
            (0., Some(2000.))
        );
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
}
