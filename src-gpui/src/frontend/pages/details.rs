//! The process details pane: the selected row looked up in the snapshot, or this computer's parts.
use super::*;
use gpui_kit::base::SelectableText;
use gpui_kit::component::scroll::{ScrollableElement, ScrollbarAxis};
use gpui_kit::component::tooltip::Tooltip;
use std::hash::{Hash, Hasher};

/// The selected row resolved against the latest processes, so filtering or
/// folding the table never makes a running process look exited.
struct Picked<'a> {
    process: Option<&'a Value>,
    members: Vec<&'a Value>,
    group: bool,
}

/// One computer part in the overview, in the performance page's order.
struct Part {
    device: String,
    glyph: &'static str,
    color: Hsla,
    kind: String,
    name: String,
    detail: String,
}

const KB: f64 = 1024.;
const MB: f64 = KB * 1024.;
const GB: f64 = MB * 1024.;
/// Width of the box around each fact name (styles.css `.facts`).
const FACT_NAME_WIDTH: f32 = 84.;
/// Room kept for the vertical scrollbar, as `scrollbar-gutter: stable` does.
const SCROLLBAR_GUTTER: f32 = 10.;
/// Pane width change per arrow key.
const KEY_STEP: f32 = 16.;
/// Largest share of the window the pane takes.
const WINDOW_SHARE: f32 = 0.45;

impl Desktop {
    pub(in crate::frontend) const DETAILS_MIN_WIDTH: f32 = 300.;
    pub(in crate::frontend) const DETAILS_MAX_WIDTH: f32 = 560.;

    /// A saved or dragged width within the pane's limits.
    pub(in crate::frontend) fn clamp_details_width(width: f32, window: &Window) -> f32 {
        let width = if width.is_finite() {
            width.round()
        } else {
            f32::from(theme::dimension("detailsWidth"))
        };
        width
            .clamp(Self::DETAILS_MIN_WIDTH, Self::DETAILS_MAX_WIDTH)
            .min(f32::from(window.viewport_size().width) * WINDOW_SHARE)
    }

    /// The width the pane occupies: never more than its share of a narrow window.
    pub(in crate::frontend) fn details_display_width(&self, window: &Window) -> f32 {
        self.details_width
            .min(f32::from(window.viewport_size().width) * WINDOW_SHARE)
            .max(1.)
    }

