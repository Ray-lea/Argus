//! The selected process's facts: what the details pane shows, and what
//! "Copy details" (Ctrl+Shift+C) puts on the clipboard as text. Both read
//! the same list, so the copy never leaves out a fact the pane shows.
use super::*;

/// How the pane shows a fact's text.
pub(super) enum Look {
    Plain,
    /// Text that can be selected, by its element's id.
    Selectable(&'static str),
    /// Quiet text, such as a reading Argus cannot take.
    Faint,
    /// Text with a hint, by its element's id.
    Hinted(String, String),
    /// Followed by a quiet part, which copies along.
    Quiet(String),
    /// A link that selects the process row with this key.
    Link(String),
}

pub(super) struct Fact {
    pub label: String,
    pub text: String,
    pub look: Look,
}

impl Fact {
    fn new(label: String, text: String, look: Look) -> Self {
        Self { label, text, look }
    }
    fn plain(label: String, text: String) -> Self {
        Self::new(label, text, Look::Plain)
    }
    fn selectable(id: &'static str, label: String, text: String) -> Self {
        Self::new(label, text, Look::Selectable(id))
    }
    /// The text as it copies: a quiet part follows the reading.
    fn copied(&self) -> String {
        match &self.look {
            Look::Quiet(quiet) if quiet.starts_with([' ', '（', '(']) => {
                format!("{}{quiet}", self.text)
            }
            Look::Quiet(quiet) => format!("{} {quiet}", self.text),
            _ => self.text.clone(),
        }
    }
}

/// What the pane lists about the selected row.
pub(super) struct Sheet {
    /// A single process's identity and state; none for an app group.
    pub process: Vec<Fact>,
    pub resources: Vec<Fact>,
    /// Counts since the process started.
    pub totals: Vec<Fact>,
    pub paths: Vec<String>,
    pub command_line: Option<String>,
    pub version: Vec<Fact>,
}

fn column(key: &str) -> String {
    label(&format!("processes.column.{key}.label"))
}

/// Text in sections, as "Copy details" puts it: the heading lines, then
/// each section that has lines under its own heading, a blank line apart.
pub(super) fn sheet_text(head: &[String], sections: &[(String, Vec<String>)]) -> String {
    let mut text = head
        .iter()
        .filter(|line| !line.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    for (heading, lines) in sections {
        if lines.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(heading);
        for line in lines {
            text.push('\n');
            text.push_str(line);
        }
    }
    text
}

impl Desktop {
    pub(super) fn details_sheet(&self, picked: &Picked<'_>, info: Option<&Value>) -> Sheet {
        let Some(process) = picked.process else {
            return Sheet {
                process: Vec::new(),
                resources: Vec::new(),
                totals: Vec::new(),
                paths: Vec::new(),
                command_line: None,
                version: Vec::new(),
            };
        };
        let mut resources = self.details_resource_facts(&picked.members, picked.group);
        let mut paths: Vec<String> = Vec::new();
        for path in picked
            .members
            .iter()
            .filter_map(|m| m["path"].as_str())
            .filter(|path| !path.is_empty())
        {
            if !paths.iter().any(|known| known == path) {
                paths.push(path.to_owned());
            }
        }
        let process_facts = if picked.group {
            let earliest = picked
                .members
                .iter()
                .filter_map(|m| model::process_metric(m, "createTime"))
                .reduce(f64::min);
            resources.push(Fact::plain(
                label("details.group.earliest"),
                earliest
                    .map(date_time)
                    .unwrap_or_else(|| label("format.missing")),
            ));
            Vec::new()
        } else {
            self.details_process_fact_list(process, info)
        };
        // The snapshot reads the command line and file version itself; the
        // details query adds what it could not, and the product name.
        let command_line = (!picked.group)
            .then(|| {
                info.and_then(|info| info["commandLine"].as_str())
                    .or_else(|| process["commandLine"].as_str())
            })
            .flatten()
            .filter(|command| !command.is_empty())
            .map(str::to_owned);
        let version = info.map(|info| &info["version"]);
        let product = version
            .and_then(|v| v["productName"].as_str())
            .filter(|s| !s.is_empty());
        let file_version = version
            .and_then(|v| v["fileVersion"].as_str())
            .or_else(|| process["version"].as_str())
            .filter(|s| !s.is_empty());
        let mut version = Vec::new();
        if let Some(product) = product {
            version.push(Fact::selectable(
                "details-product",
                label("details.product"),
                product.to_owned(),
            ));
        }
        if let Some(file_version) = file_version {
            version.push(Fact::selectable(
                "details-file-version",
                label("details.version"),
                file_version.to_owned(),
            ));
        }
        Sheet {
            process: process_facts,
            resources,
            totals: self.details_total_facts(&picked.members),
            paths,
            command_line,
            version,
        }
    }

    fn details_process_fact_list(&self, process: &Value, info: Option<&Value>) -> Vec<Fact> {
        let missing = || label("format.missing");
        let parent = values(&self.snapshot["processes"])
            .iter()
            .find(|candidate| {
                !process["ppid"].is_null()
                    && candidate["pid"] == process["ppid"]
                    && model::parent_predates(candidate, process)
            });
        let parent = match parent {
            Some(parent) => Fact::new(
                label("details.parent"),
                tr!(
                    "details.parentLink",
                    name = text(parent, "name"),
                    pid = parent["pid"]
                ),
                Look::Link(model::identity(parent)),
            ),
            None => Fact::new(
                label("details.parent"),
                tr!("details.noParent", pid = process["ppid"]),
                Look::Faint,
            ),
        };
        let localized = |field: &str, namespace: &str| {
            info.and_then(|info| info[field].as_str())
                .map(|value| label(&format!("{namespace}.{value}")))
                .unwrap_or_else(missing)
        };
        // The snapshot reads the architecture itself; the details query
        // stands in until it has.
        let architecture = process["architecture"]
            .as_str()
            .or_else(|| info.and_then(|info| info["architecture"].as_str()))
            .map(|value| label(&format!("details.arch.{value}")))
            .unwrap_or_else(missing);
        let priority = tables::base_priority(process).unwrap_or_else(missing);
        // I/O and memory priority as the menus name them.
        let level = |key: &str| {
            process[key]
                .as_str()
                .map(|value| label(&format!("processes.{key}.{value}")))
                .unwrap_or_else(missing)
        };
        let plain = |value: &Value| {
            value
                .as_u64()
                .map(|v| v.to_string())
                .unwrap_or_else(missing)
        };
        let pending = boolean(process, "countersPending");
        let on_off = |key: &str, on: &'static str, off: &'static str| {
            if pending {
                missing()
            } else {
                label(if boolean(process, key) { on } else { off })
            }
        };
        vec![
            Fact::selectable(
                "details-pid",
                label("processes.column.pid.label"),
                plain(&process["pid"]),
            ),
            parent,
            Fact::selectable(
                "details-started",
                label("details.started"),
                model::process_metric(process, "createTime")
                    .map(date_time)
                    .unwrap_or_else(missing),
            ),
            Fact::selectable(
                "details-user",
                label("details.user"),
                info.and_then(|info| info["user"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(missing),
            ),
            Fact::selectable(
                "details-integrity",
                label("details.integrity"),
                localized("integrity", "details.integrityLevel"),
            ),
            Fact::selectable(
                "details-architecture",
                label("details.architecture"),
                architecture,
            ),
            Fact::selectable("details-priority", label("details.priority"), priority),
            Fact::selectable(
                "details-io-priority",
                column("ioPriority"),
                level("ioPriority"),
            ),
            Fact::selectable(
                "details-memory-priority",
                column("memoryPriority"),
                level("memoryPriority"),
            ),
            Fact::selectable(
                "details-efficiency",
                label("common.efficiencyMode"),
                on_off("efficiency", "common.on", "common.off"),
            ),
            Fact::selectable(
                "details-suspended",
                column("suspended"),
                on_off("suspended", "processes.columns.yes", "processes.columns.no"),
            ),
            Fact::selectable(
                "details-session",
                label("details.session"),
                if pending {
                    missing()
                } else {
                    plain(&process["sessionId"])
                },
            ),
        ]
    }

    /// Resource readings of one process, or of an app group's members added up.
    fn details_resource_facts(&self, members: &[&Value], group: bool) -> Vec<Fact> {
        let missing = || label("format.missing");
        let sum = |field: &str| members.iter().map(|m| number(m, field)).sum::<f64>();
        // Only measured members count; none measured is no reading at all.
        let measured = |field: &str| {
            members
                .iter()
                .filter_map(|m| model::process_metric(m, field))
                .reduce(|a, b| a + b)
        };
        let memory = |field: &str| measured(field).map(theme::memory).unwrap_or_else(missing);
        let mut facts = vec![
            Fact::plain(label("details.privateWs"), memory("privateWs")),
            Fact::plain(label("details.workingSet"), memory("workingSet")),
        ];
        // Peaks and address ranges are a single process's.
        if !group {
            facts.push(Fact::plain(
                column("peakWorkingSet"),
                memory("peakWorkingSet"),
            ));
        }
        facts.push(Fact::plain(label("details.commit"), memory("commit")));
        if !group {
            facts.push(Fact::plain(
                column("virtualSize"),
                measured("virtualSize")
                    .map(|value| theme::bytes(value, false))
                    .unwrap_or_else(missing),
            ));
        }
        for field in ["diskRead", "diskWrite"] {
            let (value, tip) = match measured(field) {
                Some(rate) => (
                    byte_rate(rate),
                    label(&format!("processes.column.{field}.title")),
                ),
                None => (missing(), self.missing_disk_hint()),
            };
            facts.push(Fact::new(
                column(field),
                value,
                Look::Hinted(format!("details-{field}"), tip),
            ));
        }
        let io = ["ioRead", "ioWrite", "ioOther"]
            .into_iter()
            .filter_map(&measured)
            .reduce(|a, b| a + b);
        facts.push(Fact::new(
            column("io"),
            io.map(byte_rate).unwrap_or_else(missing),
            Look::Hinted("details-io".into(), label("processes.column.io.title")),
        ));
        if !values(&self.snapshot["hardware"]["gpus"]).is_empty() {
            let busiest = members
                .iter()
                .copied()
                .max_by(|a, b| number(a, "gpu").total_cmp(&number(b, "gpu")));
            let engine = busiest
                .filter(|b| !group || number(b, "gpu") > 0.)
                .and_then(|b| b["gpuEngine"].as_str())
                .filter(|engine| !engine.is_empty());
            let usage = format!("{}%", theme::cpu(sum("gpu").min(100.)));
            facts.push(match engine {
                Some(engine) => Fact::new(
                    label("common.resource.gpu"),
                    usage,
                    Look::Quiet(tr!("details.gpuEngine", engine = engine)),
                ),
                None => Fact::plain(label("common.resource.gpu"), usage),
            });
            for (key, field) in [
                ("details.gpuDedicated", "gpuDedicated"),
                ("details.gpuShared", "gpuShared"),
            ] {
                facts.push(Fact::plain(
                    label(key),
                    members
                        .iter()
                        .try_fold(0., |sum, member| {
                            member[field].as_f64().map(|value| sum + value)
                        })
                        .map(theme::memory)
                        .unwrap_or_else(missing),
                ));
            }
        }
        let network = boolean(&self.snapshot, "processNetwork");
        facts.push(if network && measured("netSent").is_none() {
            Fact::plain(label("common.resource.network"), missing())
        } else if network {
            Fact::plain(
                label("common.resource.network"),
                tr!(
                    "processes.netRates",
                    sent = self.network_rate(sum("netSent")),
                    received = self.network_rate(sum("netReceived"))
                ),
            )
        } else {
            Fact::new(
                label("common.resource.network"),
                label("details.netNeedsAdmin"),
                Look::Faint,
            )
        });
        // A group's level is that of its members' estimates added up, as in the table.
        let power = measured("power").map(columns::power_label);
        let trend = measured("powerTrend").map(columns::power_label);
        facts.push(Fact::plain(
            column("power"),
            match (power, trend) {
                (Some(level), Some(trend)) => {
                    tr!("details.powerTrend", level = level, trend = trend)
                }
                (Some(level), None) => level,
                (None, _) => missing(),
            },
        ));
        facts.push(Fact::plain(
            label("details.threads"),
            measured("threads")
                .map(theme::count)
                .unwrap_or_else(missing),
        ));
        facts.push(Fact::plain(
            label("details.handles"),
            measured("handles")
                .map(theme::count)
                .unwrap_or_else(missing),
        ));
        facts
    }

    /// Counts since the start: processor time, I/O transferred and page
    /// faults, added up over an app group's members.
    fn details_total_facts(&self, members: &[&Value]) -> Vec<Fact> {
        let measured = |field: &str| {
            members
                .iter()
                .filter_map(|m| model::process_metric(m, field))
                .reduce(|a, b| a + b)
        };
        let reading = |id: &'static str, name: String, field: &str, shown: fn(f64) -> String| {
            Fact::selectable(
                id,
                name,
                measured(field)
                    .map(shown)
                    .unwrap_or_else(|| label("format.missing")),
            )
        };
        let transferred = |value: f64| theme::bytes(value, false);
        vec![
            reading(
                "details-cpu-time",
                label("details.cpuTime"),
                "cpuTime",
                cpu_time,
            ),
            reading(
                "details-cpu-user",
                column("cpuUserTime"),
                "cpuUserTime",
                cpu_time,
            ),
            reading(
                "details-cpu-kernel",
                column("cpuKernelTime"),
                "cpuKernelTime",
                cpu_time,
            ),
            reading(
                "details-io-read",
                column("ioRead"),
                "ioReadTotal",
                transferred,
            ),
            reading(
                "details-io-write",
                column("ioWrite"),
                "ioWriteTotal",
                transferred,
            ),
            reading(
                "details-io-other",
                column("ioOther"),
                "ioOtherTotal",
                transferred,
            ),
            reading(
                "details-page-faults",
                column("pageFaults"),
                "pageFaults",
                theme::count,
            ),
        ]
    }

    /// The row's details as text, as the pane lists them ("Copy details").
    pub(in crate::frontend) fn details_text(&self, key: &str) -> Option<String> {
        let picked = self.details_pick(key)?;
        let process = picked.process?;
        let info = self
            .data
            .get(&format!("details:{key}"))
            .filter(|info| info.is_object());
        let sheet = self.details_sheet(&picked, info);
        let missing = label("format.missing");
        let lines = |facts: &[Fact]| {
            facts
                .iter()
                .filter(|fact| fact.text != missing)
                .map(|fact| {
                    tr!(
                        "details.copy.field",
                        name = &fact.label,
                        value = fact.copied()
                    )
                })
                .collect::<Vec<_>>()
        };
        let heading = |name: String| tr!("details.copy.heading", name = name);
        let name = text(process, "name").to_owned();
        let subtitle = if picked.group {
            joined([
                name,
                tr!("details.group.count", count = picked.members.len()),
            ])
        } else {
            name
        };
        let company = process["company"].as_str().unwrap_or_default().to_owned();
        let head = [model::title(process), joined([subtitle, company])];
        let mut sections = Vec::new();
        sections.push((
            heading(label("details.section.process")),
            lines(&sheet.process),
        ));
        sections.push((
            heading(label("details.section.resources")),
            lines(&sheet.resources),
        ));
        sections.push((
            heading(label("details.section.totals")),
            lines(&sheet.totals),
        ));
        let hosted = self.hosted_service_list(&picked.members);
        sections.push((
            heading(tr!("details.services", count = hosted.len())),
            hosted
                .iter()
                .map(|service| {
                    joined([
                        text(service, "displayName").to_owned(),
                        text(service, "name").to_owned(),
                    ])
                })
                .collect(),
        ));
        if picked.group {
            let mut members = picked.members.clone();
            members.sort_by(|a, b| number(a, "pid").total_cmp(&number(b, "pid")));
            sections.push((
                heading(tr!("details.group.members", count = members.len())),
                members
                    .iter()
                    .map(|member| {
                        joined([
                            format!("{} {}", label("processes.column.pid.label"), member["pid"]),
                            model::process_metric(member, "cpu")
                                .map(|v| format!("{}%", theme::cpu(v)))
                                .unwrap_or_default(),
                            model::process_metric(member, "privateWs")
                                .map(theme::memory)
                                .unwrap_or_default(),
                        ])
                    })
                    .collect(),
            ));
        }
        sections.push((heading(label("details.section.location")), sheet.paths));
        sections.push((
            heading(label("details.section.commandLine")),
            sheet.command_line.into_iter().collect(),
        ));
        if !picked.group {
            let open_files = self
                .data
                .get(&format!("open-files:{key}"))
                .and_then(|report| report["files"].as_array())
                .map(|files| {
                    files
                        .iter()
                        .filter_map(|file| file["path"].as_str())
                        .filter(|path| !path.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            sections.push((heading(label("details.section.openedFiles")), open_files));
        }
        sections.push((
            heading(label("details.section.version")),
            lines(&sheet.version),
        ));
        Some(sheet_text(&head, &sections))
    }

    /// The services hosted by these processes.
    pub(super) fn hosted_service_list(&self, members: &[&Value]) -> Vec<&Value> {
        let pids: HashSet<u64> = members.iter().filter_map(|m| m["pid"].as_u64()).collect();
        self.data
            .get("services")
            .map(values)
            .unwrap_or_default()
            .iter()
            .filter(|s| s["pid"].as_u64().is_some_and(|pid| pids.contains(&pid)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::sheet_text;

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn sections_follow_the_heading_a_blank_line_apart() {
        let text = sheet_text(
            &lines(&["Title", "title.exe"]),
            &[
                ("[A]".into(), lines(&["a: 1", "b: 2"])),
                ("[Empty]".into(), Vec::new()),
                ("[B]".into(), lines(&["c"])),
            ],
        );
        assert_eq!(text, "Title\ntitle.exe\n\n[A]\na: 1\nb: 2\n\n[B]\nc");
    }

    #[test]
    fn empty_heading_lines_are_left_out() {
        let text = sheet_text(&lines(&["Title", ""]), &[("[A]".into(), lines(&["a"]))]);
        assert_eq!(text, "Title\n\n[A]\na");
        assert_eq!(sheet_text(&[], &[("[A]".into(), lines(&["a"]))]), "[A]\na");
    }
}
