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
                let details = self.details_open.then(|| self.details_view(window, cx));
                div()
                    .flex_1()
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
                div()
                    .flex_1()
                    .min_h_0()
                    .mx_3()
                    .border_1()
                    .border_color(self.palette.stroke)
                    .rounded(cx.theme().radius)
                    .child(table)
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
        for choice in choices {
            row = row.child(self.button(
                format!("filter-{choice}"),
                label(&format!(
                    "{namespace}.filter.{choice}{}",
                    if self.page == "network" { ".label" } else { "" }
                )),
                None,
                Action::Preference(format!("filter.{}", self.page), json!(choice)),
                selected == choice,
                cx,
            ));
        }
        if self.page == "services" {
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
            for (key, _) in history::RANGES {
                row = row.child(self.button(
                    format!("range-{key}"),
                    history::range_label(key),
                    None,
                    Action::Preference("historyRange".into(), json!(key)),
                    self.history_range == key,
                    cx,
                ));
            }
        }
        row.into_any_element()
    }
    pub(in crate::frontend) fn vitals(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let sys = &self.snapshot["system"];
        let hw = &self.snapshot["hardware"];
        let gpu = values(&hw["gpus"])
            .iter()
            .map(|g| number(g, "util"))
            .fold(0f64, f64::max);
        let disk = values(&hw["disks"])
            .iter()
            .map(|d| number(d, "read") + number(d, "write"))
            .sum();
        let network = values(&hw["nics"])
            .iter()
            .map(|n| number(n, "rx") + number(n, "tx"))
            .sum();
        let mut cards = vec![
            (
                "cpu",
                tr!("common.resource.cpu"),
                format!("{}%", theme::cpu(number(sys, "cpu"))),
                "system.cpu",
                p.cpu,
                Some(100.),
            ),
            (
                "memory",
                tr!("common.resource.memory"),
                theme::memory(number(sys, "memUsed")),
                "system.memUsed",
                p.memory,
                Some(number(sys, "memTotal")),
            ),
            (
                "disk",
                tr!("common.resource.disk"),
                theme::bytes(disk, true),
                "totals.disk",
                p.io,
                None,
            ),
            (
                "network",
                tr!("common.resource.network"),
                theme::network(
                    network,
                    self.prefs.get::<String>("netUnit", "bytes".into()) == "bits",
                ),
                "totals.network",
                p.network,
                None,
            ),
        ];
        if !values(&hw["gpus"]).is_empty() {
            cards.push((
                "gpu",
                tr!("common.resource.gpu"),
                format!("{}%", theme::cpu(gpu)),
                "totals.gpu",
                p.gpu,
                Some(100.),
            ));
        }
        let mut band = div()
            .flex()
            .flex_shrink_0()
            .border_1()
            .border_color(p.stroke)
            .rounded(cx.theme().radius)
            .bg(p.layer);
        for (id, name, value, key, color, max) in cards {
            let device = match id {
                "memory" => "memory".into(),
                "disk" => values(&hw["disks"])
                    .first()
                    .map(|d| format!("disk:{}", d["info"]["index"]))
                    .unwrap_or_else(|| "cpu".into()),
                "network" => values(&hw["nics"])
                    .first()
                    .map(|n| format!("nic:{}", text(&n["info"], "id")))
                    .unwrap_or_else(|| "cpu".into()),
                "gpu" => values(&hw["gpus"])
                    .first()
                    .map(|g| format!("gpu:{}", text(&g["info"], "id")))
                    .unwrap_or_else(|| "cpu".into()),
                _ => "cpu".into(),
            };
            let chart = self.chart(
                &format!("vital-{id}"),
                &[(key, color)],
                max,
                window.rem_size() * 3.,
                false,
                cx,
            );
            band = band.child(
                self.button(
                    format!("vital-{id}"),
                    "",
                    None,
                    Action::Device(device),
                    false,
                    cx,
                )
                .flex_1()
                .min_w_0()
                .p_3()
                .flex_col()
                .items_start()
                .gap_2()
                .child(div().text_sm().text_color(p.muted).child(name))
                .child(div().text_2xl().text_color(p.ink).child(value))
                .child(chart),
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
                    self.settings_section = "privilege".into();
                    self.act(Action::Page("settings".into()), window, cx);
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
                self.load_history(window, cx);
            }
            "historyMetric" => self.history_metric = value.as_str().unwrap_or("cpu").into(),
            "historyLatest" => {
                self.history_cursor = None;
                self.load_history(window, cx);
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
                self.settings_section = value.as_str().unwrap_or("language").into()
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
    pub(in crate::frontend) fn metric(
        &self,
        title: &str,
        value: impl Into<SharedString>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .min_w_32()
            .child(
                div()
                    .text_sm()
                    .text_color(self.palette.muted)
                    .child(label(title)),
            )
            .child(div().text_xl().child(value.into()))
            .into_any_element()
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
            .py_1()
            .child(
                div()
                    .text_color(self.palette.muted)
                    .flex_1()
                    .child(label(title)),
            )
            .child(div().flex_1().child(value.into()))
            .into_any_element()
    }
}