    fn details_pick(&self, key: &str) -> Option<Picked<'_>> {
        // A category heading selected with the keyboard is no process.
        if key.is_empty() || key.starts_with("s:") || self.snapshot.is_null() {
            return None;
        }
        // The visible rows already hold their members; anything filtered out
        // or folded away is found in the snapshot instead.
        if let Some(row) = self
            .rows
            .iter()
            .find(|row| row.section.is_none() && row.key == key)
        {
            let group = key.starts_with("g:");
            return Some(Picked {
                process: if group {
                    row.members.first()
                } else {
                    Some(&row.raw)
                },
                members: row.members.iter().collect(),
                group,
            });
        }
        let processes = values(&self.snapshot["processes"]);
        if key.starts_with("g:") {
            let mut members: Vec<&Value> = processes
                .iter()
                .filter(|p| model::application_key(p) == key)
                .collect();
            members.sort_by(|a, b| number(a, "createTime").total_cmp(&number(b, "createTime")));
            return Some(Picked {
                process: members.first().copied(),
                members,
                group: true,
            });
        }
        let pid = key
            .split_once(':')
            .and_then(|(pid, _)| pid.parse::<u64>().ok());
        let process = pid.and_then(|pid| {
            processes
                .iter()
                .find(|p| p["pid"].as_u64() == Some(pid) && model::identity(p) == key)
        });
        Some(Picked {
            process,
            members: process.into_iter().collect(),
            group: false,
        })
    }

    /// Starts following a newly selected row: its history and its slow details.
    pub(in crate::frontend) fn details_track(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picked) = self.details_pick(key) else {
            return;
        };
        let prefix = if picked.group { "group" } else { "process" };
        let process = picked
            .process
            .map(|p| json!({"pid":p["pid"],"path":p["path"]}));
        for metric in ["cpu", "mem"] {
            self.timeline
                .samples
                .entry(format!("{prefix}.{key}.{metric}"))
                .or_default();
        }
        if let Some(args) = process {
            self.request(
                "process_details",
                args,
                Some(&format!("details:{key}")),
                window,
                cx,
            );
        }
    }

    pub(in crate::frontend) fn details_view(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let key = self.selected.get("processes").cloned().unwrap_or_default();
        let picked = self.details_pick(&key);
        let scroll = window
            .use_keyed_state("details-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let (body, actions, motion) = match picked {
            Some(picked) if picked.process.is_some() => {
                let actions = self.details_actions(&picked, cx);
                (
                    self.details_body(&key, &picked, window, cx),
                    Some(actions),
                    duration_of("details"),
                )
            }
            Some(_) => (self.details_exited(cx), None, motion_duration()),
            None => (self.overview_view(cx), None, motion_duration()),
        };
        let content = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("details-scroll")
                    .track_scroll(&scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pt(px(14.))
                    .pb(px(16.))
                    .pl(px(16.))
                    .pr(px(16. + SCROLLBAR_GUTTER))
                    .child(body)
                    .scrollbar(&scroll, ScrollbarAxis::Vertical),
            )
            .children(actions);
        // Another selection settles in where the last one was (DetailsPanel.tsx).
        let content = if cx.reduce_motion() {
            content.into_any_element()
        } else {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut hasher);
            content
                .with_animation(
                    ElementId::NamedInteger("details-content".into(), hasher.finish()),
                    Animation::new(motion).with_easing(ease_out()),
                    |element, progress| element.opacity(progress).top(px((1. - progress) * 6.)),
                )
                .into_any_element()
        };
        let panel = div()
            .id("process-details")
            .relative()
            .w(px(self.details_display_width(window)))
            .h_full()
            .flex()
            .flex_col()
            // The workspace card already paints the layer; the pane only adds
            // its separator (styles.css `.details`).
            .border_l_1()
            .border_color(p.stroke)
            .line_height(relative(1.4))
            .aria_label(label("shell.details.label"))
            .child(content)
            .child(self.details_splitter(window, cx));
        if cx.reduce_motion() {
            return panel.into_any_element();
        }
        // Opening the pane slides it in from the right (styles.css `panel-in`);
        // the animation restarts whenever the pane was not drawn the frame before.
        panel
            .with_animation(
                "details-panel-in",
                Animation::new(duration_of("t-slow")).with_easing(ease_out()),
                |element, progress| element.opacity(progress).left(px((1. - progress) * 24.)),
            )
            .into_any_element()
    }

    /// The handle on the pane's left edge: drag it, use the arrow keys, Home or
    /// End, or double-click to go back to the default (Splitter.tsx).
    fn details_splitter(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let focus = window
            .use_keyed_state("details-splitter-focus", cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        let dragging = self
            .drag
            .as_ref()
            .is_some_and(|(kind, _, _)| kind == "details");
        let shown = dragging || focus.is_focused(window);
        let grip = div()
            .absolute()
            .top(relative(0.5))
            .mt(px(-16.))
            .left_0()
            .w(px(2.))
            .h(px(32.))
            .rounded(px(1.))
            .bg(if dragging {
                p.accent
            } else {
                design_color("stroke-strong", cx)
            })
            .when(!shown, |grip| {
                grip.opacity(0.)
                    .group_hover("details-splitter", |style| style.opacity(1.))
            });
        div()
            .id("details-splitter")
            .group("details-splitter")
            .absolute()
            .top_0()
            .bottom_0()
            // Over the pane's border, so the grip covers the line it moves.
            .left(px(-1.))
            .w(px(8.))
            .cursor_col_resize()
            .track_focus(&focus)
            .role(Role::Splitter)
            .aria_label(label("details.resize"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if event.click_count >= 2 {
                        this.drag = None;
                        this.details_width = Self::clamp_details_width(
                            f32::from(theme::dimension("detailsWidth")),
                            window,
                        );
                        this.prefs.set("detailsWidth", this.details_width);
                    } else {
                        this.drag = Some(("details".into(), event.position, this.details_width));
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // The pane sits right of its handle: moving the handle left widens it.
                let width = match event.keystroke.key.as_str() {
                    "left" => this.details_width + KEY_STEP,
                    "right" => this.details_width - KEY_STEP,
                    "home" => Self::DETAILS_MIN_WIDTH,
                    "end" => Self::DETAILS_MAX_WIDTH,
                    _ => return,
                };
                this.details_width = Self::clamp_details_width(width, window);
                this.prefs.set("detailsWidth", this.details_width);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(grip)
            .into_any_element()
    }

    fn details_body(
        &self,
        key: &str,
        picked: &Picked<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let Some(process) = picked.process else {
            return div().into_any_element();
        };
        let info = self
            .data
            .get(&format!("details:{key}"))
            .filter(|info| info.is_object());
        let mono = theme::mono_font(cx);
        let mut subtitle = div().mt(px(2.)).text_color(p.muted).child(if picked.group {
            format!(
                "{} · {}",
                text(process, "name"),
                tr!("details.group.count", count = picked.members.len())
            )
        } else {
            text(process, "name").to_owned()
        });
        if let Some(company) = process["company"].as_str().filter(|c| !c.is_empty()) {
            subtitle = subtitle.child(company.to_owned());
        }
        let close = self.details_icon_button(
            "deselect-process",
            "DismissIcon",
            label("details.deselect"),
            Action::Select(String::new()),
            px(14.),
            cx,
        );
        let head = self.details_head(
            self.details_process_icon(process, window, cx),
            model::title(process),
            Some(subtitle),
            close.into_any_element(),
            cx,
        );
        let sum = |field: &str| picked.members.iter().map(|m| number(m, field)).sum::<f64>();
        let (cpu, memory) = (sum("cpu"), sum("privateWs"));
        let prefix = if picked.group { "group" } else { "process" };
        let charts = div()
            .mt(px(16.))
            .flex()
            .gap(px(8.))
            .child(self.details_mini_chart(
                "details-cpu",
                label("common.resource.cpu"),
                format!("{}%", theme::cpu(cpu)),
                &format!("{prefix}.{key}.cpu"),
                p.cpu,
                Some(100.),
                cx,
            ))
            .child(self.details_mini_chart(
                "details-memory",
                label("common.resource.memory"),
                theme::memory(memory),
                &format!("{prefix}.{key}.mem"),
                p.memory,
                None,
                cx,
            ));
        let mut body = div().flex().flex_col().child(head).child(charts);

        let pids: HashSet<u64> = picked
            .members
            .iter()
            .filter_map(|m| m["pid"].as_u64())
            .collect();
        let hosted: Vec<&Value> = self
            .data
            .get("services")
            .map(values)
            .unwrap_or_default()
            .iter()
            .filter(|s| s["pid"].as_u64().is_some_and(|pid| pids.contains(&pid)))
            .collect();
        if !hosted.is_empty() {
            body = body
                .child(self.details_section_title(tr!("details.services", count = hosted.len())))
                .child(self.details_services(&hosted, cx));
        }

        if picked.group {
            let mut rows = self.details_resources(&picked.members, true);
            rows.push((
                label("details.cpuTime"),
                cpu_time(sum("cpuTime")).into_any_element(),
            ));
            let earliest = picked
                .members
                .iter()
                .map(|m| number(m, "createTime"))
                .fold(f64::INFINITY, f64::min);
            rows.push((
                label("details.group.earliest"),
                date_time(earliest).into_any_element(),
            ));
            body = body
                .child(self.details_section_title(label("details.section.resources")))
                .child(self.details_facts(rows))
                .child(self.details_section_title(tr!(
                    "details.group.members",
                    count = picked.members.len()
                )))
                .child(self.details_members(&picked.members, cx));
            let mut paths: Vec<&str> = Vec::new();
            for path in picked
                .members
                .iter()
                .filter_map(|m| m["path"].as_str())
                .filter(|path| !path.is_empty())
            {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
            if !paths.is_empty() {
                body = body.child(self.details_section_title(label("details.section.location")));
                for (index, path) in paths.into_iter().enumerate() {
                    body = body.child(
                        self.details_path_row(&format!("group-path-{index}"), path, &mono, cx)
                            .when(index > 0, |row| row.mt(px(4.))),
                    );
                }
            }
        } else {
            body = body
                .child(self.details_section_title(label("details.section.process")))
                .child(self.details_facts(self.details_process_facts(process, info, cx)))
                .child(self.details_section_title(label("details.section.resources")))
                .child(self.details_facts(self.details_resources(&picked.members, false)));
            if let Some(path) = process["path"].as_str().filter(|path| !path.is_empty()) {
                body = body
                    .child(self.details_section_title(label("details.section.location")))
                    .child(self.details_path_row("process-path", path, &mono, cx));
            }
            if let Some(command) = info
                .and_then(|info| info["commandLine"].as_str())
                .filter(|command| !command.is_empty())
            {
                body = body
                    .child(self.details_section_title(label("details.section.commandLine")))
                    .child(self.details_code("process-command", command, &mono));
            }
            if info.is_some_and(|info| !boolean(info, "accessible")) {
                body = body.child(
                    div()
                        .mt(px(14.))
                        .px(px(10.))
                        .py(px(8.))
                        .flex()
                        .gap(px(8.))
                        .rounded(px(6.))
                        .bg(p.hover)
                        .text_color(p.muted)
                        .text_size(px(12.))
                        .child(
                            div()
                                .flex_shrink_0()
                                .mt(px(1.))
                                .child(icon("InfoIcon", p.muted)),
                        )
                        .child(div().flex_1().min_w_0().child(label("details.noAccess"))),
                );
            }
        }

        let version = info.map(|info| &info["version"]);
        let product = version
            .and_then(|v| v["productName"].as_str())
            .filter(|s| !s.is_empty());
        let file_version = version
            .and_then(|v| v["fileVersion"].as_str())
            .filter(|s| !s.is_empty());
        if product.is_some() || file_version.is_some() {
            let mut rows = Vec::new();
            if let Some(product) = product {
                rows.push((
                    label("details.product"),
                    selectable("details-product", product),
                ));
            }
            if let Some(file_version) = file_version {
                rows.push((
                    label("details.version"),
                    selectable("details-file-version", file_version),
                ));
            }
            body = body
                .child(self.details_section_title(label("details.section.version")))
                .child(self.details_facts(rows));
        }
        body.into_any_element()
    }

    fn details_process_facts(
        &self,
        process: &Value,
        info: Option<&Value>,
        cx: &Context<Self>,
    ) -> Vec<(String, AnyElement)> {
        let p = self.palette;
        let missing = || label("format.missing");
        let parent = values(&self.snapshot["processes"])
            .iter()
            .find(|candidate| {
                !process["ppid"].is_null()
                    && candidate["pid"] == process["ppid"]
                    && number(candidate, "createTime") <= number(process, "createTime")
            });
        let parent = match parent {
            Some(parent) => {
                let key = model::identity(parent);
                let accent = p.accent;
                Button::new("details-parent")
                    .justify_start()
                    .line_height(relative(1.4))
                    .text_color(accent)
                    .hover(|style| style.underline())
                    .child(tr!(
                        "details.parentLink",
                        name = text(parent, "name"),
                        pid = parent["pid"]
                    ))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.act(Action::Select(key.clone()), window, cx)
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .into_any_element()
            }
            None => div()
                .text_color(p.faint)
                .child(tr!("details.noParent", pid = process["ppid"]))
                .into_any_element(),
        };
        let localized = |field: &str, namespace: &str| {
            info.and_then(|info| info[field].as_str())
                .map(|value| label(&format!("{namespace}.{value}")))
                .unwrap_or_else(missing)
        };
        let priority = match process["basePriority"].as_u64() {
            Some(4) => Some("idle"),
            Some(6) => Some("belowNormal"),
            Some(8) => Some("normal"),
            Some(10) => Some("aboveNormal"),
            Some(13) => Some("high"),
            Some(24) => Some("realtime"),
            _ => None,
        }
        .map(|class| label(&format!("processes.priority.{class}")))
        .unwrap_or_else(missing);
        let plain = |value: &Value| {
            value
                .as_u64()
                .map(|v| v.to_string())
                .unwrap_or_else(missing)
        };
        vec![
            (
                label("processes.column.pid.label"),
                selectable("details-pid", plain(&process["pid"])),
            ),
            (label("details.parent"), parent),
            (
                label("details.started"),
                selectable("details-started", date_time(number(process, "createTime"))),
            ),
            (
                label("details.cpuTime"),
                selectable("details-cpu-time", cpu_time(number(process, "cpuTime"))),
            ),
            (
                label("details.user"),
                selectable(
                    "details-user",
                    info.and_then(|info| info["user"].as_str())
                        .map(str::to_owned)
                        .unwrap_or_else(missing),
                ),
            ),
            (
                label("details.integrity"),
                selectable(
                    "details-integrity",
                    localized("integrity", "details.integrityLevel"),
                ),
            ),
            (
                label("details.architecture"),
                selectable(
                    "details-architecture",
                    localized("architecture", "details.arch"),
                ),
            ),
            (
                label("details.priority"),
                selectable("details-priority", priority),
            ),
            (
                label("common.efficiencyMode"),
                selectable(
                    "details-efficiency",
                    label(if boolean(process, "efficiency") {
                        "common.on"
                    } else {
                        "common.off"
                    }),
                ),
            ),
            (
                label("details.session"),
                selectable("details-session", plain(&process["sessionId"])),
            ),
        ]
    }

    /// Resource facts of one process, or of an app group's members added up.
    fn details_resources(&self, members: &[&Value], group: bool) -> Vec<(String, AnyElement)> {
        let p = self.palette;
        let sum = |field: &str| members.iter().map(|m| number(m, field)).sum::<f64>();
        let mut rows = vec![
            (
                label("details.privateWs"),
                theme::memory(sum("privateWs")).into_any_element(),
            ),
            (
                label("details.workingSet"),
                theme::memory(sum("workingSet")).into_any_element(),
            ),
            (
                label("details.commit"),
                theme::memory(sum("commit")).into_any_element(),
            ),
        ];
        for field in ["diskRead", "diskWrite"] {
            // Only measured members count; none measured is no reading at all.
            let measured = members
                .iter()
                .filter_map(|m| m[field].as_f64())
                .reduce(|a, b| a + b);
            let (value, tip) = match measured {
                Some(rate) => (
                    byte_rate(rate),
                    label(&format!("processes.column.{field}.title")),
                ),
                None => (label("format.missing"), self.missing_disk_hint()),
            };
            rows.push((
                label(&format!("processes.column.{field}.label")),
                div()
                    .id(SharedString::from(format!("details-{field}")))
                    .child(value)
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .into_any_element(),
            ));
        }
        if !values(&self.snapshot["hardware"]["gpus"]).is_empty() {
            let busiest = members
                .iter()
                .copied()
                .max_by(|a, b| number(a, "gpu").total_cmp(&number(b, "gpu")));
            let engine = busiest
                .filter(|b| !group || number(b, "gpu") > 0.)
                .and_then(|b| b["gpuEngine"].as_str())
                .filter(|engine| !engine.is_empty());
            rows.push((
                label("common.resource.gpu"),
                div()
                    .child(format!("{}%", theme::cpu(sum("gpu").min(100.))))
                    .when_some(engine, |d, engine| {
                        d.child(
                            div()
                                .text_color(p.faint)
                                .child(tr!("details.gpuEngine", engine = engine)),
                        )
                    })
                    .flex()
                    .flex_wrap()
                    .into_any_element(),
            ));
            rows.push((
                label("details.gpuDedicated"),
                theme::memory(sum("gpuDedicated")).into_any_element(),
            ));
            rows.push((
                label("details.gpuShared"),
                theme::memory(sum("gpuShared")).into_any_element(),
            ));
        }
        rows.push((
            label("common.resource.network"),
            if boolean(&self.snapshot, "processNetwork") {
                tr!(
                    "processes.netRates",
                    sent = self.network_rate(sum("netSent")),
                    received = self.network_rate(sum("netReceived"))
                )
                .into_any_element()
            } else {
                div()
                    .text_color(p.faint)
                    .child(label("details.netNeedsAdmin"))
                    .into_any_element()
            },
        ));
        rows.push((
            label("details.threads"),
            theme::count(sum("threads")).into_any_element(),
        ));
        rows.push((
            label("details.handles"),
            theme::count(sum("handles")).into_any_element(),
        ));
        rows
    }

    /// Why a process shows no disk reading (privilege.ts `missingDiskHint`).
    fn missing_disk_hint(&self) -> String {
        let measured = values(&self.snapshot["processes"])
            .iter()
            .any(|p| !p["diskRead"].is_null() || !p["diskWrite"].is_null());
        let elevated = self
            .data
            .get("info")
            .is_some_and(|info| boolean(info, "elevated"));
        let service = self.data.get("privilege").filter(|s| s.is_object());
        let reason = match service {
            _ if measured => "pending",
            None => "unavailable",
            _ if elevated => "unavailable",
            Some(service) if text(service, "state") == "notInstalled" => "enable",
            Some(service) => {
                let version = text(service, "version");
                let app = self
                    .data
                    .get("info")
                    .map(|info| text(info, "version"))
                    .unwrap_or_default();
                if boolean(service, "incompatible")
                    || (!version.is_empty() && !app.is_empty() && version != app)
                {
                    "update"
                } else if text(service, "state") == "stopped" && boolean(service, "authorized") {
                    "start"
                } else {
                    "service"
                }
            }
        };
        label(&format!("processes.diskUnavailable.{reason}"))
    }

    fn network_rate(&self, bytes_per_second: f64) -> String {
        if self.prefs.get::<String>("netUnit", "bytes".into()) == "bits" {
            bit_rate(bytes_per_second)
        } else {
            byte_rate(bytes_per_second)
        }
    }

    fn details_services(&self, hosted: &[&Value], cx: &Context<Self>) -> AnyElement {
        let p = self.palette;
        let mut list = div().flex().flex_col().gap(px(2.));
        for service in hosted {
            let name = text(service, "name").to_owned();
            let display = text(service, "displayName").to_owned();
            let tip = service["description"]
                .as_str()
                .filter(|d| !d.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| display.clone());
            let target = name.clone();
            list = list.child(
                Button::new(SharedString::from(format!("hosted-service-{name}")))
                    .w_full()
                    .justify_start()
                    .items_baseline()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .line_height(relative(1.4))
                    .hover(move |style| style.bg(p.hover))
                    .accessibility_label(display.clone())
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .child(div().min_w_0().overflow_hidden().child(display))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(12.))
                            .text_color(p.faint)
                            .child(name),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.act(
                            Action::Preference("goToService".into(), json!(target)),
                            window,
                            cx,
                        )
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            );
        }
        list.into_any_element()
    }

    /// An app group's processes, busiest first; a click opens one on its own.
    fn details_members(&self, members: &[&Value], cx: &Context<Self>) -> AnyElement {
        let p = self.palette;
        let mut sorted = members.to_vec();
        sorted.sort_by(|a, b| {
            number(b, "cpu")
                .total_cmp(&number(a, "cpu"))
                .then_with(|| number(b, "privateWs").total_cmp(&number(a, "privateWs")))
        });
        let mut list = div().flex().flex_col().gap(px(2.)).font_features(tabular());
        for member in sorted {
            let pid = member["pid"].clone();
            let tip = tr!("details.group.open", pid = pid);
            let target = (*member).clone();
            list = list.child(
                Button::new(SharedString::from(format!(
                    "group-member-{}",
                    model::identity(member)
                )))
                .w_full()
                .justify_start()
                .items_baseline()
                .gap(px(8.))
                .px(px(8.))
                .py(px(4.))
                .rounded(px(4.))
                .line_height(relative(1.4))
                .hover(move |style| style.bg(p.hover))
                .accessibility_label(tip.clone())
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .child(format!("{} {pid}", label("processes.column.pid.label"))),
                )
                .child(
                    div()
                        .w(px(56.))
                        .flex_shrink_0()
                        .flex()
                        .justify_end()
                        .text_color(p.muted)
                        .child(format!("{}%", theme::cpu(number(member, "cpu")))),
                )
                .child(
                    div()
                        .w(px(72.))
                        .flex_shrink_0()
                        .flex()
                        .justify_end()
                        .text_color(p.muted)
                        .child(theme::memory(number(member, "privateWs"))),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.act(
                        Action::Preference("revealProcess".into(), target.clone()),
                        window,
                        cx,
                    )
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            );
        }
        list.into_any_element()
    }

    fn details_actions(&self, picked: &Picked<'_>, cx: &Context<Self>) -> AnyElement {
        let p = self.palette;
        let Some(process) = picked.process else {
            return div().into_any_element();
        };
        let args = json!({"members":picked.members,"name":model::title(process)});
        let danger = p.danger;
        let on_danger = cx.theme().danger_foreground;
        let control_hover = design_color("control-hover", cx);
        let action_button = |id: &'static str, name: String, action: Action| {
            Button::new(id)
                .flex_1()
                .min_w_0()
                .h(px(32.))
                .px(px(12.))
                .rounded(px(4.))
                .border_1()
                .accessibility_label(name.clone())
                .child(div().truncate().child(name))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.act(action.clone(), window, cx)),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        let mut row = div()
            .flex_shrink_0()
            .flex()
            .gap(px(8.))
            .px(px(16.))
            .py(px(12.))
            .border_t_1()
            .border_color(p.stroke)
            .child(
                action_button(
                    "details-end",
                    label(if picked.group {
                        "actions.endAll"
                    } else {
                        "common.endProcess"
                    }),
                    Action::Dialog("end".into(), args.clone()),
                )
                .border_color(danger.opacity(0.))
                .bg(danger)
                .text_color(on_danger)
                .hover(move |style| {
                    style.bg(Hsla {
                        l: (danger.l * 1.08).min(1.),
                        ..danger
                    })
                }),
            );
        if !picked.group {
            row = row.child(
                action_button(
                    "details-end-tree",
                    label("common.endProcessTree"),
                    Action::Dialog("endTree".into(), args),
                )
                .border_color(p.stroke)
                .bg(p.control)
                .text_color(p.ink)
                .hover(move |style| style.bg(control_hover)),
            );
        }
        row.into_any_element()
    }

    fn details_exited(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let glyph = icon("InfoIcon", p.ink)
            .w(px(32.))
            .h(px(32.))
            .into_any_element();
        let close = self.details_icon_button(
            "deselect-exited",
            "DismissIcon",
            label("common.close"),
            Action::Select(String::new()),
            px(14.),
            cx,
        );
        self.details_head(
            glyph,
            label("details.exited.title"),
            Some(
                div()
                    .mt(px(2.))
                    .text_color(p.muted)
                    .child(label("details.exited.hint")),
            ),
            close.into_any_element(),
            cx,
        )
        .into_any_element()
    }

    /// Icon, wrapping title and subtitle, and the controls on the right.
    fn details_head(
        &self,
        glyph: AnyElement,
        title: String,
        subtitle: Option<Div>,
        controls: AnyElement,
        cx: &App,
    ) -> Div {
        div()
            .flex()
            .items_start()
            .gap(px(12.))
            .child(div().size(px(32.)).flex_shrink_0().child(glyph))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_family(theme::display_font(cx))
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(relative(1.25))
                            .child(title),
                    )
                    .children(subtitle.map(|subtitle| subtitle.flex().flex_col())),
            )
            .child(controls)
    }

    fn details_process_icon(
        &self,
        process: &Value,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let size = px(32.);
        let id = process["webView"]["hostIcon"]
            .as_u64()
            .or_else(|| process["icon"].as_u64());
        if let Some(id) = id
            && let Some(png) = self.host.state::<AppState>().meta.icon_png(id as u32)
        {
            // Decoded once per icon while the pane keeps showing it.
            let image = window
                .use_keyed_state(
                    ElementId::NamedInteger("details-icon".into(), id),
                    cx,
                    move |_, _| Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec())),
                )
                .read(cx)
                .clone();
            return img(image).size(size).into_any_element();
        }
        icon(
            if process["path"].is_null() {
                "KernelIcon"
            } else {
                "AppIcon"
            },
            design_color("glyph-stroke", cx),
        )
        .w(size)
        .h(size)
        .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn details_mini_chart(
        &self,
        id: &str,
        title: String,
        value: String,
        key: &str,
        color: Hsla,
        maximum: Option<f64>,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = self.palette;
        // The trace keeps clear of the figure and the rounded corners
        // (styles.css `.mini-chart .trace`).
        let chart = self.chart(id, &[(key, color)], maximum, px(36.), false, cx);
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(74.))
            .px(px(10.))
            .py(px(8.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(6.))
            .child(
                div()
                    .absolute()
                    .top(px(33.))
                    .left(px(1.))
                    .right(px(1.))
                    .child(chart),
            )
            .child(div().text_size(px(12.)).text_color(p.muted).child(title))
            .child(
                div()
                    .font_family(theme::display_font(cx))
                    .text_size(px(18.))
                    .font_features(tabular())
                    .child(value),
            )
    }

    fn details_section_title(&self, title: String) -> AnyElement {
        div()
            .mt(px(20.))
            .mb(px(6.))
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(self.palette.muted)
            .child(title)
            .into_any_element()
    }

    /// Name and value pairs in two columns (styles.css `.facts`).
    fn details_facts(&self, rows: Vec<(String, AnyElement)>) -> AnyElement {
        let p = self.palette;
        div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .children(rows.into_iter().map(|(name, value)| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        div()
                            .w(px(FACT_NAME_WIDTH))
                            .flex_shrink_0()
                            .text_color(p.muted)
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_features(tabular())
                            .child(value),
                    )
            }))
            .into_any_element()
    }

    /// Monospaced, selectable text in a box: a path or a command line.
    fn details_code(&self, id: &str, value: &str, mono: &SharedString) -> Div {
        let p = self.palette;
        div()
            .flex_1()
            .min_w_0()
            .px(px(10.))
            .py(px(8.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(6.))
            .bg(p.layer)
            .font_family(mono.clone())
            .text_size(px(12.))
            .line_height(relative(1.5))
            .child(selectable(id, value))
    }

    fn details_path_row(
        &self,
        id: &str,
        path: &str,
        mono: &SharedString,
        cx: &Context<Self>,
    ) -> Div {
        div()
            .flex()
            .items_start()
            .gap(px(4.))
            .child(self.details_code(id, path, mono))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(self.details_icon_button(
                        format!("{id}-reveal"),
                        "FolderIcon",
                        label("common.openFileLocation"),
                        Action::Invoke("reveal_in_explorer".into(), json!({"path":path})),
                        px(14.),
                        cx,
                    ))
                    .child(self.details_icon_button(
                        format!("{id}-copy"),
                        "CopyIcon",
                        label("common.copyPath"),
                        Action::Copy(path.into(), "actions.copyWhat.path"),
                        px(14.),
                        cx,
                    )),
            )
    }

    /// A small ghost button holding only a glyph, named by its tooltip.
    fn details_icon_button(
        &self,
        id: impl Into<SharedString>,
        glyph: &str,
        name: String,
        action: Action,
        glyph_size: Pixels,
        cx: &Context<Self>,
    ) -> Button {
        let p = self.palette;
        let tip = name.clone();
        Button::new(id.into())
            .size(px(24.))
            .flex_shrink_0()
            .rounded(px(4.))
            .text_color(p.ink)
            .hover(move |style| style.bg(p.hover))
            .accessibility_label(name)
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .child(icon(glyph, p.ink).w(glyph_size).h(glyph_size))
            .on_click(cx.listener(move |this, _, window, cx| this.act(action.clone(), window, cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn overview_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let parts = self.overview_parts();
        let visible = self.prefs.get("overviewHardwareVisible", true);
        let copied = std::iter::once(label("details.overview.title"))
            .chain(parts.iter().map(|part| {
                let line = tr!(
                    "details.overview.copyPart",
                    kind = &part.kind,
                    name = &part.name
                );
                if part.detail.is_empty() {
                    line
                } else {
                    format!("{line}\n{}", part.detail)
                }
            }))
            .collect::<Vec<_>>()
            .join("\n\n");
        let visibility = label(if visible {
            "details.overview.hide"
        } else {
            "details.overview.show"
        });
        let controls = div()
            .flex()
            .gap(px(2.))
            .child(
                self.details_icon_button(
                    "overview-copy",
                    "CopyIcon",
                    label("details.overview.copy"),
                    Action::Copy(copied, "details.overview.copyWhat"),
                    px(16.),
                    cx,
                )
                .disabled(parts.is_empty()),
            )
            .child(self.details_icon_button(
                "overview-visibility",
                if visible { "EyeIcon" } else { "EyeOffIcon" },
                visibility,
                Action::Preference("overviewHardwareVisible".into(), json!(!visible)),
                px(16.),
                cx,
            ))
            .into_any_element();
        let head = self
            .details_head(
                icon("ComputerIcon", p.ink)
                    .w(px(32.))
                    .h(px(32.))
                    .into_any_element(),
                label("details.overview.title"),
                None,
                controls,
                cx,
            )
            .items_center();
        let accent = p.accent;
        let tip = |glyph: &str, content: AnyElement| {
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(14.))
                        .flex_shrink_0()
                        .mt(px(2.))
                        .child(icon(glyph, accent).w(px(14.)).h(px(14.))),
                )
                .child(div().flex_1().min_w_0().child(content))
        };
        let tips = div()
            .mt(px(16.))
            .px(px(12.))
            .py(px(10.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .border_1()
            .border_dashed()
            .border_color(design_color("stroke-strong", cx))
            .rounded(px(6.))
            .text_color(p.muted)
            .text_size(px(12.))
            .line_height(relative(1.5))
            .child(tip(
                "PointerIcon",
                label("details.overview.selectTip").into_any_element(),
            ))
            .child(tip("PanelIcon", self.overview_close_tip()));
        let mut body = div().flex().flex_col().child(head).child(tips);
        if visible {
            let ink = p.ink;
            let mut list = div().mt(px(16.)).flex().flex_col().gap(px(4.));
            for part in parts {
                let device = part.device.clone();
                list = list.child(
                    Button::new(SharedString::from(format!("overview-{}", part.device)))
                        .w_full()
                        .items_start()
                        .justify_start()
                        .gap(px(12.))
                        .p(px(8.))
                        .rounded(px(6.))
                        .line_height(relative(1.4))
                        .hover(move |style| style.bg(p.hover))
                        .accessibility_label(part.name.clone())
                        .child(
                            div()
                                .size(px(32.))
                                .flex_shrink_0()
                                .rounded(px(8.))
                                .bg(part.color.opacity(0.14))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(part.glyph, mix(part.color, ink, 0.75))),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(1.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(p.muted)
                                        .child(part.kind),
                                )
                                .child(div().font_weight(FontWeight::MEDIUM).child(part.name))
                                .when(!part.detail.is_empty(), |text| {
                                    text.child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(p.muted)
                                            .font_features(tabular())
                                            .child(part.detail),
                                    )
                                }),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.act(Action::Device(device.clone()), window, cx)
                        }))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                );
            }
            body = body.child(list);
        }
        body.into_any_element()
    }

    /// The second tip, with the toolbar's panel button drawn inside the sentence.
    fn overview_close_tip(&self) -> AnyElement {
        let p = self.palette;
        const MARK: char = '\u{FFFC}';
        let template = tr!("details.overview.closeTip", icon = MARK);
        let (before, after) = template.split_once(MARK).unwrap_or((&template, ""));
        // An invisible word about as wide as the 20 px key keeps its place in
        // the sentence; a word, unlike spaces, never breaks across lines.
        const ROOM: &str = "MM";
        let start = before.len();
        let end = start + ROOM.len();
        let sentence = StyledText::new(format!("{before}{ROOM}{after}")).with_highlights([(
            start..end,
            HighlightStyle {
                fade_out: Some(1.),
                ..Default::default()
            },
        )]);
        let layout = sentence.layout().clone();
        let glyph: Option<SharedString> = Some(gpui_kit::assets::IconNamed::path(
            theme::icon_name("PanelIcon"),
        ));
        let (control, stroke, ink) = (p.control, p.stroke, p.ink);
        div()
            .relative()
            .child(sentence)
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, cx| {
                        let (Some(from), Some(to)) = (
                            layout.position_for_index(start),
                            layout.position_for_index(end),
                        ) else {
                            return;
                        };
                        let center = if (to.y - from.y).abs() < px(1.) {
                            (from.x + to.x) / 2.
                        } else {
                            from.x + px(12.)
                        };
                        let key = Bounds::new(
                            point(
                                center - px(10.),
                                from.y + (layout.line_height() - px(18.)) / 2.,
                            ),
                            size(px(20.), px(18.)),
                        );
                        window.paint_quad(quad(
                            key,
                            px(4.),
                            control,
                            px(1.),
                            stroke,
                            BorderStyle::Solid,
                        ));
                        if let Some(glyph) = glyph.clone() {
                            let _ = window.paint_svg(
                                Bounds::new(
                                    point(key.center().x - px(6.), key.center().y - px(6.)),
                                    size(px(12.), px(12.)),
                                ),
                                glyph,
                                None,
                                TransformationMatrix::unit(),
                                ink,
                                cx,
                            );
                        }
                    },
                )
                .absolute()
                .inset_0(),
            )
            .into_any_element()
    }

    /// This computer's parts, each a way to its view on the performance page
    /// (SystemOverview.tsx `parts`).
    fn overview_parts(&self) -> Vec<Part> {
        let p = self.palette;
        let hw = &self.snapshot["hardware"];
        let cpu = &hw["cpu"];
        let memory = &hw["memory"];
        let mut cores = tr!(
            "details.overview.cores",
            count = number(cpu, "cores") as u64
        );
        if number(cpu, "performanceCores") > 0. {
            cores.push_str(&tr!(
                "details.overview.hybrid",
                performance = number(cpu, "performanceCores") as u64,
                efficiency = number(cpu, "efficiencyCores") as u64
            ));
        }
        let installed = if number(memory, "installed") > 0. {
            number(memory, "installed")
        } else {
            number(&self.snapshot["system"], "memTotal")
        };
        let mut parts = vec![
            Part {
                device: "cpu".into(),
                glyph: "ChipIcon",
                color: p.cpu,
                kind: label("details.overview.cpu"),
                name: text(cpu, "name").into(),
                detail: joined([
                    cores,
                    tr!(
                        "details.overview.threads",
                        count = number(cpu, "logical") as u64
                    ),
                    if number(cpu, "baseMhz") > 0. {
                        tr!(
                            "details.overview.base",
                            speed = format!("{:.2} GHz", number(cpu, "baseMhz") / 1000.)
                        )
                    } else {
                        String::new()
                    },
                ]),
            },
            Part {
                device: "memory".into(),
                glyph: "MemoryIcon",
                color: p.memory,
                kind: label("common.resource.memory"),
                name: joined_with(
                    " ",
                    [
                        format!("{:.0} GB", installed / GB),
                        text(memory, "kind").into(),
                    ],
                ),
                detail: joined([
                    if number(memory, "speed") > 0. {
                        format!("{} MT/s", number(memory, "speed"))
                    } else {
                        String::new()
                    },
                    if number(memory, "slotsTotal") > 0. {
                        tr!(
                            "details.overview.slots",
                            used = number(memory, "slotsUsed"),
                            total = number(memory, "slotsTotal")
                        )
                    } else {
                        String::new()
                    },
                    match text(memory, "formFactor") {
                        "onboard" => label("performance.memory.onboard"),
                        other => other.into(),
                    },
                ]),
            },
        ];
        for disk in values(&hw["disks"]) {
            let info = &disk["info"];
            let size = number(info, "size");
            parts.push(Part {
                device: format!("disk:{}", info["index"]),
                glyph: "DiskIcon",
                color: p.io,
                kind: label("details.overview.disk"),
                name: match text(info, "model") {
                    "" => tr!("performance.disk.title", index = info["index"]),
                    model => model.into(),
                },
                detail: joined([
                    if size > 0. {
                        capacity(size)
                    } else {
                        String::new()
                    },
                    disk_kind(info),
                    values(&info["volumes"])
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" "),
                ]),
            });
        }
        for nic in values(&hw["nics"]) {
            let info = &nic["info"];
            if text(info, "kind") == "virtual" {
                continue;
            }
            let (name, description) = (text(info, "name"), text(info, "description"));
            parts.push(Part {
                device: format!("nic:{}", text(info, "id")),
                glyph: "NetworkIcon",
                color: p.network,
                kind: label(&format!("performance.network.kind.{}", text(info, "kind"))),
                name: if description.is_empty() {
                    name
                } else {
                    description
                }
                .into(),
                detail: joined([
                    if name != description {
                        name.into()
                    } else {
                        String::new()
                    },
                    if number(info, "linkSpeed") > 0. {
                        link_speed(number(info, "linkSpeed"))
                    } else {
                        String::new()
                    },
                ]),
            });
        }
        for gpu in values(&hw["gpus"]) {
            let info = &gpu["info"];
            let dedicated = number(info, "dedicatedTotal");
            parts.push(Part {
                device: format!("gpu:{}", text(info, "id")),
                glyph: "GpuIcon",
                color: p.gpu,
                kind: label("details.overview.gpu"),
                name: text(info, "name").into(),
                detail: if dedicated > 0. {
                    tr!(
                        "details.overview.vram",
                        size = format!("{:.1} GB", dedicated / GB)
                    )
                } else {
                    label("details.overview.sharedMemory")
                },
            });
        }
        for battery in values(&hw["batteries"]) {
            let info = &battery["info"];
            let designed = number(info, "designedCapacity");
            let full = number(info, "fullCapacity");
            let name = joined([
                text(info, "manufacturer").to_owned(),
                text(info, "name").to_owned(),
            ]);
            parts.push(Part {
                device: format!("bat:{}", text(info, "id")),
                glyph: "BatteryIcon",
                color: p.battery,
                kind: label("details.overview.battery"),
                name: if name.is_empty() {
                    label("details.overview.battery")
                } else {
                    name
                },
                detail: joined([
                    if designed > 0. {
                        tr!(
                            "details.overview.designed",
                            capacity = format!("{:.0} Wh", (designed / 1000.).round())
                        )
                    } else {
                        String::new()
                    },
                    if designed > 0. && full > 0. {
                        tr!(
                            "details.overview.health",
                            percent = (full / designed * 100.).round()
                        )
                    } else {
                        String::new()
                    },
                ]),
            });
        }
        parts
    }
}

