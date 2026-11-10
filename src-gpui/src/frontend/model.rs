//! Renderer-independent view state and deterministic process identity/grouping.
use super::collation::{self, SortKey};
use super::columns;
use super::gpu_usage::{self, GpuUsage};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
mod sensor_layout;
pub use sensor_layout::SensorLayout;

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
/// Match the rule engine's Unicode lowercase comparison for executable names.
pub fn same_executable_name(left: &str, right: &str) -> bool {
    left.chars()
        .flat_map(char::to_lowercase)
        .eq(right.chars().flat_map(char::to_lowercase))
}
pub fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A whole-machine disk rate is unknown if any present disk lacks counters.
pub fn disk_rate(disks: &[Value]) -> Option<f64> {
    if disks.is_empty() {
        return None;
    }
    disks.iter().try_fold(0., |sum, disk| {
        Some(sum + disk["read"].as_f64()? + disk["write"].as_f64()?)
    })
}

pub fn values(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}
pub fn identity(p: &Value) -> String {
    if let Some(key) = p["instanceKey"].as_str() {
        return key.to_owned();
    }
    format!("{}:{}", p["pid"], p["createTime"])
}

pub fn parent_predates(parent: &Value, child: &Value) -> bool {
    if let Some((parent, child)) = parent["sequenceNumber"]
        .as_u64()
        .zip(child["sequenceNumber"].as_u64())
    {
        return parent <= child;
    }
    let a = number(parent, "createTime");
    let b = number(child, "createTime");
    a >= 0. && b >= 0. && a <= b
}

