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
                // A region of its own: scrolling the list or animating the details
                // pane replays the cards.
                let vitals = self.region(
                    regions::RegionKind::Vitals,
                    StyleRefinement::default()
                        .w_full()
                        .h(theme::dimension("vitalsHeight"))
                        .flex_shrink_0(),
                    cx,
                );
                let table = self.table_view(window, cx);
                // The pane follows its handle without easing while dragged.
                let dragging = self.drag.as_ref().is_some_and(|drag| drag.0 == "details");
                let width = gpui_kit::base::motion::transition(
                    "details-width",
                    if self.details_open {
                        px(self.details_display_width(window))
                    } else {
                        px(0.)
                    },
                    if dragging {
                        gpui_kit::base::motion::Transition::new(Duration::ZERO)
                    } else {
                        theme::transition()
                    },
                    window,
                    cx,
                );
                let details = (width > px(0.5)).then(|| {
                    div()
                        .w(width)
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        // Its own entrance, as in the original: the pane slides in
                        // and its content rises (details.rs).
                        .child(
                            self.region(
                                regions::RegionKind::Details,
                                StyleRefinement::default()
                                    .w(px(self.details_display_width(window)))
                                    .h_full(),
                                cx,
                            ),
                        )
                });
                // A search narrows the list: an accent ring around it says so at a glance.
                let filtered = !self.query.trim().is_empty();
                let accent = self.palette.accent;
                div()
                    .size_full()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(vitals)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .mx(px(12.))
                            .flex()
                            .relative()
                            // The original's 3 px ring (a spread box shadow) as a
                            // border just outside the card: GPUI paints a shadow
                            // under the whole box, where it would tint the
                            // translucent card itself.
                            .when(filtered, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .top(px(-3.))
                                        .left(px(-3.))
                                        .right(px(-3.))
                                        .bottom(px(-3.))
                                        .rounded(px(11.))
                                        .border(px(3.))
                                        .border_color(accent.opacity(0.16)),
                                )
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .flex()
                                    .border_1()
                                    .border_color(if filtered {
                                        accent.opacity(0.7)
                                    } else {
                                        self.palette.stroke
                                    })
                                    .rounded(px(8.))
                                    .overflow_hidden()
                                    .bg(self.palette.layer)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .min_h_0()
                                            .relative()
                                            .child(table)
                                            // The accent header separator, over the
                                            // table's own (its component colour).
                                            .when(filtered, |d| {
                                                d.child(
                                                    div()
                                                        .absolute()
                                                        .top(px(tables::PROCESS_HEADER_HEIGHT - 1.))
                                                        .left_0()
                                                        .right_0()
                                                        .h(px(1.))
                                                        .bg(accent.opacity(0.4)),
                                                )
                                            }),
                                    )
                                    .children(details),
                            ),
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
                // Stretched in the page's column: a full width plus the margins
                // would push the card's right edge and its scrollbar out of the window.
                div()
                    .flex_1()
                    .min_h_0()
                    .mx(px(12.))
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(self.palette.stroke)
                    .rounded(px(8.))
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
            if self.history_cursor.is_some() {
                row = row.child(
                    self.button(
                        "history-latest",
                        label("history.latest.label"),
                        None,
                        Action::Preference("historyLatest".into(), Value::Null),
                        false,
                        cx,
                    )
                    .border_1()
                    .border_color(self.palette.stroke)
                    .bg(self.palette.control)
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(label("history.latest.title"))
                            .build(window, cx)
                    }),
                );
            }
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
            .sum::<f64>();
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
        // Rate charts get a round top in the unit they read in, with a floor so idle
        // noise does not fill the chart.
        let peak = |key: &str| {
            self.timeline
                .samples
                .get(key)
                .map(|s| s.iter().filter_map(|(_, v)| *v).fold(0., f64::max))
                .unwrap_or(0.)
        };
        let gb = |bytes: f64| format!("{:.1}", bytes / 1073741824.);
        let split = |value: String| -> (String, String) {
            value
                .split_once(' ')
                .map(|(figure, unit)| (figure.to_owned(), unit.to_owned()))
                .unwrap_or((value, String::new()))
        };
        let disk_value = split(if disk < 1. {
            "0 KB/s".into()
        } else {
            theme::bytes(disk, true)
        });
        let net_value = split(if rx + tx < 1. {
            if bits {
                "0 Kbps".into()
            } else {
                "0 KB/s".into()
            }
        } else {
            theme::network(rx + tx, bits)
        });
        let mem_percent = (number(sys, "memUsed") / number(sys, "memTotal").max(1.) * 100.).round();
        let mut cards: Vec<VitalCard> = vec![
            (
                "cpu",
                "cpu".into(),
                "system.cpu",
                p.cpu,
                Some(100.),
                format!("{}", number(sys, "cpu").round()),
                "%".into(),
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
                Some(number(sys, "memTotal").max(1.)),
                gb(number(sys, "memUsed")),
                format!("/ {} GB", gb(number(sys, "memTotal"))),
                format!("{mem_percent}%"),
            ),
            (
                "disk",
                format!(
                    "disk:{}",
                    busiest
                        .map(|d| d["info"]["index"].clone())
                        .unwrap_or(json!(0))
                ),
                "totals.disk",
                p.io,
                Some(charts::rate_ceiling(
                    peak("totals.disk").max(1048576.),
                    false,
                )),
                disk_value.0,
                disk_value.1,
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
                format!(
                    "nic:{}",
                    values(&hw["nics"])
                        .first()
                        .map(|n| text(&n["info"], "id"))
                        .unwrap_or_default()
                ),
                "totals.network",
                p.network,
                Some(charts::rate_ceiling(
                    peak("totals.network").max(131072.),
                    bits,
                )),
                net_value.0,
                net_value.1,
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
                format!("{}", number(gpu, "util").round()),
                "%".into(),
                gpu["temperature"]
                    .as_f64()
                    .map(|v| format!("{v:.0} °C"))
                    .unwrap_or_default(),
            ));
        }
        let count = cards.len();
        let hover = p.hover;
        let press = p.press;
        let mut band = div()
            .flex()
            .flex_shrink_0()
            .h(theme::dimension("vitalsHeight"))
            .mx(px(12.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.layer);
        for (index, (id, device, key, color, max, figure, unit, detail)) in
            cards.into_iter().enumerate()
        {
            let plot = self.chart(
                &format!("vital-{id}"),
                &[(key, color)],
                max,
                theme::dimension("vitalsHeight") - px(63.),
                false,
                cx,
            );
            let name = if matches!(id, "cpu" | "gpu") {
                id.to_uppercase()
            } else {
                label(&format!("common.resource.{id}"))
            };
            let mut header = div()
                .relative()
                .flex()
                .w_full()
                .items_start()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(name.clone()),
                );
            if id == "cpu" && !values(&sys["cores"]).is_empty() {
                header = header.child(self.core_comb(cx));
            } else if !detail.is_empty() {
                header = header.child(
                    div()
                        .id(("vital-detail", index))
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(detail.clone())
                        .when(id == "network", |d| {
                            d.tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(label(
                                    "processes.vitals.netTitle",
                                ))
                                .build(window, cx)
                            })
                        }),
                );
            }
            let open = tr!("processes.vitals.open", device = name);
            band = band.child(
                Button::new(format!("vital-{id}"))
                    .relative()
                    .flex_1()
                    .when(id == "cpu", |b| b.flex_grow(1.3))
                    .min_w_0()
                    .h_full()
                    .when(index == 0, |b| b.rounded_l(px(7.)))
                    .when(index + 1 == count, |b| b.rounded_r(px(7.)))
                    .pt(px(11.))
                    .px(px(16.))
                    // The original card was a <button>: its content sits in the
                    // middle of what the top padding leaves, in the body's 1.4
                    // line height.
                    .flex()
                    .flex_col()
                    .items_start()
                    .justify_center()
                    .line_height(relative(1.4))
                    .text_color(p.ink)
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(press))
                    .when(index > 0, |d| d.border_l_1().border_color(p.stroke))
                    .accessibility_label(open.clone())
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(open.clone()).build(window, cx)
                    })
                    .child(
                        div()
                            .absolute()
                            .top(px(62.))
                            .bottom_0()
                            .left(px(if index == 0 { 4. } else { 0. }))
                            .right(px(if index + 1 == count { 4. } else { 0. }))
                            .child(plot),
                    )
                    .child(header)
                    .child(
                        div()
                            .relative()
                            .mt(px(-2.))
                            .flex()
                            .items_baseline()
                            .gap(px(10.))
                            .w_full()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .child(
                                        div()
                                            .font_family(theme::display_font(cx))
                                            .text_size(theme::dimension("figureSize"))
                                            .font_weight(FontWeight(330.))
                                            .line_height(relative(1.1))
                                            .child(figure),
                                    )
                                    .child(
                                        div()
                                            .ml(px(2.))
                                            .font_family(theme::display_font(cx))
                                            .text_size(px(15.))
                                            .text_color(p.muted)
                                            .child(unit),
                                    ),
                            )
                            .when(id == "cpu" && !detail.is_empty(), |d| {
                                d.child(div().text_size(px(12.)).text_color(p.muted).child(detail))
                            }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.act(Action::Device(device.clone()), window, cx)
                    })),
            );
        }
        band.into_any_element()
    }
    /// Per-core load, one 4 px bar per logical processor over a faint track,
    /// gliding with the chart clock.
    fn core_comb(&self, cx: &mut Context<Self>) -> AnyElement {
        let keys = (0..values(&self.snapshot["system"]["cores"]).len())
            .map(|index| format!("system.cores.{index}"))
            .collect::<Vec<_>>();
        div()
            .id("vital-cores")
            .flex_shrink_0()
            .w(charts::bars_width(keys.len()))
            .h(px(26.))
            .child(self.chart_bars("vital-core-bars", &keys, self.palette.cpu, px(26.), cx))
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(label("processes.vitals.cores"))
                    .build(window, cx)
            })
            .into_any_element()
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
                    self.reveal(&process, window, cx);
                } else {
                    self.notify_toast(
                        widgets::ToastKind::Info,
                        tr!("processes.pidExited", pid = value["pid"]),
                    );
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
            "openDetails" => {
                self.details_open = true;
                self.prefs.set("detailsPanel", true);
                if let Some(key) = value.as_str()
                    && self.selected.get(&self.page).map(String::as_str) != Some(key)
                {
                    self.act(Action::Select(key.into()), window, cx);
                }
                cx.notify();
                return;
            }
            "deleteNote" => {
                if let Some(id) = value.as_str() {
                    self.set_mark(id, "note", Value::Null, cx);
                }
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
    /// Shows a process in the list: clears the search, unfolds its category, group
    /// and ancestors, selects it, opens the details and scrolls to it.
    fn reveal(&mut self, process: &Value, window: &mut Window, cx: &mut Context<Self>) {
        self.query.clear();
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        let processes = values(&self.snapshot["processes"]);
        if self.view == "apps" {
            self.expanded.insert(model::application_key(process));
        } else if self.view == "tree" {
            let mut parent = process["ppid"].clone();
            for _ in 0..processes.len() {
                let Some(next) = processes.iter().find(|p| p["pid"] == parent) else {
                    break;
                };
                self.collapsed.remove(&model::identity(next));
                parent = next["ppid"].clone();
            }
        }
        let section = if boolean(process, "appWindow") {
            "apps"
        } else if boolean(process, "windowsProcess") {
            "windows"
        } else {
            "background"
        };
        self.sections.remove(section);
        self.prefs.set(
            "collapsedSections",
            self.sections.iter().collect::<Vec<_>>(),
        );
        self.rebuild_rows(false, cx);
        self.details_open = true;
        self.prefs.set("detailsPanel", true);
        let identity = model::identity(process);
        self.last_select = None;
        self.act(Action::Select(identity.clone()), window, cx);
        if let Some(index) = self.rows.iter().position(|row| row.key == identity)
            && let Some(table) = &self.table
        {
            table.update(cx, |state, cx| state.scroll_to_row(index, cx));
        }
    }
}

/// A vitals segment: id, device to open, series, colour, chart top, figure, unit, detail.
type VitalCard = (
    &'static str,
    String,
    &'static str,
    Hsla,
    Option<f64>,
    String,
    String,
    String,
);