/// Selectable plain text with a stable identity.
fn selectable(id: &str, value: impl ToString) -> AnyElement {
    SelectableText::new(
        ElementId::Name(SharedString::from(id.to_owned())),
        value.to_string(),
    )
    .into_any_element()
}

/// The parts that are known, in a line: "a · b".
fn joined(items: impl IntoIterator<Item = String>) -> String {
    joined_with(" · ", items)
}

fn joined_with(separator: &str, items: impl IntoIterator<Item = String>) -> String {
    items
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join(separator)
}

/// Mixes `amount` of `color` with the rest of `other` (CSS `color-mix` in sRGB).
fn mix(color: Hsla, other: Hsla, amount: f32) -> Hsla {
    let (a, b) = (color.to_rgb(), other.to_rgb());
    let channel = |x: f32, y: f32| x * amount + y * (1. - amount);
    Rgba {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: channel(a.a, b.a),
    }
    .into()
}

/// A theme color the native palette does not carry.
fn design_color(key: &str, cx: &App) -> Hsla {
    let design = theme::design();
    let mode = if cx.theme().mode == ThemeMode::Dark {
        "dark"
    } else {
        "light"
    };
    theme::parse_color(
        design[mode][key]
            .as_str()
            .or_else(|| design["light"][key].as_str())
            .expect("theme color"),
    )
}

fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

/// The theme's decelerating curve (`--ease-out`).
fn ease_out() -> impl Fn(f32) -> f32 {
    let curve = theme::design()["light"]["ease-out"]
        .as_str()
        .and_then(|css| css.strip_prefix("cubic-bezier(")?.strip_suffix(')'))
        .map(|points| {
            points
                .split(',')
                .filter_map(|point| point.trim().parse::<f32>().ok())
                .collect::<Vec<_>>()
        })
        .filter(|points| points.len() == 4)
        .unwrap_or_else(|| vec![0.1, 0.9, 0.2, 1.]);
    gpui_kit::base::animation::cubic_bezier(curve[0], curve[1], curve[2], curve[3])
}

fn motion_duration() -> Duration {
    theme::motion_duration()
}

/// A theme duration such as `t-slow`; "details" is the 200 ms settle of
/// another selection (DetailsPanel.tsx).
fn duration_of(key: &str) -> Duration {
    if key == "details" {
        return Duration::from_millis(200);
    }
    theme::design()["light"][key]
        .as_str()
        .and_then(|value| value.trim_end_matches("ms").parse().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(theme::motion_duration)
}

/// Byte rate with its unit even when idle (format.ts `fmtRate(v, false)`).
fn byte_rate(bytes_per_second: f64) -> String {
    if bytes_per_second < 1. {
        "0 KB/s".into()
    } else if bytes_per_second < MB {
        format!("{:.0} KB/s", (bytes_per_second / KB).round().max(1.))
    } else if bytes_per_second < GB {
        format!("{:.1} MB/s", bytes_per_second / MB)
    } else {
        format!("{:.2} GB/s", bytes_per_second / GB)
    }
}

/// Rate in bits per second (format.ts `fmtBits`).
fn bit_rate(bytes_per_second: f64) -> String {
    let bits = bytes_per_second * 8.;
    if bits < 1. {
        "0 Kbps".into()
    } else if bits < 1e6 {
        format!("{:.*} Kbps", if bits < 1e4 { 1 } else { 0 }, bits / 1e3)
    } else if bits < 1e9 {
        format!("{:.*} Mbps", if bits < 1e7 { 2 } else { 1 }, bits / 1e6)
    } else {
        format!("{:.2} Gbps", bits / 1e9)
    }
}

/// CPU time as h:mm:ss.
fn cpu_time(milliseconds: f64) -> String {
    let seconds = (milliseconds.max(0.) / 1000.) as u64;
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

/// Month, day and time of day in the interface language's usual order, as
/// `Intl.DateTimeFormat` writes them; a zero start is system startup.
fn date_time(unix_ms: f64) -> String {
    if unix_ms <= 0. || !unix_ms.is_finite() {
        return label("format.sinceBoot");
    }
    let Ok(value) = time::OffsetDateTime::from_unix_timestamp((unix_ms / 1000.).floor() as i64)
    else {
        return label("format.missing");
    };
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let value = value.to_offset(offset);
    let (month, day) = (u8::from(value.month()), value.day());
    let (hour, minute, second) = (value.hour(), value.minute(), value.second());
    match argus_core::i18n::locale() {
        argus_core::i18n::Locale::EnUs => {
            let period = if hour < 12 { "AM" } else { "PM" };
            let hour = match hour % 12 {
                0 => 12,
                hour => hour,
            };
            format!("{month}/{day}, {hour:02}:{minute:02}:{second:02} {period}")
        }
        argus_core::i18n::Locale::ZhCn => {
            format!("{month}/{day} {hour:02}:{minute:02}:{second:02}")
        }
    }
}

/// Capacity in binary units, the way Windows reports drive sizes.
fn capacity(bytes: f64) -> String {
    if bytes >= 1024. * GB {
        format!("{:.1} TB", bytes / (1024. * GB))
    } else {
        format!("{:.0} GB", (bytes / GB).round())
    }
}

fn link_speed(bits_per_second: f64) -> String {
    if bits_per_second >= 1e9 {
        let gbps = format!("{:.1}", bits_per_second / 1e9);
        format!("{} Gbps", gbps.trim_end_matches(".0"))
    } else {
        format!("{:.0} Mbps", (bits_per_second / 1e6).round())
    }
}

/// Media and bus of a disk: "SSD (NVMe)".
fn disk_kind(info: &Value) -> String {
    let bus = match text(info, "bus") {
        "nvme" => "NVMe",
        "usb" => "USB",
        "sata" => "SATA",
        _ => "",
    };
    let media = match info["ssd"].as_bool() {
        Some(true) => "SSD".to_owned(),
        Some(false) => "HDD".to_owned(),
        None if boolean(info, "removable") => label("performance.disk.removableMedia"),
        None => String::new(),
    };
    joined_with(
        " ",
        [
            media,
            if bus.is_empty() {
                String::new()
            } else {
                format!("({bus})")
            },
        ],
    )
}

#[cfg(test)]
mod tests {
    // Named imports keep the glob-imported GPUI `test` macro out of scope.
    use super::{GB, MB, bit_rate, byte_rate, capacity, cpu_time, disk_kind, joined, link_speed};
    use serde_json::json;

    #[test]
    fn rates_match_the_table_formats() {
        assert_eq!(byte_rate(0.), "0 KB/s");
        assert_eq!(byte_rate(10.), "1 KB/s");
        assert_eq!(byte_rate(3. * MB), "3.0 MB/s");
        assert_eq!(bit_rate(0.), "0 Kbps");
        assert_eq!(bit_rate(1000.), "8.0 Kbps");
        assert_eq!(bit_rate(1e6), "8.00 Mbps");
    }

    #[test]
    fn cpu_time_counts_whole_hours() {
        assert_eq!(cpu_time(0.), "0:00:00");
        assert_eq!(cpu_time(343_806.9), "0:05:43");
        assert_eq!(cpu_time(90_061_000.), "25:01:01");
    }

    #[test]
    fn hardware_strings_follow_windows_units() {
        assert_eq!(capacity(2e12), "1.8 TB");
        assert_eq!(capacity(512. * GB), "512 GB");
        assert_eq!(link_speed(1.201e9), "1.2 Gbps");
        assert_eq!(link_speed(1e9), "1 Gbps");
        assert_eq!(link_speed(1e8), "100 Mbps");
        assert_eq!(
            disk_kind(&json!({"bus":"nvme","ssd":true,"removable":false})),
            "SSD (NVMe)"
        );
        assert_eq!(joined(["a".into(), String::new(), "b".into()]), "a · b");
    }
}
