//! Page composition keeps domain queries separate from native widgets.
mod details;
mod history;
mod performance;
mod settings;
use super::*;
use gpui_kit::component::ActiveTheme;
impl Desktop {
    pub(in crate::frontend) fn page_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.page.as_str() {
            "processes" => {
                if self.snapshot.is_null() {
                    return div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(tr!("shell.loadingProcesses"))
                        .into_any_element();
                }
                let vitals = self.vitals(window, cx);
                let table = self.table_view(window, cx);
                let width = gpui_kit::base::motion::transition(
                    "details-width",
                    if self.details_open {
                        px(self.details_width)
                    } else {
                        px(0.)
                    },
                    theme::transition(),
                    window,
                    cx,
                );
                let details = (width > px(0.5)).then(|| {
                    div()
                        .w(width)
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(self.details_view(window, cx))
                });
                div()
                    .size_full()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .px_3()
                    .child(vitals)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .border_1()
                            .border_color(self.palette.stroke)
                            .rounded(cx.theme().radius)
                            .overflow_hidden()
                            .bg(self.palette.layer)
                            .child(div().flex_1().min_w_0().min_h_0().child(table))
                            .children(details),
                    )
                    .into_any_element()
            }
            "performance" => self.performance_view(window, cx),
            "history" => self.history_view(window, cx),
            "settings" => self.settings_view(window, cx),
            "services" | "startup" | "network" => {
                let table = self.table_view(window, cx);
                let footer = match self.page.as_str() {
                    "services" => {
                        let items = self.data.get("services").map(values).unwrap_or_default();
                        let running = items
                            .iter()
                            .filter(|s| text(s, "state") == "running")
                            .count();
                        Some((
                            tr!("services.summary", running = running, total = items.len()),
                            label("services.manageHint"),
                        ))
                    }
                    "startup" => {
                        let items = self.data.get("startup").map(values).unwrap_or_default();
                        let enabled = items.iter().filter(|s| boolean(s, "enabled")).count();
                        Some((
                            tr!("startup.summary", enabled = enabled, total = items.len()),
                            label("startup.note"),
                        ))
                    }
                    "network" => {
                        let items = self.data.get("connections").map(values).unwrap_or_default();
                        let listen = items
                            .iter()
                            .filter(|c| text(c, "state") == "listen")
                            .count();
                        let est = items
                            .iter()
                            .filter(|c| text(c, "state") == "established")
                            .count();
                        let udp = items
                            .iter()
                            .filter(|c| text(c, "protocol") == "udp")
                            .count();
                        Some((
                            format!(
                                "{} {} · {} {} · {} {}",
                                label("connections.filter.listen.label"),
                                listen,
                                label("connections.filter.established.label"),
                                est,
                                label("connections.filter.udp.label"),
                                udp
                            ),
                            label("connections.note"),
                        ))
                    }
                    _ => None,
                };
                div()
                    .size_full()
                    .min_h_0()
                    .mx_3()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(self.palette.stroke)
                    .rounded(cx.theme().radius)
                    .overflow_hidden()
                    .bg(self.palette.layer)
                    .child(div().flex_1().min_h_0().child(table))
                    .when_some(footer, |d, (summary, note)| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .px_3()
                                .py_1p5()
                                .border_t_1()
                                .border_color(self.palette.stroke)
                                .flex()
                                .justify_between()
                                .items_center()
                                .gap_3()
                                .text_xs()
                                .text_color(self.palette.muted)
                                .child(summary)
                                .child(div().truncate().child(note)),
                        )
                    })
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }
    pub(in crate::frontend) fn page_controls(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let choices: Vec<&str> = match self.page.as_str() {
            "services" => vec!["all", "running", "stopped"],
            "network" => vec!["all", "listen", "established", "udp"],
            _ => vec![],
        };
        let namespace = if self.page == "network" {
            "connections"
        } else {
            &self.page
        };
        let selected = self
            .filters
            .get(&self.page)
            .map(String::as_str)
            .unwrap_or("all");
        let mut row = div().flex().items_center().gap_1();
        if !choices.is_empty() {
            row = row.child(
                self.segments(
                    "page-filter",
                    choices
                        .into_iter()
                        .map(|choice| {
                            (
                                label(&format!(
                                    "{namespace}.filter.{choice}{}",
                                    if self.page == "network" { ".label" } else { "" }
                                )),
                                None,
                                Action::Preference(format!("filter.{}", self.page), json!(choice)),
                                selected == choice,
                            )
                        })
                        .collect(),
                    cx,
                ),
            );
        }
        if self.page == "services" {
            if let Some(sel) = self.selected.get("services")
                && let Some(svc) = self
                    .data
                    .get("services")
                    .map(values)
                    .unwrap_or_default()
                    .iter()
                    .find(|s| text(s, "name") == sel)
            {
                let state = text(svc, "state");
                let svc_name = text(svc, "name").to_owned();
                for (id, key, cmd, active) in [
                    (
                        "svc-start",
                        "services.control.start",
                        "start_service",
                        state == "stopped",
                    ),
                    (
                        "svc-stop",
                        "services.control.stop",
                        "stop_service",
                        state == "running",
                    ),
                    (
                        "svc-restart",
                        "services.control.restart",
                        "restart_service",
                        state == "running",
                    ),
                ] {
                    if active {
                        row = row.child(self.button(
                            id,
                            label(key),
                            None,
                            Action::Invoke(cmd.into(), json!({"name": svc_name})),
                            false,
                            cx,
                        ));
                    }
                }
            }
            row = row.child(self.button(
                "services-console",
                tr!("services.control.openConsole"),
                None,
                Action::Invoke("open_services_console".into(), json!({})),
                false,
                cx,
            ));
        }
        if self.page == "history" {
            row = row.child(
                self.segments(
                    "history-range",
                    history::RANGES
                        .into_iter()
                        .map(|(key, _)| {
                            (
                                history::range_label(key),
                                None,
                                Action::Preference("historyRange".into(), json!(key)),
                                self.history_range == key,
                            )
                        })
                        .collect(),
                    cx,
                ),
            );
        }
        row.into_any_element()
    }
    pub(in crate::frontend) fn vitals(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let sys = &self.snapshot["system"];
        let hw = &self.snapshot["hardware"];
        let disk = values(&hw["disks"])
            .iter()
            .map(|d| number(d, "read") + number(d, "write"))
            .sum();
        let busiest = values(&hw["disks"])
            .iter()
            .max_by(|a, b| number(a, "active").total_cmp(&number(b, "active")));
        let gpu = values(&hw["gpus"])
            .iter()
            .max_by(|a, b| number(a, "util").total_cmp(&number(b, "util")));
        let rx = values(&hw["nics"])
            .iter()
            .map(|n| number(n, "rx"))
            .sum::<f64>();
        let tx = values(&hw["nics"])
            .iter()
            .map(|n| number(n, "tx"))
            .sum::<f64>();
        let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
        let mut cards = vec![
            (
                "cpu",
                "cpu".into(),
                "system.cpu",
                p.cpu,
                Some(100.),
                format!("{}%", theme::cpu(number(sys, "cpu"))),
                hw["cpuMhz"]
                    .as_f64()
                    .map(|v| format!("{:.2} GHz", v / 1000.))
                    .unwrap_or_default(),
            ),
            (
                "memory",
                "memory".into(),
                "system.memUsed",
                p.memory,
                Some(number(sys, "memTotal")),
                format!("{:.1}", number(sys, "memUsed") / 1073741824.),
                format!(
                    "/ {}  ·  {:.0}%",
                    theme::memory(number(sys, "memTotal")),
                    number(sys, "memUsed") / number(sys, "memTotal").max(1.) * 100.
                ),
            ),
            (
                "disk",
                busiest
                    .map(|d| format!("disk:{}", d["info"]["index"]))
                    .unwrap_or_default(),
                "totals.disk",
                p.io,
                None,
                theme::bytes(disk, true),
                busiest
                    .map(|d| {
                        tr!(
                            "processes.vitals.diskActive",
                            value = number(d, "active").round() as u64
                        )
                    })
                    .unwrap_or_default(),
            ),
            (
                "network",
                values(&hw["nics"])
                    .first()
                    .map(|n| format!("nic:{}", text(&n["info"], "id")))
                    .unwrap_or_default(),
                "totals.network",
                p.network,
                None,
                theme::network(rx + tx, bits),
                tr!(
                    "processes.vitals.netRates",
                    received = theme::network(rx, bits),
                    sent = theme::network(tx, bits)
                ),
            ),
        ];
        if let Some(gpu) = gpu {
            cards.push((
                "gpu",
                format!("gpu:{}", text(&gpu["info"], "id")),
                "totals.gpu",
                p.gpu,
                Some(100.),
                format!("{}%", theme::cpu(number(gpu, "util"))),
                gpu["temperature"]
                    .as_f64()
                    .map(|v| format!("{v:.0} °C"))
                    .unwrap_or_default(),
            ));
        }
        let mut band = div()
            .flex()
            .flex_shrink_0()
            .h(theme::dimension("vitalsHeight"))
            .border_1()
            .border_color(p.stroke)
            .rounded_lg()
            .bg(p.layer)
            .overflow_hidden();
        for (index, (id, device, key, color, max, value, detail)) in cards.into_iter().enumerate() {
            let plot = self.chart(
                &format!("vital-{id}"),
                &[(key, color)],
                max,
                theme::dimension("vitalsHeight") - px(62.),
                false,
                cx,
            );
            let mut header = div()
                .flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted)
                        .child(label(&format!("common.resource.{id}"))),
                );
            if id == "cpu" && !values(&sys["cores"]).is_empty() {
                let keys = values(&sys["cores"])
                    .iter()
                    .enumerate()
                    .map(|(i, _)| (format!("system.cores.{i}"), color))
                    .collect::<Vec<_>>();
                header = header.child(
                    div().w_24().child(
                        self.chart(
                            "vital-core-bars",
                            &keys
                                .iter()
                                .map(|(k, c)| (k.as_str(), *c))
                                .collect::<Vec<_>>(),
                            Some(100.),
                            px(22.),
                            false,
                            cx,
                        ),
                    ),
                );
            } else {
                header = header.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(p.muted)
                        .child(if id == "memory" {
                            format!(
                                "{:.0}%",
                                number(sys, "memUsed") / number(sys, "memTotal").max(1.) * 100.
                            )
                        } else {
                            detail.clone()
                        }),
                );
            }
            let (figure, unit) = value.split_once(' ').unwrap_or((&value, ""));
            let (figure, unit) = figure
                .strip_suffix('%')
                .map(|v| (v, "%"))
                .unwrap_or((figure, unit));
            band = band.child(
                self.button(
                    format!("vital-{id}"),
                    "",
                    None,
                    Action::Device(device),
                    false,
                    cx,
                )
                .relative()
                .flex_1()
                .when(id == "cpu", |b| b.flex_grow(1.3))
                .min_w_0()
                .h_full()
                .rounded_none()
                .px_4()
                .py_3()
                .flex_col()
                .items_start()
                .justify_start()
                .gap_1()
                .when(index > 0, |d| d.border_l_1().border_color(p.stroke))
                .child(div().absolute().bottom_0().left_0().right_0().child(plot))
                .child(header)
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_1()
                        .w_full()
                        .overflow_hidden()
                        .child(
                            div()
                                .text_size(theme::dimension("figureSize"))
                                .font_weight(FontWeight::LIGHT)
                                .child(figure.to_owned()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(p.muted)
                                .child(if id == "memory" {
                                    format!("/ {}", theme::memory(number(sys, "memTotal")))
                                } else {
                                    unit.to_owned()
                                }),
                        )
                        .when(id == "cpu", |d| {
                            d.child(div().pl_2().text_xs().text_color(p.muted).child(detail))
                        }),
                ),
            );
        }
        band.into_any_element()
    }
    pub(in crate::frontend) fn preference(
        &mut self,
        key: &str,
        value: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match key {
            "finishOnboarding" | "configurePrivilege" => {
                self.request(
                    "complete_onboarding",
                    json!({"version":welcome::configuration()["onboardingVersion"]}),
                    Some("settings"),
                    window,
                    cx,
                );
                self.act(Action::CloseOverlay, window, cx);
                if key == "configurePrivilege" {
                    self.preference("settingsSection", json!("privilege"), window, cx);
                }
                return;
            }
            "uiMotion" => {
                self.prefs.set(
                    "motion.ui",
                    match value.as_str() {
                        Some("on") => Some(true),
                        Some("off") => Some(false),
                        _ => None,
                    },
                );
                self.apply_motion(cx);
                cx.notify();
                return;
            }
            "motion.chartMode" => {
                self.prefs.set(key, value);
                self.apply_motion(cx);
                cx.notify();
                return;
            }
            "navPosition" => self.nav_top = value.as_str() != Some("side"),
            "navExpanded" => self.nav_expanded = value.as_bool().unwrap_or(false),
            "historyRange" => {
                self.history_range = value.as_str().unwrap_or("1h").into();
                self.history_cursor = None;
                self.history_hover = None;
                self.load_history(window, cx);
            }
            "historyMetric" => self.history_metric = value.as_str().unwrap_or("cpu").into(),
            "historyLatest" => {
                self.history_cursor = None;
                self.history_hover = None;
                self.load_history(window, cx);
            }
            "goToService" => {
                if let Some(name) = value.as_str() {
                    self.act(Action::Page("services".into()), window, cx);
                    self.selected.insert("services".into(), name.into());
                }
                return;
            }
            "clearSearch" => {
                self.query.clear();
                self.search.update(cx, |s, cx| s.set_value("", window, cx));
                self.rebuild_rows(false, cx);
                return;
            }
            "revealProcess" => {
                let found = values(&self.snapshot["processes"])
                    .iter()
                    .find(|p| {
                        p["pid"] == value["pid"]
                            && (value["createTime"].is_null()
                                || p["createTime"] == value["createTime"])
                    })
                    .cloned();
                if let Some(process) = found {
                    self.act(Action::CloseOverlay, window, cx);
                    self.act(Action::Page("processes".into()), window, cx);
                    self.query = process["pid"].to_string();
                    self.search
                        .update(cx, |s, cx| s.set_value(self.query.clone(), window, cx));
                    self.rebuild_rows(false, cx);
                    self.details_open = true;
                    self.act(Action::Select(model::identity(&process)), window, cx);
                } else {
                    self.toast = Some((
                        tr!("processes.pidExited", pid = value["pid"]),
                        Instant::now(),
                    ));
                }
                return;
            }
            "ruleToggle" | "ruleDelete" => {
                let mut rules = values(&self.data["settings"]["rules"]).to_vec();
                if key == "ruleDelete" {
                    rules.retain(|r| r["id"] != value);
                } else {
                    for rule in &mut rules {
                        if rule["id"] == value {
                            rule["enabled"] = json!(!boolean(rule, "enabled"));
                        }
                    }
                }
                self.request(
                    "set_rules",
                    json!({"rules":rules}),
                    Some("settings"),
                    window,
                    cx,
                );
                return;
            }
            "updateCheck" | "updateChannel" | "updateRoute" | "updateProxy" => {
                let mut settings = self.data["settings"].clone();
                settings[key] = value;
                self.request("set_update_settings",json!({"check":settings["updateCheck"],"channel":settings["updateChannel"],"route":settings["updateRoute"],"proxy":settings["updateProxy"]}),Some("settings"),window,cx);
                return;
            }
            "pin" => {
                let id = value.as_str().unwrap_or_default();
                self.set_mark(
                    id,
                    "pinned",
                    json!(!boolean(&self.marks()[id], "pinned")),
                    cx,
                );
                return;
            }
            _ if key.starts_with("dialog.") => {
                if let Some(modal) = &mut self.modal {
                    if !modal.args.is_object() {
                        modal.args = json!({});
                    }
                    modal.args[&key[7..]] = value;
                }
                cx.notify();
                return;
            }
            "settingsSection" => {
                self.settings_section = value.as_str().unwrap_or("language").into();
                self.settings_target = settings::SECTIONS
                    .iter()
                    .position(|section| *section == self.settings_section);
                if self.page != "settings" {
                    self.act(Action::Page("settings".into()), window, cx);
                }
            }
            _ if key.starts_with("filter.") => {
                self.filters
                    .insert(key[7..].into(), value.as_str().unwrap_or("all").into());
                self.rebuild_rows(false, cx);
            }
            _ => {}
        }
        self.prefs.set(key, value);
        cx.notify();
    }
    pub(in crate::frontend) fn property(
        &self,
        title: &str,
        value: impl Into<SharedString>,
    ) -> AnyElement {
        div()
            .flex()
            .items_start()
            .gap_4()
            .py_0()
            .line_height(relative(1.5))
            .child(
                div()
                    .text_color(self.palette.muted)
                    .w_32()
                    .flex_shrink_0()
                    .child(label(title)),
            )
            .child(div().flex_1().child(value.into()))
            .into_any_element()
    }
}
