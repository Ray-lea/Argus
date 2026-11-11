//! Page composition keeps domain queries separate from native widgets.
mod about;
mod details;
mod history;
mod performance;
mod settings;
mod vitals;
use super::*;
pub(super) use details::{cpu_time, date_time};
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
                let vitals = self.vitals_band(window, cx);
                let table = self.table_view(window, cx);
                // The pane follows its handle without easing while dragged.
                let dragging = self.drag.as_ref().is_some_and(|drag| drag.0 == "details");
                let target = if self.details_shown() {
                    px(self.details_display_width(window))
                } else {
                    px(0.)
                };
                let width = gpui_kit::base::motion::transition(
                    "details-width",
                    target,
                    if dragging {
                        gpui_kit::base::motion::Transition::new(Duration::ZERO)
                    } else {
                        theme::transition()
                    },
                    window,
                    cx,
                );
                // While the pane eases to another width (opening, closing, a key
                // step), the list keeps the whole card and is only drawn narrower,
                // and the pane keeps its final place and is drawn moved (details.rs):
                // both replay their last paint instead of laying out their rows
                // again every frame, and settle at the end.
                let sliding = (width - target).abs() > px(0.01);
                let (reserved, hidden) = if sliding {
                    (px(0.), width)
                } else {
                    (width, px(0.))
                };
                let details = (width > px(0.5)).then(|| {
                    (
                        div().w(width).h_full().flex_shrink_0(),
                        self.details_pane(width, window, cx),
                    )
                });
                let (spacer, pane) = details.unzip();
                let list = super::shift::clipped(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .bottom_0()
                        .right(reserved)
                        .child(table),
                    px(0.),
                    hidden,
                );
                // Drawn here rather than in the table, at the edge the list shows.
                let bar = self
                    .table_scrollbar(cx)
                    .map(|bar| super::shift::shifted(bar, point(-hidden, px(0.)), 1.));
                // A search narrows the list: an accent ring around it says so at a glance.
                let filtered = !self.query.trim().is_empty();
                let accent = self.palette.accent;
                div()
                    .relative()
                    .size_full()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(vitals)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .mx(px(12.))
                            .flex()
                            .relative()
                            // The interface's 3 px ring (a spread box shadow) as a
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
                                    .child(list)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .min_h_0()
                                            .relative()
                                            // The accent header separator, over the
                                            // table's own (its component colour).
                                            .when(filtered, |d| {
                                                d.child(
                                                    div()
                                                        .absolute()
                                                        .top(
                                                            tables::process_header_height()
                                                                - px(1.),
                                                        )
                                                        .left_0()
                                                        .right_0()
                                                        .h(px(1.))
                                                        .bg(accent.opacity(0.4)),
                                                )
                                            }),
                                    )
                                    .children(spacer)
                                    .children(pane)
                                    .children(bar),
                            ),
                    )
                    .into_any_element()
            }
            "performance" => self.performance_view(window, cx),
            "history" => self.history_view(window, cx),
            "settings" => self.settings_view(window, cx),
            "about" => self.about_view(window, cx),
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
                        let items = self.connections.as_deref().map(values).unwrap_or_default();
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
                                .text_size(theme::font(12.))
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
        let choices: &[&str] = tables::filter_choices(&self.page).map_or(&[], |(_, c)| c);
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
                        .iter()
                        .map(|&choice| {
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
                // A plain `.btn` whose text is not a `.btn-text`: it keeps its
                // label at every compact level of the toolbar.
                row = row.child(
                    self.btn(
                        "history-latest",
                        label("history.latest.label"),
                        None,
                        widgets::Tone::Plain,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.act(
                            Action::Preference("historyLatest".into(), Value::Null),
                            window,
                            cx,
                        )
                    }))
                    .tooltip(|window, cx| {
                        widgets::tooltip(label("history.latest.title"), window, cx)
                    }),
                );
            }
        }
        row.into_any_element()
    }
    pub(in crate::frontend) fn preference(
        &mut self,
        key: &str,
        value: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match key {
            "hideSystem.processes" | "hideSystem.network" => {
                self.prefs.set(key, value.as_bool().unwrap_or(false));
                self.rebuild_rows(false, cx);
                cx.notify();
                return;
            }
            model::USER_GROUPING => {
                self.prefs
                    .set(key, model::UserGrouping::parse(&value).key());
                self.rebuild_rows(false, cx);
                cx.notify();
                return;
            }
            "processColumnsAutoWidth" => {
                let mut preferences = columns::Preferences::load(&self.prefs);
                preferences.auto_width = value.as_bool().unwrap_or(true);
                if !preferences.auto_width && preferences.widths.is_empty() {
                    self.capture_process_widths(&mut preferences, cx);
                }
                // Turned on again: every column follows its contents.
                if preferences.auto_width {
                    preferences.overrides.clear();
                }
                self.set_process_columns(preferences, cx);
                return;
            }
            "processColumnsFullWidth" => {
                let mut preferences = columns::Preferences::load(&self.prefs);
                preferences.full_width = value.as_bool().unwrap_or(false);
                self.set_process_columns(preferences, cx);
                return;
            }
            "fitProcessColumn" => {
                if let Some(key) = value.as_str() {
                    self.fit_process_column_now(key, cx);
                }
                return;
            }
            "processColumnsReset" => {
                self.set_process_columns(columns::Preferences::default(), cx);
                return;
            }
            "processSort" => {
                if let Some(key) = value["key"].as_str() {
                    self.sort_processes(key, value["desc"].as_bool().unwrap_or(false), cx);
                }
                return;
            }
            "toggleProcessColumn" => {
                if let Some(key) = value.as_str() {
                    self.toggle_process_column(key, cx);
                }
                return;
            }
            "columnsCategory" => {
                self.column_category(value.as_str().unwrap_or("all"), window, cx);
                return;
            }
            "finishOnboarding" => {
                self.finish_onboarding(window, cx);
                return;
            }
            "configurePrivilege" => {
                self.request(
                    "complete_onboarding",
                    json!({"version":welcome::configuration()["onboardingVersion"]}),
                    Some("settings"),
                    window,
                    cx,
                );
                self.act(Action::CloseOverlay, window, cx);
                self.preference("settingsSection", json!("privilege"), window, cx);
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
            "windowMotion" => {
                self.prefs.set(
                    "motion.window",
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
            "uiScale" => {
                let scale = appearance::SCALE.parse(&value.to_string());
                self.ui_scale = scale;
                self.prefs.set(key, scale);
                // Once this update is over: the new scale resizes the viewport at
                // once, and the window's bounds observers update the desktop,
                // which may not be updated twice at a time.
                window.defer(cx, move |window, cx| {
                    appearance::apply_scale(scale, window, cx)
                });
                cx.notify();
                return;
            }
            "fontSize" => {
                let size = appearance::FONT_SIZE.parse(&value.to_string());
                self.font_size = size;
                self.prefs.set(key, size);
                appearance::apply_font_size(size);
                // Applying the theme again carries the size to the components;
                // every view draws its text again.
                self.set_theme(self.palette.dark, window, cx);
                window.refresh();
                return;
            }
            performance::rail::HIDE => {
                self.set_device_hidden(&value, window);
                cx.notify();
                return;
            }
            performance::rail::RESTORE => {
                self.restore_devices();
                cx.notify();
                return;
            }
            performance::rail::DISK_ORDER => {
                self.set_disk_order(performance::rail::DiskOrder::parse(
                    value.as_str().unwrap_or_default(),
                ));
                cx.notify();
                return;
            }
            performance::rail::RESET_ORDER => {
                self.reset_device_order();
                cx.notify();
                return;
            }
            "diskPage" => self.disk_page = value.as_str().unwrap_or("overview").into(),
            "refreshDiskDetails" => {
                if let Some(index) = value.as_u64() {
                    self.read_disk_details(index, window, cx);
                }
                return;
            }
            star::REMIND => {
                // Turned off while its card shows: the card leaves.
                if value.as_bool() == Some(false) {
                    self.dismiss_star();
                }
            }
            "navPosition" => self.nav_top = value.as_str() != Some("side"),
            "navExpanded" => self.nav_expanded = value.as_bool().unwrap_or(false),
            "historyRange" => {
                self.history_range = value.as_str().unwrap_or("1h").into();
                self.history_hover = None;
                self.load_history(window, cx);
            }
            "historyMetric" => self.history_metric = value.as_str().unwrap_or("cpu").into(),
            "historyPeak" => {
                if let Some(at) = value.as_i64() {
                    self.history_hover = None;
                    self.history_cursor = Some(at);
                    self.data.insert("historyPeakAt".into(), json!(at));
                    self.load_history(window, cx);
                    cx.notify();
                }
                return;
            }
            "historyLatest" => {
                self.data.remove("historyPeakAt");
                self.history_cursor = None;
                self.history_hover = None;
                self.load_history(window, cx);
            }
            "goToService" => {
                if let Some(name) = value.as_str() {
                    self.act(Action::Page("services".into()), window, cx);
                    // Like showService: a remembered search or filter must not
                    // hide the service being shown.
                    self.preference("clearSearch", Value::Null, window, cx);
                    self.set_filter("services", "all", cx);
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
                            && (value["historyFrame"].is_null()
                                || history::recorded_target(
                                    &value,
                                    value["historyFrame"].as_i64(),
                                    std::slice::from_ref(*p),
                                )
                                .is_some())
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
                self.reveal_details();
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
                self.set_filter(&key[7..], value.as_str().unwrap_or("all"), cx);
                cx.notify();
                return;
            }
            _ => {}
        }
        self.prefs.set(key, value);
        cx.notify();
    }
    /// Shows a process in the list: clears the search, unfolds its category, group
    /// and ancestors, selects it, opens the details and scrolls to it.
    fn reveal(&mut self, process: &Value, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .visibility_rules("processes")
            .is_some_and(|rules| rules.hides_process(process))
        {
            self.prefs.set(visibility::preference("processes"), false);
        }
        self.query.clear();
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        if let Some(users) = self.user_groups() {
            let group = users.id_of(process).to_owned();
            self.fold_user(&group, false);
        }
        let app = self.app_row_key(process);
        let processes = values(&self.snapshot["processes"]);
        if self.view == "apps" {
            self.expanded.insert(app);
        } else if self.view == "tree" {
            let mut child = process;
            let mut visited = HashSet::from([model::identity(process)]);
            for _ in 0..processes.len() {
                let Some(parent_pid) = child["ppid"].as_u64().filter(|pid| *pid != 0) else {
                    break;
                };
                let Some(next) = processes.iter().find(|p| {
                    p["pid"].as_u64() == Some(parent_pid)
                        && number(p, "createTime") <= number(child, "createTime")
                }) else {
                    break;
                };
                if !visited.insert(model::identity(next)) {
                    break;
                }
                self.tree_expanded.insert(model::identity(next));
                child = next;
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
        self.reveal_details();
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