/// Identity and independent GPU/network sources can precede native counters.
pub fn process_metric(process: &Value, key: &str) -> Option<f64> {
    let key = columns::raw_key(key);
    if key == "createTime" && number(process, key) <= 0. {
        return None;
    }
    if boolean(process, "countersPending")
        && !matches!(
            key,
            "pid"
                | "ppid"
                | "createTime"
                | "gpu"
                | "gpuDedicated"
                | "gpuShared"
                | "netSent"
                | "netReceived"
        )
    {
        return None;
    }
    if matches!(key, "netSent" | "netReceived") && number(process, "createTime") < 0. {
        return None;
    }
    process[key].as_f64().or_else(|| {
        process[key]
            .as_bool()
            .map(|value| f64::from(u8::from(value)))
    })
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
    /// A row of a service, startup or connection list: no process members.
    pub fn item(raw: Value, key: String, title: String) -> Self {
        Self {
            key,
            raw,
            title,
            sort_name: SortKey::default(),
            detail: String::new(),
            members: Vec::new(),
            depth: 0,
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
        self.metric_value(key)
    }

    /// The value is the last successful reading while its source is pending.
    /// Independent GPU/network samples and static identity fields remain current.
    pub fn native_pending(&self, key: &str) -> bool {
        !matches!(
            key,
            "pid"
                | "ppid"
                | "sessionId"
                | "createTime"
                | "gpu"
                | "gpuDedicated"
                | "gpuShared"
                | "net"
                | "netSent"
                | "netReceived"
        ) && columns::definition(key).is_some_and(|column| {
            !matches!(column.format, columns::Format::Name | columns::Format::Text)
        }) && self.members.iter().any(|member| {
            boolean(member, "stale")
                || boolean(member, "sampleReused")
                || boolean(member, "countersPending")
        })
    }

    /// Both display and sorting use the last successful counters; null stays unknown.
    fn metric_value(&self, key: &str) -> Option<f64> {
        if matches!(key, "gpuDedicated" | "gpuShared") {
            return self.members.iter().try_fold(0., |sum, member| {
                process_metric(member, key).map(|value| sum + value)
            });
        }
        if columns::definition(key).is_some_and(|column| !column.aggregate) {
            if self.section.is_some() || self.members.len() != 1 {
                return None;
            }
            return process_metric(&self.members[0], key);
        }
        let key = match key {
            "mem" => "privateWs",
            other => other,
        };
        if key == "io" {
            return self
                .members
                .iter()
                .filter_map(|p| {
                    if boolean(p, "countersPending") {
                        None
                    } else {
                        Some(number(p, "ioRead") + number(p, "ioWrite") + number(p, "ioOther"))
                    }
                })
                .reduce(|sum, value| sum + value);
        }
        if key == "net" {
            return self
                .members
                .iter()
                .filter(|p| number(p, "createTime") >= 0.)
                .map(|p| number(p, "netReceived") + number(p, "netSent"))
                .reduce(|sum, value| sum + value);
        }
        if key == "gpu"
            && let Some(usage) = &self.gpu
        {
            return Some(usage.gpu);
        }
        self.members
            .iter()
            .filter_map(|p| process_metric(p, key))
            .reduce(|sum, value| sum + value)
    }

    /// Shared identity text is shown for an app group; mixed values stay unknown.
    pub fn column_text(&self, key: &str) -> Option<&str> {
        if self.section.is_some() {
            return None;
        }
        if key == "gpuEngine"
            && let Some(usage) = &self.gpu
        {
            return usage.engine.as_deref();
        }
        let key = columns::raw_key(key);
        let first = self.members.first()?[key]
            .as_str()
            .filter(|value| !value.is_empty())?;
        self.members
            .iter()
            .all(|member| member[key].as_str() == Some(first))
            .then_some(first)
    }
    /// The GPU usage a heading folds in: a group's aggregate or the process's own reading.
    pub fn gpu_usage(&self) -> Cow<'_, GpuUsage> {
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
    pub tree_expanded: &'a HashSet<String>,
    pub sections: &'a HashSet<String>,
    /// Display names of the services each service-hosting process runs, by PID.
    pub hosted: &'a HashMap<u64, Vec<String>>,
    pub visibility: Option<&'a super::visibility::Rules>,
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
/// The Name column prefers the application/file description in every view.
/// Expanded app members retain their executable identities below the app row.
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
/// The interface's comparator: names collate in the interface language with
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
        key if columns::definition(key)
            .is_some_and(|column| column.format == columns::Format::Text) =>
        {
            match (a.column_text(key), b.column_text(key)) {
                (Some(a), Some(b)) => direct(collation::compare(a, b)),
                (None, None) => Ordering::Equal,
                (None, _) => Ordering::Greater,
                (_, None) => Ordering::Less,
            }
            .then_with(by_name)
        }
        key => {
            let scale = match key {
                "cpu" | "gpu" => 10.,
                "mem" => 1. / 104858.,
                "io" | "diskRead" | "diskWrite" | "net" => 1. / 1024.,
                _ => 1.,
            };
            // Missing readings (a disk interval not measured yet) sort last either way.
            match (a.metric_value(key), b.metric_value(key)) {
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
    let visible = |p: &&Value| {
        options
            .visibility
            .is_none_or(|rules| !rules.hides_process(p))
    };
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
    let named_process = |p: &Value, depth: usize| {
        let name = text(p, "name");
        let primary = service_title(p, options.hosted).unwrap_or_else(|| title(p));
        let detail = if !p["webView"].is_null() {
            process_detail(p)
        } else if primary != name {
            name.to_owned()
        } else {
            String::new()
        };
        Row::labeled(p.clone(), depth, primary, detail)
    };
    let mut top = Vec::new();
    match options.mode {
        "apps" => {
            let mut order = Vec::new();
            let mut groups: HashMap<String, Vec<&Value>> = HashMap::new();
            for p in processes.iter().filter(visible) {
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
        "tree" => return tree_rows(processes, options, &query, &matches, &named_process),
        _ => {
            top = processes
                .iter()
                .filter(visible)
                .filter(|p| matches(p))
                .map(|p| named_process(p, 0))
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
fn parent_of<'a>(process: &Value, by_pid: &HashMap<u64, &'a Value>) -> Option<&'a Value> {
    process["ppid"]
        .as_u64()
        .filter(|ppid| *ppid != 0)
        .and_then(|ppid| by_pid.get(&ppid).copied())
        .filter(|parent| parent["pid"] != process["pid"] && parent_predates(parent, process))
}

/// Only actual parents are expanded; leaves that later gain children stay folded.
pub fn tree_parents(processes: &[Value]) -> HashSet<String> {
    let by_pid: HashMap<u64, &Value> = processes
        .iter()
        .filter_map(|process| process["pid"].as_u64().map(|pid| (pid, process)))
        .collect();
    processes
        .iter()
        .filter_map(|process| parent_of(process, &by_pid))
        .map(identity)
        .collect()
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
    let visible_process = |p: &&Value| {
        options
            .visibility
            .is_none_or(|rules| !rules.hides_process(p))
    };
    let by_pid: HashMap<u64, &Value> = processes
        .iter()
        .filter(visible_process)
        .filter_map(|p| p["pid"].as_u64().map(|pid| (pid, p)))
        .collect();
    // A parent must predate its child; otherwise the parent PID was reused.
    let mut children: HashMap<String, Vec<&Value>> = HashMap::new();
    let mut roots = Vec::new();
    for p in processes.iter().filter(visible_process) {
        match parent_of(p, &by_pid) {
            Some(parent) => children.entry(identity(parent)).or_default().push(p),
            None => roots.push(p),
        }
    }
    let searching = !query.is_empty();
    let mut visible = HashSet::new();
    if searching {
        for process in processes
            .iter()
            .filter(visible_process)
            .filter(|p| matches(p))
        {
            let mut current = Some(process);
            while let Some(p) = current {
                if !visible.insert(identity(p)) {
                    break;
                }
                current = parent_of(p, &by_pid);
            }
        }
    }
    struct Walk<'a, F> {
        children: &'a HashMap<String, Vec<&'a Value>>,
        visible: &'a HashSet<String>,
        searching: bool,
        expanded: &'a HashSet<String>,
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
                        row.expandable && (self.searching || self.expanded.contains(&row.key));
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
        expanded: options.tree_expanded,
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

pub const CPU_TEMPERATURE_SERIES: &str = "derived.cpuTemperature";

/// Match the live figure for each sample, before the available source changes.
pub fn cpu_temperature(hw: &Value) -> Option<f64> {
    let sensors = &hw["cpuSensors"];
    sensors["packageCelsius"]
        .as_f64()
        .or_else(|| {
            values(&sensors["coreTemperatures"])
                .iter()
                .filter_map(|core| core["celsius"].as_f64())
                .reduce(f64::max)
        })
        .or_else(|| {
            values(&hw["thermal"])
                .iter()
                .filter_map(|zone| zone["celsius"].as_f64())
                .reduce(f64::max)
        })
}

pub struct Timeline {
    pub samples: HashMap<String, VecDeque<(f64, Option<f64>)>>,
    /// Sample times of the process and app histories, oldest first.
    times: VecDeque<f64>,
    /// CPU and private working set keyed by a stable process lifetime identity.
    processes: HashMap<String, Usage>,
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
            // The same lifetime identity used by selection and process rows.
            let (id, metric) = rest.rsplit_once('.')?;
            let usage = self.processes.get(id)?;
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
            (CPU_TEMPERATURE_SERIES.into(), cpu_temperature(hardware)),
            ("totals.disk".into(), disk_rate(values(&hardware["disks"]))),
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
        // while only an unknown reading breaks a curve.
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
    /// Records every process and app, dropping exited ones.
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
            if p["pid"].as_u64().is_none() {
                continue;
            }
            let usage = self.processes.entry(identity(p)).or_default();
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
            let reading = |key: &str| {
                if boolean(p, "stale")
                    || boolean(p, "sampleReused")
                    || boolean(p, "countersPending")
                {
                    f64::NAN
                } else {
                    p[key].as_f64().unwrap_or(f64::NAN)
                }
            };
            usage.push(reading("cpu"), reading("privateWs"), keep);
        }
        self.processes.retain(|_, usage| usage.tick == tick);
        // Missing process readings also leave a gap in their app's aggregate.
        let latest = |values: &VecDeque<f64>| values.back().copied().unwrap_or(f64::NAN);
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

    #[test]
    fn executable_names_match_case_variants_in_non_ascii_names() {
        for (left, right) in [
            ("EDITOR.EXE", "editor.exe"),
            ("Редактор.exe", "РЕДАКТОР.EXE"),
            ("Éditeur.exe", "éditeur.EXE"),
        ] {
            assert!(same_executable_name(left, right), "{left} != {right}");
        }
        assert!(!same_executable_name("Редактор.exe", "Другой.exe"));
        assert!(!same_executable_name("editor.exe", "editor.exe.bak"));
    }

    #[test]
    fn name_and_image_columns_keep_their_roles_in_every_view() {
        let processes = vec![
            json!({"pid":1,"createTime":1,"name":"zeta.exe","description":"Alpha App","appWindow":true}),
            json!({"pid":2,"createTime":2,"name":"alpha.exe","description":"Zulu App","appWindow":true}),
        ];
        for mode in ["apps", "list", "tree"] {
            let rows = rows_of(&processes, mode, "", "name", false, &HashMap::new());
            assert_eq!(pids(&rows), [1, 2]);
            let first = rows.iter().find(|row| row.section.is_none()).unwrap();
            assert_eq!(first.title, "Alpha App");
            assert_eq!(first.detail, "zeta.exe");
            assert_eq!(first.column_text("imageName"), Some("zeta.exe"));
        }
        let unknown = rows_of(
            &[json!({"pid":3,"createTime":1,"name":"unknown.exe"})],
            "list",
            "",
            "cpu",
            false,
            &HashMap::new(),
        );
        assert_eq!(unknown[0].title, "unknown.exe");
        assert!(unknown[0].detail.is_empty());
    }

    #[test]
    fn optional_process_columns_keep_instance_identity_and_group_resource_totals() {
        let processes = vec![
            json!({"pid":1,"ppid":9,"sessionId":1,"basePriority":8,"createTime":100,"name":"sample.exe","path":"C:/sample.exe","company":"Example","workingSet":1000,"cpuUserTime":200,"ioReadTotal":400,"virtualSize":2_199_023_255_552u64,"peakWorkingSet":3000,"gpuDedicated":0,"gpuShared":null}),
            json!({"pid":2,"ppid":9,"sessionId":1,"basePriority":8,"createTime":200,"name":"sample.exe","path":"C:/sample.exe","company":"Example","workingSet":2000,"cpuUserTime":300,"ioReadTotal":600,"virtualSize":2_199_023_255_552u64,"peakWorkingSet":4000,"gpuDedicated":4096,"gpuShared":2048}),
        ];
        let rows = rows_of(&processes, "apps", "", "workingSet", true, &HashMap::new());
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.metric("workingSet"), Some(3000.));
        assert_eq!(row.metric("cpuUserTime"), Some(500.));
        assert_eq!(row.metric("ioReadTotal"), Some(1000.));
        assert_eq!(row.metric("gpuDedicated"), Some(4096.));
        assert_eq!(row.metric("gpuShared"), None);
        for key in [
            "pid",
            "ppid",
            "sessionId",
            "createTime",
            "basePriority",
            "virtualSize",
            "peakWorkingSet",
        ] {
            assert_eq!(row.metric(key), None);
        }
        assert_eq!(row.column_text("path"), Some("C:/sample.exe"));
        assert_eq!(row.column_text("company"), Some("Example"));
        let process = Row::process(processes[0].clone(), 0);
        assert_eq!(process.metric("ppid"), Some(9.));
        assert_eq!(process.metric("basePriority"), Some(8.));
        assert_eq!(process.metric("virtualSize"), Some(2_199_023_255_552.));
        assert_eq!(process.metric("peakWorkingSet"), Some(3000.));
        assert_eq!(process.metric("gpuDedicated"), Some(0.));
        assert_eq!(process.column_text("imageName"), Some("sample.exe"));
    }

    #[test]
    fn extra_text_and_numeric_columns_sort_with_missing_values_last() {
        let processes = vec![
            json!({"pid":1,"createTime":1,"name":"one.exe","company":"Team10","workingSet":40}),
            json!({"pid":2,"createTime":2,"name":"two.exe","company":"Team2","workingSet":20}),
            json!({"pid":3,"createTime":3,"name":"three.exe"}),
        ];
        assert_eq!(
            pids(&rows_of(
                &processes,
                "list",
                "",
                "company",
                false,
                &HashMap::new()
            )),
            [2, 1, 3]
        );
        assert_eq!(
            pids(&rows_of(
                &processes,
                "list",
                "",
                "company",
                true,
                &HashMap::new()
            )),
            [1, 2, 3]
        );
        assert_eq!(
            pids(&rows_of(
                &processes,
                "list",
                "",
                "workingSet",
                false,
                &HashMap::new()
            )),
            [2, 1, 3]
        );
        assert_eq!(
            pids(&rows_of(
                &processes,
                "list",
                "",
                "workingSet",
                true,
                &HashMap::new()
            )),
            [1, 2, 3]
        );
    }
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
                tree_expanded: &empty,
                sections: &empty,
                hosted,
                visibility: None,
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
    fn visibility_filters_each_view_before_grouping_and_keeps_user_children() {
        let processes = vec![
            json!({"pid":10,"ppid":0,"createTime":1,"name":"svchost.exe","windowsProcess":true,"cpu":50}),
            json!({"pid":20,"ppid":10,"createTime":2,"name":"worker.exe","cpu":3}),
            json!({"pid":30,"ppid":10,"createTime":3,"name":"cmd.exe","windowsProcess":true,"cpu":2}),
            json!({"pid":40,"ppid":0,"createTime":1,"name":"svchost.exe","windowsProcess":false,"cpu":1}),
        ];
        let rules = crate::frontend::visibility::Rules::default();
        let empty = HashSet::new();
        let hosted = HashMap::new();
        for mode in ["apps", "tree", "list"] {
            for query in ["", "svchost"] {
                let rows = process_rows(
                    &processes,
                    ProcessOptions {
                        mode,
                        query,
                        sort: "pid",
                        descending: false,
                        expanded: &empty,
                        tree_expanded: &empty,
                        sections: &empty,
                        hosted: &hosted,
                        visibility: Some(&rules),
                    },
                );
                assert_eq!(
                    pids(&rows),
                    if query.is_empty() {
                        vec![20, 30, 40]
                    } else {
                        vec![40]
                    }
                );
                assert!(rows.iter().all(|row| row.depth == 0));
            }
        }
        assert_eq!(processes.len(), 4);
        assert_eq!(
            pids(&rows_of(&processes, "list", "", "pid", false, &hosted)),
            [10, 20, 30, 40]
        );
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
    fn views_title_processes_by_identity_and_role() {
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
                ("App", "app.exe")
            );
            assert_eq!(rows[1].title, apps[1].title);
            assert_eq!(rows[1].detail, apps[1].detail);
        }
    }
    #[test]
    fn the_tree_starts_folded_and_expands_only_requested_parents() {
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
        assert_eq!(pids(&rows), [5, 4, 1]);
        assert!(rows[2].expandable && !rows[2].expanded);
        let empty = HashSet::new();
        let parents = tree_parents(&processes);
        assert_eq!(
            parents,
            HashSet::from([identity(&processes[0]), identity(&processes[1])])
        );
        let expanded = |opened: &HashSet<String>| {
            process_rows(
                &processes,
                ProcessOptions {
                    mode: "tree",
                    query: "",
                    sort: "name",
                    descending: false,
                    expanded: &empty,
                    tree_expanded: opened,
                    sections: &empty,
                    hosted: &none,
                    visibility: None,
                },
            )
        };
        assert_eq!(
            pids(&expanded(&HashSet::from([identity(&processes[0])]))),
            [5, 4, 1, 2]
        );
        let rows = expanded(&parents);
        assert_eq!(pids(&rows), [5, 4, 1, 2, 3]);
        assert_eq!(
            rows.iter().map(|row| row.depth).collect::<Vec<_>>(),
            [0, 0, 0, 1, 2]
        );
        assert!(rows[2].expandable && rows[2].expanded);
        assert!(!rows[4].expandable && !rows[4].expanded);
        let found = rows_of(&processes, "tree", "leaf", "name", false, &none);
        assert_eq!(pids(&found), [1, 2, 3]);
        assert_eq!(
            pids(&rows_of(&processes, "tree", "", "name", false, &none)),
            [5, 4, 1]
        );
    }

    #[test]
    fn a_reused_pid_does_not_inherit_the_previous_tree_expansion() {
        let processes = vec![
            json!({"pid":1,"ppid":0,"createTime":99,"name":"root.exe"}),
            json!({"pid":2,"ppid":1,"createTime":100,"name":"child.exe"}),
        ];
        let empty = HashSet::new();
        let old = HashSet::from(["1:1".into()]);
        let rows = process_rows(
            &processes,
            ProcessOptions {
                mode: "tree",
                query: "",
                sort: "pid",
                descending: false,
                expanded: &empty,
                tree_expanded: &old,
                sections: &empty,
                hosted: &HashMap::new(),
                visibility: None,
            },
        );
        assert_eq!(pids(&rows), [1]);
        assert!(!rows[0].expanded);
    }

    #[test]
    fn unavailable_disk_counters_do_not_become_zero_or_a_complete_total() {
        let known = json!({"read":1024,"write":2048});
        let missing = json!({"read":null,"write":null});
        assert_eq!(super::disk_rate(&[]), None);
        assert_eq!(super::disk_rate(std::slice::from_ref(&known)), Some(3072.));
        assert_eq!(super::disk_rate(&[known, missing]), None);
        assert_eq!(super::disk_rate(&[json!({"read":0,"write":0})]), Some(0.));
    }

    #[test]
    fn parent_zero_keeps_roots_outside_the_idle_process() {
        let processes = vec![
            json!({"pid":0,"ppid":0,"createTime":0,"name":"idle"}),
            json!({"pid":4,"ppid":0,"createTime":1,"name":"system"}),
            json!({"pid":8,"ppid":4,"createTime":2,"name":"worker"}),
        ];
        let rows = rows_of(&processes, "tree", "", "pid", false, &HashMap::new());
        assert_eq!(pids(&rows), [0, 4]);
        assert!(!rows[0].expandable);
        assert!(rows[1].expandable);
        assert_eq!(tree_parents(&processes), HashSet::from(["4:1".into()]));
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
                tree_expanded: &empty,
                sections: &empty,
                hosted: &HashMap::new(),
                visibility: None,
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
                tree_expanded: &empty,
                sections: &empty,
                hosted: &HashMap::new(),
                visibility: None,
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
        assert_eq!(group.column_text("gpuEngine"), Some("GPU 0 - 3D"));
        // The heading folds the group in by engine: 40% + 30% on one 3D node.
        assert_eq!(rows[0].section.as_deref(), Some("background"));
        assert_eq!(rows[0].metric("gpu"), Some(70.));
        // Rows labelled with one busy engine add up on it, as the interface's heading does.
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
                tree_expanded: &empty,
                sections: &empty,
                hosted: &HashMap::new(),
                visibility: None,
            },
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].members.len(), 2);
        assert_eq!(rows[1].raw["pid"], 42);
    }
    #[test]
    fn identity_only_rows_keep_selection_and_history_when_counters_arrive() {
        let pending = json!({"pid":42,"ppid":5,"createTime":-1,"sequenceNumber":100,"instanceKey":"42:seq:100","name":"new.exe","cpu":0,"privateWs":0,"ioRead":0,"countersPending":true});
        let mut ready = pending.clone();
        ready["createTime"] = json!(1234);
        ready["cpu"] = json!(30);
        ready["privateWs"] = json!(4096);
        ready["countersPending"] = json!(false);
        let row = Row::process(pending.clone(), 0);
        assert_eq!(row.metric("pid"), Some(42.));
        assert_eq!(row.metric("cpu"), None);
        assert_eq!(row.metric("mem"), None);
        assert_eq!(row.metric("io"), None);
        assert_eq!(row.metric("createTime"), None);
        assert_eq!(identity(&pending), identity(&ready));
        let mut timeline = Timeline::new();
        timeline.push(&json!({"timestamp":0,"processes":[pending]}), 1000.);
        timeline.push(&json!({"timestamp":1000,"processes":[ready]}), 1000.);
        assert_eq!(
            timeline.points("process.42:seq:100.cpu").unwrap(),
            vec![(0., None), (1000., Some(30.))]
        );
        let parent = json!({"pid":5,"sequenceNumber":90,"createTime":-1});
        let child = json!({"pid":42,"sequenceNumber":100,"createTime":-1});
        assert!(parent_predates(&parent, &child));
        assert!(!parent_predates(&child, &parent));
    }

    #[test]
    fn a_process_query_outage_keeps_the_last_numeric_order() {
        let mut processes = vec![
            json!({"pid":1,"createTime":1,"name":"z.exe","cpu":80,"privateWs":2000}),
            json!({"pid":2,"createTime":1,"name":"a.exe","cpu":10,"privateWs":1000}),
        ];
        let before = rows_of(&processes, "list", "", "cpu", true, &HashMap::new());
        for process in &mut processes {
            process["stale"] = json!(true);
        }
        let after = rows_of(&processes, "list", "", "cpu", true, &HashMap::new());
        assert_eq!(pids(&before), [1, 2]);
        assert_eq!(pids(&after), pids(&before));
        assert_eq!(
            after
                .iter()
                .map(|row| row.metric("cpu"))
                .collect::<Vec<_>>(),
            [Some(80.), Some(10.)]
        );
    }

    #[test]
    fn pending_refresh_keeps_values_visible_without_repeating_the_graph() {
        let mut timeline = Timeline::new();
        for (index, reused) in [false, true, false].into_iter().enumerate() {
            let process = json!({"pid":1,"createTime":1,"name":"app.exe","path":"C:/app.exe","cpu":30,"privateWs":1000,"stale":false,"sampleReused":reused});
            let row = Row::process(process.clone(), 0);
            assert_eq!(row.metric("cpu"), Some(30.));
            assert_eq!(row.metric("mem"), Some(1000.));
            timeline.push(
                &json!({"timestamp":index*1000,"system":{"cpu":12},"processes":[process]}),
                1000.,
            );
        }
        for key in ["process.1:1.cpu", "group.g:image:c:\\app.exe.cpu"] {
            assert_eq!(
                timeline.points(key).unwrap(),
                vec![(0., Some(30.)), (1000., None), (2000., Some(30.))]
            );
        }
    }

    #[test]
    fn stale_process_readings_keep_identity_and_independent_counters() {
        let processes = vec![
            json!({"pid":1,"ppid":9,"createTime":1,"name":"app.exe","path":"C:/app.exe","cpu":30,"privateWs":1000,"ioRead":100,"gpu":20,"netReceived":1000,"stale":true}),
            json!({"pid":2,"ppid":9,"createTime":2,"name":"app.exe","path":"C:/app.exe","cpu":10,"privateWs":500,"ioRead":200,"gpu":5,"netReceived":500}),
        ];
        let row = Row::process(processes[0].clone(), 0);
        assert_eq!(row.metric("pid"), Some(1.));
        assert_eq!(row.metric("cpu"), Some(30.));
        assert_eq!(row.metric("mem"), Some(1000.));
        assert_eq!(row.metric("io"), Some(100.));
        assert!(row.native_pending("cpu"));
        assert!(!row.native_pending("pid"));
        assert!(!row.native_pending("gpu"));
        assert!(!row.native_pending("net"));
        assert_eq!(row.metric("diskRead"), None);
        assert_eq!(row.metric("gpu"), Some(20.));
        assert_eq!(row.metric("net"), Some(1000.));
        let rows = rows_of(&processes, "apps", "", "cpu", true, &HashMap::new());
        assert_eq!(rows[0].metric("cpu"), Some(40.));
        assert_eq!(rows[0].metric("mem"), Some(1500.));
        assert_eq!(rows[0].metric("net"), Some(1500.));

        let mut timeline = Timeline::new();
        for (index, stale) in [false, true, false].into_iter().enumerate() {
            let mut process = processes[0].clone();
            process["stale"] = json!(stale);
            timeline.push(
                &json!({"timestamp":index*1000,"system":{"cpu":12},"processes":[process]}),
                1000.,
            );
        }
        for key in ["process.1:1.cpu", "group.g:image:c:\\app.exe.cpu"] {
            let points = timeline.points(key).unwrap();
            assert_eq!(
                points,
                vec![(0., Some(30.)), (1000., None), (2000., Some(30.))]
            );
        }
        assert!(
            timeline
                .points("system.cpu")
                .unwrap()
                .iter()
                .all(|(_, value)| *value == Some(12.))
        );
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
            // them, across longer intervals (a minimized window samples less often).
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

    #[test]
    fn cpu_temperature_history_survives_source_and_hottest_core_changes() {
        let mut timeline = Timeline::new();
        let readings = [
            json!({"cpuSensors":{"packageCelsius":68},"thermal":[{"celsius":55}]}),
            json!({"cpuSensors":{"state":"pending"},"thermal":[{"celsius":56}]}),
            json!({"cpuSensors":{"coreTemperatures":[{"celsius":67},{"celsius":65}]}}),
            json!({"cpuSensors":{"coreTemperatures":[{"celsius":66},{"celsius":71}]}}),
            json!({"cpuSensors":{"packageCelsius":69}}),
            json!({"cpuSensors":{"state":"noData"},"thermal":[]}),
        ];
        for (index, hardware) in readings.into_iter().enumerate() {
            timeline.push(&json!({"timestamp":index*1000,"hardware":hardware}), 1000.);
        }
        let points = timeline.points(CPU_TEMPERATURE_SERIES).unwrap();
        assert_eq!(
            points,
            vec![
                (0., Some(68.)),
                (1000., Some(56.)),
                (2000., Some(67.)),
                (3000., Some(71.)),
                (4000., Some(69.)),
                (5000., None),
            ]
        );
    }
}
